# TetherCraft docs

Start here: [README.md](../README.md) (status, quickstart, feature matrix),
[HELP.md](../HELP.md) (operator guide), [HANDOVER.md](../HANDOVER.md)
(current work state).

| Doc | What |
|---|---|
| [../INSTALL.md](../INSTALL.md) | Pre-release binary, source build, Windows service, MCP setup, uninstall |
| [COMPETITIVE_ANALYSIS.md](COMPETITIVE_ANALYSIS.md) | Why TetherCraft exists: remote-app money-grabs, the RustDesk story, tech-support scam mitigation (R1-R7), feature gaps |
| [USE_CASES.md](USE_CASES.md) | Supported use cases vs. restricted/unsupported ones (QuickSupport, stealth, silent install, privacy mode) |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Crate map, SPAKE2 + ChaCha20-Poly1305 E2E flow, persistent WGC + H.264 pipeline |
| [IOS_PLAN.md](IOS_PLAN.md) | Tier 5 iOS controller plan: thin SwiftUI shell + Rust FFI + VideoToolbox hardware H.264 |
| [TOOLS.md](TOOLS.md) | Every CLI verb, flags, required permission, examples |
| [CONFIGURATION.md](CONFIGURATION.md) | Password files, identity, store, ports, service config |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Build, gates, workspace layout, commit discipline |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Failure modes and fixes, CLI and dev |
| [BENCH.md](BENCH.md) | Measured numbers (loopback + persistent WGC 1080p + cross-OS VM) |
| [VMTEST.md](VMTEST.md) | VM test notes |
| [THREAT_MODEL.md](THREAT_MODEL.md) | Threat model |

Crate READMEs live beside the code: `crates/tethercraft-*/README.md`.
The GUI Help tab renders the short form of this stack inside the app
(`crates/tethercraft-app/src/help.rs`); keep the two in sync when behavior
changes.
