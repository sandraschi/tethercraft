---
updated: 2026-11-24T13:30:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier1]
---

## What's happening right now

Tier 0 scaffold is done and Tier 1 domain logic is implemented and green:
11-crate pure-Rust workspace, native wire protocol, Ed25519 keys with fingerprint
words, SPAKE2 PAKE, host-enforced stranger policy (R2), onboarding gate with
24-hour outgoing-only mode (R1/R7), OTP codes, SQLite address book and audit,
CLI with --json, stdio MCP read tools. 31 tests pass; clippy and fmt are clean.

## Last concrete action

Built the full workspace in `D:\Dev\repos\tethercraft` (31 tests green,
`cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo fmt --all --check` clean, CLI `status --json` and `id --json` verified
live). Wrote O1 `projects/remotecraft/spec/LEGAL_NOTES.md` and O3
`projects/mediacraft/LICENSE_CHECK.md` in mcp-central-docs. No RustDesk source
opened. Claim file: `operations/session-log/craft-claim-opencode-tethercraft-2026-11-24.md`.

## Next step

Two-machine live run over Tailscale (Goliath plus one more): QUIC handshake with
pinned keys, real WGC capture, latency measured into `docs/BENCH.md`; then
weekend verification by agy (fresh clone, tests, clippy, fmt, README matrix).

## Blockers

Second Windows machine and Tailscale path for the live latency run. Nothing else.

## Context the next agent needs

- Tier 1 is Partial by design, not done: `connect`, `screenshot`, `wake`, live
  QUIC connect, and live capture all return explicit `NotImplemented` (never fake
  success). The weekend verifier ticks Tier 1 against PRD 6/17 rows, then Tier 2
  starts per HANDOVER_CURSOR.md (C2: host service, file transfer, R3-R6).
- Clean room holds: implementation built only from PRD/RESEARCH plus own design.
  The spec team owns `projects/remotecraft/spec/`; implementation never reads it
  as code, only the cursor session consumes it for Tier 3b later.
- O1 answer: interop from observation is lawful in principle (Directive
  2009/24/EC art. 6, SAS v WPL), but Sandra needs a lawyer's review before
  public release; spec team must not read RustDesk protocol definition files
  until she approves. Public rustdesk.com servers are not a granted path for
  third-party clients; default stays self-hosted or direct IP.
- No Storytold licence allowlist exists in mcd; O3 records the conservative rule
  and per-crate verdicts (`dxgcap` and `mupdf` forbidden, 7-Zip subprocess for
  RAR, PdfCraft CLI for PDF pages).
