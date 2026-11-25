//! Live TCP session transport (Tier 1 exit milestone).
//!
//! Length-prefixed native envelopes over TCP: pairing with fingerprint
//! words, SPAKE2 handshake with key confirmation, then frames one way and
//! input events the other. QUIC is the later upgrade; the framing and the
//! handshake move over unchanged. First-use trust comes from comparing the
//! fingerprint words shown on both ends.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{fingerprint_words, parse_public_key, public_key_bytes, LoginRateLimiter};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tethercraft_core::TetherError;
use tethercraft_proto::{Envelope, MessageKind};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

/// Default host port. Outside the RustDesk 21115-21119 range on purpose.
pub const DEFAULT_SESSION_PORT: u16 = 21120;

pub struct SessionStream {
    stream: TcpStream,
    pub peer_key: Option<[u8; 32]>,
}

impl SessionStream {
    pub fn new(stream: TcpStream) -> Result<Self, TetherError> {
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        Ok(Self {
            stream,
            peer_key: None,
        })
    }

    pub fn set_frame_timeouts(&mut self) -> Result<(), TetherError> {
        self.stream
            .set_read_timeout(Some(FRAME_TIMEOUT))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        Ok(())
    }

    /// A second handle to the same socket (one half reads, the other writes).
    pub fn clone_stream(&self) -> Result<TcpStream, TetherError> {
        self.stream
            .try_clone()
            .map_err(|e| TetherError::Invalid(format!("clone: {e}")))
    }

    pub fn send_envelope(&mut self, env: &Envelope) -> Result<(), TetherError> {
        let bytes = env.encode()?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid("message too large".to_string()));
        }
        let len = bytes.len() as u32;
        self.stream
            .write_all(&len.to_be_bytes())
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        self.stream
            .write_all(&bytes)
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        self.stream
            .flush()
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        Ok(())
    }

    pub fn recv_envelope(&mut self) -> Result<Envelope, TetherError> {
        let mut prefix = [0u8; 4];
        self.stream
            .read_exact(&mut prefix)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        let len = u32::from_be_bytes(prefix) as usize;
        if len == 0 || len > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid(format!(
                "bad frame length {len} (prefix {prefix:?})"
            )));
        }
        let mut buf = vec![0u8; len];
        self.stream
            .read_exact(&mut buf)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        Envelope::decode(&buf)
    }

    /// Receive with a bounded wait. `Ok(None)` means the wait expired with no
    /// complete message.
    ///
    /// WARNING: does not fix up a partially read length prefix, so a timeout
    /// mid-message desyncs the stream. Do not use this on a session socket;
    /// read on a dedicated thread instead (see `serve_one`).
    pub fn recv_envelope_wait(&mut self, wait: Duration) -> Result<Option<Envelope>, TetherError> {
        self.stream
            .set_read_timeout(Some(wait))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        let mut prefix = [0u8; 4];
        match self.stream.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                return Ok(None)
            }
            Err(e) => return Err(TetherError::Invalid(format!("recv: {e}"))),
        }
        let len = u32::from_be_bytes(prefix) as usize;
        if len == 0 || len > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid(format!(
                "bad frame length {len} (prefix {prefix:?})"
            )));
        }
        let mut buf = vec![0u8; len];
        self.stream
            .read_exact(&mut buf)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        Ok(Some(Envelope::decode(&buf)?))
    }
}

pub fn listen(addr: &str) -> Result<TcpListener, TetherError> {
    TcpListener::bind(addr).map_err(|e| TetherError::Invalid(format!("bind: {e}")))
}

