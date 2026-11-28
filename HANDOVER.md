---
updated: 2026-11-28T03:10:00+01:00
agent: opencode-musespark
status: in-progress (handed to agy - problem report below)
tags: [tethercraft, rust, remote-desktop, tier2, handover, problem-report]
---

## PROBLEM REPORT FOR AGY (read this first)

User goal: copy RustDesk functionality and framerate. The codec question is
answered, the framerate blockers are ranked, and one self-inflicted incident
needs your review. Tree is green and pushed; nothing is left half-edited.

### 1. The codec crate is fine - no shopping needed

`openh264` 0.9 with default `source` feature statically compiles vendored
OpenH264 C++ into the binary at build time. No network download happens
(the old `h264.rs` comment claiming a build-time download was wrong and is
now fixed). Offline-safe, reproducible, BSD-2-Clause bindings. Alternatives
checked and rejected: `h264-reader` parses but does not decode pixels,
`ffmpeg-next` needs system ffmpeg, `x264` bindings are GPL (license poison
for MIT/Apache). RustDesk-class software H.264 is exactly this crate.

Wiring status: host pump already encodes (`H264Encoder`, reused across
frames, recreated on resolution change) and sends `FrameH264`; controller
`StreamDecoder` already decodes to RGBA; 570x over raw measured
(`docs/BENCH.md`, 14.5 KB vs 8.3 MB). Nothing to "wire" at the codec layer.

### 2. What actually caps framerate, ranked

1. Real capture is one-shot: `live_frame(2s)` grabs a fresh WGC session per
   frame (~400 ms each). Persistent capture session is the real fix and is
   not started. This alone caps real 1080p at ~1-2 fps regardless of codec.
2. No adaptive bitrate/resolution: `H264Encoder::new(w, h)` uses OpenH264
   defaults; `EncoderConfig` (bitrate, usage type ScreenContentRealTime) is
   not exposed. Second after persistent capture.
3. No hardware encoder: `hardware_available()` returns false on all
   platforms (Media Foundation / NVENC not started).
4. The musl probe builds with the `h264` feature off by design (lean,
   dependency-free binary), so cross-OS runs need `--codec raw`. If the
   probe ever needs H.264, that is a deliberate product decision, not a bug.
5. Measured today: loopback synthetic 21.1 fps, NAT raw 7-9 fps
   (bandwidth-bound, 921 KB/frame), real 1080p ~1-2 fps. Full table in
   `docs/BENCH.md`.

### 3. Incident: deadline pacing broke `serve_one_h264_fake_session`

While answering "wire the codec", the previous agent changed the pump from
trailing `sleep(per_frame)` to deadline pacing (`next_tick` grid) in
`spawn_pump` (`crates/tethercraft-host/src/serve.rs`). Result:
`serve_one_h264_fake_session` failed twice in a row, alone, in ~1.04 s,
on `assert!(r.frames_sent >= 3)`. Reverting the hunk (restoring trailing
sleep) makes it pass; the revert is committed and pushed. Cause NOT
established. Do not re-land deadline pacing without answering this.

Facts for the investigation:
- The client side got far enough that the report assert (line ~951) fired
  before the client asserts, so the host loop exited early with <3 sends.
- Prime suspect area: serve-loop `recv_timeout(tick)` + latest-wins
  interaction with the new first-frame timing, or a `frame-send-failed`
  exit. The test never prints `report.ended` or the audit tail - adding
  that output is step one and will likely name the cause in one run.
- Bisect is clean: only that hunk changed behavior; the `h264.rs` comment
  fix in the same commit is unrelated.

### 4. Open threads (yours now)

- Bare `tethercraft` opens the GUI (`unwrap_or(Cmd::Gui)`, tested,
  verified live 8 s on Goliath). `start.ps1`/`start.bat` work.
- GUI in-window dial is not wired (Connect starts the local session;
  password prompt + background handshake missing), same for the TOFU-notice
  surface in the window flow.
- `start.ps1` Floor check is off (Tier 3 workstation chrome not built).
  Honest, documented in `scripts/craft.config.psd1`.
- Relay operator can read sessions until E2E lands: self-host only.
- Windows guest VM still needs one interactive login before guest control
  works; Ubuntu e2e recipe in `docs/VMTEST.md` works as of 2026-11-28.

// earlier log below - still current unless noted above

## What's happening right now

Window has Board, Session, and Help sections now (top tabs): the peer
kanban moved into a `board.rs` module (Ready / First contact / Needs an
address columns, filter, sorted cards, tier badges, Copy ID), the operator
guide lives in `help.rs` with testable ASCII-only topics, and background
sessions drain frames while the user browses. Craft launchers vendored via
the fleet sync script: `start.ps1`/`start.bat` (smoke passes: builds plus
CLI status), `scripts/` configs, xtask, installer files. Floor check is
honestly off (Tier 3 workstation chrome not built); docs stack added
(`docs/` hub, TOOLS, CONFIGURATION, DEVELOPMENT, TROUBLESHOOTING) and the
README/HELP GUI sections updated. Earlier: RustDesk-style peer cards with
search, pinned-key dots, double-click connect.

