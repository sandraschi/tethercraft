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
        #[arg(long)]
        password_file: Option<String>,
        #[arg(long)]
        headless: Option<String>,
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
    Platform,
    Gui,
    Server {
        #[arg(long, default_value_t = 21116)]
        port: u16,
    },
    Host {
        #[arg(long, default_value_t = 21120)]
        port: u16,
        #[arg(long)]
        password_file: Option<String>,
        #[arg(long)]
        grants: Option<String>,
        #[arg(long, default_value_t = 10)]
        fps: u8,
        #[arg(long, default_value_t = false)]
        fake_frames: bool,
    },
    Serve {
        peer: String,
        #[arg(long)]
        password_file: Option<String>,
        #[arg(long, default_value_t = false)]
        view_only: bool,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = 10)]
        frames: u32,
        #[arg(long, default_value_t = false)]
        input_probe: bool,
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
        #[arg(long)]
        addr: Option<String>,
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

/// Read a password from a file (or stdin with `-`). Never from argv, never logged.
fn load_password(path: Option<&str>) -> Result<Vec<u8>, TetherError> {
    match path {
        Some("-") => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                .map_err(|e| TetherError::Invalid(format!("stdin: {e}")))?;
            Ok(s.trim_end_matches(['\r', '\n']).as_bytes().to_vec())
        }
        Some(p) => {
            let s = std::fs::read_to_string(p)
                .map_err(|e| TetherError::Invalid(format!("password file: {e}")))?;
            Ok(s.trim_end_matches(['\r', '\n']).as_bytes().to_vec())
        }
        None => Err(TetherError::Invalid(
            "a password is required: pass --password-file, or - to read stdin".to_string(),
        )),
    }
}

/// Parse `--grants keyboard,touch` into host permissions.
fn parse_grants(spec: &str) -> Result<Vec<tethercraft_core::Permission>, TetherError> {
    use tethercraft_core::Permission as P;
    let mut out = Vec::new();
    for part in spec.split(',') {
        let p = match part.trim() {
            "" => continue,
            "keyboard" | "input" => P::Keyboard,
            "touch" => P::Touch,
            "clipboard" => P::Clipboard,
            "file" | "files" => P::File,
            "audio" => P::Audio,
            "terminal" => P::Terminal,
            "privacy" => P::Privacy,
            "camera" => P::Camera,
            "tunnel" => P::Tunnel,
            "restart" => P::Restart,
            other => {
                return Err(TetherError::Invalid(format!(
                    "unknown grant '{other}' (keyboard, touch, clipboard, file, audio, terminal, privacy, camera, tunnel, restart)"
                )))
            }
        };
        out.push(p);
    }
    Ok(out)
}

fn grant_names(grants: &[tethercraft_core::Permission]) -> Vec<String> {
    grants
        .iter()
        .map(|g| format!("{g:?}").to_lowercase())
        .collect()
}

/// Open the controller GUI window. Returns after the window closes.
fn run_gui(peers: Vec<(String, String)>, live: Option<ui::LiveSource>) -> Result<(), TetherError> {
    let (outgoing, needs_answer, marker) = read_onboarding();
    let mut app = ui::ControllerUi::new(peers, outgoing, needs_answer, marker);
    if let Some(l) = live {
        app.set_live(l);
    }
    eframe::run_native(
        "TetherCraft",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(app) as Box<dyn eframe::App>)),
    )
    .map_err(|e| TetherError::Invalid(format!("gui failed: {e}")))?;
    Ok(())
}

