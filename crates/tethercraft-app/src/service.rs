//! Windows host service (SCM) plus the config-file host loop.
//!
//! Install, start, stop and uninstall need an elevated prompt; SCM itself is
//! the audit trail for those. `service-run` is hidden plumbing: it only reads
//! the JSON config at `%PROGRAMDATA%\TetherCraft\host.json` (or `--config`)
//! and hosts sessions exactly like `tethercraft host`. There is no
//! silent-install path: install is an explicit admin command and the default
//! config grants nothing until the admin edits it.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tethercraft_core::{Permission, TetherError};

#[cfg(windows)]
pub const SERVICE_NAME: &str = "TetherCraftHost";
#[cfg(windows)]
pub const SERVICE_DISPLAY: &str = "TetherCraft Host";

/// On-disk config. Only `password_file` is required; everything else falls
/// back to the same defaults as the `host` CLI verb.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileConfig {
    pub port: Option<u16>,
    pub grants: Option<String>,
    pub fps: Option<u8>,
    pub fake_frames: Option<bool>,
    pub codec: Option<String>,
    pub pin_key: Option<String>,
    pub identity_file: Option<String>,
    pub share: Option<String>,
    pub password_file: String,
    pub via: Option<String>,
}

pub struct ResolvedHost {
    pub port: u16,
    pub grants: Vec<Permission>,
    pub fps: u8,
    pub fake_frames: bool,
    pub codec: String,
    pub pin_key: Option<String>,
    pub identity_file: PathBuf,
    pub share: Option<PathBuf>,
    pub password: Vec<u8>,
    pub via: Option<String>,
}

pub fn default_config_path() -> Result<PathBuf, TetherError> {
    match std::env::var("PROGRAMDATA") {
        Ok(base) => Ok(PathBuf::from(base).join("TetherCraft").join("host.json")),
        Err(_) => Err(TetherError::Invalid(
            "PROGRAMDATA is not set; pass --config explicitly".to_string(),
        )),
    }
}

/// Parse `keyboard,touch,...` into host permissions. Shared with the CLI.
pub fn parse_grants(spec: &str) -> Result<Vec<Permission>, TetherError> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let p = match part.trim() {
            "" => continue,
            "keyboard" | "input" => Permission::Keyboard,
            "touch" => Permission::Touch,
            "clipboard" => Permission::Clipboard,
            "file" | "files" => Permission::File,
            "audio" => Permission::Audio,
            "terminal" => Permission::Terminal,
            "privacy" => Permission::Privacy,
            "camera" => Permission::Camera,
            "tunnel" => Permission::Tunnel,
            "restart" => Permission::Restart,
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

fn grant_names(grants: &[Permission]) -> Vec<String> {
    grants
        .iter()
        .map(|g| format!("{g:?}").to_lowercase())
        .collect()
}

fn store_path() -> String {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{base}/tethercraft/store.sqlite3")
}

fn load_password_file(path: &str) -> Result<Vec<u8>, TetherError> {
    if path.trim().is_empty() {
        return Err(TetherError::Invalid(
            "config password_file is empty: point it at a password file".to_string(),
        ));
    }
    let s = std::fs::read_to_string(path)
        .map_err(|e| TetherError::Invalid(format!("password file: {e}")))?;
    Ok(s.trim_end_matches(['\r', '\n']).as_bytes().to_vec())
}

pub fn load_file_config(path: &Path) -> Result<FileConfig, TetherError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| TetherError::Invalid(format!("config {}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| TetherError::Invalid(format!("config {}: {e}", path.display())))
}

pub fn resolve_from_file(path: &Path) -> Result<ResolvedHost, TetherError> {
    let cfg = load_file_config(path)?;
    let fps = cfg.fps.unwrap_or(10);
    if fps == 0 || fps > 60 {
        return Err(TetherError::Invalid("fps must be 1..=60".to_string()));
    }
    let codec = cfg.codec.unwrap_or_else(|| "h264".to_string());
    if codec != "raw" && codec != "h264" {
        return Err(TetherError::Invalid(
            "codec must be raw or h264".to_string(),
        ));
    }
    if let Some(pin) = &cfg.pin_key {
        let clean = pin.trim();
        if clean.len() != 64 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TetherError::Invalid(
                "pin key must be 64 hex chars".to_string(),
            ));
        }
    }
    let password = load_password_file(&cfg.password_file)?;
    if password.is_empty() {
        return Err(TetherError::Invalid("empty password".to_string()));
    }
    let grants = match cfg.grants {
        Some(g) => parse_grants(&g)?,
        None => Vec::new(),
    };
    let identity_file = match cfg.identity_file {
        Some(p) => PathBuf::from(p),
        None => Path::new(&store_path())
            .parent()
            .unwrap_or(Path::new("."))
            .join("host_identity.key"),
    };
    Ok(ResolvedHost {
        port: cfg.port.unwrap_or(21120),
        grants,
        fps,
        fake_frames: cfg.fake_frames.unwrap_or(false),
        codec,
        pin_key: cfg.pin_key,
        identity_file,
        share: cfg.share.map(PathBuf::from),
        password,
        via: cfg.via,
    })
}

