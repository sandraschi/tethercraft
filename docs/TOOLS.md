# CLI and MCP tools reference

Running `tethercraft` with no subcommand opens the controller window
(same as `tethercraft gui`): that is what double-click, `start.bat`, and
shortcuts execute. Clicking Connect (or double-clicking a peer card) opens the
in-window dial modal with masked password prompt, optional `--via` relay, and
in-window TOFU banner on first contact.

Every verb supports `--json` and the global `--password-file` (plus `--via`
for relay dialing on session verbs). Passwords never go on the command line.
Session verbs dial through `live_session` with SPAKE2 PAKE + ChaCha20-Poly1305
E2E encryption (including over `--via` relays): TOFU on first contact (loud
warning, verify the fingerprint words out of band), pinned key after that,
`--via` requires a known key.

## Address book and status

| Verb | What |
|---|---|
| `status` | Daemon and onboarding state |
| `health [peer] [--watch ollama,docker,opencode,antigravity,cursor]` | Structured "all is well" check (CPU, RAM, NVIDIA GPU, disks, watched apps); runs locally (for DevCraft/scripts) or remotely over E2E session |
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

## MCP server tools (`tethercraft-mcp`, stdio JSON-RPC)

Read tools are open. Every mutating tool requires `confirm: true`, is enforced
per-message on the host, and is written to the local audit log. Passwords come
from `password_file` (never inline params).

| Tool | Access | Notes |
|---|---|---|
| `status` | read-only | Tier, E2E cipher (`chacha20-poly1305`), peer count, open session count |
| `capabilities` | read-only | Supported vs. unsupported use cases, R1-R7 invariants, E2E transport summary |
| `health` | read-only | Host "all is well" check (CPU, RAM, GPU, disks, ollama/docker/opencode/agy/cursor); local if `session` omitted, or remote over E2E session |
| `peers`, `peer_info` | read-only | Address book rows (id, alias, addr, tier, key state) |
| `sessions` | read-only | Currently open live session peer ids |
| `audit` | read-only | Local audit log tail (`limit`, default 50, max 500) |
| `connect` | `confirm: true` | Dial a peer via SPAKE2 + E2E (`password_file`, optional `via`), hold live session |
| `disconnect` | open | Send `SessionEnd` and close a live session |
| `screenshot` | `confirm: true` | Save one decoded frame to a local path |
| `input` | `confirm: true` | Inject permission-gated input event |
| `exec` | `confirm: true` | Run a command over the terminal channel (30 s kill, 64 KiB cap) |
| `file_list`, `file_get`, `file_put`, `file_rename`, `file_rm`, `file_mkdir`, `file_hash` | `confirm: true` | Jailed share-root file operations |
| `chat_send` | `confirm: true` | Send session chat text |
| `tunnel_open`, `tunnel_send`, `tunnel_close` | `confirm: true` | TCP tunnel channels over the E2E session |
| `restart`, `shutdown`, `lock` | `confirm: true` | Host power controls (permission-gated) |
