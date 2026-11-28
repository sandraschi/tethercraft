# TetherCraft docs

Start here: [README.md](../README.md) (status, quickstart, feature matrix),
[HELP.md](../HELP.md) (operator guide), [HANDOVER.md](../HANDOVER.md)
(current work state).

| Doc | What |
|---|---|
| [TOOLS.md](TOOLS.md) | Every CLI verb, flags, required permission, examples |
| [CONFIGURATION.md](CONFIGURATION.md) | Password files, identity, store, ports, service config |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Build, gates, workspace layout, commit discipline |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Failure modes and fixes, CLI and dev |
| [BENCH.md](BENCH.md) | Measured numbers (loopback + capture) |
| [VMTEST.md](VMTEST.md) | VM test notes |
| [THREAT_MODEL.md](THREAT_MODEL.md) | Threat model |

Crate READMEs live beside the code: `crates/tethercraft-*/README.md`.
The GUI Help tab renders the short form of this stack inside the app
(`crates/tethercraft-app/src/help.rs`); keep the two in sync when behavior
changes.
