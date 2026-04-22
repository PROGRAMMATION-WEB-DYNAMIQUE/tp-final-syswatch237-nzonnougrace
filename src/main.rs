// src/main.rs
use chrono::Local;
use std::fmt;
use sysinfo::{System, Process};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::fs::OpenOptions;

const AUTH_TOKEN: &str = "ENSPD2026";

// --- Types métier ---

#[derive(Debug, Clone)]
struct CpuInfo {
    usage_percent: f32,
    core_count: usize,
}

#[derive(Debug, Clone)]
struct MemInfo {
    total_mb: u64,
    used_mb: u64,
    free_mb: u64,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory_mb: u64,
}

#[derive(Debug, Clone)]
struct SystemSnapshot {
    timestamp: String,
    cpu: CpuInfo,
    memory: MemInfo,
    top_processes: Vec<ProcessInfo>,
}

// --- Affichage humain (Trait Display) ---

impl fmt::Display for CpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CPU: {:.1}% ({} cœurs)", self.usage_percent, self.core_count)
    }
}

impl fmt::Display for MemInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MEM: {}MB utilisés / {}MB total ({} MB libres)",
            self.used_mb, self.total_mb, self.free_mb
        )
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  [{:>6}] {:<25} CPU:{:>5.1}%  MEM:{:>5}MB",
            self.pid, self.name, self.cpu_usage, self.memory_mb
        )
    }
}

impl fmt::Display for SystemSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== SysWatch — {} ===", self.timestamp)?;
        writeln!(f, "{}", self.cpu)?;
        writeln!(f, "{}", self.memory)?;
        writeln!(f, "--- Top Processus ---")?;
        for p in &self.top_processes {
            writeln!(f, "{}", p)?;
        }
        write!(f, "=====================")
    }
}

// --- Erreurs custom (exo 2) ---

#[derive(Debug)]
enum SysWatchError {
    CollectionFailed(String),
}

impl fmt::Display for SysWatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysWatchError::CollectionFailed(msg) => write!(f, "Erreur collecte: {}", msg),
        }
    }
}

impl std::error::Error for SysWatchError {}

// --- Collecte système ---

fn collect_snapshot() -> Result<SystemSnapshot, SysWatchError> {
    let mut sys = System::new_all();
    sys.refresh_all();

    // Petite pause pour que sysinfo ait des valeurs CPU non nulles
    std::thread::sleep(std::time::Duration::from_millis(500));
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_info().cpu_usage();
    let core_count = sys.cpus().len();

    if core_count == 0 {
        return Err(SysWatchError::CollectionFailed("Aucun CPU détecté".to_string()));
    }

    let total_mb = sys.total_memory() / 1024 / 1024;
    let used_mb = sys.used_memory() / 1024 / 1024;
    let free_mb = sys.free_memory() / 1024 / 1024;

    // Top 5 processus par consommation CPU
    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .values()
        .map(|p: &Process| ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().to_string(),
            cpu_usage: p.cpu_usage(),
            memory_mb: p.memory() / 1024 / 1024,
        })
        .collect();

    // Correction: utilisation de partial_cmp correcte
    processes.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(5);

    Ok(SystemSnapshot {
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        cpu: CpuInfo { usage_percent: cpu_usage, core_count },
        memory: MemInfo { total_mb, used_mb, free_mb },
        top_processes: processes,
    })
}

// --- Formatage des réponses ---

