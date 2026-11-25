# Changelog

## 0.8.0 - 2026-11-25

- H.264 software codec over the wire: `tethercraft-codec::h264` on Cisco's
  OpenH264 prebuilt (BSD crate, royalty-covered binary, no self-build),
  `FrameH264` envelope kind, `--codec raw|h264` (default h264) with per-frame
  fallback accounting, decode on both live paths (GUI and headless).
- Measured loopback, real 1080p WGC: 145,503 bytes for 10 frames vs 82,944,080
  raw (570x). Decoded output verified as real pixels, not flat.
- 103 tests green; clippy and fmt clean.

## 0.7.0 - 2026-11-25

- Cross-OS session verified: static Linux controller (`tethercraft-probe`,
  x86_64-musl) in an Ubuntu 24.04 VirtualBox guest drove a Windows host
  through NAT. Dial 2 ms, handshake 4 ms, synthetic 640x360 at 9.1 fps,
  real 1920x1080 capture, input applied, wrong password refused both ends.
- `tethercraft-probe`: GUI-free controller for cross-OS checks.
- `docs/BENCH.md` records the numbers and why raw RGBA is the current limit.
- 98 tests green; clippy and fmt clean.

## 0.6.0 - 2026-11-24

- **Live sessions work on one machine.** Native TCP transport with length
  prefixed envelopes, SPAKE2 handshake with key confirmation, fingerprint words
  on both ends, host policy checked per message, audit written, graceful end.
- Real Graphics Capture (WGC) on Windows: primary monitor frame as RGBA, used by
  `tethercraft host` without `--fake-frames`.
- Real input injection via enigo: pointer, click, scroll, keys, touch gestures.
- CLI: `host`, `serve`, live `connect` (GUI or `--headless`), `book add --addr`.
- Fixed a real bug found by the live run: a timed-out read lost part of a length
  prefix and desynced the stream, which looked like a corrupt peer. Reads are now
  blocking on dedicated threads; the unsafe helper is documented as unusable.
- Measured numbers in `docs/BENCH.md` (loopback only so far).
- 98 tests green; clippy and fmt clean.

## 0.5.0 - 2026-11-24

- Tier 5 started: touch input kinds with host permission mapping and range
  validation, controller-only role enforcement (the iOS rule, on every
  platform), fixed gesture map, frame pacing and battery caps, Android host
  permission state machine, `tethercraft platform` verb. Same native protocol:
  a mobile controller already speaks the same envelopes.
- 92 tests green; clippy and fmt clean. Device builds need real hardware.

## 0.4.0 - 2026-11-24

- Tier 4 started: `.tetherrec` session recorder with validated playback,
  privacy mode state machine plus RGBA blanking, monitor pick and virtual
  display request types, camera session state with frame validation, voice
  call state machine with PCM bounds, print spool queue, signed update check
  (signature first, then shape, then no-downgrade), `recordings list` and
  `update-check` CLI verbs, PrivacyRequest/PrivacyEnd wire kinds.
- 87 tests green; clippy and fmt clean. OS backends (camera device, spooler,
  virtual display driver, audio duplex, update download) still pending.

## 0.3.0 - 2026-11-24

- Tier 3 started: rendezvous + relay server over loopback TCP (register,
  heartbeat, lookup, relay mailbox routing of opaque frames), NAT candidate
  ordering, `tethercraft server` verb, and the egui controller window
  (`tethercraft gui`: peer sidebar, session view, End session, Ctrl+wheel
  zoom, audit tail, R1 gate with 20-second wait and persisted answer).
- 73 tests green; clippy and fmt clean. Live WAN run and Linux/macOS still
  pending (no such machines this week).

## 0.2.0 - 2026-11-24

- Tier 2 domain logic: Tier 2 wire kinds, host file manager (root-jailed),
  terminal registry, host-side agent sessions (view-only default, rate limit,
  kill switch), R4 scam shield engine, folder sync planner with local apply,
  sealed deploy configs (Argon2id plus XChaCha20-Poly1305, ed25519-signed),
  R3 promotion tokens and cooling, R5 risk scorer, R6 evidence bundle and peer
  blocklist, chat log and notes, CLI files/sync/broadcast/collect/batch/
  restart/shutdown/lock/exec/tunnel verbs, MCP confirm-gated write tools.
- 64 tests green; clippy and fmt clean. Live host service run still pending.

## 0.1.0 - 2026-11-24

- Tier 0 scaffold: 11-crate workspace, deny lints, CI, threat model doc, native proto schema.
- Tier 1 start: keys, fingerprint words, SPAKE2 handshake state, host policy (R2), onboarding state (R1/R7), address book and audit store, CLI with --json, MCP read tools.
