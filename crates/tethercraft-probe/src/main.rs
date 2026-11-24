//! Headless controller probe: connect to a TetherCraft host from another
//! machine, run the handshake, collect frames, and report the numbers.
//!
//! Kept free of the GUI, capture, store and codec crates so it cross-compiles
//! to Linux for cross-OS testing against a Windows host.

use std::time::{Duration, Instant};

use tethercraft_core::TetherError;
use tethercraft_net::session::{self, SessionStream};
use tethercraft_proto::{Envelope, MessageKind};

struct Args {
    addr: String,
    password: String,
    frames: u32,
    out: Option<String>,
    input_probe: bool,
    json: bool,
}

fn parse_args() -> Result<Args, TetherError> {
    let mut a = Args {
        addr: String::new(),
        password: String::new(),
        frames: 1,
        out: None,
        input_probe: false,
        json: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--addr" => {
                a.addr = it
                    .next()
                    .ok_or_else(|| TetherError::Invalid("--addr needs a value".to_string()))?
            }
            "--password-file" => {
                let p = it
                    .next()
                    .ok_or_else(|| TetherError::Invalid("--password-file needs a value".to_string()))?;
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| TetherError::Invalid(format!("password file: {e}")))?;
                a.password = text.trim_end_matches(['\r', '\n']).to_string();
            }
            "--frames" => {
                let v = it
                    .next()
                    .ok_or_else(|| TetherError::Invalid("--frames needs a value".to_string()))?;
                a.frames = v
                    .parse()
                    .map_err(|_| TetherError::Invalid("--frames must be a number".to_string()))?;
            }
            "--out" => a.out = Some(
                it.next()
                    .ok_or_else(|| TetherError::Invalid("--out needs a value".to_string()))?,
            ),
            "--input-probe" => a.input_probe = true,
            "--json" => a.json = true,
            other => {
                return Err(TetherError::Invalid(format!(
                    "unknown argument {other} (use --addr, --password-file, --frames, --out, --input-probe, --json)"
                )))
            }
        }
    }
    if a.addr.is_empty() || a.password.is_empty() {
        return Err(TetherError::Invalid(
            "--addr and --password-file are required".to_string(),
        ));
    }
    Ok(a)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), TetherError> {
    let args = parse_args()?;
    let signing = tethercraft_net::generate_device_key()?;
    let started = Instant::now();
    let stream = session::dial(&args.addr, Duration::from_secs(10))?;
    let dial_ms = started.elapsed().as_millis();
    let mut sess = SessionStream::new(stream)?;
    let hs_start = Instant::now();
    let auth = session::controller_handshake(
        &mut sess,
        "linux-probe",
        &signing,
        args.password.as_bytes(),
    )?;
    let handshake_ms = hs_start.elapsed().as_millis();

    if args.input_probe {
        let payload =
            session::json_bytes(&tethercraft_input::InputEvent::MouseScroll { dx: 0, dy: 0 })?;
        sess.send_envelope(&Envelope::new(MessageKind::InputEvent, [0u8; 32], payload))?;
    }

    sess.set_frame_timeouts()?;
    let mut got = 0u32;
    let mut first_frame_ms: Option<u128> = None;
    let mut bytes_total = 0usize;
    let mut dims = (0u32, 0u32);
    let frames_start = Instant::now();
    let mut saved = false;

    while got < args.frames {
        let env = sess.recv_envelope()?;
        match env.kind {
            MessageKind::Frame => {
                let (w, h, rgba) = session::decode_frame(&env.payload)?;
                dims = (w, h);
                bytes_total = bytes_total.saturating_add(env.payload.len());
                got = got.saturating_add(1);
                if first_frame_ms.is_none() {
                    first_frame_ms = Some(started.elapsed().as_millis());
                }
                if !saved {
                    if let Some(p) = &args.out {
                        session::write_ppm(p, w, h, &rgba)?;
                        saved = true;
                    }
                }
            }
            MessageKind::SessionEnd => break,
            _ => {}
        }
    }
    let frames_ms = frames_start.elapsed().as_millis();

    let host_id = auth.host_id.clone();
    let words = auth.host_words.join(" ");
    let grants = auth.grants.len();

    if args.json {
        println!(
            "{{\"host\":\"{host_id}\",\"words\":\"{words}\",\"grants\":{grants},\"dial_ms\":{dial_ms},\"handshake_ms\":{handshake_ms},\"frames\":{got},\"frames_ms\":{frames_ms},\"width\":{},\"height\":{},\"bytes\":{bytes_total}}}",
            dims.0, dims.1
        );
    } else {
        println!("host {host_id}");
        println!("words {words}");
        println!("grants {grants}");
        println!("dial {dial_ms} ms, handshake {handshake_ms} ms");
        println!(
            "frames {got} in {frames_ms} ms ({w}x{h}, {kb} KiB total)",
            w = dims.0,
            h = dims.1,
            kb = bytes_total / 1024
        );
        if let Some(f) = first_frame_ms {
            println!("first frame at {f} ms after dial start");
        }
    }
    Ok(())
}
