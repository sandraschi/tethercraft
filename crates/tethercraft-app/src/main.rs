//! tethercraft CLI: status, id, peers, peer-info, connect, screenshot, book, audit, wake.
//! Every verb supports --json. Passwords never go on the command line.

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use tethercraft_core::TetherError;
use tethercraft_net::{fingerprint_hex, fingerprint_words, generate_device_key, public_key_bytes};

mod ui;

#[derive(Parser)]
#[command(name = "tethercraft", version)]
struct Cli {
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Status,
    Id,
    Peers,
    PeerInfo {
        peer: String,
    },
    Connect {
        peer: String,
        #[arg(long, default_value_t = false)]
        view_only: bool,
    },
    Screenshot {
        peer: String,
        out: String,
    },
    Book {
        #[command(subcommand)]
        op: BookOp,
    },
    Audit {
        #[arg(long)]
        peer: Option<String>,
    },
    Wake {
        peer: String,
    },
    Files {
        #[command(subcommand)]
        op: FilesOp,
    },
    Sync {
        peer: String,
        local: String,
        remote: String,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, value_enum, default_value_t = SyncDirection::Both)]
        direction: SyncDirection,
        #[arg(long, value_enum, default_value_t = SyncConflict::Newer)]
        conflict: SyncConflict,
        #[arg(long, default_value_t = false)]
        delete: bool,
        #[arg(long, default_value_t = false)]
        confirm: bool,
    },
    BroadcastFile {
        peers: String,
        local: String,
        remote: String,
    },
    CollectFiles {
        peers: String,
        remote_dir: String,
        local_dir: String,
    },
    Batch {
        manifest: String,
    },
    Restart {
        peer: String,
    },
    Shutdown {
        peer: String,
    },
    Lock {
        peer: String,
    },
    Exec {
        peer: String,
        #[arg(last = true)]
        command: Vec<String>,
    },
    Tunnel {
        peer: String,
        mapping: String,
    },
    Recordings {
        #[command(subcommand)]
        op: RecordOp,
    },
    UpdateCheck {
        metadata: String,
        #[arg(long)]
        current: String,
        #[arg(long)]
        key: String,
    },
    Gui,
    Server {
        #[arg(long, default_value_t = 21116)]
        port: u16,
    },
}

#[derive(Subcommand)]
enum BookOp {
    List,
    Add {
        peer: String,
        key: String,
        #[arg(long)]
        alias: Option<String>,
    },
    Rm {
        peer: String,
    },
}

#[derive(Subcommand)]
enum FilesOp {
    Ls {
        peer: String,
        path: String,
    },
    Get {
        peer: String,
        remote: String,
        local: String,
    },
    Put {
        peer: String,
        local: String,
        remote: String,
    },
    Mv {
        peer: String,
        from: String,
        to: String,
    },
    Rm {
        peer: String,
        path: String,
    },
    Mkdir {
        peer: String,
        path: String,
    },
}

#[derive(Subcommand)]
enum RecordOp {
    List {
        #[arg(long)]
        dir: Option<String>,
    },
}

