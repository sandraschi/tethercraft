# CLI tools reference

Every verb supports `--json` and the global `--password-file` (plus `--via`
for relay dialing on session verbs). Passwords never go on the command line.
Session verbs dial through `live_session`: TOFU on first contact (loud
warning, verify the fingerprint words out of band), pinned key after that,
`--via` requires a known key.

## Address book and status

| Verb | What |
|---|---|
| `status` | Daemon and onboarding state |
| `id` | This device id plus fingerprint words |
| `peers` | Address book rows (id, alias, addr, tier, key state) |
| `peer-info <peer>` | One row |
| `book add <peer> <key-hex> --addr host:port [--alias]` | Add or pin a peer (`--pin-key` style strictness per peer) |
| `book rm <peer>` | Forget a peer |
| `book promote|demote <peer>` | Move between stranger, trusted_helper, managed |
| `audit [--peer]` | Audit log tail |

## Live session verbs (permission-gated on the host)

| Verb | Permission | Notes |
|---|---|---|
| `connect <peer> [--view-only] [--headless out.ppm]` | view | GUI window, or one frame headless |
| `serve <peer> [--out f] [--frames n] [--input-probe]` | view | Headless capture run |
| `screenshot <peer> --out f` | view | One PNG frame to a file |
| `files ls|get|put|mv|rm|mkdir` | file | Paths are share-root-relative |
| `sync <peer> <local> <remote> [--direction] [--conflict] [--dry-run] [--delete --confirm]` | file | Content diff by SHA-256; deletes need both flags |
| `exec <peer> -- <command>` | terminal | 30 s kill, 64 KiB output cap |
| `chat <peer> <text>` | none (session) | Stored in the host session log |
| `tunnel <peer> <local-port:remote-target>` | tunnel | Local TCP forward through the session, Ctrl+C stops |
| `restart|shutdown <peer>` | restart | Delay 5 s default |
| `lock <peer>` | restart | Immediate workstation lock |
| `broadcast-file <peers> <local> <remote>` | file | One file to many peers (comma-separated) |
| `collect-files <peers> <remote-dir> <local-dir>` | file | Pull trees into `<local-dir>/<peer>/` |
| `batch <manifest.json>` | mixed | `{"peer": ..., "ops": [{...exec|put|get|chat}]}` |
| `wake <peer>` | none | WOL needs the LAN run today |

## Hosting and relay

| Verb | Notes |
|---|---|
| `host [--port 21120] [--grants ...] [--fps] [--codec raw|h264] [--share DIR] [--pin-key] [--identity-file] [--fake-frames]` | Direct listen loop. `--via SERVER` awaits relay pairing instead |
| `server [--port 21116]` | Rendezvous + relay mailbox + bridge/claim pairing |
| `service install [--config]`, `start`, `stop`, `uninstall` | Windows SCM host (needs elevation) |
| `gui` | Controller window (Board, Session, Help sections) |
| `platform`, `update-check`, `recordings` | Local info verbs |

`update-check` verifies a metadata signature before reporting; it never
auto-installs.
