---
updated: 2026-11-24T22:30:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, live-session]
---

## What's happening right now

Real sessions work on a single machine. `tethercraft host` serves, `tethercraft
connect` dials, the SPAKE2 handshake completes, WGC frames arrive, and input is
injected only when the host granted the permission. 98 tests pass; clippy and
fmt are clean. What is still missing before a two-machine demo is the network
run and a codec.

## Last concrete action

Built the live milestone end to end and verified it on Goliath:

- `net::session`: TCP transport, length-prefixed envelopes, `HostHandshake`
  params struct, SPAKE2 with key confirmation, fingerprint words both ways,
  frame codec (RGBA), PPM writer, headless controller.
- `capture::wgc`: real Windows Graphics Capture, primary monitor, BGRA to RGBA,
  one-shot with timeout. Verified: 1920x1080 frame, 6220817 byte PPM.
- `host::inject`: enigo injection for pointer, click, scroll, keys, touch.
- `host::serve`: accept, handshake, refuse blocked peers, pump latest frame,
  enforce the permission per message, audit everything, session cap.
- CLI `host`, `serve`, live `connect` (GUI or headless), `book add --addr`.
- Real measurements in `docs/BENCH.md`: 60 synthetic 640x360 frames in 2848 ms
  (21.1 fps) loopback; input applied when granted, denied and audited when not;
  wrong password rejected and rate limited.

Bugs found and fixed by running it, not by reading it:

1. A read timeout mid-message consumed part of a length prefix, desyncing the
   stream; the next parse saw garbage lengths. Reads now block on a dedicated
   thread and the timeout helper is documented as unsafe for sessions.
2. Accepted sockets inherited the listener's non-blocking mode on Windows
   (found earlier with the relay), which is why the reader is explicit now.
3. An external prober on this machine sends `FOO / HTTP/1.1` to every new
   listening port. The host rejects it cleanly and audits it. Reproduced against
   a bare PowerShell listener, so it is not our traffic and not a bug.

## Next step

VirtualBox guest via vmcraft: same binary, `--addr` pointing at the guest, run
the three checks (fake frames, real frames, input grant and denial) and record
the latency in `docs/BENCH.md`. Then the codec, which is the only thing that
makes frame rate meaningful.

## Blockers

None for the VM run. Second physical PC still needed for Tailscale and the
two-network hole-punch numbers.

## Context the next agent needs

- There is no codec yet: frames are raw RGBA. 640x360 is 921 KB per frame, so
  21 fps on loopback is a bandwidth number, not a rendering limit.
- The host identity is ephemeral per `host` run (`id` and `words` printed at
  start). Peer identity is not persisted yet, so `book add` needs the current
  run's id, or any placeholder id: the handshake trusts the password plus the
  printed words, and verification of the pinned key is not enforced yet. That
  is the next security gap to close before real use, and it is why this is
  loopback-tested only.
- Test stores: the peers added while testing were backed up to
  `%LOCALAPPDATA%\tethercraft\store.sqlite3.testbak` and the store reset.
- Clean room holds. O1/O2/O3 state unchanged. Max 10 files per commit, LF, no
  em dash.
