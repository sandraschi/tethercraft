# Configuration

## Passwords

Session verbs need a password via `--password-file PATH` (or `-` for stdin
in interactive CLI use, never over MCP). The file holds the raw password;
a trailing newline is trimmed. Empty password is refused. The service reads
`password_file` from its JSON config instead.

## Identity and store

| Item | Location |
|---|---|
| Address book + audit (`store.sqlite3`) | `%LOCALAPPDATA%\tethercraft\` |
| Host identity key (`host_identity.key`) | Next to the store by default, `--identity-file` overrides |
| R1 onboarding marker (`onboarding.txt`) | Next to the store |
| Service config (`host.json`) | `%PROGRAMDATA%\TetherCraft\` |
| GUI peer board | Reads the store on launch; cards show id, alias, addr, tier, key state |

First host run generates an identity and prints fingerprint words; first
controller connect records the host key loudly (TOFU). Reinstalls change the
key: verify out of band, then re-add with `book add`.

## Ports

| Use | Default |
|---|---|
| Host sessions | 21120 (`--port`), deliberately outside the RustDesk 21115-21119 range |
| Rendezvous + relay server | 21116 (`server --port`) |
| Tunnel local ends | Whatever `local-port` the `tunnel` verb is given (loopback only) |

Ports are flags, never hardcoded. Direct LAN or Tailscale is the default;
`--via SERVER` dials or awaits through the relay instead.

## `host.json` (Windows service)

```json
{
  "port": 21120,
  "grants": "keyboard,clipboard",
  "fps": 10,
  "fake_frames": false,
  "codec": "h264",
  "pin_key": null,
  "identity_file": null,
  "share": null,
  "password_file": "C:\\ProgramData\\TetherCraft\\pw.txt",
  "via": null
}
```

Only `password_file` is required. `service install` writes this template
when missing (grants empty: nothing allowed until the admin edits it) and
registers `service-run`; `start`, `stop`, `uninstall` manage it. The service
runs as LocalSystem on demand: the password file must be readable by
LocalSystem. `via` holds a rendezvous address for NAT relay mode.
