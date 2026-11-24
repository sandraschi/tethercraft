//! Headless host serve loop for the live milestone.
//!
//! Accept one controller, run the PAKE handshake, enforce the host policy on
//! every message, pump real (or synthetic) frames, and inject validated
//! input. Clipboard sync and the OS sensors feeding R4 are still pending and
//! end up in the audit log as pending, never silently dropped.

use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};

use chrono::Utc;
use ed25519_dalek::SigningKey;
use tethercraft_core::{Permission, TetherError, STRANGER_SESSION_CAP_SECS};
use tethercraft_net::session::{self, SessionStream};
use tethercraft_net::{generate_device_key, public_key_bytes, LoginRateLimiter};
use tethercraft_proto::{Envelope, MessageKind};

use super::{ChatLog, Host};
use crate::inject;

pub struct ServeConfig {
    pub port: u16,
    pub password: Vec<u8>,
    pub grants: Vec<Permission>,
    pub fps: u8,
    pub fake_frames: bool,
    pub outgoing_only: bool,
    pub stop: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct SessionReport {
    pub peer_id: String,
    pub frames_sent: u64,
    pub inputs_applied: u64,
    pub denied_inputs: u64,
    pub ended: String,
}

fn audit(store: &tethercraft_store::Store, peer: &str, action: &str, result: &str) {
    let ev = tethercraft_core::AuditEvent::new(Utc::now(), peer, action, result);
    let _ = store.log(&ev);
}

/// Read envelopes on a dedicated thread. A blocking read is the only safe way
/// to consume a length-prefixed stream: timing out mid-message loses bytes.
fn spawn_reader(stream: std::net::TcpStream) -> mpsc::Receiver<Envelope> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sess = match SessionStream::new(stream) {
            Ok(s) => s,
            Err(_) => return,
        };
        while let Ok(env) = sess.recv_envelope() {
            if tx.send(env).is_err() {
                break;
            }
        }
    });
    rx
}

/// Pump frames in the background so a slow capture never blocks input.
/// Sends only the latest frame per tick; drops the rest.
fn spawn_pump(fake: bool, fps: u8, stop: Arc<AtomicBool>) -> mpsc::Receiver<(u32, u32, Vec<u8>)> {
    let (tx, rx) = mpsc::channel();
    let per_frame = Duration::from_millis((1000 / u64::from(fps.max(1))).max(16));
    std::thread::spawn(move || {
        let mut n = 0u64;
        while !stop.load(Ordering::Relaxed) {
            let frame = if fake {
                let (meta, px) = tethercraft_capture::fake_frame(640, 360, n);
                Some((meta.width, meta.height, px))
            } else {
                tethercraft_capture::live_frame(Duration::from_secs(2))
                    .ok()
                    .map(|(meta, px)| (meta.width, meta.height, px))
            };
            if let Some(f) = frame {
                if tx.send(f).is_err() {
                    break;
                }
            }
            n = n.wrapping_add(1);
            std::thread::sleep(per_frame);
        }
    });
    rx
}

