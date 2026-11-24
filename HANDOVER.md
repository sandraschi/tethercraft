---
updated: 2026-11-24T18:15:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier5]
---

## What's happening right now

Tier 5 is started as honest groundwork: touch input, controller-only roles,
gesture map, pacing and battery policy, and the Android host permission flow
are real and tested. 92 tests pass; clippy and fmt are clean.

## Last concrete action

Added `core::mobile` (Role gate, TouchGesture map to Permission::Touch,
frame_interval_ms, battery_fps_cap, AndroidHost state machine,
platform_info), TouchTap/TouchHold/TouchSwipe wire kinds and input events
with range validation, host mapping to Permission::Touch (strangers denied),
and the `tethercraft platform` verb. No RustDesk source opened.

## Next step

Weekend verification by agy over Tiers 1-5 (fresh clone, tests, clippy, fmt,
README matrix), then the two-machine WAN run. Tier 6 Pro-class (web console,
OIDC/LDAP, strategies, device groups) needs a new handover before starting.

## Blockers

Real devices for Tier 5 builds (Android phone, iPhone, NDK/Xcode toolchain).
Second Windows machine and Tailscale path for the live run. Standing OS
backend remainder: host service, ConPTY, WGC capture, OS sensors, camera
device, spooler, virtual display driver, audio duplex, update download.

## Context the next agent needs

- Honest Partial inventory, Tier 5: protocol (same envelopes), touch policy,
  roles, pacing, Android flow order are real; store builds, OS bindings, and
  the web client need hardware and a new handover.
- Test count 92. libFuzzer harnesses still pending.
- Clean room holds. O1/O2/O3 state unchanged.
- Commit discipline held: max 10 files per commit, LF, no em dash.
