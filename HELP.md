# TetherCraft Help & Operator Guide

Pure-Rust remote desktop with native wire protocol, SPAKE2 authentication, WGC capture, host-enforced scam resistance (R1–R7), unified CLI, egui controller window, and stdio MCP server.

## 1. Get Started (Two Ends on One Machine)

1. **Start a Host (Synthetic Frames — No GPU Needed)**:
   ```powershell
   tethercraft host --port 21120 --password-file pw.txt --grants keyboard --fake-frames
   ```
   Drop `--fake-frames` on Windows for live Windows Graphics Capture (WGC).
2. **Trust the Host & Connect**:
   ```powershell
   tethercraft book add goliath <host-id-hex> --addr 127.0.0.1:21120 --alias Goliath
   tethercraft serve goliath --password-file pw.txt --frames 10 --out frame.ppm
   ```
   Compare the fingerprint words printed by the host with the words reported by the controller - first-use trust is that comparison, not a certificate authority.
3. **Launch the Controller GUI**: Run `tethercraft gui` (or `tethercraft connect <peer>`) to open the native egui controller with Board, Session, and Help tabs: a RustDesk-style peer kanban (Ready, First contact, Needs an address), live session stream, always-visible **End session** button, `Ctrl+Wheel` zoom, audit tail, and the in-app operator guide. Double-click `start.bat` (or `.\start.ps1`) to build and launch it. Full verb list in `docs/TOOLS.md`.
4. **Headless Scripted Check**: Run `tethercraft connect <peer> --headless out.ppm` to run a session without a window and save a captured frame.

## 2. Functions & Crate Layout

- **Core & Policy (`tethercraft-core`, `tethercraft-host`)**: Command bus, per-message permission checks, peer tiers (`R2`), install-time onboarding gate (`R1`), and audit log events.
- **Wire & Crypto (`tethercraft-proto`, `tethercraft-net`)**: Native `serde` + `postcard` versioned wire schema, Ed25519 device keys, fingerprint words, SPAKE2 PAKE handshake, and QUIC pinned-key transport configuration.
- **Capture, Codec & Input (`tethercraft-capture`, `tethercraft-codec`, `tethercraft-input`)**: Windows WGC with DXGI fallback, hardware/VP9 codec interface, and host-permission-gated input injection.
- **Storage, Server & Cross-OS Probe (`tethercraft-store`, `tethercraft-server`, `tethercraft-probe`)**: SQLite address book & audit store, self-hosted rendezvous/relay server (Tier 3), and headless Linux-musl cross-compilable controller probe.
- **CLI, GUI & MCP (`tethercraft-app`, `tethercraft-mcp`)**: Unified CLI with `--json`, egui controller window with Board/Session/Help sections, and stateful stdio MCP server (read tools open, mutating tools need `confirm=true`).

## 3. Scam Resistance & Safety Invariants (R1–R7)

- **R1 (Install Gate)**: First run asks *"Did someone call, email, text or show a pop-up telling you to install this?"* with a 20-second wait. Answering *yes* or *not sure* locks the host into 24-hour outgoing-only mode with a visible banner.
- **R2 (Stranger Defaults)**: Every unknown peer starts in the **Stranger** tier: view-only, no file transfer, no terminal, no privacy mode/blanking, no input blocking, no unattended access, 30-minute hard cap, and local recording enabled.
- **R3–R6**: Trusted-helper promotion requires an in-person token or 24-hour cooling period; Scam Shield blurs/blocks banking/password windows in Stranger sessions; risk scoring can freeze input and capture an evidence bundle with one-click block.
- **R7**: Consumer builds always enforce R1 — no onboarding-free quick-support executable.

## 4. Crate Documentation

- [tethercraft-core](crates/tethercraft-core/README.md) · [tethercraft-proto](crates/tethercraft-proto/README.md) · [tethercraft-net](crates/tethercraft-net/README.md) · [tethercraft-capture](crates/tethercraft-capture/README.md) · [tethercraft-codec](crates/tethercraft-codec/README.md) · [tethercraft-input](crates/tethercraft-input/README.md)
- [tethercraft-host](crates/tethercraft-host/README.md) · [tethercraft-server](crates/tethercraft-server/README.md) · [tethercraft-store](crates/tethercraft-store/README.md) · [tethercraft-mcp](crates/tethercraft-mcp/README.md) · [tethercraft-app](crates/tethercraft-app/README.md) · [tethercraft-probe](crates/tethercraft-probe/README.md)
