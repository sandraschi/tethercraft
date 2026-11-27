# TetherCraft

Pure-Rust remote desktop with RustDesk 1.5.1 feature parity as the goal, plus a
complete CLI and an MCP server on one command bus. Session B build, Tier 0 + Tier 1,
native protocol only, Windows only. Clean room: no RustDesk source is opened.

> Scope this week: Tier 0 scaffold (C0), Tier 1 Windows-to-Windows over LAN and
> Tailscale (C1) including scam resistance R1, R2, R7, then Tier 2 (C2):
> host-side file/chat/terminal/tunnel management, folder sync planner, agent
> sessions, and scam resistance R3-R6. Tier 3 started: loopback rendezvous +
> relay server and the egui controller window. No RustDesk compat, no Linux
> or macOS.

## Status

Tier 0 done. Tiers 1-5 domain logic done. **Live single-machine sessions work
today**: real SPAKE2 handshake, real WGC capture, frames over TCP, input
injection, host policy enforced per message, audit written. Live over sessions
too: file transfer, exec, power, TCP tunnels and chat, all permission-gated;
rendezvous relay with bridge/claim pairing for NAT (`--via`), host as a Windows
service from a JSON config, and a stateful MCP server with confirm-gated live
tools. Not yet done: the two-machine LAN/Tailscale run, codecs, OS sensors. See
`HANDOVER.md` and `docs/BENCH.md` for measured numbers.

## Controller UI

`tethercraft gui` opens the L3 egui window: a RustDesk-style peer board
(cards with alias, id, address, and a green key-pinned / amber
first-contact dot), a search box filtering by name, id, or address, click to
select, double-click (or the Connect button) to start, session view,
always-visible End session button while connected, Ctrl + wheel zoom, audit
tail, and the R1 install gate (20-second wait, answer stored in
`onboarding.txt` next to the store; yes/not-sure enables 24 h
outgoing-only with a banner).

`tethercraft connect <peer>` dials the host, runs the handshake, and opens the
same window on the live stream: pointer and keyboard input are forwarded to the
host while the session is not `--view-only`. `tethercraft connect <peer>
--headless <out.ppm>` runs the same session without a window and saves a frame,
which is what the scripted checks use.

## Quick start, two ends on one machine

```powershell
# terminal 1: host, synthetic frames (no GPU needed)
tethercraft host --port 21120 --password-file pw.txt --grants keyboard --fake-frames

# terminal 2: trust the host, then connect
tethercraft book add goliath <host-id-hex> --addr 127.0.0.1:21120 --alias Goliath
tethercraft serve goliath --password-file pw.txt --frames 10 --out frame.ppm
```

Drop `--fake-frames` for real WGC capture. Compare the fingerprint words printed
by the host with the ones the controller reports; first-use trust is that
comparison, not a certificate authority.

## Layout

> **Operator Guide & Crate Docs:** See [`HELP.md`](HELP.md) for the quickstart tour, scam-resistance summary (R1–R7), and links to all 12 crate sub-READMEs (`crates/tethercraft-*/README.md`).

| Crate | Role |
|---|---|
| `tethercraft-core` | Command bus, permissions, peer tiers (R2), onboarding state (R1), audit events |
| `tethercraft-proto` | Native wire schema (own design, serde + postcard), versioned envelope |
| `tethercraft-net` | Ed25519 device keys, fingerprint words, SPAKE2 PAKE, QUIC pinned-key transport config |
| `tethercraft-capture` | WGC with DXGI fallback on Windows; `NotImplemented` elsewhere |
| `tethercraft-codec` | Hardware encoder plus VP9 software fallback (interface + VP9 path marker) |
| `tethercraft-input` | Input injection interface, permission-checked by host |
| `tethercraft-host` | Host policy enforcement: every message checked, stranger defaults, session caps |
| `tethercraft-server` | Self-hosted rendezvous, relay mailbox, and bridge/claim pairing for NAT (loopback-tested raw byte pipe) |
| `tethercraft-store` | SQLite address book, settings, audit log |
| `tethercraft-mcp` | stdio MCP server with live sessions: read tools open, mutating tools need confirm and are audited |
| `tethercraft-app` | CLI (`status`, `id`, `peers`, `peer-info`, `connect`, `screenshot`, `book`, `audit`, `wake`, `files`, `sync`, `broadcast-file`, `collect-files`, `batch`, `restart`, `shutdown`, `lock`, `exec`, `tunnel`, `chat`, `service`, `recordings`, `update-check`, `platform`, `host`, `serve`, `gui`, `server`) with `--json`, global `--via` relay dialing and `--password-file`, plus the egui controller window |
| `tethercraft-probe` | Headless controller for cross-OS testing; no GUI, capture, store or codec deps, so it cross-compiles (`x86_64-unknown-linux-musl`) and runs as a Linux controller against a Windows host |

## Feature matrix (PRD section 6 rows)

