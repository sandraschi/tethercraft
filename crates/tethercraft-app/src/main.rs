//! tethercraft CLI: status, id, peers, peer-info, connect, screenshot, book, audit, wake.
//! Every verb supports --json. Passwords never go on the command line.

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use tethercraft_core::TetherError;
use tethercraft_net::{fingerprint_hex, fingerprint_words, generate_device_key, public_key_bytes};

mod board;
mod help;
mod service;
mod ui;

#[derive(Parser)]
#[command(name = "tethercraft", version)]
struct Cli {
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    /// Password file for session verbs (connect, serve, files, exec, ...).
    /// Never pass a password on the command line; it would land in history.
    #[arg(long, global = true)]
    password_file: Option<String>,
    /// Rendezvous server for relayed sessions (arbitrary targets behind NAT).
    /// When set, controller verbs dial through this server instead of direct.
    #[arg(long, global = true)]
    via: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Status,
    Health {
        /// Optional remote peer to check over an E2E session (omit for local machine).
        peer: Option<String>,
        /// Comma-separated apps/services to watch (default: ollama,docker,opencode,antigravity,cursor).
        #[arg(long, value_delimiter = ',')]
        watch: Vec<String>,
    },
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
    Chat {
        peer: String,
        text: String,
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
        grants: Option<String>,
        #[arg(long, default_value_t = 10)]
        fps: u8,
        #[arg(long, default_value_t = false)]
        fake_frames: bool,
        #[arg(long, default_value_t = "h264".to_string())]
        codec: String,
        #[arg(long)]
        pin_key: Option<String>,
        #[arg(long)]
        identity_file: Option<String>,
        #[arg(long)]
        share: Option<String>,
    },
    Serve {
        peer: String,
        #[arg(long, default_value_t = false)]
        view_only: bool,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = 10)]
        frames: u32,
        #[arg(long, default_value_t = false)]
        input_probe: bool,
    },
    Service {
        #[command(subcommand)]
        op: ServiceOp,
    },
    #[command(hide = true)]
    ServiceRun {
        #[arg(long)]
        config: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServiceOp {
    Install {
        #[arg(long)]
        config: Option<String>,
    },
    Uninstall,
    Start,
    Stop,
}

#[derive(Subcommand)]
enum BookOp {
    List,
    Add {
        peer: String,
        #[arg(default_value = "")]
        key: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        addr: Option<String>,
    },
    Rm {
        peer: String,
    },
    Promote {
        peer: String,
    },
    Demote {
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
/// Monotonic request ids, starting at 1 (zero means "unset" on the wire).
fn next_req(r: &mut u64) -> u64 {
    *r = r.wrapping_add(1);
    if *r == 0 {
        *r = 1;
    }
    *r
}

/// Recursively list + hash a remote tree through one session. Directories
/// recurse; every file gets a SHA-256 so the sync planner diffs content,
/// not just names.
fn remote_tree(
    sess: &mut tethercraft_net::session::SessionStream,
    req: &mut u64,
    remote_root: &str,
) -> Result<Vec<tethercraft_host::sync::FileMeta>, TetherError> {
    let mut out = Vec::new();
    let mut dirs = vec![remote_root.trim_end_matches('/').to_string()];
    if dirs[0].is_empty() {
        dirs[0] = ".".to_string();
    }
    while let Some(dir) = dirs.pop() {
        let entries = tethercraft_net::rpc::file_list(sess, next_req(req), &dir)?;
        for e in entries {
            let rel = if dir == "." {
                e.name.clone()
            } else {
                format!("{}/{}", dir.trim_start_matches("./"), e.name)
            };
            let rel = rel.trim_start_matches('/').to_string();
            if e.is_dir {
                dirs.push(rel);
            } else {
                let (hash, size, mtime) =
                    tethercraft_net::rpc::file_hash(sess, next_req(req), &rel)?;
                out.push(tethercraft_host::sync::FileMeta {
                    rel,
                    size,
                    hash,
                    mtime: mtime as i64,
                });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn sync_direction(direction: SyncDirection) -> tethercraft_host::sync::Direction {
    match direction {
        SyncDirection::Push => tethercraft_host::sync::Direction::Push,
        SyncDirection::Pull => tethercraft_host::sync::Direction::Pull,
        SyncDirection::Both => tethercraft_host::sync::Direction::Both,
    }
}

fn sync_conflict(conflict: SyncConflict) -> tethercraft_host::sync::Conflict {
    match conflict {
        SyncConflict::Newer => tethercraft_host::sync::Conflict::Newer,
        SyncConflict::KeepBoth => tethercraft_host::sync::Conflict::KeepBoth,
        SyncConflict::Ask => tethercraft_host::sync::Conflict::Ask,
    }
}

fn sync_preflight(delete: bool, confirm: bool) -> Result<(), TetherError> {
    if delete && !confirm {
        return Err(TetherError::Denied(
            "sync deletes need --delete plus --confirm".to_string(),
        ));
    }
    Ok(())
}

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

/// Parse `--grants keyboard,touch` into host permissions. Lives in the
/// service module so the config-file path shares it.
fn parse_grants(spec: &str) -> Result<Vec<tethercraft_core::Permission>, TetherError> {
    service::parse_grants(spec)
}

fn grant_names(grants: &[tethercraft_core::Permission]) -> Vec<String> {
    grants
        .iter()
        .map(|g| format!("{g:?}").to_lowercase())
        .collect()
}

/// Open the controller GUI window. Returns after the window closes.
fn run_gui(
    peers: Vec<ui::PeerCard>,
    live: Option<ui::LiveSource>,
    tofu: Option<ui::TofuNotice>,
) -> Result<(), TetherError> {
    let (outgoing, needs_answer, marker) = read_onboarding();
    let mut app = ui::ControllerUi::new(peers, outgoing, needs_answer, marker);
    app.pinned_keys = pinned_keys_map();
    app.on_tofu = Some(Box::new(|peer, key| {
        let _ = tofu_record(peer, key);
    }));
    app.tofu_notice = tofu;
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

fn pinned_keys_map() -> std::collections::HashMap<String, [u8; 32]> {
    let mut out = std::collections::HashMap::new();
    if let Ok(rows) = open_store().and_then(|s| s.list_peers()) {
        for r in rows {
            if let Ok(bytes) = hex::decode(r.pubkey_hex.trim()) {
                if bytes.len() == 32 {
                    let mut k = [0u8; 32];
                    k.copy_from_slice(&bytes);
                    out.insert(r.peer_id, k);
                }
            }
        }
    }
    out
}

fn peer_list() -> Vec<ui::PeerCard> {
    match open_store().and_then(|s| s.list_peers()) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| ui::PeerCard {
                id: r.peer_id,
                alias: r.alias.unwrap_or_default(),
                addr: r.addr,
                has_key: !r.pubkey_hex.trim().is_empty(),
                tier: r.tier,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Resolve a peer name to (address, pinned key). The pinned key is the
/// book's recorded host key, if any; empty means first contact (TOFU).
fn peer_addr_key(peer: &str) -> Result<(String, Option<[u8; 32]>), TetherError> {
    let s = open_store()?;
    let peers = s.list_peers()?;
    match peers.iter().find(|r| r.peer_id == peer) {
        Some(r) => match &r.addr {
            Some(a) if !a.trim().is_empty() => {
                let key = match r.pubkey_hex.trim() {
                    "" => None,
                    hex_str => {
                        let raw = hex::decode(hex_str).map_err(|_| {
                            TetherError::Invalid(format!("peer {peer} has a bad recorded key"))
                        })?;
                        if raw.len() != 32 {
                            return Err(TetherError::Invalid(format!(
                                "peer {peer} has a bad recorded key"
                            )));
                        }
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&raw);
                        Some(arr)
                    }
                };
                Ok((a.clone(), key))
            }
            _ => Err(TetherError::Invalid(format!(
                "peer {peer} has no address: re-add it with --addr host:port"
            ))),
        },
        None => Err(TetherError::Invalid(format!("unknown peer {peer}"))),
    }
}

/// Run one TCP tunnel: listen locally, bridge every inbound connection to
/// `target` through the live session on its own channel. Runs until Ctrl+C
/// or the session ends. Ctrl+C is the documented stop: closing the listener
/// drops every channel, and the host cleans up its ends.
fn tunnel_command(
    live: &mut LiveSession,
    local_port: u16,
    target: &str,
) -> Result<(), TetherError> {
    use std::sync::mpsc;
    let listener = std::net::TcpListener::bind(format!("127.0.0.1:{local_port}"))
        .map_err(|e| TetherError::Invalid(format!("tunnel listen: {e}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| TetherError::Invalid(format!("tunnel listen: {e}")))?;
    println!("tunnel up: 127.0.0.1:{local_port} -> {target} (Ctrl+C to stop)");
    // channel id -> writer to the local socket
    let mut channels: std::collections::HashMap<u32, mpsc::Sender<Vec<u8>>> =
        std::collections::HashMap::new();
    let mut next_channel: u32 = 1;
    // (channel, bytes) arriving from the host side.
    let (tun_tx, tun_rx) = mpsc::channel::<(u32, Vec<u8>)>();
    // reader threads, one per accepted connection
    let mut readers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    loop {
        // 1. Accept new local connections (nonblocking).
        match listener.accept() {
            Ok((stream, _)) => {
                if channels.len() >= 16 {
                    drop(stream);
                    eprintln!("tunnel: too many channels (max 16), refused one");
                    continue;
                }
                let channel = next_channel;
                next_channel = next_channel.wrapping_add(1).max(1);
                if let Err(e) = tethercraft_net::rpc::tunnel_open(&mut live.sess, channel, target) {
                    eprintln!("tunnel: host refused channel {channel}: {e}");
                    continue;
                }
                let (to_local_tx, to_local_rx) = mpsc::channel::<Vec<u8>>();
                channels.insert(channel, to_local_tx);
                let tun_tx = tun_tx.clone();
                readers.push(std::thread::spawn(move || {
                    use std::io::Read;
                    let mut s = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => {
                            let _ = tun_tx.send((channel, Vec::new()));
                            return;
                        }
                    };
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(60)));
                    let mut w = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => {
                            let _ = tun_tx.send((channel, Vec::new()));
                            return;
                        }
                    };
                    let mut buf = vec![0u8; 60 * 1024];
                    // Writer half: controller -> host direction for this channel.
                    // (Detached on purpose: it ends when the socket or the
                    // channel closes.)
                    std::thread::spawn(move || {
                        use std::io::Write;
                        for chunk in to_local_rx {
                            if w.write_all(&chunk).is_err() || w.flush().is_err() {
                                break;
                            }
                        }
                    });
                    // Reader half: host -> controller.
                    loop {
                        match s.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if tun_tx.send((channel, buf[..n].to_vec())).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    let _ = tun_tx.send((channel, Vec::new()));
                }));
                println!("tunnel: channel {channel} open");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(TetherError::Invalid(format!("tunnel accept: {e}"))),
        }
        // 2. Pump one session message (frames dropped, tunnel data forwarded).
        match live
            .sess
            .recv_envelope_wait(std::time::Duration::from_millis(50))
        {
            Err(_) => continue,
            Ok(None) => {}
            Ok(Some(env)) => match env.kind {
                tethercraft_proto::MessageKind::SessionEnd => {
                    println!("tunnel: host ended the session");
                    break;
                }
                tethercraft_proto::MessageKind::TunnelData => {
                    if let Ok(block) = tethercraft_net::session::parse_json::<
                        tethercraft_proto::TunnelBlock,
                    >(&env.payload)
                    {
                        let bytes = hex::decode(&block.data_hex).unwrap_or_default();
                        if let Some(tx) = channels.get(&block.channel) {
                            if tx.send(bytes).is_err() {
                                channels.remove(&block.channel);
                            }
                        }
                    }
                }
                tethercraft_proto::MessageKind::TunnelClose => {
                    if let Ok(c) = tethercraft_net::session::parse_json::<
                        tethercraft_proto::TunnelClose,
                    >(&env.payload)
                    {
                        channels.remove(&c.channel);
                        println!("tunnel: channel {} closed by host", c.channel);
                    }
                }
                _ => {}
            },
        }
        // 3. Forward host-bound bytes that arrived on reader threads.
        while let Ok((channel, bytes)) = tun_rx.try_recv() {
            if bytes.is_empty() {
                channels.remove(&channel);
                let _ = tethercraft_net::rpc::tunnel_close(&mut live.sess, channel);
                continue;
            }
            // Chunk already 60 KiB-bounded by the reader.
            if tethercraft_net::rpc::tunnel_send(&mut live.sess, channel, &bytes).is_err() {
                channels.remove(&channel);
            }
        }
    }
    Ok(())
}

/// Record the host key on first contact (TOFU). Prints the words loudly: the
/// human verifies them out of band, same as the SSH first-connect ritual.
fn tofu_record(peer: &str, key: &[u8; 32]) -> Result<(), TetherError> {
    let s = open_store()?;
    s.set_pubkey(peer, &hex::encode(key))?;
    Ok(())
}

/// An authenticated session plus TOFU bookkeeping. One session per command;
/// the controller never multiplexes unrelated verbs.
struct LiveSession {
    sess: tethercraft_net::session::SessionStream,
    req: u64,
}

impl LiveSession {
    fn next_req(&mut self) -> u64 {
        self.req = self.req.wrapping_add(1);
        if self.req == 0 {
            self.req = 1;
        }
        self.req
    }

    fn finish(mut self) -> Result<(), TetherError> {
        let end = tethercraft_proto::Envelope::new(
            tethercraft_proto::MessageKind::SessionEnd,
            [0u8; 32],
            Vec::new(),
        );
        let _ = self.sess.send_envelope(&end);
        Ok(())
    }
}

/// Dial, handshake, enforce the pinned key (or record it loudly on first
/// contact), and hand back a ready session. Every live verb goes through here.
/// If `via` is set, dial through the rendezvous relay instead of direct.
fn live_session(
    peer: &str,
    password: &[u8],
    via: Option<&str>,
) -> Result<LiveSession, TetherError> {
    let (addr, expected) = peer_addr_key(peer)?;
    let had_pin = expected.is_some();
    let signing = tethercraft_net::generate_device_key()?;
    let stream = match via {
        Some(server) => {
            let expected =
                expected.ok_or_else(|| TetherError::Invalid(format!(
                    "relay needs a known host key for {peer}: connect directly first (or pin the key)"
                )))?;
            tethercraft_net::session::relay_connect(
                server,
                "tethercraft-cli",
                &hex::encode(expected),
            )?
        }
        None => tethercraft_net::session::dial(&addr, std::time::Duration::from_secs(10))?,
    };
    let mut sess = tethercraft_net::session::SessionStream::new(stream)?;
    let auth = tethercraft_net::session::controller_handshake(
        &mut sess,
        "tethercraft-cli",
        &signing,
        password,
        expected,
    )
    .map_err(|e| TetherError::Invalid(format!("connect: {e}")))?;
    sess.set_frame_timeouts()?;
    if !had_pin {
        tofu_record(peer, &auth.host_key)?;
        eprintln!(
            "recorded new host key for {peer}: VERIFY the words out of band before trusting it"
        );
    }
    Ok(LiveSession { sess, req: 0 })
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
    // Bare run opens the controller window: that is what double-click,
    // start.bat, and desktop shortcuts execute.
    match cli.cmd.unwrap_or(Cmd::Gui) {
        Cmd::Status => {
            print_out(
                cli.json,
                "tethercraft tier3 (native, windows, E2E ChaCha20-Poly1305, persistent WGC H.264)"
                    .to_string(),
                json!({"ok": true, "tier": 3, "transport": "native-session-e2e", "e2e_cipher": "chacha20-poly1305"}),
            );
            Ok(())
        }
        Cmd::Health { peer, watch } => {
            let rep = match peer {
                None => tethercraft_host::health::collect_health(0, &watch),
                Some(p) => {
                    let password = load_password(cli.password_file.as_deref())?;
                    let mut live = live_session(&p, &password, cli.via.as_deref())?;
                    let rep = tethercraft_net::rpc::health(&mut live.sess, 1, &watch)?;
                    let _ = live.sess.send_envelope(&tethercraft_proto::Envelope::new(
                        tethercraft_proto::MessageKind::SessionEnd,
                        [0u8; 32],
                        Vec::new(),
                    ));
                    rep
                }
            };
            let mut lines = Vec::new();
            lines.push(format!(
                "host {} ({}) - {}",
                rep.host,
                rep.os,
                if rep.status == "ok" {
                    "ALL IS WELL"
                } else {
                    "WARNINGS"
                }
            ));
            if let Some(cpu) = rep.cpu_load_pct {
                lines.push(format!("  cpu: {cpu}%"));
            }
            if let (Some(u), Some(t)) = (rep.ram_used_gb, rep.ram_total_gb) {
                lines.push(format!("  ram: {u:.1} / {t:.1} GB"));
            }
            if let Some(g) = &rep.gpu {
                lines.push(format!(
                    "  gpu: {} ({} C, {}% util, {}/{} MiB VRAM, {:.0} W)",
                    g.name, g.temp_c, g.util_pct, g.vram_used_mib, g.vram_total_mib, g.power_w
                ));
            }
            for d in &rep.disks {
                let flag = if d.warn { " [WARN]" } else { "" };
                lines.push(format!(
                    "  disk {}: {:.1}/{:.1} GB free ({}% used){flag}",
                    d.name, d.free_gb, d.total_gb, d.used_pct
                ));
            }
            for a in &rep.apps {
                let mark = if a.running { "ok" } else { "DOWN" };
                lines.push(format!("  app {}: {mark} ({})", a.name, a.detail));
            }
            if !rep.warnings.is_empty() {
                lines.push(format!("  warnings: {}", rep.warnings.join("; ")));
            }
            print_out(cli.json, lines.join("\n"), json!(rep));
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
            headless,
        } => {
            let (addr, expected) = peer_addr_key(&peer)?;
            let had_pin = expected.is_some();
            // TOFU flow: dial with the recorded pin when there is one, then
            // record a new key loudly on first contact.
            let password = load_password(cli.password_file.as_deref())?;
            match headless {
                Some(out) => {
                    let got = tethercraft_net::session::run_headless_controller_pinned(
                        &addr,
                        &password,
                        &out,
                        expected,
                        cli.via.as_deref(),
                    )
                    .map_err(|e| TetherError::Invalid(format!("connect: {e}")))?;
                    if !had_pin {
                        tofu_record(&peer, &got)?;
                        eprintln!(
                            "recorded new host key for {peer}: VERIFY the words out of band before trusting it"
                        );
                    }
                    print_out(cli.json, format!("saved {out}"), json!({"saved": out}));
                    Ok(())
                }
                None => {
                    let (live, key, words) = ui::connect_live_pinned(
                        &addr,
                        &password,
                        view_only,
                        expected,
                        cli.via.as_deref(),
                    )?;
                    let tofu = if !had_pin {
                        tofu_record(&peer, &key)?;
                        eprintln!(
                            "recorded new host key for {peer}: VERIFY the words out of band before trusting it"
                        );
                        Some(ui::TofuNotice {
                            peer_id: peer.clone(),
                            words,
                            key_hex: hex::encode(key),
                        })
                    } else {
                        None
                    };
                    run_gui(peer_list(), Some(live), tofu)
                }
            }
        }
        Cmd::Screenshot { peer, out } => {
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let mut dec = tethercraft_net::session::StreamDecoder::new()
                .map_err(|e| TetherError::Invalid(format!("decoder: {e}")))?;
            let (w, h, rgba) = tethercraft_net::rpc::screenshot(&mut live.sess, &mut dec)?;
            tethercraft_net::session::write_ppm(&out, w, h, &rgba)?;
            let r = live.finish();
            print_out(
                cli.json,
                format!("saved screenshot {out} ({w}x{h})"),
                json!({"saved": out, "width": w, "height": h}),
            );
            r
        }
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
            BookOp::Rm { peer } => {
                let s = open_store()?;
                s.remove_peer(&peer)?;
                print_out(cli.json, format!("forgot {peer}"), json!({"removed": peer}));
                Ok(())
            }
            BookOp::Promote { peer } => {
                let s = open_store()?;
                match promote_step(&s, &peer, chrono::Utc::now())? {
                    PromoteOutcome::Started(at) => print_out(
                        cli.json,
                        format!(
                            "{peer}: promotion started (R3). Run this same command again after {} to confirm; nobody on a call can shorten that wait.",
                            (at + chrono::Duration::seconds(tethercraft_core::PROMOTION_COOLDOWN_SECS))
                                .format("%Y-%m-%d %H:%M UTC")
                        ),
                        json!({"promotion": "started", "peer": peer, "started_at": at.to_rfc3339()}),
                    ),
                    PromoteOutcome::Waiting { secs_left } => print_out(
                        cli.json,
                        format!("{peer}: cooling period not over, {secs_left} s left"),
                        json!({"promotion": "waiting", "peer": peer, "secs_left": secs_left}),
                    ),
                    PromoteOutcome::Promoted => print_out(
                        cli.json,
                        format!("{peer} is now a trusted helper"),
                        json!({"promoted": peer}),
                    ),
                }
                Ok(())
            }
            BookOp::Demote { peer } => {
                let s = open_store()?;
                s.clear_promotion(&peer)?;
                s.set_tier(&peer, "stranger")?;
                print_out(
                    cli.json,
                    format!("{peer} is a stranger again"),
                    json!({"demoted": peer}),
                );
                Ok(())
            }
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
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                let entries = tethercraft_net::rpc::file_list(&mut live.sess, req, &path)?;
                let r = live.finish();
                if cli.json {
                    println!("{}", json!({"entries": entries}));
                } else {
                    for e in &entries {
                        let kind = if e.is_dir { "dir " } else { "file" };
                        println!("{kind} {:>10}  {}", e.size, e.name);
                    }
                }
                r
            }
            FilesOp::Get {
                peer,
                remote,
                local,
            } => {
                if local.is_empty() {
                    return Err(TetherError::Invalid("local path required".to_string()));
                }
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                let data = tethercraft_net::rpc::file_get(&mut live.sess, req, &remote)?;
                if let Some(parent) = std::path::Path::new(&local).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| TetherError::Invalid(format!("local dir: {e}")))?;
                    }
                }
                std::fs::write(&local, &data)
                    .map_err(|e| TetherError::Invalid(format!("local write: {e}")))?;
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("got {} ({} bytes)", remote, data.len()),
                    json!({"saved": local, "bytes": data.len()}),
                );
                r
            }
            FilesOp::Put {
                peer,
                local,
                remote,
            } => {
                let data = std::fs::read(&local)
                    .map_err(|e| TetherError::Invalid(format!("local read: {e}")))?;
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                tethercraft_net::rpc::file_put(&mut live.sess, req, &remote, &data)?;
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("sent {} ({} bytes)", local, data.len()),
                    json!({"sent": remote, "bytes": data.len()}),
                );
                r
            }
            FilesOp::Mv { peer, from, to } => {
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                tethercraft_net::rpc::file_rename(&mut live.sess, req, &from, &to)?;
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("moved {from} to {to}"),
                    json!({"moved": true}),
                );
                r
            }
            FilesOp::Rm { peer, path } => {
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                tethercraft_net::rpc::file_simple(
                    &mut live.sess,
                    req,
                    tethercraft_proto::MessageKind::FileRm,
                    &path,
                )?;
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("removed {path}"),
                    json!({"removed": true}),
                );
                r
            }
            FilesOp::Mkdir { peer, path } => {
                let password = load_password(cli.password_file.as_deref())?;
                let mut live = live_session(&peer, &password, cli.via.as_deref())?;
                let req = live.next_req();
                tethercraft_net::rpc::file_simple(
                    &mut live.sess,
                    req,
                    tethercraft_proto::MessageKind::FileMkdir,
                    &path,
                )?;
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("created {path}"),
                    json!({"created": true}),
                );
                r
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
            sync_preflight(delete, confirm)?;
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let root = std::path::Path::new(&local);
            let scanned = tethercraft_host::sync::scan_local(root)?;
            let mut req_id = 0u64;
            let remote_root = remote.trim_end_matches('/');
            let remote_root = if remote_root.is_empty() {
                "."
            } else {
                remote_root
            };
            let remote_files = remote_tree(&mut live.sess, &mut req_id, remote_root)?;
            let plan = tethercraft_host::sync::plan(
                &scanned,
                &remote_files,
                sync_direction(direction),
                sync_conflict(conflict),
                delete,
            );
            let summary = plan.summary();
            if dry_run || !plan.needs_answer.is_empty() {
                let r = live.finish();
                print_out(
                    cli.json,
                    format!("sync plan (dry run): {summary}"),
                    json!({"plan": summary, "dry_run": true, "needs_answer": plan.needs_answer.len()}),
                );
                if !plan.needs_answer.is_empty() {
                    return Err(TetherError::Denied(
                        "sync needs answers first: re-run with --conflict newer or keep-both"
                            .to_string(),
                    ));
                }
                return r;
            }
            let mut pushed = 0usize;
            for f in &plan.to_push {
                let path = root.join(&f.rel);
                let data = std::fs::read(&path)
                    .map_err(|e| TetherError::Invalid(format!("local read: {e}")))?;
                let remote_path = format!("{remote_root}/{p}", p = f.rel.trim_start_matches('/'));
                let req = next_req(&mut req_id);
                tethercraft_net::rpc::file_put(&mut live.sess, req, &remote_path, &data)?;
                pushed += 1;
            }
            let mut pulled = 0usize;
            for f in &plan.to_pull {
                let req = next_req(&mut req_id);
                let data = tethercraft_net::rpc::file_get(&mut live.sess, req, &f.rel)?;
                let path = root.join(&f.rel);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| TetherError::Invalid(format!("local dir: {e}")))?;
                }
                std::fs::write(&path, &data)
                    .map_err(|e| TetherError::Invalid(format!("local write: {e}")))?;
                pulled += 1;
            }
            let mut deleted = 0usize;
            for rel in plan.delete_local.iter().chain(plan.delete_remote.iter()) {
                if plan.delete_local.contains(rel) {
                    let path = root.join(rel);
                    if path.is_file() {
                        std::fs::remove_file(&path)
                            .map_err(|e| TetherError::Invalid(format!("local delete: {e}")))?;
                        deleted += 1;
                    }
                } else {
                    tethercraft_net::rpc::file_simple(
                        &mut live.sess,
                        next_req(&mut req_id),
                        tethercraft_proto::MessageKind::FileRm,
                        rel,
                    )?;
                    deleted += 1;
                }
            }
            let r = live.finish();
            print_out(
                cli.json,
                format!("synced: pushed {pushed}, pulled {pulled}, deleted {deleted} ({summary})"),
                json!({"pushed": pushed, "pulled": pulled, "deleted": deleted, "plan": summary}),
            );
            r
        }
        Cmd::BroadcastFile {
            peers,
            local,
            remote,
        } => {
            let names: Vec<&str> = peers
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            if names.is_empty() {
                return Err(TetherError::Invalid(
                    "peers must be comma-separated ids".to_string(),
                ));
            }
            let data = std::fs::read(&local)
                .map_err(|e| TetherError::Invalid(format!("local read: {e}")))?;
            let password = load_password(cli.password_file.as_deref())?;
            let mut done = 0usize;
            for name in &names {
                let mut live = live_session(name, &password, cli.via.as_deref())?;
                let req = live.next_req();
                tethercraft_net::rpc::file_put(&mut live.sess, req, &remote, &data)?;
                live.finish()?;
                done += 1;
            }
            print_out(
                cli.json,
                format!("broadcast {local} to {done} peers"),
                json!({"peers": done, "bytes": data.len()}),
            );
            Ok(())
        }
        Cmd::CollectFiles {
            peers,
            remote_dir,
            local_dir,
        } => {
            let names: Vec<&str> = peers
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            if names.is_empty() {
                return Err(TetherError::Invalid(
                    "peers must be comma-separated ids".to_string(),
                ));
            }
            if local_dir.is_empty() {
                return Err(TetherError::Invalid("local dir required".to_string()));
            }
            let password = load_password(cli.password_file.as_deref())?;
            let mut files = 0usize;
            let mut bytes = 0u64;
            for name in &names {
                let mut live = live_session(name, &password, cli.via.as_deref())?;
                let mut req_id = 0u64;
                let tree = remote_tree(&mut live.sess, &mut req_id, &remote_dir)?;
                for f in &tree {
                    let data = tethercraft_net::rpc::file_get(
                        &mut live.sess,
                        next_req(&mut req_id),
                        &f.rel,
                    )?;
                    let rel = f
                        .rel
                        .trim_start_matches(&remote_dir.trim_end_matches('/').to_string()[..]);
                    let rel = rel.trim_start_matches('/');
                    let dest = std::path::Path::new(&local_dir).join(name).join(rel);
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| TetherError::Invalid(format!("local dir: {e}")))?;
                    }
                    bytes += data.len() as u64;
                    std::fs::write(&dest, &data)
                        .map_err(|e| TetherError::Invalid(format!("local write: {e}")))?;
                    files += 1;
                }
                live.finish()?;
            }
            print_out(
                cli.json,
                format!(
                    "collected {files} files ({bytes} bytes) from {} peers",
                    names.len()
                ),
                json!({"files": files, "bytes": bytes}),
            );
            Ok(())
        }
        Cmd::Batch { manifest } => {
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| TetherError::Invalid(format!("manifest unreadable: {e}")))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| TetherError::Invalid(format!("manifest not JSON: {e}")))?;
            let peer = value.get("peer").and_then(|v| v.as_str()).ok_or_else(|| {
                TetherError::Invalid("manifest needs a string field \"peer\"".to_string())
            })?;
            let ops = value
                .get("ops")
                .and_then(|v| v.as_array())
                .ok_or_else(|| TetherError::Invalid("manifest needs an ops array".to_string()))?;
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(peer, &password, cli.via.as_deref())?;
            let mut ran = 0usize;
            let mut results: Vec<serde_json::Value> = Vec::new();
            for op in ops {
                let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
                match name {
                    "exec" => {
                        let command = op.get("command").and_then(|v| v.as_str()).unwrap_or("");
                        if command.is_empty() {
                            return Err(TetherError::Invalid(
                                "batch exec needs a command".to_string(),
                            ));
                        }
                        let req = live.next_req();
                        let (code, out) = tethercraft_net::rpc::exec(&mut live.sess, req, command)?;
                        results.push(json!({"op": "exec", "exit": code, "output": out}));
                        ran += 1;
                    }
                    "put" => {
                        let local = op.get("local").and_then(|v| v.as_str()).unwrap_or("");
                        let remote = op.get("remote").and_then(|v| v.as_str()).unwrap_or("");
                        let data = std::fs::read(local)
                            .map_err(|e| TetherError::Invalid(format!("local read: {e}")))?;
                        let req = live.next_req();
                        tethercraft_net::rpc::file_put(&mut live.sess, req, remote, &data)?;
                        results.push(json!({"op": "put", "remote": remote, "bytes": data.len()}));
                        ran += 1;
                    }
                    "get" => {
                        let remote = op.get("remote").and_then(|v| v.as_str()).unwrap_or("");
                        let local = op.get("local").and_then(|v| v.as_str()).unwrap_or("");
                        if local.is_empty() {
                            return Err(TetherError::Invalid(
                                "batch get needs local + remote".to_string(),
                            ));
                        }
                        let req = live.next_req();
                        let data = tethercraft_net::rpc::file_get(&mut live.sess, req, remote)?;
                        if let Some(parent) = std::path::Path::new(local).parent() {
                            if !parent.as_os_str().is_empty() {
                                std::fs::create_dir_all(parent)
                                    .map_err(|e| TetherError::Invalid(format!("local dir: {e}")))?;
                            }
                        }
                        std::fs::write(local, &data)
                            .map_err(|e| TetherError::Invalid(format!("local write: {e}")))?;
                        results.push(json!({"op": "get", "remote": remote, "bytes": data.len()}));
                        ran += 1;
                    }
                    "chat" => {
                        let text = op.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        tethercraft_net::rpc::chat_send(&mut live.sess, text)?;
                        results.push(json!({"op": "chat", "sent": true}));
                        ran += 1;
                    }
                    other => {
                        return Err(TetherError::Invalid(format!(
                            "unknown batch op: {other} (want exec, put, get, chat)"
                        )));
                    }
                }
            }
            let r = live.finish();
            print_out(
                cli.json,
                format!("batch ran {ran} ops on {peer}"),
                json!({"ops": ran, "results": results}),
            );
            r
        }
        Cmd::Restart { peer } => {
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let req = live.next_req();
            tethercraft_net::rpc::power(
                &mut live.sess,
                req,
                tethercraft_proto::MessageKind::Restart,
                5,
            )?;
            let r = live.finish();
            print_out(
                cli.json,
                format!("{peer} restarting"),
                json!({"restart": true}),
            );
            r
        }
        Cmd::Shutdown { peer } => {
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let req = live.next_req();
            tethercraft_net::rpc::power(
                &mut live.sess,
                req,
                tethercraft_proto::MessageKind::Shutdown,
                5,
            )?;
            let r = live.finish();
            print_out(
                cli.json,
                format!("{peer} shutting down"),
                json!({"shutdown": true}),
            );
            r
        }
        Cmd::Lock { peer } => {
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let req = live.next_req();
            tethercraft_net::rpc::power(
                &mut live.sess,
                req,
                tethercraft_proto::MessageKind::Lock,
                0,
            )?;
            let r = live.finish();
            print_out(cli.json, format!("{peer} locked"), json!({"locked": true}));
            r
        }
        Cmd::Exec { peer, command } => {
            if command.is_empty() {
                return Err(TetherError::Invalid("exec needs a command".to_string()));
            }
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            let req = live.next_req();
            let cmdline = command.join(" ");
            let (code, output) = tethercraft_net::rpc::exec(&mut live.sess, req, &cmdline)?;
            let r = live.finish();
            if cli.json {
                println!("{}", json!({"exit": code, "output": output}));
            } else {
                println!("exit code: {code}");
                println!("{output}");
            }
            r
        }
        Cmd::Tunnel { peer, mapping } => {
            let (port, target) = parse_mapping(&mapping)?;
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            tunnel_command(&mut live, port, &target)
        }
        Cmd::Chat { peer, text } => {
            let password = load_password(cli.password_file.as_deref())?;
            let mut live = live_session(&peer, &password, cli.via.as_deref())?;
            tethercraft_net::rpc::chat_send(&mut live.sess, &text)?;
            let r = live.finish();
            print_out(cli.json, format!("sent to {peer}"), json!({"sent": true}));
            r
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
        Cmd::Gui => run_gui(peer_list(), None, None),
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
            grants,
            fps,
            fake_frames,
            codec,
            pin_key,
            identity_file,
            share,
        } => {
            if fps == 0 || fps > 60 {
                return Err(TetherError::Invalid("fps must be 1..=60".to_string()));
            }
            if codec != "raw" && codec != "h264" {
                return Err(TetherError::Invalid(
                    "codec must be raw or h264".to_string(),
                ));
            }
            if let Some(pin) = &pin_key {
                let clean = pin.trim();
                if clean.len() != 64 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(TetherError::Invalid(
                        "pin key must be 64 hex chars".to_string(),
                    ));
                }
            }
            let password = load_password(cli.password_file.as_deref())?;
            if password.is_empty() {
                return Err(TetherError::Invalid("empty password".to_string()));
            }
            let grant_list = match grants {
                Some(g) => parse_grants(&g)?,
                None => Vec::new(),
            };
            let id_path = match identity_file {
                Some(p) => std::path::PathBuf::from(p),
                None => std::path::Path::new(&store_path())
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("host_identity.key"),
            };
            let (signing, fresh) = tethercraft_host::serve::load_or_create_identity(&id_path)?;
            let pubkey = tethercraft_net::public_key_bytes(&signing);
            let id = hex::encode(pubkey);
            let words = tethercraft_net::fingerprint_words(&pubkey).join(" ");
            if fresh {
                eprintln!(
                    "new host identity generated: VERIFY these words out of band on first connect"
                );
            }
            let store = open_store()?;
            let listener = tethercraft_net::session::listen(&format!("0.0.0.0:{port}"))?;
            let cfg = tethercraft_host::serve::ServeConfig {
                port,
                password,
                grants: grant_list.clone(),
                fps,
                fake_frames,
                outgoing_only: false,
                outgoing_gate: Some(current_outgoing_only),
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                codec: codec.clone(),
                pin_key: pin_key.clone(),
                share_root: share.map(std::path::PathBuf::from),
            };
            print_out(
                cli.json,
                format!(
                    "hosting on 0.0.0.0:{port}\nid {id}\nwords: {words}\ngrants: {g:?}\nframes: {f}\ncodec: {codec}",
                    g = grant_names(&grant_list),
                    f = if fake_frames { "synthetic" } else { "live (WGC)" }
                ),
                json!({
                    "port": port,
                    "id": id,
                    "words": words,
                    "grants": grant_names(&grant_list),
                    "fake_frames": fake_frames,
                    "codec": codec,
                }),
            );
            if let Some(server) = cli.via.as_deref() {
                eprintln!("awaiting controller via relay {server} as {id}");
                loop {
                    match tethercraft_net::session::relay_await(server, &id) {
                        Ok(stream) => {
                            match tethercraft_host::serve::serve_stream(
                                stream, "relay", &cfg, &signing, &store,
                            ) {
                                Ok(report) => {
                                    print_out(
                                        cli.json,
                                        format!(
                                            "session with {p}: frames {f} ({b} bytes), inputs {i}, denied {d}, fallbacks {fb}, ended {e}",
                                            p = report.peer_id,
                                            f = report.frames_sent,
                                            b = report.bytes_sent,
                                            i = report.inputs_applied,
                                            d = report.denied_inputs,
                                            fb = report.codec_fallbacks,
                                            e = report.ended
                                        ),
                                        json!({
                                            "peer": report.peer_id,
                                            "frames": report.frames_sent,
                                            "bytes": report.bytes_sent,
                                            "inputs": report.inputs_applied,
                                            "denied": report.denied_inputs,
                                            "fallbacks": report.codec_fallbacks,
                                            "codec": report.codec,
                                            "ended": report.ended,
                                        }),
                                    );
                                }
                                Err(e) => eprintln!("session error: {e}"),
                            }
                        }
                        Err(e) => {
                            eprintln!("relay wait failed (retrying in 5s): {e}");
                            std::thread::sleep(std::time::Duration::from_secs(5));
                        }
                    }
                }
            }
            loop {
                match tethercraft_host::serve::serve_one(&listener, &cfg, &signing, &store) {
                    Ok(report) => {
                        print_out(
                            cli.json,
                            format!(
                                "session with {p}: frames {f} ({b} bytes), inputs {i}, denied {d}, fallbacks {fb}, ended {e}",
                                p = report.peer_id,
                                f = report.frames_sent,
                                b = report.bytes_sent,
                                i = report.inputs_applied,
                                d = report.denied_inputs,
                                fb = report.codec_fallbacks,
                                e = report.ended
                            ),
                            json!({
                                "peer": report.peer_id,
                                "frames": report.frames_sent,
                                "bytes": report.bytes_sent,
                                "inputs": report.inputs_applied,
                                "denied": report.denied_inputs,
                                "fallbacks": report.codec_fallbacks,
                                "codec": report.codec,
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
            view_only,
            out,
            frames,
            input_probe,
        } => {
            let (addr, expected) = peer_addr_key(&peer)?;
            let had_pin = expected.is_some();
            let password = load_password(cli.password_file.as_deref())?;
            let via_target;
            let via = match (cli.via.as_deref(), expected) {
                (Some(server), Some(key)) => {
                    via_target = hex::encode(key);
                    Some((server, via_target.as_str()))
                }
                (Some(_), None) => {
                    return Err(TetherError::Invalid(format!(
                        "relay needs a known host key for {peer}: connect directly first (or pin the key)"
                    )));
                }
                (None, _) => None,
            };
            let (saved, key) = tethercraft_net::session::run_headless_controller_count(
                &addr,
                &password,
                frames,
                out.as_deref(),
                input_probe,
                expected,
                via,
            )?;
            if !had_pin {
                tofu_record(&peer, &key)?;
                eprintln!(
                    "recorded new host key for {peer}: VERIFY the words out of band before trusting it"
                );
            }
            print_out(
                cli.json,
                format!("received {n} frames, saved {saved}", n = frames),
                json!({"frames": frames, "saved": saved, "view_only": view_only}),
            );
            Ok(())
        }
        Cmd::Service { op } => match op {
            ServiceOp::Install { config } => service::install(config.as_deref()),
            ServiceOp::Uninstall => service::uninstall(),
            ServiceOp::Start => service::start_svc(),
            ServiceOp::Stop => service::stop_svc(),
        },
        Cmd::ServiceRun { config } => {
            let path = match config {
                Some(p) => std::path::PathBuf::from(p),
                None => service::default_config_path()?,
            };
            #[cfg(windows)]
            {
                // Under SCM this blocks as the service until stop. In a
                // console the dispatch fails, so host directly instead.
                match service::run_dispatcher() {
                    Ok(()) => return Ok(()),
                    Err(e) => eprintln!("not under SCM ({e}); hosting in console instead"),
                }
            }
            // In a console Ctrl+C kills the process, same as the `host` verb.
            service::run_host_service(
                &path,
                cli.json,
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

/// Result of one `book promote` call (R3: promotion is a two-step, 24 h flow).
#[derive(Debug, PartialEq, Eq)]
enum PromoteOutcome {
    Started(chrono::DateTime<chrono::Utc>),
    Waiting { secs_left: i64 },
    Promoted,
}

/// R3 promotion through `core::trust`: the first call starts the cooling
/// period and persists its start; only a later call after the cooldown can
/// promote. A repeated request never restarts the clock. There is deliberately
/// no in-person shortcut on the CLI: a scammer can talk a victim into typing a
/// flag, so that path waits for the QR pairing flow.
fn promote_step(
    store: &tethercraft_store::Store,
    peer: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<PromoteOutcome, TetherError> {
    let row = store
        .list_peers()?
        .into_iter()
        .find(|p| p.peer_id == peer)
        .ok_or_else(|| TetherError::Invalid(format!("unknown peer {peer}")))?;
    if row.tier != "stranger" {
        return Err(TetherError::Invalid(format!(
            "{peer} is already {}; only strangers are promoted",
            row.tier
        )));
    }
    // Local operator on the CLI, no session is driven from here.
    let req = tethercraft_core::PeerPolicy::stranger(peer).request_promotion(false)?;
    match store.promotion_started(peer)? {
        None => Ok(PromoteOutcome::Started(store.begin_promotion(peer, now)?)),
        Some(started) => match req.confirm_cooling(started, now, true, false) {
            Ok(_) => {
                store.set_tier(peer, "trusted_helper")?;
                store.clear_promotion(peer)?;
                Ok(PromoteOutcome::Promoted)
            }
            Err(TetherError::Denied(msg)) if msg.starts_with("cooling period not over") => {
                let left = tethercraft_core::PROMOTION_COOLDOWN_SECS
                    - now.signed_duration_since(started).num_seconds();
                Ok(PromoteOutcome::Waiting {
                    secs_left: left.max(0),
                })
            }
            Err(e) => Err(e),
        },
    }
}
/// Live R1 state for the host accept path: true while the persisted onboarding
/// answer says someone is pressuring the user (24 h outgoing-only window).
/// Re-read on every incoming session so a new answer applies immediately.
fn current_outgoing_only() -> bool {
    read_onboarding().0
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
    fn bare_run_defaults_to_gui() {
        let cli = Cli::try_parse_from(["tethercraft"]).unwrap_or_else(|_| std::process::exit(1));
        assert!(cli.cmd.is_none());
        // The default the launcher relies on: no subcommand opens the window.
        assert!(matches!(cli.cmd.unwrap_or(Cmd::Gui), Cmd::Gui));
        let status = Cli::try_parse_from(["tethercraft", "status"])
            .unwrap_or_else(|_| std::process::exit(1));
        assert!(matches!(status.cmd, Some(Cmd::Status)));
    }

    #[test]
    fn cli_parses_new_verbs() {
        assert!(Cli::try_parse_from(["tethercraft", "files", "ls", "p", "docs"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "sync", "p", ".", "/r"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "exec", "p", "--", "whoami"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "tunnel", "p", "8080:x"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "recordings", "list"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "platform"]).is_ok());
        assert!(Cli::try_parse_from(["tethercraft", "book", "promote", "p"]).is_ok());
        assert!(Cli::try_parse_from([
            "tethercraft",
            "host",
            "--codec",
            "raw",
            "--pin-key",
            &"ab".repeat(32)
        ])
        .is_ok());
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

#[cfg(test)]
mod promote_tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[allow(clippy::panic)]
    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test step failed: {e:?}"),
        }
    }

    fn store_with(peer: &str) -> tethercraft_store::Store {
        let s = must(tethercraft_store::Store::open_in_memory());
        must(s.add_peer(peer, None, "ab", None));
        s
    }

    fn start_time() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 28, 12, 0, 0)
            .single()
            .unwrap_or_default()
    }

    #[test]
    fn promote_needs_the_24h_cooling_period() {
        let s = store_with("p");
        let t0 = start_time();
        assert!(matches!(
            promote_step(&s, "p", t0),
            Ok(PromoteOutcome::Started(_))
        ));
        // Asking again an hour later neither promotes nor restarts the clock.
        let t1 = t0 + Duration::hours(1);
        assert_eq!(
            must(promote_step(&s, "p", t1)),
            PromoteOutcome::Waiting {
                secs_left: 23 * 3600
            }
        );
        assert_eq!(must(s.list_peers())[0].tier, "stranger");
        let t2 = t0 + Duration::hours(24);
        assert_eq!(must(promote_step(&s, "p", t2)), PromoteOutcome::Promoted);
        assert_eq!(must(s.list_peers())[0].tier, "trusted_helper");
        assert!(must(s.promotion_started("p")).is_none());
    }

    #[test]
    fn promote_refuses_unknown_and_already_trusted_peers() {
        let s = store_with("p");
        let t0 = start_time();
        assert!(promote_step(&s, "ghost", t0).is_err());
        must(promote_step(&s, "p", t0));
        must(promote_step(&s, "p", t0 + Duration::hours(25)));
        assert!(promote_step(&s, "p", t0 + Duration::hours(26)).is_err());
    }
}
