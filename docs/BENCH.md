# BENCH

Tier 1 exit wants two real Windows machines over Tailscale with measured latency.
This file records what has actually been measured, on which machine, so nothing
here is estimated.

## 2026-11-24, Goliath, loopback (single machine)

First live session, native TCP transport, both ends on the same box. This proves
the software path (handshake, capture, encode, wire, decode, input) but says
nothing about network latency yet.

| Path | Measured | Notes |
|---|---|---|
| Handshake (hello, welcome, SPAKE2, key confirm, session start) | under 100 ms | 3 loopback runs, all succeeded first try |
| Synthetic frames 640x360 RGBA, pump 30 fps, 60 frames requested | 60 frames in 2848 ms = 21.1 fps | per-frame round trip dominates; frame is sent raw RGBA, no codec yet |
| Real WGC capture, primary monitor 1920x1080 | 1 frame, 6220817 byte PPM written | `tethercraft-capture::live_frame`, WGC on Goliath |
| Input injection, granted keyboard | inputs 1, denied 0 | zero-delta scroll through enigo, no pointer movement |
| Input injection, no grants | inputs 0, denied 1 | audit: "stranger peer ... lacks Keyboard" |
| Wrong password | rejected host-side, audit "password proof failed" | client gets "host closed the session: wrong password, or the host refused" |
| Stray local prober (FOO / HTTP/1.1 to any new listener) | rejected cleanly, audited | not our traffic: reproducible against a bare PowerShell listener |

Raw RGBA dominates the frame numbers: 640x360x4 is 921 KB per frame. A codec
(VP9 or hardware H.264) is the next real win, and it is what makes frame rate
meaningful. Until then these numbers are a floor, not a target.

## Pending

| Path | Why pending |
|---|---|
| Goliath to a VirtualBox guest over host-only or bridged LAN | VM not set up yet |
| Goliath to a second PC over Tailscale, glass-to-glass | needs the second machine |
| UDP hole punching across two networks | needs the second network |
| Codec-backed frame rate and bitrate | codec crate not wired yet |
