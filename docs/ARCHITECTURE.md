# TetherCraft Architecture

TetherCraft is a pure-Rust workspace (MIT OR Apache-2.0) built around a single
policy-enforced command bus shared by the GUI, CLI, Windows host service, and
MCP server.

## Workspace layout (12 crates)

| Crate | Layer | Responsibility |
|---|---|---|
| `tethercraft-core` | L1 | Permissions, peer tiers (R2), onboarding state (R1), scam shield (R4), risk scoring (R5), evidence bundle (R6), audit events |
| `tethercraft-proto` | L1 | Native versioned wire envelope (`serde` + `postcard`), Tier 1-2 message kinds, strict parser bounds |
| `tethercraft-net` | L1 | Ed25519 identity + 8-word fingerprints, SPAKE2 PAKE handshake, ChaCha20-Poly1305 E2E session cipher, relay client, RPC helpers |
| `tethercraft-capture` | L1 | Persistent Windows Graphics Capture (`WgcStream` / `CaptureStream`) + one-shot `grab_primary` |
| `tethercraft-codec` | L1 | Vendored Cisco OpenH264 (`H264Encoder` / `H264Decoder`, BGRA/RGBA -> reused `YUVBuffer`, `ScreenContentRealTime`), recording format |
| `tethercraft-input` | L1 | Input event types and bounds validation |
| `tethercraft-host` | L1 | Host session loop, frame pump, per-message policy enforcement, enigo injection, jailed file manager, exec, power, TCP tunnels, chat |
| `tethercraft-server` | L1 | Self-hosted rendezvous + relay server (`await_bridge`, `bridge`, `check`, `claim` opaque byte pipe) |
| `tethercraft-store` | L1 | SQLite address book (pinned keys, tiers, aliases), settings, audit log |
| `tethercraft-mcp` | L2 | Stateful stdio MCP server over live sessions (confirm-gated mutating tools, audited) |
| `tethercraft-app` | L2/L3 | Main binary: CLI verbs, headless controller, host/server/service runners, and egui controller window (Board, Session, Help) |
| `tethercraft-probe` | L2 | Lean musl-compatible headless controller for cross-OS Linux -> Windows testing (no GUI/codec/capture deps) |

## Session crypto and transport flow

1. **Identity + TOFU**: Each host holds a persistent Ed25519 keypair (8-word fingerprint). Known peers pin the key; mismatches fail closed (`Denied`). First contact records the key with an out-of-band verification warning (CLI stderr + GUI `TofuNotice` banner).
2. **SPAKE2 PAKE**: Controller and host authenticate the password via SPAKE2 over Ed25519 with explicit `OtpProof` confirmation and host-side login rate limiting. No password hash or guessable challenge crosses the wire.
3. **End-to-end ChaCha20-Poly1305 AEAD**: Immediately after `OtpProof` and before `SessionStart`, both sides derive independent directional 256-bit keys via SHA-256 bound to the SPAKE2 shared secret and both Ed25519 device public keys (`k_c2h`, `k_h2c`). Every subsequent envelope is encrypted and authenticated with strictly monotonic per-direction nonces.
4. **Full-duplex split**: `SessionStream::clone_session()` shares directional `Arc<Mutex<DirCipher>>` state across the reader and writer threads on both host and controller so nonce reuse is structurally impossible.
5. **NAT relay (`--via`)**: When direct dial is unavailable, host and controller pair via `tethercraft-server` using one-time claim tokens. Because E2E encryption runs inside `SessionStream`, the relay only pipes opaque ciphertext and cannot read or tamper with frames, input, files, or exec.

## Video capture and frame pump pipeline

```
WGC (OS compositor, BGRA8)
  -> WgcStream single latest-frame slot (pre-encode drop on backpressure)
  -> spawn_pump (deadline-paced fps grid)
  -> H264Encoder::encode_bgra (reused YUVBuffer, ScreenContentRealTime, Complexity::Low)
  -> bounded mpsc::sync_channel(2) (never drops post-encode H.264 NALs)
  -> serve loop (5 ms incoming poll slice, immediate input wakeup)
  -> SessionStream (ChaCha20-Poly1305 AEAD) -> TCP / Relay
  -> Controller StreamDecoder (OpenH264 -> RGBA) -> egui texture
```

Measured on Goliath (real 1920x1080 desktop, release build, loopback): 32.5 fps end-to-end (incl. handshake and WGC startup), ~485 B/frame average on desktop content, 0 codec fallbacks. See [BENCH.md](BENCH.md).

## Security and scam-resistance boundaries

- **Host is the sole enforcement point**: Every `InputEvent`, file op, exec, tunnel, and power message is checked against the peer's tier (`Stranger`, `TrustedHelper`, `Managed`) and explicit session grants on the host.
- **R1-R7 in code**: See [THREAT_MODEL.md](THREAT_MODEL.md) and [COMPETITIVE_ANALYSIS.md](COMPETITIVE_ANALYSIS.md).