/// Default local recordings directory next to the store.
fn recordings_dir() -> std::path::PathBuf {
    let store = store_path();
    match std::path::Path::new(&store).parent() {
        Some(d) => d.join("recordings"),
        None => std::path::PathBuf::from("recordings"),
    }
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum SyncDirection {
    Push,
    Pull,
    #[default]
    Both,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum SyncConflict {
    #[default]
    Newer,
    KeepBoth,
    Ask,
}

/// Parse a `local-port:remote-target` tunnel mapping. Real validation, no network.
fn parse_mapping(s: &str) -> Result<(u16, String), TetherError> {
    let (left, right) = s.split_once(':').ok_or_else(|| {
        TetherError::Invalid("mapping must be local-port:remote-target".to_string())
    })?;
    let port: u16 = left
        .parse()
        .map_err(|_| TetherError::Invalid("local port must be 1..=65535".to_string()))?;
    if port == 0 || right.is_empty() {
        return Err(TetherError::Invalid("bad tunnel mapping".to_string()));
    }
    Ok((port, right.to_string()))
}

/// Sync preflight: deletes need `--delete` plus `--confirm`, like the host apply path.
fn sync_preflight(delete: bool, confirm: bool) -> Result<(), TetherError> {
    if delete && !confirm {
        return Err(TetherError::Denied(
            "sync deletes need --delete plus --confirm".to_string(),
        ));
    }
    Ok(())
}

fn known_peer(peer: &str) -> Result<(), TetherError> {
    if peer.is_empty() {
        return Err(TetherError::Invalid("empty peer".to_string()));
    }
    let s = open_store()?;
    let peers = s.list_peers()?;
    if peers.iter().all(|r| r.peer_id != peer) {
        return Err(TetherError::Invalid(format!("unknown peer {peer}")));
    }
    Ok(())
}

fn live_only(what: &str) -> Result<(), TetherError> {
    Err(TetherError::NotImplemented(format!(
        "{what} needs a live session (Tier 2 host service)"
    )))
}

fn store_path() -> String {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{base}/tethercraft/store.sqlite3")
}

fn print_out(as_json: bool, human: String, value: serde_json::Value) {
    if as_json {
        println!("{value}");
    } else {
        println!("{human}");
    }
}

fn run() -> Result<(), TetherError> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Status => {
            print_out(
                cli.json,
                "tethercraft tier1 (native, windows) - live transport pending two-machine run"
                    .to_string(),
                json!({"ok": true, "tier": 1, "transport": "pending-live-run"}),
            );
            Ok(())
        }
        Cmd::Id => {
            let key = generate_device_key()?;
            let pk = public_key_bytes(&key);
            let hexk = hex::encode(pk);
            let words = fingerprint_words(&pk).join(" ");
            let fp = fingerprint_hex(&pk);
            // Note: fresh key shown per call until identity persistence lands.
            print_out(
                cli.json,
                format!("id {hexk}\nfingerprint {fp}\nwords: {words}"),
                json!({"id": hexk, "fingerprint": fp, "words": words}),
            );
            Ok(())
        }
        Cmd::Peers => {
            let s = open_store()?;
            let peers = s.list_peers()?;
            print_out(
                cli.json,
                format!("{} peers", peers.len()),
                json!({"peers": peers}),
            );
            Ok(())
        }
        Cmd::PeerInfo { peer } => {
            let s = open_store()?;
            let peers = s.list_peers()?;
            match peers.iter().find(|r| r.peer_id == peer) {
                Some(row) => {
                    print_out(cli.json, format!("peer {}", row.peer_id), json!(row));
                    Ok(())
                }
                None => Err(TetherError::Invalid(format!("unknown peer {peer}"))),
            }
        }
        Cmd::Connect { peer, view_only } => {
            // Tier 1: validate the peer exists, then report partial (no fake session).
            let s = open_store()?;
            let peers = s.list_peers()?;
            if peers.iter().all(|r| r.peer_id != peer) {
                return Err(TetherError::Invalid(format!("unknown peer {peer}")));
            }
            Err(TetherError::NotImplemented(format!(
                "live connect to {peer} (view_only={view_only}) needs the two-machine QUIC run"
            )))
        }
        Cmd::Screenshot { peer, out } => Err(TetherError::NotImplemented(format!(
            "screenshot of {peer} to {out} needs a live session (Tier 1 exit)"
        ))),
        Cmd::Book { op } => match op {
            BookOp::List => {
                let s = open_store()?;
                let peers = s.list_peers()?;
                print_out(
                    cli.json,
                    format!("{} peers", peers.len()),
                    json!({"peers": peers}),
                );
                Ok(())
            }
            BookOp::Add { peer, key, alias } => {
                let s = open_store()?;
                s.add_peer(&peer, alias.as_deref(), &key)?;
                print_out(cli.json, format!("added {peer}"), json!({"added": peer}));
                Ok(())
            }
            BookOp::Rm { peer } => Err(TetherError::NotImplemented(format!(
                "remove {peer} ships with Tier 2 store verbs"
            ))),
        },
        Cmd::Audit { peer } => {
            let s = open_store()?;
            let log = s.list_audit(100)?;
            let filtered: Vec<_> = match peer {
                Some(p) => log.into_iter().filter(|e| e.peer_id == p).collect(),
                None => log,
            };
            print_out(
                cli.json,
                format!("{} events", filtered.len()),
                json!({"events": filtered}),
            );
            Ok(())
        }
        Cmd::Wake { peer } => {
            // WOL packet send is Tier 1 live work; validate input honestly.
            if peer.is_empty() {
                return Err(TetherError::Invalid("empty peer".to_string()));
            }
            Err(TetherError::NotImplemented(format!(
                "wake {peer}: WOL send needs the LAN run"
            )))
        }
        Cmd::Files { op } => match op {
            FilesOp::Ls { peer, path } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&path)?;
                live_only(&format!("files ls {peer} {path}"))
            }
            FilesOp::Get {
                peer,
                remote,
                local,
            } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&remote)?;
                if local.is_empty() {
                    return Err(TetherError::Invalid("local path required".to_string()));
                }
                live_only(&format!("files get {peer} {remote}"))
            }
            FilesOp::Put {
                peer,
                local,
                remote,
            } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&remote)?;
                if std::path::Path::new(&local).exists() {
                    live_only(&format!("files put {peer} {remote}"))
                } else {
                    Err(TetherError::Invalid(format!("local file missing: {local}")))
                }
            }
            FilesOp::Mv { peer, from, to } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&from)?;
                tethercraft_proto::validate_transfer_path(&to)?;
                live_only(&format!("files mv {peer}"))
            }
            FilesOp::Rm { peer, path } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&path)?;
                live_only(&format!("files rm {peer} {path}"))
            }
            FilesOp::Mkdir { peer, path } => {
                known_peer(&peer)?;
                tethercraft_proto::validate_transfer_path(&path)?;
                live_only(&format!("files mkdir {peer} {path}"))
            }
        },
        Cmd::Sync {
            peer,
            local,
            remote,
            dry_run,
            direction,
            conflict,
            delete,
            confirm,
        } => {
            known_peer(&peer)?;
            sync_preflight(delete, confirm)?;
            // Real local scan: the plan-first half of folder sync works today.
            let root = std::path::Path::new(&local);
            let scanned = tethercraft_host::sync::scan_local(root)?;
            let (dir, conf) = match (direction, conflict) {
                (SyncDirection::Push, SyncConflict::Newer) => ("push", "newer"),
                (SyncDirection::Push, SyncConflict::KeepBoth) => ("push", "keep-both"),
                (SyncDirection::Push, SyncConflict::Ask) => ("push", "ask"),
                (SyncDirection::Pull, SyncConflict::Newer) => ("pull", "newer"),
                (SyncDirection::Pull, SyncConflict::KeepBoth) => ("pull", "keep-both"),
                (SyncDirection::Pull, SyncConflict::Ask) => ("pull", "ask"),
                (SyncDirection::Both, SyncConflict::Newer) => ("both", "newer"),
                (SyncDirection::Both, SyncConflict::KeepBoth) => ("both", "keep-both"),
                (SyncDirection::Both, SyncConflict::Ask) => ("both", "ask"),
            };
            print_out(
                cli.json,
                format!(
                    "sync plan: {n} local files scanned (direction {dir}, conflict {conf})",
                    n = scanned.len()
                ),
                json!({"scanned": scanned.len(), "direction": dir, "conflict": conf, "dry_run": dry_run}),
            );
            live_only(&format!("files sync {peer} {local} {remote}"))
        }
        Cmd::BroadcastFile {
            peers,
            local,
            remote,
        } => {
            if peers.split(',').any(|p| p.trim().is_empty()) {
                return Err(TetherError::Invalid(
                    "peers must be comma-separated ids".to_string(),
                ));
            }
            tethercraft_proto::validate_transfer_path(&remote)?;
            if !std::path::Path::new(&local).exists() {
                return Err(TetherError::Invalid(format!("local file missing: {local}")));
            }
            live_only("broadcast-file")
        }
        Cmd::CollectFiles {
            peers,
            remote_dir,
            local_dir,
        } => {
            if peers.split(',').any(|p| p.trim().is_empty()) {
                return Err(TetherError::Invalid(
                    "peers must be comma-separated ids".to_string(),
                ));
            }
            tethercraft_proto::validate_transfer_path(&remote_dir)?;
            if local_dir.is_empty() {
                return Err(TetherError::Invalid("local dir required".to_string()));
            }
            live_only("collect-files")
        }
        Cmd::Batch { manifest } => {
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| TetherError::Invalid(format!("manifest unreadable: {e}")))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| TetherError::Invalid(format!("manifest not JSON: {e}")))?;
            if value.get("ops").is_none() {
                return Err(TetherError::Invalid(
                    "manifest needs an ops array".to_string(),
                ));
            }
            live_only("batch manifest")
        }
        Cmd::Restart { peer } => {
            known_peer(&peer)?;
            live_only(&format!("restart {peer}"))
        }
        Cmd::Shutdown { peer } => {
            known_peer(&peer)?;
            live_only(&format!("shutdown {peer}"))
        }
        Cmd::Lock { peer } => {
            known_peer(&peer)?;
            live_only(&format!("lock {peer}"))
        }
        Cmd::Exec { peer, command } => {
            known_peer(&peer)?;
            if command.is_empty() {
                return Err(TetherError::Invalid("exec needs a command".to_string()));
            }
            live_only(&format!("exec {peer} (terminal channel, permission-gated)"))
        }
        Cmd::Tunnel { peer, mapping } => {
            known_peer(&peer)?;
            let (port, target) = parse_mapping(&mapping)?;
            live_only(&format!("tunnel {peer} {port}:{target}"))
        }
        Cmd::Recordings { op } => match op {
            RecordOp::List { dir } => {
                let root = match dir {
                    Some(d) => std::path::PathBuf::from(d),
                    None => recordings_dir(),
                };
                let mut found: Vec<serde_json::Value> = Vec::new();
                if root.is_dir() {
                    let mut entries: Vec<_> = std::fs::read_dir(&root)
                        .map_err(|e| TetherError::Invalid(format!("recordings unreadable: {e}")))?
                        .filter_map(|e| e.ok())
                        .collect();
                    entries.sort_by_key(|e| e.file_name());
                    for entry in entries {
                        let path = entry.path();
                        if path.extension().and_then(|x| x.to_str()) != Some("tetherrec") {
                            continue;
                        }
                        let meta = entry
                            .metadata()
                            .map_err(|e| TetherError::Invalid(format!("stat: {e}")))?;
                        found.push(serde_json::json!({
                            "file": path.file_name(),
                            "bytes": meta.len(),
                        }));
                    }
                }
                print_out(
                    cli.json,
                    format!("{} recordings", found.len()),
                    json!({"recordings": found}),
                );
                Ok(())
            }
        },
        Cmd::UpdateCheck {
            metadata,
            current,
            key,
        } => {
            let text = std::fs::read_to_string(&metadata)
                .map_err(|e| TetherError::Invalid(format!("metadata unreadable: {e}")))?;
            let signed: tethercraft_net::update::SignedUpdate = serde_json::from_str(&text)
                .map_err(|e| TetherError::Invalid(format!("metadata not JSON: {e}")))?;
            let key_bytes = hex::decode(key.trim())
                .map_err(|_| TetherError::Invalid("operator key must be hex".to_string()))?;
            if key_bytes.len() != 32 {
                return Err(TetherError::Invalid(
                    "operator key must be 32 bytes".to_string(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key_bytes);
            let vk = tethercraft_net::parse_public_key(&arr)?;
            match tethercraft_net::update::verify_update(&signed, &current, &vk)? {
                tethercraft_net::update::UpdateDecision::UpToDate => {
                    print_out(
                        cli.json,
                        "up to date".to_string(),
                        json!({"update": "none"}),
                    );
                    Ok(())
                }
                tethercraft_net::update::UpdateDecision::Available { version, url } => {
                    print_out(
                        cli.json,
                        format!("update available: {version}"),
                        json!({"update": version, "url": url}),
                    );
                    Ok(())
                }
            }
        }
        Cmd::Gui => {
            let s = open_store()?;
            let peers = s.list_peers().unwrap_or_default();
            let list: Vec<(String, String)> = peers
                .into_iter()
                .map(|r| (r.peer_id, r.alias.unwrap_or_default()))
                .collect();
            let (outgoing, needs_answer, marker) = read_onboarding();
            let app = ui::ControllerUi::new(list, outgoing, needs_answer, marker);
            eframe::run_native(
                "TetherCraft",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Ok(Box::new(app) as Box<dyn eframe::App>)),
            )
            .map_err(|e| TetherError::Invalid(format!("gui failed: {e}")))?;
            Ok(())
        }
        Cmd::Server { port } => {
            let listener = std::net::TcpListener::bind(format!("0.0.0.0:{port}"))
                .map_err(|e| TetherError::Invalid(format!("bind: {e}")))?;
            print_out(
                cli.json,
                format!("rendezvous+relay on 0.0.0.0:{port}"),
                json!({"port": port}),
            );
            tethercraft_server::serve(
                listener,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
        }
    }
}

fn onboarding_marker_path() -> std::path::PathBuf {
    let store = store_path();
    let dir = std::path::Path::new(&store);
    match dir.parent() {
        Some(d) => d.join("onboarding.txt"),
        None => std::path::PathBuf::from("onboarding.txt"),
    }
}

/// Read the persisted R1 answer. Returns (outgoing_only, needs_answer, marker_path).
fn read_onboarding() -> (bool, bool, Option<std::path::PathBuf>) {
    let path = onboarding_marker_path();
    let now = chrono::Utc::now();
    match std::fs::read_to_string(&path) {
        Ok(text) => match ui::parse_marker(&text) {
            Some(m) if m.answered => (m.active(now), false, Some(path)),
            _ => (false, true, Some(path)),
        },
        Err(_) => (false, true, Some(path)),
    }
}

fn open_store() -> Result<tethercraft_store::Store, TetherError> {
    let path = store_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Fall back to in-memory when the file cannot be opened (CI without profile).
    match tethercraft_store::Store::open_file(&path) {
        Ok(s) => Ok(s),
        Err(_) => tethercraft_store::Store::open_in_memory(),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_parses() {
        assert!(parse_mapping("8080:10.0.0.5:3389").is_ok());
        assert!(parse_mapping("no-colon").is_err());
        assert!(parse_mapping("0:x").is_err());
        assert!(parse_mapping("99999:x").is_err());
        assert!(parse_mapping("8080:").is_err());
    }

    #[test]
    fn sync_preflight_gates_deletes() {
        assert!(sync_preflight(false, false).is_ok());
        assert!(sync_preflight(true, true).is_ok());
        assert!(sync_preflight(true, false).is_err());
    }

    #[test]
    fn cli_parses_new_verbs() {
        assert!(Cli::try_parse_from(["tethercraft", "files", "ls", "p", "docs"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "sync", "p", ".", "/r"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "exec", "p", "--", "whoami"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "tunnel", "p", "8080:x"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "recordings", "list"]).is_ok());
        assert!(Cli::try_parse_from([
            "tethercraft",
            "update-check",
            "m.json",
            "--current",
            "0.3.0",
            "--key",
            "ab"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "gui"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "server"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "server", "--port", "21116"]).is_ok());
    }
}