fn report(json: bool, report: &tethercraft_host::serve::SessionReport) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "peer": report.peer_id,
                "frames": report.frames_sent,
                "bytes": report.bytes_sent,
                "inputs": report.inputs_applied,
                "denied": report.denied_inputs,
                "fallbacks": report.codec_fallbacks,
                "codec": report.codec,
                "ended": report.ended,
            })
        );
    } else {
        println!(
            "session with {p}: frames {f} ({b} bytes), inputs {i}, denied {d}, fallbacks {fb}, ended {e}",
            p = report.peer_id,
            f = report.frames_sent,
            b = report.bytes_sent,
            i = report.inputs_applied,
            d = report.denied_inputs,
            fb = report.codec_fallbacks,
            e = report.ended
        );
    }
}

/// Host sessions from a config file until `stop` is set. Direct listen by
/// default; when `via` holds a rendezvous address, await relay pairing
/// instead (same as `host` with a relay). `stop` is checked between
/// sessions; a blocked accept is woken by the stopper with a dummy connect.
pub fn run_host_service(
    config_path: &Path,
    json: bool,
    stop: Arc<AtomicBool>,
) -> Result<(), TetherError> {
    let params = resolve_from_file(config_path)?;
    let (signing, fresh) = tethercraft_host::serve::load_or_create_identity(&params.identity_file)?;
    let pubkey = tethercraft_net::public_key_bytes(&signing);
    let id = hex::encode(pubkey);
    let words = tethercraft_net::fingerprint_words(&pubkey).join(" ");
    if fresh {
        eprintln!("new host identity generated: VERIFY these words out of band on first connect");
    }
    let store = tethercraft_store::Store::open_file(&store_path())
        .map_err(|e| TetherError::Invalid(format!("store: {e}")))?;
    let cfg = tethercraft_host::serve::ServeConfig {
        port: params.port,
        password: params.password,
        grants: params.grants.clone(),
        fps: params.fps,
        fake_frames: params.fake_frames,
        outgoing_only: false,
        stop: stop.clone(),
        codec: params.codec.clone(),
        pin_key: params.pin_key.clone(),
        share_root: params.share.clone(),
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "port": params.port,
                "id": id,
                "words": words,
                "grants": grant_names(&params.grants),
                "fake_frames": params.fake_frames,
                "codec": params.codec,
            })
        );
    } else if let Some(server) = params.via.as_deref() {
        println!("hosting via relay {server} as {id}");
        println!("id {id}");
        println!("words: {words}");
    } else {
        println!("hosting on 0.0.0.0:{port}", port = params.port);
        println!("id {id}");
        println!("words: {words}");
    }
    if let Some(server) = params.via.as_deref() {
        while !stop.load(Ordering::Relaxed) {
            match tethercraft_net::session::relay_await_cancel(server, &id, &stop) {
                Ok(stream) => {
                    match tethercraft_host::serve::serve_stream(
                        stream, "relay", &cfg, &signing, &store,
                    ) {
                        Ok(r) => report(json, &r),
                        Err(e) => eprintln!("session error: {e}"),
                    }
                }
                Err(e) => {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    eprintln!("relay wait failed (retrying in 5s): {e}");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }
        }
        return Ok(());
    }
    let listener =
        tethercraft_net::session::listen(&format!("0.0.0.0:{port}", port = params.port))?;
    while !stop.load(Ordering::Relaxed) {
        match tethercraft_host::serve::serve_one(&listener, &cfg, &signing, &store) {
            Ok(r) => report(json, &r),
            Err(e) => eprintln!("session error: {e}"),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn template_config() -> String {
    let example = FileConfig {
        port: Some(21120),
        grants: Some(String::new()),
        fps: Some(10),
        fake_frames: Some(false),
        codec: Some("h264".to_string()),
        pin_key: None,
        identity_file: None,
        share: None,
        password_file: String::new(),
        via: None,
    };
    serde_json::to_string_pretty(&example).unwrap_or_else(|_| String::from("{}"))
}

#[cfg(windows)]
mod imp {
    use super::{
        default_config_path, resolve_from_file, run_host_service, SERVICE_DISPLAY, SERVICE_NAME,
    };
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use tethercraft_core::TetherError;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl,
            ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    fn svc<T>(r: windows_service::Result<T>, what: &str) -> Result<T, TetherError> {
        r.map_err(|e| TetherError::Invalid(format!("service {what}: {e}")))
    }

    fn exe() -> Result<PathBuf, TetherError> {
        std::env::current_exe().map_err(|e| TetherError::Invalid(format!("current exe: {e}")))
    }

    pub fn install(config: Option<&str>) -> Result<(), TetherError> {
        let path = match config {
            Some(p) => PathBuf::from(p),
            None => default_config_path()?,
        };
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| TetherError::Invalid(format!("config dir: {e}")))?;
            }
            std::fs::write(&path, super::template_config())
                .map_err(|e| TetherError::Invalid(format!("write template: {e}")))?;
            println!("wrote template config to {}", path.display());
            println!("edit password_file (and grants) in that file, then re-run service install");
        }
        resolve_from_file(&path)?;
        let manager = svc(
            ServiceManager::local_computer(
                None::<&str>,
                ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
            ),
            "open manager",
        )?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::OnDemand,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe()?,
            launch_arguments: vec![
                OsString::from("service-run"),
                OsString::from("--config"),
                OsString::from(path.as_os_str()),
            ],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let service = svc(
            manager.create_service(&info, ServiceAccess::CHANGE_CONFIG),
            "create",
        )?;
        svc(
            service.set_description("TetherCraft remote host (explicit install only)"),
            "describe",
        )?;
        println!(
            "installed {SERVICE_NAME} (on demand; config: {})",
            path.display()
        );
        println!("start it with: tethercraft service start (needs elevation)");
        Ok(())
    }

    pub fn uninstall() -> Result<(), TetherError> {
        let manager = svc(
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT),
            "open manager",
        )?;
        let service = svc(
            manager.open_service(
                SERVICE_NAME,
                ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
            ),
            "open",
        )?;
        let _ = service.stop();
        svc(service.delete(), "delete")?;
        println!("uninstalled {SERVICE_NAME}");
        Ok(())
    }

    pub fn start_svc() -> Result<(), TetherError> {
        let manager = svc(
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT),
            "open manager",
        )?;
        let service = svc(
            manager.open_service(SERVICE_NAME, ServiceAccess::START),
            "open",
        )?;
        svc(service.start::<&str>(&[]), "start")?;
        println!("started {SERVICE_NAME}");
        Ok(())
    }

    pub fn stop_svc() -> Result<(), TetherError> {
        let manager = svc(
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT),
            "open manager",
        )?;
        let service = svc(
            manager.open_service(SERVICE_NAME, ServiceAccess::STOP),
            "open",
        )?;
        svc(service.stop(), "stop")?;
        println!("stopped {SERVICE_NAME}");
        Ok(())
    }

    pub fn run_dispatcher() -> Result<(), TetherError> {
        svc(
            service_dispatcher::start(SERVICE_NAME, ffi_service_main),
            "dispatch",
        )
    }

    define_windows_service!(ffi_service_main, service_main);

    fn service_main(_args: Vec<OsString>) {
        // Our launch arguments live on the process command line
        // (ImagePath), not in the service args: parse --config there.
        let mut config = default_config_path().unwrap_or_else(|_| PathBuf::from("host.json"));
        let mut it = std::env::args_os();
        while let Some(a) = it.next() {
            if a == "--config" {
                if let Some(v) = it.next() {
                    config = PathBuf::from(v);
                }
            }
        }
        if let Err(e) = run_service_with(config) {
            eprintln!("service error: {e}");
        }
    }

    fn run_service_with(config: PathBuf) -> windows_service::Result<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_handler = stop.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let event_handler = move |control| -> ServiceControlHandlerResult {
            match control {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    stop_handler.store(true, Ordering::Relaxed);
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        })?;
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            let _ = run_host_service(&config, true, worker_stop);
        });
        let _ = shutdown_rx.recv();
        // Wake a blocked direct-mode accept so the worker sees the flag.
        // Relay mode aborts on the flag itself (relay_await_cancel).
        unblock_accept();
        let _ = worker.join();
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        })?;
        Ok(())
    }

    /// Best-effort wake for a worker blocked in direct-mode accept. The
    /// service config port is re-read; failures just mean the worker is in
    /// relay mode or already gone.
    fn unblock_accept() {
        let port = default_config_path()
            .ok()
            .and_then(|p| super::load_file_config(&p).ok())
            .and_then(|c| c.port)
            .unwrap_or(21120);
        let _ = std::net::TcpStream::connect(format!("127.0.0.1:{port}"));
    }
}

#[cfg(not(windows))]
mod imp {
    use tethercraft_core::TetherError;

    pub fn install(_config: Option<&str>) -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "windows service install needs Windows".to_string(),
        ))
    }

    pub fn uninstall() -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "windows service uninstall needs Windows".to_string(),
        ))
    }

    pub fn start_svc() -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "windows service start needs Windows".to_string(),
        ))
    }

    pub fn stop_svc() -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "windows service stop needs Windows".to_string(),
        ))
    }
}

#[cfg(windows)]
pub use imp::run_dispatcher;
pub use imp::{install, start_svc, stop_svc, uninstall};
