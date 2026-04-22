// src/master.rs
// Interface maître SysWatch — tourne sur le PC du professeur

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const AUTH_TOKEN: &str = "ENSPD2026";
const PORT: u16 = 7878;

fn machines() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("PC-01-TSEFACK".to_string(), "192.168.1.101".to_string());
    m.insert("PC-02-FOKAM".to_string(), "192.168.1.102".to_string());
    m.insert("PC-03-NZEUTEM".to_string(), "192.168.1.103".to_string());
    m.insert("ateba".to_string(), "192.168.1.105".to_string());
    // Ajouter d'autres machines ici
    m
}

struct AgentSession {
    name: String,
    ip: String,
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl AgentSession {
    fn connect(name: &str, ip: &str) -> Result<Self, String> {
        let addr = format!("{}:{}", ip, PORT);
        
        // Correction: parsing correct de l'adresse
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| format!("Adresse invalide: {}", e))?
            .next()
            .ok_or_else(|| "Impossible de résoudre l'adresse".to_string())?;

        let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2))
            .map_err(|e| format!("Connexion refusée: {}", e))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Erreur timeout: {}", e))?;

        let mut session = AgentSession {
            name: name.to_string(),
            ip: ip.to_string(),
            stream: stream.try_clone().map_err(|e| format!("Clone stream: {}", e))?,
            reader: BufReader::new(stream),
        };

        // Authentification
        session.read_until_prompt("TOKEN: ")?;
        session.send(AUTH_TOKEN)?;
        let resp = session.read_line()?;
        if resp.trim() != "OK" {
            return Err("Token refusé".to_string());
        }

        Ok(session)
    }

    fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.stream
            .write_all(format!("{}\n", cmd).as_bytes())
            .map_err(|e| e.to_string())
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line)
    }

    fn read_until_end(&mut self) -> Result<String, String> {
        let mut result = String::new();
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line.trim() == "END" {
                        break;
                    }
                    result.push_str(&line);
                }
                Err(_) => break,
            }
        }
        Ok(result)
    }

    fn read_until_prompt(&mut self, prompt: &str) -> Result<(), String> {
        let mut buf = Vec::new();
        let prompt_bytes = prompt.as_bytes();
        
        loop {
            let mut byte = [0u8; 1];
            self.reader
                .read_exact(&mut byte)
                .map_err(|e| format!("Erreur lecture: {}", e))?;
            
            buf.push(byte[0]);
            
            if buf.len() >= prompt_bytes.len() {
                let last_bytes = &buf[buf.len() - prompt_bytes.len()..];
                if last_bytes == prompt_bytes {
                    return Ok(());
                }
            }
        }
    }

    fn run_command(&mut self, cmd: &str) -> String {
        match self.send(cmd) {
            Err(e) => format!("Erreur envoi: {}", e),
            Ok(_) => self
                .read_until_end()
                .unwrap_or_else(|e| format!("Erreur lecture: {}", e)),
        }
    }
}

fn scan_machines() -> Vec<(String, String, bool)> {
    let machines = machines();
    let mut results = vec![];

    println!("Scan du réseau...");
    for (name, ip) in &machines {
        let addr = format!("{}:{}", ip, PORT);
        let reachable = addr
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .and_then(|socket_addr| {
                TcpStream::connect_timeout(&socket_addr, Duration::from_secs(1)).ok()
            })
            .is_some();
        
        let status = if reachable { "✓ EN LIGNE" } else { "✗ HORS LIGNE" };
        println!("  {} ({}) — {}", name, ip, status);
        results.push((name.clone(), ip.clone(), reachable));
    }
    results
}

fn connect_to(name: &str, ip: &str) -> Option<AgentSession> {
    match AgentSession::connect(name, ip) {
        Ok(s) => {
            println!("  [✓] Connecté à {} ({})", name, ip);
            Some(s)
        }
        Err(e) => {
            println!("  [✗] {} ({}) — {}", name, ip, e);
            None
        }
    }
}

fn print_menu() {
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║        SYSWATCH MASTER — ENSPD 2026         ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  scan          — lister les machines         ║");
    println!("║  select <nom>  — cibler une machine          ║");
    println!("║  all <cmd>     — envoyer cmd à toutes        ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Commandes disponibles sur les agents :      ║");
    println!("║  cpu / mem / ps / all                        ║");
    println!("║  msg <texte>   — afficher message            ║");
    println!("║  install <pkg> — installer un logiciel       ║");
    println!("║  shutdown      — éteindre la machine         ║");
    println!("║  reboot        — redémarrer                  ║");
    println!("║  abort         — annuler extinction          ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  help          — afficher ce menu            ║");
    println!("║  quit          — quitter le master           ║");
    println!("╚══════════════════════════════════════════════╝");
}

fn main() {
    print_menu();

    let machines_list = machines();
    let mut selected_name: Option<String> = None;
    let stdin = std::io::stdin();

    loop {
        let prompt = match &selected_name {
            Some(name) => format!("[master@{}]> ", name),
            None => "[master]> ".to_string(),
        };
        print!("{}", prompt);
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() {
            continue;
        }
        let input = input.trim().to_string();

        if input.is_empty() {
            continue;
        }

        match input.as_str() {
            "quit" | "exit" => {
                println!("Au revoir.");
                break;
            }

            "help" => print_menu(),

            "scan" => {
                scan_machines();
            }

            _ if input.starts_with("select ") => {
                let name = input[7..].trim().to_string();
                if machines_list.contains_key(&name) {
                    selected_name = Some(name.clone());
                    println!("Machine sélectionnée : {}", name);
                } else {
                    println!("Machine inconnue : '{}'. Lance 'scan' pour voir les machines disponibles.", name);
                }
            }

            _ if input.starts_with("all ") => {
                let cmd = input[4..].trim().to_string();
                println!("Envoi de '{}' à toutes les machines...", cmd);

                for (name, ip) in &machines_list {
                    print!("  {} — ", name);
                    std::io::stdout().flush().unwrap();
                    match connect_to(name, ip) {
                        Some(mut session) => {
                            let response = session.run_command(&cmd);
                            let first_line = response.lines().next().unwrap_or("(vide)");
                            println!("{}", first_line);
                        }
                        None => println!("hors ligne"),
                    }
                }
            }

            cmd => {
                match &selected_name.clone() {
                    None => println!("Aucune machine sélectionnée. Utilise 'select <nom>' ou 'all <cmd>'."),
                    Some(name) => {
                        let ip = machines_list[name].clone();
                        match connect_to(name, &ip) {
                            None => println!("Machine hors ligne."),
                            Some(mut session) => {
                                let response = session.run_command(cmd);
                                println!("{}", response);
                            }
                        }
                    }
                }
            }
        }
    }
}