pub fn dial(addr: &str, timeout: Duration) -> Result<TcpStream, TetherError> {
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|e| TetherError::Invalid(format!("resolve: {e}")))?;
    let mut last = "no address".to_string();
    for a in &mut addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e.to_string(),
        }
    }
    Err(TetherError::Invalid(format!("dial failed: {last}")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    pub id: String,
    pub key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomePayload {
    pub id: String,
    pub key_hex: String,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakePayload {
    pub msg_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofPayload {
    pub confirm_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPayload {
    pub grants: Vec<tethercraft_core::Permission>,
    pub fps: u8,
}

pub fn json_bytes<T: Serialize>(v: &T) -> Result<Vec<u8>, TetherError> {
    serde_json::to_vec(v).map_err(|e| TetherError::Invalid(format!("encode: {e}")))
}

pub fn parse_json<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, TetherError> {
    serde_json::from_slice(bytes).map_err(|e| TetherError::Invalid(format!("decode: {e}")))
}

fn confirm_of(key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"tethercraft-session-confirm");
    h.update(key);
    hex::encode(&h.finalize()[..16])
}

fn expect_kind(env: &Envelope, want: MessageKind, who: &str) -> Result<(), TetherError> {
    if env.kind == want {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!(
            "{who}: expected {want:?}, got {:?}",
            env.kind
        )))
    }
}

/// After the PAKE messages a close means the host rejected us (bad password or
/// policy), not a broken network. Say so instead of leaking an IO error.
fn recv_or_refused(stream: &mut SessionStream) -> Result<Envelope, TetherError> {
    stream.recv_envelope().map_err(|_| {
        TetherError::Denied(
            "host closed the session: wrong password, or the host refused".to_string(),
        )
    })
}

pub struct ControllerAuth {
    pub host_id: String,
    pub host_words: Vec<String>,
    pub grants: Vec<tethercraft_core::Permission>,
    pub fps: u8,
}

pub fn controller_handshake(
    stream: &mut SessionStream,
    my_id: &str,
    signing: &SigningKey,
    password: &[u8],
) -> Result<ControllerAuth, TetherError> {
    if my_id.is_empty() {
        return Err(TetherError::Invalid("device id required".to_string()));
    }
    if password.is_empty() {
        return Err(TetherError::Invalid("password required".to_string()));
    }
    let my_pub = public_key_bytes(signing);
    // 1. Pairing hello.
    let hello = HelloPayload {
        id: my_id.to_string(),
        key_hex: hex::encode(my_pub),
    };
    stream.send_envelope(&Envelope::new(
        MessageKind::PairingHello,
        my_pub,
        json_bytes(&hello)?,
    ))?;
    // 2. Host welcome with fingerprint words. The human compares these.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PairingWelcome, "controller")?;
    let welcome: WelcomePayload = parse_json(&back.payload)?;
    if welcome.id.is_empty() || welcome.words.is_empty() {
        return Err(TetherError::Invalid("bad welcome".to_string()));
    }
    let host_key = parse_public_key(&hex::decode(&welcome.key_hex).unwrap_or_default())?;
    stream.peer_key = Some(host_key.to_bytes());
    // 3. PAKE: we are side A.
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let pw = Password::new(password);
    let id_a = Identity::new(b"controller");
    let id_b = Identity::new(b"host");
    let (state_a, msg_a) = Spake2::<Ed25519Group>::start_a(&pw, &id_a, &id_b);
    stream.send_envelope(&Envelope::new(
        MessageKind::PakeA,
        my_pub,
        json_bytes(&PakePayload {
            msg_hex: hex::encode(&msg_a),
        })?,
    ))?;
    // 4. Host message, finish, confirm our key.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PakeB, "controller")?;
    let pake_b: PakePayload = parse_json(&back.payload)?;
    let msg_b = hex::decode(&pake_b.msg_hex)
        .map_err(|_| TetherError::Invalid("bad pake message".to_string()))?;
    let key_a = state_a
        .finish(&msg_b)
        .map_err(|e| TetherError::Invalid(format!("pake finish: {e:?}")))?;
    let confirm = confirm_of(&key_a);
    stream.send_envelope(&Envelope::new(
        MessageKind::OtpProof,
        my_pub,
        json_bytes(&ProofPayload {
            confirm_hex: confirm.clone(),
        })?,
    ))?;
    // 5. Host proof must match; then the session terms arrive.
    let back = recv_or_refused(stream)?;
    expect_kind(&back, MessageKind::OtpProof, "controller")?;
    let proof: ProofPayload = parse_json(&back.payload)?;
    if proof.confirm_hex != confirm {
        return Err(TetherError::Denied(
            "session key mismatch (wrong password?)".to_string(),
        ));
    }
    let back = recv_or_refused(stream)?;
    expect_kind(&back, MessageKind::SessionStart, "controller")?;
    let start: StartPayload = parse_json(&back.payload)?;
    Ok(ControllerAuth {
        host_id: welcome.id,
        host_words: welcome.words,
        grants: start.grants,
        fps: start.fps,
    })
}

pub struct HostAuth {
    pub peer_id: String,
    pub peer_key: [u8; 32],
}

/// Everything the host needs to run a handshake except the socket, key and limiter.
pub struct HostHandshake<'a> {
    pub my_id: &'a str,
    pub password: &'a [u8],
    pub peer_label: &'a str,
    pub fps: u8,
    pub grants: Vec<tethercraft_core::Permission>,
}

pub fn host_handshake(
    stream: &mut SessionStream,
    signing: &SigningKey,
    hs: HostHandshake<'_>,
    limiter: &mut LoginRateLimiter,
) -> Result<HostAuth, TetherError> {
    let HostHandshake {
        my_id,
        password,
        peer_label,
        fps,
        grants,
    } = hs;
    if my_id.is_empty() {
        return Err(TetherError::Invalid("device id required".to_string()));
    }
    let my_pub = public_key_bytes(signing);
    // 1. Controller hello.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PairingHello, "host")?;
    let hello: HelloPayload = parse_json(&back.payload)?;
    if hello.id.is_empty() {
        return Err(TetherError::Invalid("bad hello".to_string()));
    }
    let peer_key = parse_public_key(&hex::decode(&hello.key_hex).unwrap_or_default())?;
    stream.peer_key = Some(peer_key.to_bytes());
    // 2. Welcome with our fingerprint words (shown locally too).
    let words = fingerprint_words(&my_pub);
    let welcome = WelcomePayload {
        id: my_id.to_string(),
        key_hex: hex::encode(my_pub),
        words,
    };
    stream.send_envelope(&Envelope::new(
        MessageKind::PairingWelcome,
        my_pub,
        json_bytes(&welcome)?,
    ))?;
    // 3. PAKE side B.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PakeA, "host")?;
    let pake_a: PakePayload = parse_json(&back.payload)?;
    let msg_a = hex::decode(&pake_a.msg_hex)
        .map_err(|_| TetherError::Invalid("bad pake message".to_string()))?;
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let pw = Password::new(password);
    let id_a = Identity::new(b"controller");
    let id_b = Identity::new(b"host");
    let (state_b, msg_b) = Spake2::<Ed25519Group>::start_b(&pw, &id_a, &id_b);
    stream.send_envelope(&Envelope::new(
        MessageKind::PakeB,
        my_pub,
        json_bytes(&PakePayload {
            msg_hex: hex::encode(&msg_b),
        })?,
    ))?;
    // 4. Controller proof must match our key.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::OtpProof, "host")?;
    let proof: ProofPayload = parse_json(&back.payload)?;
    let key_b = state_b
        .finish(&msg_a)
        .map_err(|_| TetherError::Denied("password proof failed".to_string()))?;
    if proof.confirm_hex != confirm_of(&key_b) {
        limiter.record_failure(peer_label);
        return Err(TetherError::Denied("password proof failed".to_string()));
    }
    limiter.record_success(peer_label);
    // 5. Confirm and send session terms.
    stream.send_envelope(&Envelope::new(
        MessageKind::OtpProof,
        my_pub,
        json_bytes(&ProofPayload {
            confirm_hex: confirm_of(&key_b),
        })?,
    ))?;
    stream.send_envelope(&Envelope::new(
        MessageKind::SessionStart,
        my_pub,
        json_bytes(&StartPayload { grants, fps })?,
    ))?;
    Ok(HostAuth {
        peer_id: hello.id,
        peer_key: peer_key.to_bytes(),
    })
}

/// Raw RGBA frame wire format: width u32 LE, height u32 LE, then bytes.
pub fn encode_frame(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, TetherError> {
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(rgba);
    Ok(out)
}

pub fn decode_frame(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), TetherError> {
    if bytes.len() < 8 {
        return Err(TetherError::Invalid("frame too short".to_string()));
    }
    let mut w = [0u8; 4];
    let mut h = [0u8; 4];
    w.copy_from_slice(&bytes[..4]);
    h.copy_from_slice(&bytes[4..8]);
    let (width, height) = (u32::from_le_bytes(w), u32::from_le_bytes(h));
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    let need = width as usize * height as usize * 4;
    if bytes.len() - 8 != need {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    Ok((width, height, bytes[8..].to_vec()))
}

/// Minimal binary PPM writer: no encoder dependency, viewable everywhere.
pub fn write_ppm(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), TetherError> {
    use std::io::Write;
    let need = width as usize * height as usize * 4;
    if rgba.len() != need {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    let mut out = Vec::with_capacity(32 + need / 4 * 3);
    let header = format!("P6\n{width} {height}\n255\n");
    out.extend_from_slice(header.as_bytes());
    let mut i = 0;
    while i + 4 <= rgba.len() {
        out.push(rgba[i]);
        out.push(rgba[i + 1]);
        out.push(rgba[i + 2]);
        i += 4;
    }
    let mut f =
        std::fs::File::create(path).map_err(|e| TetherError::Invalid(format!("save: {e}")))?;
    f.write_all(&out)
        .map_err(|e| TetherError::Invalid(format!("save: {e}")))?;
    Ok(())
}

/// Connect, handshake, then read frames for a bounded window and save the
/// first one. Used for scripted milestone checks and CI-style runs.
pub fn run_headless_controller(
    addr: &str,
    password: &[u8],
    out_path: &str,
) -> Result<u32, TetherError> {
    run_headless_controller_count(addr, password, 1, Some(out_path), false)
}

pub fn run_headless_controller_count(
    addr: &str,
    password: &[u8],
    want_frames: u32,
    out_path: Option<&str>,
    input_probe: bool,
) -> Result<u32, TetherError> {
    let signing = tethercraft_net_device_key()?;
    let stream = dial(addr, Duration::from_secs(10))?;
    let mut sess = SessionStream::new(stream)?;
    let auth = controller_handshake(&mut sess, "headless-controller", &signing, password)?;
    sess.set_frame_timeouts()?;
    if input_probe {
        // Benign zero-delta scroll: exercises the whole input path (parse,
        // host permission check, injection call) without moving the pointer.
        let payload = json_bytes(&tethercraft_input::InputEvent::MouseScroll { dx: 0, dy: 0 })?;
        sess.send_envelope(&Envelope::new(MessageKind::InputEvent, [0u8; 32], payload))?;
    }
    let mut got = 0u32;
    let mut saved_first = false;
    let mut decoder = StreamDecoder::new()?;
    while got < want_frames {
        match sess.recv_envelope() {
            Ok(env) => match env.kind {
                MessageKind::Frame | MessageKind::FrameH264 => {
                    if let Some((w, h, rgba)) = decoder.decode(env.kind, &env.payload)? {
                        got = got.saturating_add(1);
                        if !saved_first {
                            if let Some(p) = out_path {
                                write_ppm(p, w, h, &rgba)?;
                            }
                            saved_first = true;
                        }
                    }
                }
                MessageKind::SessionEnd => break,
                _ => {}
            },
            Err(e) => {
                return Err(TetherError::Invalid(format!(
                    "recv failed after {got} frames: {e}"
                )))
            }
        }
    }
    let _ = sess.send_envelope(&Envelope::new(
        MessageKind::SessionEnd,
        [0u8; 32],
        Vec::new(),
    ));
    let _ = auth;
    Ok(got)
}

fn tethercraft_net_device_key() -> Result<SigningKey, TetherError> {
    crate::generate_device_key()
}

/// H.264 frame payload: width u32 LE, height u32 LE, then NAL units.
pub fn encode_frame_h264(width: u32, height: u32, nal: &[u8]) -> Result<Vec<u8>, TetherError> {
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if nal.is_empty() || nal.len() > MAX_MESSAGE_BYTES - 8 {
        return Err(TetherError::Invalid("bad h264 payload".to_string()));
    }
    let mut out = Vec::with_capacity(8 + nal.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(nal);
    Ok(out)
}

pub fn decode_frame_h264(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), TetherError> {
    if bytes.len() < 8 {
        return Err(TetherError::Invalid("h264 frame too short".to_string()));
    }
    let mut w = [0u8; 4];
    let mut h = [0u8; 4];
    w.copy_from_slice(&bytes[..4]);
    h.copy_from_slice(&bytes[4..8]);
    let (width, height) = (u32::from_le_bytes(w), u32::from_le_bytes(h));
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if bytes.len() == 8 {
        return Err(TetherError::Invalid("empty h264 payload".to_string()));
    }
    Ok((width, height, bytes[8..].to_vec()))
}

/// Stateful decoder for a live stream: raw frames pass through, H.264 NALs
/// go through a persistent decoder. `Ok(None)` means the decoder needs more
/// data; the caller skips the frame, it is not an error.
pub struct StreamDecoder {
    h264: tethercraft_codec::h264::H264Decoder,
}

impl StreamDecoder {
    pub fn new() -> Result<Self, TetherError> {
        Ok(Self {
            h264: tethercraft_codec::h264::H264Decoder::new()?,
        })
    }

    pub fn decode(
        &mut self,
        kind: MessageKind,
        payload: &[u8],
    ) -> Result<Option<(u32, u32, Vec<u8>)>, TetherError> {
        match kind {
            MessageKind::Frame => {
                let (w, h, px) = decode_frame(payload)?;
                Ok(Some((w, h, px)))
            }
            MessageKind::FrameH264 => {
                let (w, h, nal) = decode_frame_h264(payload)?;
                match self.h264.decode(&nal)? {
                    Some((dw, dh, px)) => {
                        if dw != w || dh != h {
                            return Err(TetherError::Invalid(
                                "h264 dimensions disagree with header".to_string(),
                            ));
                        }
                        Ok(Some((dw, dh, px)))
                    }
                    None => Ok(None),
                }
            }
            _ => Err(TetherError::Invalid("not a frame".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn test_key(byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[byte; 32])
    }

    fn loopback_pair() -> (SessionStream, SessionStream) {
        let listener = listen("127.0.0.1:0").unwrap_or_else(|_| std::process::exit(111));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (s, _) = listener
                .accept()
                .unwrap_or_else(|_| std::process::exit(112));
            tx.send(SessionStream::new(s).map_err(|e| e.to_string()))
                .unwrap_or(());
        });
        let c = dial(&format!("127.0.0.1:{port}"), Duration::from_secs(5))
            .unwrap_or_else(|_| std::process::exit(113));
        let host = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or(Err("no host".to_string()));
        let host = host.unwrap_or_else(|_| std::process::exit(114));
        let controller = SessionStream::new(c).unwrap_or_else(|_| std::process::exit(115));
        (controller, host)
    }

    #[test]
    fn handshake_same_password() {
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(1);
        let hk = test_key(2);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-1",
                    password: b"correct horse",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: vec![tethercraft_core::Permission::Keyboard],
                },
                &mut limiter,
            )
        });
        let auth = controller_handshake(&mut c, "ctrl-1", &ck, b"correct horse").unwrap_or(
            ControllerAuth {
                host_id: String::new(),
                host_words: Vec::new(),
                grants: Vec::new(),
                fps: 0,
            },
        );
        let host_auth = handle
            .join()
            .unwrap_or(Err(TetherError::Invalid("join".to_string())));
        assert!(host_auth.is_ok());
        assert!(auth.host_id == "host-1");
        assert!(auth.host_words.len() == 8);
        assert!(auth.grants == vec![tethercraft_core::Permission::Keyboard]);
        assert!(c.peer_key.is_some());
    }

    #[test]
    fn handshake_wrong_password_fails() {
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(1);
        let hk = test_key(2);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-1",
                    password: b"right",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: Vec::new(),
                },
                &mut limiter,
            )
        });
        assert!(controller_handshake(&mut c, "ctrl-1", &ck, b"wrong").is_err());
        let host_auth = handle.join().unwrap_or(Ok(HostAuth {
            peer_id: String::new(),
            peer_key: [0u8; 32],
        }));
        assert!(host_auth.is_err());
    }

    #[test]
    fn frame_codec_roundtrip() {
        let rgba = vec![9u8; 4 * 4 * 4];
        let bytes = encode_frame(4, 4, &rgba).unwrap_or_default();
        assert!(!bytes.is_empty());
        let (w, h, back) = decode_frame(&bytes).unwrap_or((0, 0, Vec::new()));
        assert!(w == 4 && h == 4);
        assert!(back == rgba);
        assert!(encode_frame(0, 4, &rgba).is_err());
        assert!(decode_frame(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode_frame(b"junk").is_err());
    }

    #[test]
    fn frame_h264_payload_roundtrip() {
        let nal = vec![0u8, 0, 0, 1, 0x65, 1, 2, 3];
        let bytes = encode_frame_h264(320, 200, &nal).unwrap_or_default();
        assert!(!bytes.is_empty());
        let (w, h, back) = decode_frame_h264(&bytes).unwrap_or((0, 0, Vec::new()));
        assert!(w == 320 && h == 200);
        assert!(back == nal);
        assert!(encode_frame_h264(0, 200, &nal).is_err());
        assert!(encode_frame_h264(320, 200, &[]).is_err());
        assert!(decode_frame_h264(b"short").is_err());
    }

    #[test]
    fn stream_decoder_passes_raw_through() {
        let rgba = vec![7u8; 4 * 4 * 4];
        let bytes = encode_frame(4, 4, &rgba).unwrap_or_default();
        let mut dec = match StreamDecoder::new() {
            Ok(d) => d,
            Err(_) => {
                // No Cisco binary on this machine: construction must fail
                // consistently, and the live path reports it instead of running.
                assert!(StreamDecoder::new().is_err());
                return;
            }
        };
        let got = dec.decode(MessageKind::Frame, &bytes).unwrap_or(None);
        assert!(got == Some((4, 4, rgba)));
        assert!(dec.decode(MessageKind::Ping, &bytes).is_err());
    }
}
