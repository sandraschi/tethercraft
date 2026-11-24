---
updated: 2026-11-24T16:30:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier3, ui]
---

## What's happening right now

Tier 3 is started and the controller UI exists: rendezvous + relay server
tested over loopback TCP, and `tethercraft gui` opens a real egui window with
peer sidebar, session view, End session button, zoom, audit tail, and the R1
gate. 73 tests pass; clippy and fmt are clean.

## Last concrete action

Rebuilt `tethercraft-server` as a real loopback TCP service (register,
heartbeat, lookup, relay mailbox routing of opaque hex frames; relay never
parses payloads) plus NAT candidate ordering, with loopback tests that caught
and fixed a real bug (accepted sockets inheriting nonblocking mode on
Windows). Added `crates/tethercraft-app/src/ui.rs` (ControllerUi, R1 modal
with 20-second wait, persisted `onboarding.txt` marker, zoom clamp, bounded
audit) and `tethercraft gui` / `tethercraft server` CLI verbs. No RustDesk
source opened.

## Next step

Weekend verification by agy over Tiers 1-3 (fresh clone, tests, clippy, fmt,
README matrix, scammer-scenario review), then the two-machine WAN run and the
live frame path into the GUI session view.

## Blockers

Second Windows machine and Tailscale path for the live run. Linux/macOS
hardware for Tier 3 platform work. The Windows host service (SYSTEM plus
secure-desktop helper, ConPTY backend, WGC wiring, OS sensors) is still the
owner-session remainder.

## Context the next agent needs

- Honest Partial inventory, Tier 3: rendezvous/relay protocol and routing are
  real over loopback; UDP hole punching across NATs, WebSocket transport, and
  all-codecs work are NotImplemented; Linux/macOS need hardware nobody has
  this week. The GUI renders synthetic frames until the live capture path
  lands; all gates, zoom, End session, and the R1 modal are real.
- Test count 73 (>= 50 Tier 2 gate holds). libFuzzer harnesses still pending;
  malformed-input tests cover the same parsers.
- Clean room holds. O1/O2/O3 state unchanged.
- Commit discipline held: max 10 files per commit, LF, no em dash.
