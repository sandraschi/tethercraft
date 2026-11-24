//! tethercraft CLI: status, id, peers, peer-info, connect, screenshot, book, audit, wake.
//! Every verb supports --json. Passwords never go on the command line.

use clap::{Parser, Subcommand};
use serde_json::json;
use tethercraft_core::TetherError;
use tethercraft_net::{fingerprint_hex, fingerprint_words, generate_device_key, public_key_bytes};

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