fn format_response(snapshot: &SystemSnapshot, command: &str) -> String {
    let cmd = command.trim().to_lowercase();

    match cmd.as_str() {
        "cpu" => format!(
            "[CPU]\n{}\n\nHistorique:\n{}\n",
            snapshot.cpu,
            (0..10)
                .map(|i| {
                    let threshold = (snapshot.cpu.usage_percent / 10.0) as usize;
                    if i < threshold { "█" } else { "░" }
                })
                .collect::<Vec<_>>()
                .join("") + &format!(" {:.1}%", snapshot.cpu.usage_percent)
        ),

        "mem" => {
            let percent = (snapshot.memory.used_mb as f64 / snapshot.memory.total_mb as f64) * 100.0;
            let bar: String = (0..20)
                .map(|i| if i < (percent / 5.0) as usize { '█' } else { '░' })
                .collect();
            format!(
                "[MÉMOIRE]\n{}\n[{}] {:.1}%\n",
                snapshot.memory, bar, percent
            )
        },

        "ps" | "procs" => {
            let lines: String = snapshot
                .top_processes
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}. {}", i + 1, p))
                .collect::<Vec<_>>()
                .join("\n");
            format!("[PROCESSUS — Top {}]\n{}\n", snapshot.top_processes.len(), lines)
        },

        "shutdown" => {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("shutdown")
                    .args(["/s", "/t", "5"])
                    .spawn()
                    .ok();
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new("shutdown")
                    .args(["-h", "+1"])
                    .spawn()
                    .ok();
            }
            "SHUTDOWN programmé dans 5 secondes.\n".to_string()
        }

        "reboot" => {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("shutdown")
                    .args(["/r", "/t", "5"])
                    .spawn()
                    .ok();
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new("shutdown")
                    .args(["-r", "+1"])
                    .spawn()
                    .ok();
            }
            "REBOOT programmé dans 5 secondes.\n".to_string()
        }

        "abort" => {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("shutdown")
                    .args(["/a"])
                    .spawn()
                    .ok();
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new("shutdown")
                    .args(["-c"])
                    .spawn()
                    .ok();
            }
            "Extinction annulée.\n".to_string()
        }

        _ if cmd.starts_with("msg ") => {
            let text = &cmd[4..];
            println!("\n╔══════════════════════════════════════╗");
            println!("║  MESSAGE DU PROFESSEUR               ║");
            println!("║  {}{}║", text, " ".repeat(38usize.saturating_sub(text.len())));
            println!("╚══════════════════════════════════════╝\n");
            format!("Message affiché sur la machine cible.\n")
        }

        _ if cmd.starts_with("install ") => {
            let package = cmd[8..].trim().to_string();
            std::thread::spawn(move || {
                #[cfg(target_os = "windows")]
                {
                    std::process::Command::new("winget")
                        .args(["install", "--silent", &package])
                        .status()
                        .ok();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    eprintln!("Installation non supportée sur cette plateforme");
                }
            });
            format!("Installation de '{}' lancée en arrière-plan.\n", &cmd[8..])
        }

        "all" | "" => format!("{}\n", snapshot),

        "help" => concat!(
            "Commandes disponibles:\n",
            "  cpu   — Usage CPU + barre\n",
            "  mem   — Mémoire RAM\n",
            "  ps    — Top processus\n",
            "  all   — Vue complète\n",
            "  help  — Cette aide\n",
            "  quit  — Fermer la connexion\n",
        ).to_string(),

        "quit" | "exit" => "BYE\n".to_string(),

        _ => format!("Commande inconnue: '{}'. Tape 'help'.\n", command.trim()),
    }
}

// --- Gestion des logs ---

fn log_event(message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {}\n", timestamp, message);

    print!("{}", line);

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("syswatch.log")
    {
        let _ = file.write_all(line.as_bytes());
    }
}

// --- Thread de rafraîchissement ---

fn snapshot_refresher(snapshot: Arc<Mutex<SystemSnapshot>>) {
    loop {
        thread::sleep(Duration::from_secs(5));
        match collect_snapshot() {
            Ok(new_snap) => {
                let mut snap = snapshot.lock().unwrap();
                *snap = new_snap;
                println!("[refresh] Métriques mises à jour");
            }
            Err(e) => eprintln!("[refresh] Erreur: {}", e),
        }
    }
}

// --- Gestion des clients ---

fn handle_client(mut stream: TcpStream, snapshot: Arc<Mutex<SystemSnapshot>>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "inconnu".to_string());
    log_event(&format!("[+] Connexion de {}", peer));

    // Demander le token
    let _ = stream.write_all(b"TOKEN: ");
    let mut reader = BufReader::new(stream.try_clone().expect("Clone failed"));
    let mut token_line = String::new();
    
    if reader.read_line(&mut token_line).is_err() || token_line.trim() != AUTH_TOKEN {
        let _ = stream.write_all(b"UNAUTHORIZED\n");
        log_event(&format!("[!] Accès refusé depuis {}", peer));
        return;
    }
    let _ = stream.write_all(b"OK\n");
    log_event(&format!("[✓] Authentifié: {}", peer));

    // Boucle de commandes
    for line in reader.lines() {
        match line {
            Ok(cmd) => {
                let cmd = cmd.trim().to_string();
                log_event(&format!("[{}] commande: '{}'", peer, cmd));

                if cmd.eq_ignore_ascii_case("quit") {
                    let _ = stream.write_all(b"BYE\n");
                    break;
                }

                let response = {
                    let snap = snapshot.lock().unwrap();
                    format_response(&snap, &cmd)
                };

                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(b"\nEND\n");
            }
            Err(_) => break,
        }
    }

    log_event(&format!("[-] Déconnexion de {}", peer));
}

// --- Main ---

fn main() {
    println!("SysWatch démarrage...");

    let initial = collect_snapshot().expect("Impossible de collecter les métriques initiales");
    println!("Métriques initiales OK:\n{}", initial);

    let shared_snapshot = Arc::new(Mutex::new(initial));

    // Thread de rafraîchissement
    {
        let snap_clone = Arc::clone(&shared_snapshot);
        thread::spawn(move || snapshot_refresher(snap_clone));
    }

    // Serveur TCP
    let listener = TcpListener::bind("0.0.0.0:7878").expect("Impossible de bind le port 7878");
    println!("Serveur en écoute sur port 7878...");
    println!("Connecte-toi avec: telnet localhost 7878");
    println!("Ctrl+C pour arrêter.\n");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let snap_clone = Arc::clone(&shared_snapshot);
                thread::spawn(move || handle_client(stream, snap_clone));
            }
            Err(e) => eprintln!("Erreur connexion entrante: {}", e),
        }
    }
}
