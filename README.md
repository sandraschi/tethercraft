# TetherCraft

Pure-Rust remote desktop with RustDesk 1.5.1 feature parity as the goal, plus a
complete CLI and an MCP server on one command bus. Session B build, Tier 0 + Tier 1,
native protocol only, Windows only. Clean room: no RustDesk source is opened.

> Scope this week: Tier 0 scaffold (C0) and Tier 1 Windows-to-Windows over LAN and
> Tailscale (C1), including scam resistance R1, R2, R7. No relay server, no RustDesk
> compat, no Linux or macOS.

## Status

Tier 0 done, Tier 1 in progress. See `HANDOVER.md` for the live state.

## Layout

| Crate | Role |
|---|---|
| `tethercraft-core` | Command bus, permissions, peer tiers (R2), onboarding state (R1), audit events |
| `tethercraft-proto` | Native wire schema (own design, serde + postcard), versioned envelope |
| `tethercraft-net` | Ed25519 device keys, fingerprint words, SPAKE2 PAKE, QUIC pinned-key transport config |
| `tethercraft-capture` | WGC with DXGI fallback on Windows; `NotImplemented` elsewhere |
| `tethercraft-codec` | Hardware encoder plus VP9 software fallback (interface + VP9 path marker) |
| `tethercraft-input` | Input injection interface, permission-checked by host |
| `tethercraft-host` | Host policy enforcement: every message checked, stranger defaults, session caps |
| `tethercraft-server` | Self-hosted rendezvous and relay placeholder (Tier 3, not implemented) |
| `tethercraft-store` | SQLite address book, settings, audit log |
| `tethercraft-mcp` | stdio MCP server, read tools |
| `tethercraft-app` | CLI (`status`, `id`, `peers`, `peer-info`, `connect`, `screenshot`, `book`, `audit`, `wake`) with `--json`, plus egui controller placeholder |

## Feature matrix (PRD section 6 rows)

| Area | Feature | Tier | Status |
|---|---|---|---|
| Session | view and control, view-only, per-permission custom access | 1 | Partial |
| Session | multi-monitor switch and show-all, restore last monitor on reconnect | 2 | Planned |
| Session | scaling modes, quality presets, fps cap, remote resolution change, HiDPI/Retina | 1-2 | Planned |
| Session | remote cursor, show my cursor, relative mouse mode, stylus | 2-3 | Planned |
| Session | keyboard modes map / translate / legacy, international layouts, Wayland keyboard | 2-3 | Planned |
| Session | configurable session shortcuts | 2 | Planned |
| Session | Ctrl+Alt+Del / SAS, lock, restart remote, logon-screen access, UAC elevation on demand | 2 | Planned |
| Session | connect to a specific Windows session; terminal-server sessions | 3 | Planned |
| Session | tabs or separate windows | 1 | Planned |
| Session | screenshot | 1 | Partial |
| Session | recording (local and host-side storage, visible to host) | 4 | Planned |
| Video | software codecs VP9 and AV1, hardware H.264/H.265, zero-copy GPU path, 4:4:4 | 1 (one codec), 3 (all) | Partial |
| Video | virtual display for headless Windows, headless Linux | 4 | Planned |
| Video | privacy mode incl. multi-monitor; macOS privacy mode | 4 | Planned |
| Data | clipboard text and images | 1 | Partial |
| Data | clipboard files (copy on one side, paste on the other) | 2 | Planned |
| Data | file transfer manager: browse, upload, download, resume, folders | 2 | Planned |
| Data | audio from remote | 2 | Planned |
| Data | text chat, session notes | 2 | Planned |
| Data | voice call | 4 | Planned |
| Data | camera view of remote host | 4 | Planned |
| Data | remote printer (Windows to Windows first) | 4 | Planned |
| Data | TCP tunnel (port forwarding), RDP over tunnel | 2 | Planned |
| Data | terminal with PTY, persistent sessions, reconnection buffer | 2 | Planned |
| Access | one-time password (alphanumeric / numeric), permanent password, approve by click, both | 1 | Partial |
| Access | 2FA (TOTP), trusted devices | 2 | Planned |
| Access | per-connection and per-user permissions (keyboard, clipboard, file, audio, camera, terminal, printer, tunnel, restart, recording, privacy) | 1-2 | Partial |
| Access | IP allowlist, LAN discovery, direct IP access | 1 | Planned |
| Access | audit log with controller attribution | 1 | Partial |
| Net | direct LAN and Tailscale/WireGuard addresses | 1 | Partial |
| Net | rendezvous + relay server (self-hosted, one binary), UDP/TCP hole punching, IPv6, relay fallback, force relay | 3 | Planned |
| Net | WebSocket transport for proxies and web client; SOCKS5 and HTTP proxy | 3-4 | Planned |
| Manage | address book with tags, aliases, favourites, recent, discovered | 1 | Partial |
| Manage | Wake-on-LAN | 1 | Partial |
| Manage | deployment: config string import/export, silent install, MSI, set/get ID, set password | 2 | Planned |
| Manage | signed auto-update (metadata signature verified before download) | 4 | Planned |
| Platform | Windows x64 and ARM64 (controller and host) | 1 | Partial |
| Platform | Linux X11 and Wayland, macOS (controller and host) | 3 | Planned |
| Platform | web client (controller only) | 4 | Planned |
| Platform | Android (controller and host), iOS (controller) | 5 | Planned |
| Pro-class | web console, OIDC/LDAP, strategies, multiple relays, device groups, custom client generator | 6 | Planned |
| Scam R1 | install-time onboarding, 24 h outgoing-only on yes | 1 | Partial |
| Scam R2 | peer tiers enforced on host, stranger defaults, 30 min cap, local recording | 1 | Partial |
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
