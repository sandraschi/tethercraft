# Troubleshooting

## Wrong password

The host rate-limits logins: wait and retry. Check the password file for a
trailing newline (trimmed automatically) and confirm the peer name. The
password never appears on the command line or in logs.

## Tampered key refused

The host reinstalled (new identity) or something is feeding a different key.
Verify the fingerprint words with the host owner out of band, then re-add
with `book add` if the reinstall is legitimate. Relay dials never do TOFU:
connect directly once first to pin the key.

## Relay never pairs

The host must be awaiting with the same server address the controller
bridges to (`--via` on both sides), and the controller needs the pinned key.
`server` must be reachable from both ends. Until end-to-end encryption
lands, self-host the relay: the operator can read paired sessions.

## Service fails to start

Install and start need elevation. The `password_file` path in `host.json`
must be readable by LocalSystem. Check the JSON parses
(`service install` validates it). `service-run --config PATH` in a console
runs the same loop in the foreground for debugging.

## Empty board

Add a peer: `tethercraft book add <peer> <host-key-hex> --addr host:port`.
A card with no address needs `--addr`; re-add it. Stranger-tier peers stay
restricted (no files, terminal, tunnel, power) until promoted.

## Dev issues

`cargo` not found: open a fresh shell after installing Rust (rustup), or
check `%USERPROFILE%\.cargo\bin` is on PATH. A test that touches sockets
and fails in CI-only style (`exit(111)` paths) means loopback is blocked:
the test bails instead of failing. `start.ps1` build failures print the
cargo error inline; `-NoBuild` runs the last build as-is.
