# Development

Pure-Rust ArtCraft workspace. Native protocol only, Windows-first; no
webapp, no ports to claim (session ports are runtime flags, see
CONFIGURATION.md). `cargo` must be on PATH (`%USERPROFILE%\.cargo\bin`;
install Rust from https://rustup.rs).

## Layout

`crates/tethercraft-{core,proto,net,capture,codec,input,host,server,store,mcp,app,probe}`.
L1 is core+store, L2 is the MCP server plus CLI verbs in `app`,
L3 is the egui controller window (`app/src/ui.rs`, peer board in
`board.rs`, operator guide in `help.rs`).

## Launcher

`.\start.ps1` (double-click `start.bat`) builds and runs the desktop app so
the window always runs current code. Flags: `-Release`, `-NoBuild`, `-New`,
`-Smoke`, `-Mcp` (stdio server), `-Cli <args>` (e.g. `-Cli status`).
Per-repo settings live in `scripts\craft.config.psd1`. Vendored scaffold
(`start.ps1`, `scripts\craft-*.ps1`, `xtask`, `installer`) comes from
`mcp-central-docs/templates/craft-app`; do not hand-edit the vendored
copies, change the template or the config instead.

## Gates (run all three before every commit)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Denials are workspace-wide: `unwrap_used`, `expect_used`, `panic`, `todo`,
`unimplemented`. Unbuilt paths return `TetherError::NotImplemented`, never a
panic and never fake success. GUI code is covered through data-level tests
(board columns/filter/sort, help topics, section gating); `egui` rendering
itself is exercised by running the window.

## Commit discipline

Single trunk `main`, LF only, max 10 files per commit, ASCII only (no em
dash anywhere: code, docs, commits, chat). Never commit build output
(`target/`, `dist/`, `*.exe`, `*.zip`, `*.mcpb`, `*.pdb`, `*.bak`, `.env`).
Clean room holds: never open, search, or quote RustDesk sources; public
user docs are allowed. Scam resistance is mandatory: no silent-install,
hidden-tray, or screen-hiding options in consumer builds.
