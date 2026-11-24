---
updated: 2026-11-24T15:00:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier2]
---

## What's happening right now

Tier 2 domain logic is implemented and green (64 tests, clippy and fmt clean).
Host-side file manager, terminal registry, agent sessions, scam shield, sync
planner, sealed deploy configs, R3-R6 policy engines, new CLI verbs and
confirm-gated MCP write tools all exist and are tested. Live transport and the
host service run are still pending.

## Last concrete action

Extended the workspace: `core` gained `trust`/`risk`/`agent`/`deploy` modules,
`proto` gained 19 Tier 2 message kinds, `net` gained sealed deploy configs
(Argon2id plus XChaCha20-Poly1305, ed25519-signed), `host` gained
`files`/`terminal`/`agent`/`shield`/`sync` modules plus a chat log, `store`
gained the R6 blocklist, the CLI gained files/sync/broadcast/collect/batch/
restart/shutdown/lock/exec/tunnel verbs, and the MCP server gained sessions
plus confirm-gated write tools with audit. 64 tests pass. No RustDesk source
opened.

## Next step

Weekend verification by agy covers Tiers 1 and 2 (fresh clone, tests, clippy,
fmt, README matrix, scammer-scenario review); then the two-machine live run
over Tailscale with latency into `docs/BENCH.md`.

## Blockers

Second Windows machine and Tailscale path for the live run. The Windows host
service (SYSTEM service, secure-desktop helper, ConPTY backend, WGC wiring,
OS sensors for the R4 shield) is Tier 2 remainder work for the owner session.

## Context the next agent needs

- Honest Partial inventory: sync planner and local apply are fully real;
  ConPTY backend, WGC capture, QUIC handshake, and OS sensors (browser URL,
  frontmost window) return `NotImplemented`. The R4 shield engine, R5 scorer,
  R3 promotion, and sealed configs are real policy code with tests.
- Tier 2 exit needs >= 50 tests: at 64 with the scripted scammer-scenario test
  (`risk::scammer_script_hits_every_signal`), host stranger-denial tests, and
  traversal tests in `files` and `proto`. libFuzzer harnesses are still pending;
  malformed-input tests cover the same parsers today.
- Clean room holds. O1/O2/O3 state is unchanged from the Tier 1 handover.
- Commit discipline held: max 10 files per commit, LF, no em dash.
