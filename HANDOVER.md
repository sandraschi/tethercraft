---
updated: 2026-11-26T15:50:00+01:00
agent: opencode-musespark
status: in-progress
tags: [tethercraft, rust, remote-desktop, release, v0.9.0]
---

## What's happening right now

v0.9.0 is released as a pre-release for lab use only: tag pushed, GitHub
release published with the plain `tethercraft.exe` asset, release notes stating
the scope and the gap list honestly. 106 tests green at the tag commit.

## Last concrete action

Added `net::controller_handshake` expected-key check (mismatch is `Denied`),
`store.set_pubkey`/`set_tier`/`find_by_key`/`remove_peer`, `book add` with an
optional key, `book promote/demote/remove`, `host --identity-file` (default
next to the store) with `load_or_create_identity`, host-side TOFU auto-record
with stored-tier application, and `--pin-key` strict mode. Verified live on
Goliath: keyless peer records with a loud out-of-band warning, second connect
is silent, tampered key is refused, identical host id across restarts, wrong
password still denied and rate-limited. Cleaned all test peers from the real
address book (now 0 peers) and restored the store backup note below.
Cut v0.9.0 on Sandra's go: workspace version bumped to match the changelog
(binary reports it), all gates re-run green, smoke-tested the exact artifact
(TOFU record with warning, quiet second connect, tampered key refused,
identical host id across restarts), tag pushed, GitHub pre-release published
with `tethercraft.exe` and honest lab-only notes.

## Next step

Second PC for Tailscale and real WAN/LAN numbers, then the codec re-measure
there. The remaining security item is the GUI path: `connect_live` takes the
pin but the window flow has no TOFU-notice surface yet.

## Blockers

Second physical PC. Windows guest `win11-automated` still needs one interactive
login before guest control works. D: is tight; build trees were cleaned again
after the release build.

## Context the next agent needs

- Trust model now: password plus pinned host key (TOFU with loud first-contact
  warning), stored tiers survive reconnects, `--pin-key` for strict single-key
  hosts, `book remove` forgets. Host keys that were recorded before this change
  do not exist; every peer re-records on next connect.
- Verified numbers: H.264 14.5 KB/frame vs raw 8.3 MB (570x), 642x360 loopback
  at 21 fps synthetic, 1080p real capture at ~1.6 fps (capture-bound one-shot
  WGC, not transport).
- VM state: `win11-automated` at `ready-1080p-2026-10-10`, powered off.
  `ubu-2404-lts` powered off, test files removed.
- Clean room holds. O3 unchanged (verified versions already recorded).
- Commit discipline held: max 10 files per commit, LF, no em dash.
- Pre-existing test store: `%LOCALAPPDATA%\tethercraft\store.sqlite3.testbak`
  is the backup from the Tier 1 session; the live store is intentionally empty.
