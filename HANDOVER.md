---
updated: 2026-11-25T00:10:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, live-session, cross-os]
---

## What's happening right now

Sessions work across two operating systems: a static Linux controller
(`tethercraft-probe`) in the Ubuntu VirtualBox guest drove the Windows host
through NAT, with real WGC capture and input applied. 98 tests pass, clippy and
fmt are clean. The Windows guest route is blocked by guest-control permissions,
not by the app.

## Last concrete action

Added `crates/tethercraft-probe` (no GUI, capture, store or codec deps),
cross-compiled it to `x86_64-unknown-linux-musl`, copied it into `ubu-2404-lts`
with guestcontrol, and ran three checks against `tethercraft host` on Goliath:
synthetic frames, real 1920x1080 WGC capture, wrong password. Measured numbers
are in `docs/BENCH.md`; the recipe is `docs/VMTEST.md`. Tried the Windows guest
first: guest control refuses the `user` account ("restricted") after the first
successful command, so it was restored to baseline and left powered off.

Housekeeping: `D:` reached zero free space during the musl build. Deleted
`target/debug` and the musl target directory (3.7 GB); the release binary was
kept. D: has little headroom, so run builds knowing that.

## Next step

Wire a codec. The measurements say the bottleneck is raw RGBA (8.3 MB per
1080p frame): VP9 via libvpx or hardware H.264 through Windows Media Foundation,
behind the existing `tethercraft-codec` interface, with the frame path re-tested
on the same Linux probe.

## Blockers

- Windows VM test needs one interactive login in `win11-automated` before guest
  control will accept the account.
- Second physical PC still needed for Tailscale and real LAN/WAN latency.
- D: is nearly full; delete build trees before large builds.

## Context the next agent needs

- Trust is password plus printed fingerprint words. The pinned peer key is
  collected but NOT enforced yet. Do not use this over an untrusted network
  until that check exists.
- Frames are raw RGBA in a length-prefixed envelope; `tethercraft-probe --json`
  prints dial, handshake and frame timings for comparisons.
- Host identity is ephemeral per `host` run (`id` and `words` printed at start).
- VM state: `win11-automated` at `ready-1080p-2026-10-10`, powered off, no extra
  snapshots, NAT rules removed. `ubu-2404-lts` powered off, test files deleted
  from `/home/user`, snapshot list untouched.
- Clean room holds. O1/O2/O3 unchanged. Max 10 files per commit, LF, no em dash.