| Area | Feature | Tier | Status |
|---|---|---|---|
| Session | view and control, view-only, per-permission custom access | 1 | Partial |
| Session | multi-monitor switch and show-all, restore last monitor on reconnect | 2 | Planned |
| Session | scaling modes, quality presets, fps cap, remote resolution change, HiDPI/Retina | 1-2 | Partial |
| Session | remote cursor, show my cursor, relative mouse mode, stylus | 2-3 | Planned |
| Session | keyboard modes map / translate / legacy, international layouts, Wayland keyboard | 2-3 | Planned |
| Session | configurable session shortcuts | 2 | Planned |
| Session | Ctrl+Alt+Del / SAS, lock, restart remote, logon-screen access, UAC elevation on demand | 2 | Planned |
| Session | connect to a specific Windows session; terminal-server sessions | 3 | Planned |
| Session | tabs or separate windows | 1 | Planned |
| Session | screenshot | 1 | Partial |
| Session | recording (local and host-side storage, visible to host) | 4 | Partial |
| Video | software codecs VP9 and AV1, hardware H.264/H.265, zero-copy GPU path, 4:4:4 | 1 (one codec), 3 (all) | Partial || Video | virtual display for headless Windows, headless Linux | 4 | Partial |
| Video | privacy mode incl. multi-monitor; macOS privacy mode | 4 | Partial |
| Data | clipboard text and images | 1 | Partial |
| Data | clipboard files (copy on one side, paste on the other) | 2 | Partial |
| Data | file transfer manager: browse, upload, download, resume, folders | 2 | Partial |
| Data | audio from remote | 2 | Partial |
| Data | text chat, session notes | 2 | Partial |
| Data | voice call | 4 | Partial |
| Data | camera view of remote host | 4 | Partial |
| Data | remote printer (Windows to Windows first) | 4 | Partial |
| Data | TCP tunnel (port forwarding), RDP over tunnel | 2 | Partial |
| Data | terminal with PTY, persistent sessions, reconnection buffer | 2 | Partial |
| Access | one-time password (alphanumeric / numeric), permanent password, approve by click, both | 1 | Partial |
| Access | 2FA (TOTP), trusted devices | 2 | Partial |
| Access | per-connection and per-user permissions (keyboard, clipboard, file, audio, camera, terminal, printer, tunnel, restart, recording, privacy) | 1-2 | Partial |
| Access | IP allowlist, LAN discovery, direct IP access | 1 | Planned |
| Access | audit log with controller attribution | 1 | Partial |
| Net | direct LAN and Tailscale/WireGuard addresses | 1 | Partial || Net | rendezvous + relay server (self-hosted, one binary), UDP/TCP hole punching, IPv6, relay fallback, force relay | 3 | Partial |
| Net | WebSocket transport for proxies and web client; SOCKS5 and HTTP proxy | 3-4 | Planned |
| Manage | address book with tags, aliases, favourites, recent, discovered | 1 | Partial |
| Manage | Wake-on-LAN | 1 | Partial |
| Manage | deployment: config string import/export, silent install, MSI, set/get ID, set password | 2 | Partial |
| Manage | signed auto-update (metadata signature verified before download) | 4 | Partial |
| Platform | Windows x64 and ARM64 (controller and host) | 1 | Partial || Platform | Linux X11 and Wayland, macOS (controller and host) | 3 | Planned |
| Platform | web client (controller only) | 4 | Planned |
| Platform | Android (controller and host), iOS (controller) | 5 | Partial |
| Pro-class | web console, OIDC/LDAP, strategies, multiple relays, device groups, custom client generator | 6 | Planned |
| Scam R1 | install-time onboarding, 24 h outgoing-only on yes | 1 | Partial |
| Scam R2 | peer tiers enforced on host, stranger defaults, 30 min cap, local recording | 1 | Partial |
| Scam R3 | trusted-helper promotion via in-person token or 24 h cooling from local UI | 2 | Partial |
| Scam R4 | scam shield blurring and input-blocking banking/payment/password windows in stranger sessions | 2 | Partial |
| Scam R5 | risk scoring with input freeze and capture-excluded interstitial | 2 | Partial |
| Scam R6 | evidence bundle and one-click peer block | 2 | Partial |
| Scam R7 | consumer builds always run R1, no quick-support exe | 1 | Partial |

Partial means the domain model and policy enforcement exist and are tested; live
video transport and the second-machine latency run are still pending.

## Build

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p tethercraft-app -- status --json
```

## Scam resistance (summary)

R1: first run asks "Did someone call, email, text or show a pop-up telling you to
install this?" Yes or not-sure enables 24-hour outgoing-only mode. R2: every new
peer is Stranger (view-only, no files, terminal, privacy mode, blanking, input
blocking, unattended; 30-minute cap; local recording on). R7: consumer builds have
no onboarding-free quick-support exe. Details in `docs/THREAT_MODEL.md` and PRD
section 17.