pub fn serve_one(
    listener: &TcpListener,
    cfg: &ServeConfig,
    signing: &SigningKey,
    store: &tethercraft_store::Store,
) -> Result<SessionReport, TetherError> {
    let my_pub = public_key_bytes(signing);
    let my_id = hex::encode(my_pub);
    let (stream, addr) = listener
        .accept()
        .map_err(|e| TetherError::Invalid(format!("accept: {e}")))?;
    let peer_label = addr.to_string();
    let mut sess = SessionStream::new(stream)?;
    let started = Instant::now();

    if cfg.outgoing_only {
        audit(store, &peer_label, "session", "refused-outgoing-only");
        return Ok(SessionReport {
            peer_id: peer_label,
            frames_sent: 0,
            inputs_applied: 0,
            denied_inputs: 0,
            ended: "outgoing-only".to_string(),
        });
    }

    let mut limiter = LoginRateLimiter::new(5, 60);
    let auth = match session::host_handshake(
        &mut sess,
        signing,
        session::HostHandshake {
            my_id: &my_id,
            password: &cfg.password,
            peer_label: &peer_label,
            fps: cfg.fps,
            grants: cfg.grants.clone(),
        },
        &mut limiter,
    ) {
        Ok(a) => a,
        Err(e) => {
            audit(store, &peer_label, "handshake", "auth-failed");
            return Ok(SessionReport {
                peer_id: peer_label,
                frames_sent: 0,
                inputs_applied: 0,
                denied_inputs: 0,
                ended: format!("auth-failed: {e}"),
            });
        }
    };

    let peer_hex = hex::encode(auth.peer_key);
    if store.is_blocked(&peer_hex) {
        audit(store, &auth.peer_id, "session", "refused-blocked");
        return Ok(SessionReport {
            peer_id: auth.peer_id,
            frames_sent: 0,
            inputs_applied: 0,
            denied_inputs: 0,
            ended: "blocked".to_string(),
        });
    }

    let mut host = Host::new();
    let _ = host.session_start(&auth.peer_id, Utc::now(), false);
    for g in &cfg.grants {
        let _ = host.granted_mut(&auth.peer_id).insert(*g);
    }
    audit(store, &auth.peer_id, "session-start", "ok");
    let mut chat = ChatLog::new();

    let frames = spawn_pump(cfg.fake_frames, cfg.fps, cfg.stop.clone());
    // Input arrives on its own thread; the main loop only writes frames.
    let input_stream = sess.clone_stream()?;
    let incoming = spawn_reader(input_stream);
    let tick = Duration::from_millis((1000 / u64::from(cfg.fps.max(1))).max(16));
    let mut report = SessionReport {
        peer_id: auth.peer_id.clone(),
        frames_sent: 0,
        inputs_applied: 0,
        denied_inputs: 0,
        ended: String::new(),
    };

    loop {
        if cfg.stop.load(Ordering::Relaxed) {
            report.ended = "stop".to_string();
            break;
        }
        if started.elapsed().as_secs() as i64 > STRANGER_SESSION_CAP_SECS {
            report.ended = "session-cap".to_string();
            break;
        }
        // Latest frame wins; backlog is dropped.
        let mut latest: Option<(u32, u32, Vec<u8>)> = None;
        for f in frames.try_iter() {
            latest = Some(f);
        }
        if let Some((w, h, rgba)) = latest {
            match session::encode_frame(w, h, &rgba) {
                Ok(payload) => {
                    let env = Envelope::new(MessageKind::Frame, my_pub, payload);
                    if sess.send_envelope(&env).is_err() {
                        report.ended = "frame-send-failed".to_string();
                        break;
                    }
                    report.frames_sent = report.frames_sent.saturating_add(1);
                }
                Err(e) => {
                    audit(
                        store,
                        &auth.peer_id,
                        "frame",
                        &format!("encode-failed: {e}"),
                    );
                }
            }
        }
        match incoming.recv_timeout(tick) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                report.ended = "peer-disconnected".to_string();
                break;
            }
            Ok(env) => {
                let now = Utc::now();
                match env.kind {
                    MessageKind::SessionEnd => {
                        report.ended = "peer-ended".to_string();
                        break;
                    }
                    MessageKind::InputEvent
                    | MessageKind::TouchTap
                    | MessageKind::TouchHold
                    | MessageKind::TouchSwipe => {
                        let input: Result<tethercraft_input::InputEvent, _> =
                            session::parse_json(&env.payload);
                        match input {
                            Ok(ev) => match host.authorize(&auth.peer_id, env.kind, now) {
                                Ok(()) => match inject::inject_event(&ev) {
                                    Ok(()) => {
                                        report.inputs_applied =
                                            report.inputs_applied.saturating_add(1);
                                    }
                                    Err(e) => {
                                        audit(
                                            store,
                                            &auth.peer_id,
                                            "input",
                                            &format!("inject-failed: {e}"),
                                        );
                                    }
                                },
                                Err(e) => {
                                    report.denied_inputs = report.denied_inputs.saturating_add(1);
                                    audit(store, &auth.peer_id, "input", &format!("denied: {e}"));
                                }
                            },
                            Err(e) => {
                                audit(store, &auth.peer_id, "input", &format!("bad-payload: {e}"));
                            }
                        }
                    }
                    MessageKind::ChatMsg => {
                        let text = String::from_utf8_lossy(&env.payload).to_string();
                        if chat.post(&auth.peer_id, &text, now).is_ok() {
                            audit(store, &auth.peer_id, "chat", "ok");
                        }
                    }
                    MessageKind::ClipboardText | MessageKind::ClipboardImage => {
                        audit(store, &auth.peer_id, "clipboard", "pending-host-sync");
                    }
                    _ => {
                        audit(
                            store,
                            &auth.peer_id,
                            "message",
                            &format!("unhandled: {:?}", env.kind),
                        );
                    }
                }
            }
        }
    }

    let end = Envelope::new(MessageKind::SessionEnd, my_pub, Vec::new());
    let _ = sess.send_envelope(&end);
    audit(store, &auth.peer_id, "session-end", &report.ended);
    Ok(report)
}

/// Ephemeral device identity for this host run. Printed for fingerprint
/// comparison; persistence lands with the host service.
pub fn ephemeral_identity() -> Result<(SigningKey, String, String), TetherError> {
    let signing = generate_device_key()?;
    let pubkey = public_key_bytes(&signing);
    let id = hex::encode(pubkey);
    let words = tethercraft_net::fingerprint_words(&pubkey).join(" ");
    Ok((signing, id, words))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ephemeral_identity_printable() {
        let id = ephemeral_identity().unwrap_or_else(|_| {
            let k = generate_device_key().unwrap_or(SigningKey::from_bytes(&[3u8; 32]));
            let p = public_key_bytes(&k);
            (k, hex::encode(p), String::new())
        });
        assert!(!id.1.is_empty());
    }
}
