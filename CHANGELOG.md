# Changelog

## 0.3.0 - 2026-11-24

- Tier 3 started: rendezvous + relay server over loopback TCP (register,
  heartbeat, lookup, relay mailbox routing of opaque frames), NAT candidate
  ordering, `tethercraft server` verb, and the egui controller window
  (`tethercraft gui`: peer sidebar, session view, End session, Ctrl+wheel
  zoom, audit tail, R1 gate with 20-second wait and persisted answer).
- 73 tests green; clippy and fmt clean. Live WAN run and Linux/macOS still
  pending (no such machines this week).

## 0.2.0 - 2026-11-24

- Tier 2 domain logic: Tier 2 wire kinds, host file manager (root-jailed),
  terminal registry, host-side agent sessions (view-only default, rate limit,
  kill switch), R4 scam shield engine, folder sync planner with local apply,
  sealed deploy configs (Argon2id plus XChaCha20-Poly1305, ed25519-signed),
  R3 promotion tokens and cooling, R5 risk scorer, R6 evidence bundle and peer
  blocklist, chat log and notes, CLI files/sync/broadcast/collect/batch/
  restart/shutdown/lock/exec/tunnel verbs, MCP confirm-gated write tools.
- 64 tests green; clippy and fmt clean. Live host service run still pending.

## 0.1.0 - 2026-11-24

- Tier 0 scaffold: 11-crate workspace, deny lints, CI, threat model doc, native proto schema.
- Tier 1 start: keys, fingerprint words, SPAKE2 handshake state, host policy (R2), onboarding state (R1/R7), address book and audit store, CLI with --json, MCP read tools.
