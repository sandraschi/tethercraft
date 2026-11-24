---
updated: 2026-11-24T17:30:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier4]
---

## What's happening right now

Tier 4 is started: recording, privacy mode, monitors and virtual display
types, camera, voice call, print spooler, and the signed update check are
implemented as real domain code with tests. 87 tests pass; clippy and fmt
are clean.

## Last concrete action

Added `codec::rec` (`.tetherrec` recorder with validated playback),
`host::privacy` (request/approve/deny state machine plus RGBA blanking),
`host::printer` (spool queue with states and caps), `host::camera`
(session state plus frame validation), `host::voice` (ring/accept/mute/
hangup plus PCM bounds), monitor pick and virtual display requests in
`capture`, `net::update` (sign/verify with no-downgrade rule), `recordings
list` and `update-check` CLI verbs, and PrivacyRequest/PrivacyEnd wire kinds
(host denies them for strangers). No RustDesk source opened.

## Next step

Weekend verification by agy over Tiers 1-4 (fresh clone, tests, clippy, fmt,
README matrix, scammer-scenario review), then the two-machine WAN run and the
live frame path into the GUI session view.

## Blockers

Second Windows machine and Tailscale path for the live run. Linux/macOS
hardware for Tier 3 platform work. OS backends pending: camera device,
Win32 spooler, virtual display driver, audio duplex, update download, host
service (SYSTEM plus secure-desktop helper), ConPTY, WGC capture, OS sensors.

## Context the next agent needs

- Honest Partial inventory, Tier 4: recorder format and playback, privacy
  engine and blanking, spool queue, camera/voice state and validation, update
  verify-or-refuse are fully real; OS device backends and the update download
  return `NotImplemented`. Web client stays Planned (needs a new handover).
- Test count 87. libFuzzer harnesses still pending; malformed-input tests
  cover the same parsers.
- Clean room holds. O1/O2/O3 state unchanged.
- Commit discipline held: max 10 files per commit, LF, no em dash.
