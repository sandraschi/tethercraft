---
updated: 2026-11-28T02:18:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, tier2, relay, service, mcp, gui-board]
---

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
