# BENCH

Tier 1 exit wants two real Windows machines over Tailscale with measured latency.
This file records what has actually been measured, on which machine, so nothing
here is estimated.

## 2026-11-28, Goliath, loopback: persistent WGC + tuned OpenH264 (real 1080p)

Host without `--fake-frames` (`tethercraft host --codec h264`), release build,
persistent `CaptureStream` (WGC BGRA8 direct to `H264Encoder::encode_bgra`,
`UsageType::ScreenContentRealTime`, `Complexity::Low`, 4 Mbps target, reused
`YUVBuffer`, bounded `sync_channel(2)` pre-encode drop, deadline pacing).
Measured with `tethercraft serve <peer> --frames N --json` on a 1920x1080 desktop.

| Target | Frames | Wall time (incl. handshake + WGC init) | Effective fps | Wire bytes (total) | Per frame | Fallbacks |
|---|---|---|---|---|---|---|
| 30 fps | 30 | 1708 ms | 17.6 fps | 25,600 | 853 B | 0 |
| 60 fps | 60 | 1846 ms | 32.5 fps | 29,104 | 485 B | 0 |

Real 1080p loopback jumped from ~1.6 fps (one-shot WGC) to 32.5 fps end-to-end
(~20x faster; steady-state after ~400 ms handshake/WGC startup is ~40+ fps)
while keeping zero codec fallbacks and sub-KB average frame size on desktop
content.

## 2026-11-25, Goliath, loopback: H.264 vs raw (real WGC 1080p)

Host without `--fake-frames`, 10 frames each, `--fps 5`. Wall times here are
capture-bound (one WGC grab per frame, roughly 400 ms on this box), not
network-bound: the codec win shows in bytes, which is what a LAN or WAN
would feel.

| Codec | Frames | Wire bytes (8-byte header + payload each) | Per frame |
|---|---|---|---|
| raw RGBA | 10 | 82,944,080 | 8.3 MB |
| H.264 (Cisco OpenH264 prebuilt) | 10 | 145,503, 0 fallbacks | 14.5 KB |

That is a 570x reduction on a real desktop. The decoded frame saved as PPM is
full 1920x1080 with sensible pixel statistics (mean 17, variance 878 on a dark
desktop: real content, not flat). Wall time was similar either way (5.6 s raw,
6.4 s H.264) because WGC one-shot grabs dominate; a persistent capture
session is the next capture-side win, the codec was the transport-side one.

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

## 2026-11-28, Goliath to Ubuntu 24.04 VirtualBox guest (repeat, synthetic raw)

Same recipe as 11-24 (`ubu-2404-lts` at `clean-desktop-2026-11-22`, NAT via
10.0.2.2, musl probe rebuilt from current main). Host: `tethercraft host
--port 21150 --grants keyboard,file --codec raw --fake-frames`. Purpose: prove
current main still crosses the OS boundary after the relay/MCP/service work.

| Path | Measured | Notes |
|---|---|---|
| Dial (TCP connect through NAT) | 2 ms | identical to 11-24 |
| Handshake (hello, welcome, SPAKE2, confirm, session start) | 45 ms | synthetic run |
| Synthetic frames 640x360 raw, 10 fps target, 10 frames | 1289 ms total, 9216080 bytes | 9.2 MB in ~1.3 s = 7.1 MB/s, 7.8 fps |
| Fingerprint words over the wire | `echidna kestrel lumen trail zephyr eagle wolf garnet`, identical both ends | TOFU comparison held across OSes |
| Input with `--grants keyboard` (`--input-probe`) | host applied 1, denied 0 | zero-delta scroll through the inject path |
| Saved frame `/home/user/live.ppm` | 691215 bytes, valid P6 640x360 header | kept in the guest for the run, not exported |
| Host session report | `session with linux-probe: frames 10 (9216080 bytes), inputs 1, denied 0, ended peer-disconnected` | clean shutdown from the controller side |

Musl probe binary was ~1 MB static; the musl target dir was deleted after the
build (D: space) and the VM powered back off. No new bottleneck found: the
frame rate is still raw-RGBA bandwidth bound, same as 11-24.

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
