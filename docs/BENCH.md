# BENCH

Tier 1 exit wants two real Windows machines over Tailscale with measured latency.
This file records what has actually been measured, on which machine, so nothing
here is estimated.

## 2026-11-24, Goliath to Ubuntu 24.04 VirtualBox guest (two operating systems, one physical machine)

Guest: `ubu-2404-lts`, 4 vCPU, NAT (guest reaches the host at 10.0.2.2).
Controller: `tethercraft-probe`, a static x86_64-musl build of the same
`tethercraft-net` session code, no GUI. Host: `tethercraft host` on Windows
with real WGC capture. This crosses an OS boundary and a virtual NIC, but not
a physical network.

| Path | Measured | Notes |
|---|---|---|
| Dial (TCP connect through NAT) | 2 ms | 4 runs, all identical |
| Handshake (hello, welcome, SPAKE2, confirm, session start) | 4 ms (synthetic run), 32 ms (real capture run) | the 32 ms includes the host starting its real capture pipeline |
| Synthetic frames 640x360, 10 fps target, 10 frames | 1095 ms total, 9216080 bytes | 9.2 MB in ~1.1 s = 8.4 MB/s, 9.1 fps |
| Real WGC frames 1920x1080, 5 fps target, 5 frames | 4264 ms total, 41472040 bytes | 1.2 fps; raw RGBA at 8.3 MB per frame, so this is bandwidth bound |
| Input with `--grants keyboard` | host applied 1 input | zero-delta scroll through the enigo path |
| Wrong password | host: "password proof failed"; guest: "host closed the session: wrong password, or the host refused" | rate limiter records the failure |
| Stray local prober | rejected and audited | same FOO probe as the loopback runs |

What this run proves: the wire format, handshake and frame path work between
two different operating systems; the host enforces permissions per message
against a remote peer; and the frame limiter is bytes, not CPU. What it does
not prove: latency over a real LAN or WAN, or anything about hole punching.

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
| Goliath to the Windows 11 guest (`win11-automated`) | Guest control refuses the `user` account ("account is restricted"), so nothing can be copied in or started. Host Additions are healthy (7.2.6); needs one interactive login in the VM, then the same recipe works. VM restored to its `ready-1080p-2026-10-10` baseline and left powered off. |
| Goliath to a second PC over Tailscale, glass-to-glass | needs the second machine |
| UDP hole punching across two networks | needs the second network |
| Codec-backed frame rate and bitrate | codec crate not wired yet; raw RGBA is the current bottleneck |
