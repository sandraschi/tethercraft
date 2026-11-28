# TetherCraft Help & Operator Guide

Pure-Rust remote desktop with native wire protocol, SPAKE2 authentication, ChaCha20-Poly1305 E2E session encryption, persistent WGC 1080p H.264 capture, host-enforced scam resistance (R1-R7), unified CLI, egui controller window, and stdio MCP server.

## 1. Get Started (Two Ends on One Machine)

1. **Start a Host (Synthetic Frames - No GPU Needed)**:
   ```powershell
   tethercraft host --port 21120 --password-file pw.txt --grants keyboard --fake-frames
   ```
   Drop `--fake-frames` on Windows for live persistent Windows Graphics Capture (WGC) + OpenH264.
2. **Trust the Host & Connect**:
   ```powershell
   tethercraft book add goliath <host-id-hex> --addr 127.0.0.1:21120 --alias Goliath
   tethercraft serve goliath --password-file pw.txt --frames 10 --out frame.ppm
   ```
   Compare the fingerprint words printed by the host with the words reported by the controller - first-use trust is that comparison, not a certificate authority.
3. **Launch the Controller GUI**: Run `tethercraft` (or `tethercraft gui` / `tethercraft connect <peer>`) to open the native egui controller with Board, Session, and Help tabs: a peer kanban (Ready, First contact, Needs an address), in-window Connect modal (masked password, optional `--via` relay, view-only, background SPAKE2 + E2E handshake), in-window TOFU warning banner on first contact, live session stream, always-visible **End session** button, `Ctrl+Wheel` zoom, audit tail, and the in-app operator guide. Double-click `start.bat` (or `.\start.ps1`) to build and launch it. Full CLI and MCP reference in [docs/TOOLS.md](docs/TOOLS.md).
4. **Headless Scripted Check**: Run `tethercraft connect <peer> --headless out.ppm` to run a session without a window and save a captured frame.

## 2. Functions & Crate Layout

- **Core & Policy (`tethercraft-core`, `tethercraft-host`)**: Command bus, per-message permission checks, peer tiers (`R2`), install-time onboarding gate (`R1`), mobile gesture/pacing groundwork, and audit log events.
- **Wire & Crypto (`tethercraft-proto`, `tethercraft-net`)**: Native `serde` + `postcard` versioned wire schema, Ed25519 device keys, 8-word fingerprints, SPAKE2 PAKE handshake, and ChaCha20-Poly1305 E2E session encryption over direct TCP and relayed (`--via`) connections.
- **Capture, Codec & Input (`tethercraft-capture`, `tethercraft-codec`, `tethercraft-input`)**: Persistent Windows WGC capture (`WgcStream` BGRA8), vendored Cisco OpenH264 (`ScreenContentRealTime`), and host-permission-gated input injection.
- **Storage, Server & Cross-OS Probe (`tethercraft-store`, `tethercraft-server`, `tethercraft-probe`)**: SQLite address book & audit store, self-hosted rendezvous/relay server (opaque E2E byte pipe), and headless Linux-musl cross-compilable controller probe.
- **CLI, GUI & MCP (`tethercraft-app`, `tethercraft-mcp`)**: Unified CLI with `--json`, egui controller window with Board/Session/Help sections, and stateful stdio MCP server (read tools open, mutating tools need `confirm=true`).

## 3. Scam Resistance & Safety Invariants (R1-R7)

- **R1 (Install Gate)**: First run asks *"Did someone call, email, text or show a pop-up telling you to install this?"* with a 20-second wait. Answering *yes* or *not sure* locks the host into 24-hour outgoing-only mode with a visible banner.
- **R2 (Stranger Defaults)**: Every unknown peer starts in the **Stranger** tier: view-only, no file transfer, no terminal, no privacy mode/blanking, no input blocking, no unattended access, 30-minute hard cap, and local recording enabled.
- **R3-R6**: Trusted-helper promotion requires an in-person token or 24-hour cooling period; Scam Shield blurs/blocks banking/password windows in Stranger sessions; risk scoring can freeze input and capture an evidence bundle with one-click block.
- **R7**: Consumer builds always enforce R1 - no onboarding-free QuickSupport executable and no stealth/hidden tray mode.

## 4. Use Cases (Supported vs. Not Supported)

- **Supported:** Own machines over LAN/Tailscale, safe family IT support (with R1-R7 guardrails), scripted CLI and confirm-gated MCP agent automation, self-hosted E2E NAT relay (`tethercraft server`), planned Tier 5 iOS/Android mobile controllers ([docs/IOS_PLAN.md](docs/IOS_PLAN.md)), and planned Tier 4/5 low-latency **Game Streaming** (under development / TODO, [docs/GAME_STREAMING.md](docs/GAME_STREAMING.md)).
- **Restricted / Not Supported:** Onboarding-free full-control QuickSupport for strangers, covert/stealth monitoring or hidden tray, silent consumer install / RMM-MDM, screen blanking for Strangers, and public vendor-operated cloud relay. Full rationale and competitive comparison in [docs/USE_CASES.md](docs/USE_CASES.md) and [docs/COMPETITIVE_ANALYSIS.md](docs/COMPETITIVE_ANALYSIS.md).

## 5. Crate Documentation

- [tethercraft-core](crates/tethercraft-core/README.md) | [tethercraft-proto](crates/tethercraft-proto/README.md) | [tethercraft-net](crates/tethercraft-net/README.md) | [tethercraft-capture](crates/tethercraft-capture/README.md) | [tethercraft-codec](crates/tethercraft-codec/README.md) | [tethercraft-input](crates/tethercraft-input/README.md)
- [tethercraft-host](crates/tethercraft-host/README.md) | [tethercraft-server](crates/tethercraft-server/README.md) | [tethercraft-store](crates/tethercraft-store/README.md) | [tethercraft-mcp](crates/tethercraft-mcp/README.md) | [tethercraft-app](crates/tethercraft-app/README.md) | [tethercraft-probe](crates/tethercraft-probe/README.md)