Tier 2 command bus is live over real sessions on loopback: file transfer
(list/get/put/rename/rm/mkdir/hash), folder sync planner with remote diff,
broadcast/collect/batch, exec with 30 s kill and 64 KiB cap, power
(restart/shutdown/lock), TCP tunnels, chat, screenshots. Relay-first NAT
traversal is in: server `await_bridge`/`bridge`/`check`/`claim` ops with
one-time tokens and raw byte piping (loopback-tested, operator can read
sessions until E2E encryption lands, so self-host only for now), `--via`
dialing on every controller verb plus headless `serve` and GUI connect, host
relay-await mode, and a stoppable await for the Windows service. MCP is
stateful: connect/disconnect/sessions plus confirm-gated exec, files,
tunnel, power, chat and input over live sessions, all audited. Host runs as
a Windows SCM service from `%PROGRAMDATA%\TetherCraft\host.json`
(`service install|start|stop|uninstall`, hidden `service-run`).

## Last concrete action

2026-11-28 e2e with the Ubuntu VM (`ubu-2404-lts`): rebuilt musl probe from
current main, Goliath hosted `--codec raw --fake-frames`, guest ran 10
frames plus input probe over NAT. Dial 2 ms, handshake 45 ms, 10 frames in
1289 ms, words identical both ends, host applied the probe input with 0
denied, frame saved in-guest as valid P6. Numbers appended to `docs/BENCH.md`;
VM powered back off, musl target dir deleted.

Peer board commit on main, all gates green (clippy with `-D warnings`, fmt
check, whole workspace tests: 114 passed): `PeerCard` with
addr plus pinned-key state, search box with result count, card grid with
selection ring, per-card Connect plus double-click to start, empty-book hint
with the `book add` command. README Controller UI section updated.

Kanban/sections/help/docs/launchers on main, all gates green (clippy with
`-D warnings`, fmt check, whole workspace tests: 123 passed,
`start.ps1 -Smoke` ok): `board.rs` (columns, filter, sort, tests),
`help.rs` (6 topics, ASCII and coverage tests), top section tabs with
session gating plus background frame drain, `docs/` stack and README/HELP
updates, vendored craft scaffold with Floor honestly off.

Five commits on main, all gates green (clippy with `-D warnings`, fmt check,
whole workspace tests: 112 passed): live session verbs, relay bridge plus
dial paths and host relay-await with headless `--via`, stateful MCP, Windows
host service with stoppable relay await, and a double-hex file-hash fix
(`hash_bytes` already returns hex) found by a new loopback file roundtrip
test (list/get/hash against a real host with a trusted_helper peer).

## Next step

Two-machine LAN/Tailscale run and the codec re-measure there; relay E2E
encryption design (self-host-only until then); the GUI TOFU-notice surface
(`connect_live` takes the pin but the window flow has no notice yet);
unsigned areas stay honest in the README matrix.

## Blockers

Second physical PC. Windows guest `win11-automated` still needs one
interactive login before guest control works. Relay operator readability is
a known interim weakness, not a blocker for lab use.

## Context the next agent needs

- Trust model: password plus pinned host key (TOFU with loud first-contact
  warning), stored tiers survive reconnects, `--pin-key` for strict hosts,
  `book remove` forgets. File/terminal/tunnel verbs are denied for
  strangers by design; tests promote the peer to trusted_helper first.
- Relay pairing: host `await_bridge` with its pubkey-hex id, controller
  `bridge` to that id, both `claim` the token, server pipes raw bytes.
  Controller relay dials require a known host key (no relay TOFU).
- Service runs as LocalSystem on demand; config template grants nothing
  until the admin edits it. No silent install, no hidden tray.
- MCP mutating tools need `confirm=true`, passwords come from
  `password_file`, sessions are one per peer id.
- Verified numbers unchanged: H.264 14.5 KB/frame vs raw 8.3 MB (570x),
  642x360 loopback at 21 fps synthetic, 1080p real capture at ~1.6 fps
  (capture-bound one-shot WGC, not transport).
- VM state: `win11-automated` at `ready-1080p-2026-10-10`, powered off.
  `ubu-2404-lts` powered off, test files removed.
- Clean room holds. Commit discipline held: max 10 files per commit, LF,
  no em dash, ASCII only.
- Pre-existing test store: `%LOCALAPPDATA%\tethercraft\store.sqlite3.testbak`
  is the backup from the Tier 1 session; the live store is intentionally empty.