fn peer_list() -> Vec<(String, String)> {
    match open_store().and_then(|s| s.list_peers()) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| (r.peer_id, r.alias.unwrap_or_default()))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn peer_addr(peer: &str) -> Result<String, TetherError> {
    let s = open_store()?;
    let peers = s.list_peers()?;
    match peers.iter().find(|r| r.peer_id == peer) {
        Some(r) => match &r.addr {
            Some(a) if !a.trim().is_empty() => Ok(a.clone()),
            _ => Err(TetherError::Invalid(format!(
                "peer {peer} has no address: re-add it with --addr host:port"
            ))),
        },
        None => Err(TetherError::Invalid(format!("unknown peer {peer}"))),
    }
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
        Cmd::Connect {
            peer,
            view_only,
            password_file,
            headless,
        } => {
            let addr = peer_addr(&peer)?;
            match headless {
                Some(out) => {
                    let password = load_password(password_file.as_deref())?;
                    tethercraft_net::session::run_headless_controller(&addr, &password, &out)
                        .map_err(|e| TetherError::Invalid(format!("connect: {e}")))?;
                    print_out(cli.json, format!("saved {out}"), json!({"saved": out}));
                    Ok(())
                }
                None => {
                    let password = load_password(password_file.as_deref())?;
                    let live = ui::connect_live(&addr, &password, view_only)?;
                    run_gui(peer_list(), Some(live))
                }
            }
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
            BookOp::Add {
                peer,
                key,
                alias,
                addr,
            } => {
                let s = open_store()?;
                s.add_peer(&peer, alias.as_deref(), &key, addr.as_deref())?;
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
        Cmd::Platform => {
            let info = tethercraft_core::mobile::platform_info();
            print_out(
                cli.json,
                format!("platform {} {}", info.os, info.arch),
                json!({"platform": info}),
            );
            Ok(())
        }
        Cmd::Gui => run_gui(peer_list(), None),
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
        Cmd::Host {
            port,
            password_file,
            grants,
            fps,
            fake_frames,
        } => {
            if fps == 0 || fps > 60 {
                return Err(TetherError::Invalid("fps must be 1..=60".to_string()));
            }
            let password = load_password(password_file.as_deref())?;
            if password.is_empty() {
                return Err(TetherError::Invalid("empty password".to_string()));
            }
            let grant_list = match grants {
                Some(g) => parse_grants(&g)?,
                None => Vec::new(),
            };
            let (signing, id, words) = tethercraft_host::serve::ephemeral_identity()?;
            let store = open_store()?;
            let listener = tethercraft_net::session::listen(&format!("0.0.0.0:{port}"))?;
            let cfg = tethercraft_host::serve::ServeConfig {
                port,
                password,
                grants: grant_list.clone(),
                fps,
                fake_frames,
                outgoing_only: false,
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            };
            print_out(
                cli.json,
                format!(
                    "hosting on 0.0.0.0:{port}\nid {id}\nwords: {words}\ngrants: {g:?}\nframes: {f}",
                    g = grant_names(&grant_list),
                    f = if fake_frames { "synthetic" } else { "live (WGC)" }
                ),
                json!({
                    "port": port,
                    "id": id,
                    "words": words,
                    "grants": grant_names(&grant_list),
                    "fake_frames": fake_frames,
                }),
            );
            loop {
                match tethercraft_host::serve::serve_one(&listener, &cfg, &signing, &store) {
                    Ok(report) => {
                        print_out(
                            cli.json,
                            format!(
                                "session with {p}: frames {f}, inputs {i}, denied {d}, ended {e}",
                                p = report.peer_id,
                                f = report.frames_sent,
                                i = report.inputs_applied,
                                d = report.denied_inputs,
                                e = report.ended
                            ),
                            json!({
                                "peer": report.peer_id,
                                "frames": report.frames_sent,
                                "inputs": report.inputs_applied,
                                "denied": report.denied_inputs,
                                "ended": report.ended,
                            }),
                        );
                    }
                    Err(e) => eprintln!("session error: {e}"),
                }
            }
        }
        Cmd::Serve {
            peer,
            password_file,
            view_only,
            out,
            frames,
            input_probe,
        } => {
            let addr = peer_addr(&peer)?;
            let password = load_password(password_file.as_deref())?;
            let saved = tethercraft_net::session::run_headless_controller_count(
                &addr,
                &password,
                frames,
                out.as_deref(),
                input_probe,
            )?;
            print_out(
                cli.json,
                format!("received {n} frames, saved {saved}", n = frames),
                json!({"frames": frames, "saved": saved, "view_only": view_only}),
            );
            Ok(())
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
        assert!(Cli::try_parse_from(["tethercraft", "platform"]).is_ok());
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
