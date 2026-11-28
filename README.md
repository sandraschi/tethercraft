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

## Why TetherCraft

Remote desktop tools suffer from two repeating problems: **consecutive money-grabs** by formerly free or cheap tools (LogMeIn killing its free tier, TeamViewer locking out family-support users as "commercial", AnyDesk following the same path), and **tech-support scam takeovers** where scammers talk elderly relatives into installing signed, zero-friction remote-control apps to drain their bank accounts. RustDesk broke the proprietary cloud trap with open-source self-hosting, but remains AGPL/Pro-split, GUI-first (no full CLI or MCP bus), and ad-hoc-stranger friendly. TetherCraft is a clean-room MIT/Apache-2.0 sibling with a unified GUI/CLI/MCP command bus, E2E encryption over relays, and built-in R1-R7 anti-scam defaults. Full breakdown: [docs/COMPETITIVE_ANALYSIS.md](docs/COMPETITIVE_ANALYSIS.md).

## Status

Tier 0 done. Tiers 1-5 domain logic done; Tier 3 security and GUI dial landed (2026-11-28, 128 workspace tests green). **Live sessions work today**: SPAKE2 handshake + ChaCha20-Poly1305 E2E session encryption (over direct TCP and relayed `--via` sessions), persistent WGC 1080p capture + vendored OpenH264 (`ScreenContentRealTime`, 32.5 fps loopback measured), input injection, host policy enforced per message, GUI in-window dial modal with masked password prompt and TOFU first-contact notice, audit written. Live over sessions too: file transfer, exec, power, TCP tunnels and chat, all permission-gated; rendezvous relay with bridge/claim pairing for NAT (`--via`), host as a Windows service from a JSON config, and a stateful MCP server with confirm-gated live tools. Not yet done: hardware H.264 MFT/NVENC encoder, two-machine LAN/Tailscale run, OS sensors. See [HANDOVER.md](HANDOVER.md) and [docs/BENCH.md](docs/BENCH.md) for measured numbers.

## Install

Pre-release `v0.9.0` binary (`tethercraft.exe`, unsigned lab use) and source build instructions live in [INSTALL.md](INSTALL.md). Double-clicking `start.bat` (or `.\start.ps1`) in a clone builds and launches the GUI window.

## Documentation

| Doc | Contents |
|---|---|
| [INSTALL.md](INSTALL.md) | Pre-release binary, source build, Windows host service, MCP setup, uninstall |
| [docs/COMPETITIVE_ANALYSIS.md](docs/COMPETITIVE_ANALYSIS.md) | Remote-app money-grabs, the RustDesk story, tech-support scam mitigation (R1-R7), comparison matrix |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate map, SPAKE2 + ChaCha20-Poly1305 E2E crypto, persistent WGC + H.264 pipeline |
| [HELP.md](HELP.md) | Operator quickstart tour, scam-resistance summary (R1-R7), crate sub-README links |
| [docs/TOOLS.md](docs/TOOLS.md) | Full CLI verb reference, flags, required permissions |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Password files, identity, SQLite store, ports, service config |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security and scam-resistance test targets |
| [docs/BENCH.md](docs/BENCH.md) | Measured frame rate, wire bytes, and cross-OS VM numbers |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Build gates, workspace layout, clean-room and commit rules |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Common failure modes and fixes |

## Controller UI

`tethercraft gui` opens the L3 egui window, structured in Board, Session,
and Help sections (tabs in the top bar). The Board is a RustDesk-style peer
kanban: Ready to connect, First contact (verify words), and Needs an
address columns; cards carry alias, id, address, trust tier, and a green
key-pinned / amber first-contact dot. Click selects, double-click (or the
Connect button) starts, Copy ID puts the id on the clipboard, and the search
box filters by name, id, tier, or address. The Session section shows the
live stream (or the headless hint when idle); Help renders the operator
guide in-app. Always-visible End session button while connected, Ctrl +
wheel zoom, audit tail, and the R1 install gate (20-second wait, answer
stored in `onboarding.txt` next to the store; yes/not-sure enables 24 h
outgoing-only with a banner). Double-click `start.bat` (or `.\start.ps1`)
builds and launches the window so it always runs current code.

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

> **Operator Guide & Crate Docs:** See [`HELP.md`](HELP.md) for the quickstart tour, scam-resistance summary (R1–R7), and links to all 12 crate sub-READMEs (`crates/tethercraft-*/README.md`). The [`docs/`](docs/README.md) stack has the CLI reference ([TOOLS](docs/TOOLS.md)), [configuration](docs/CONFIGURATION.md), [development](docs/DEVELOPMENT.md), and [troubleshooting](docs/TROUBLESHOOTING.md) guides.

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
