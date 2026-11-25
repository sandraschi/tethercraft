---
updated: 2026-11-25T01:30:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, live-session, cross-os, h264]
---

## What's happening right now

H.264 runs over the wire. Real 1080p WGC frames encode to 14.5 KB each instead
of 8.3 MB raw (570x), with zero fallbacks over 10 measured frames. 103 tests
pass; clippy and fmt are clean. Pinned-key enforcement and host identity
persistence are the next security items before any untrusted-network use.

## Last concrete action

Added `tethercraft-codec::h264` on Cisco's OpenH264 prebuilt (crate resolved
0.9.10, BSD-2-Clause, binary downloaded at build time), a `FrameH264` envelope
kind mapped to no-permission (like `Frame`), `--codec raw|h264` (default h264)
with per-frame fallback accounting in the report, `StreamDecoder` for both live
paths, and `book promote/demote` plus `store.set_tier`/`find_by_key` groundwork
for the TOFU policy already live in `serve` (`--pin-key`, auto-record). Measured
loopback and recorded the table in `docs/BENCH.md`; decoded output verified as
real pixels. No RustDesk source opened.

## Next step

Pinned-key enforcement plus host identity persistence (the two open security
gaps), then the VirtualBox Windows guest or a second PC for real network
numbers. The web client stays Planned.

## Blockers

Second Windows machine for Tailscale and real LAN/WAN latency. Windows guest
`win11-automated` still needs one interactive login before guest control works.
D: is tight; delete build trees before large builds.

## Context the next agent needs

- Trust today: SPAKE2 password plus printed fingerprint words, plus TOFU key
  recording with stored tiers, plus optional `--pin-key` strict mode. The pinned
  key is NOT yet enforced by default on every session.
- Frames: H.264 NALs in `FrameH264`, raw fallback counted, never silent. The
  probe is raw-only and says so when the host streams H.264.
- VM state: `win11-automated` at `ready-1080p-2026-10-10`, powered off, no extra
  snapshots, no NAT rules. `ubu-2404-lts` powered off, test files removed.
- Codec licence: O3 updated with exact versions (openh264 0.9.10, windows-capture
  1.5.0, enigo 0.2.1, all permissive, no self-build).
- Clean room holds. Max 10 files per commit, LF, no em dash.
