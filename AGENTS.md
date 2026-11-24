---
title: "AGENTS.md - TetherCraft"
category: repo
status: active
audience: all agents
last_updated: 2026-11-24
---

# TetherCraft - agent rules

Pure-Rust ArtCraft workspace (Tier 0 + Tier 1, session B). Native protocol only.
Windows only in Tier 1. No relay server, no compat, no Linux or macOS host work.

## Hard rules

- LF only (`* text=auto eol=lf`). Single trunk `main`. Max 10 files per commit.
- Never commit build output (`target/`, `dist/`, `*.exe`, `*.zip`, `*.mcpb`, `*.pdb`, `*.bak`, `.env`).
- Deny `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` in every crate.
  Unbuilt paths return `TetherError::NotImplemented`, never a panic and never fake success.
- No em dash character anywhere (code, docs, commits, chat). Use hyphen - only.
- Clean room: never open, search, fetch or quote any RustDesk source tree
  (`github.com/rustdesk/*`, `D:\Dev\repos\rustdesk`, `D:\Dev\repos\rustdesk-server`),
  deepwiki RustDesk code pages, or `.proto` files. Public user docs
  (rustdesk.com/docs) are allowed. Interop spec lives in
  `mcp-central-docs/projects/remotecraft/spec/` and is written by a different session.
- Scam resistance is mandatory: R1 onboarding gate, R2 stranger tier on host,
  R7 no onboarding-free consumer builds. Never add a silent-install, hidden-tray,
  or screen-hiding option to consumer builds.
- Never put passwords on the command line or in logs. Keyring, prompt, or file
  descriptor only.
- Proof by running: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`. README feature matrix stays honest.

## Layout

Workspace crates, all prefixed `tethercraft-`: `core`, `proto`, `net`, `capture`,
`codec`, `input`, `host`, `server`, `store`, `mcp`, `app`. L1 is core+store,
L2 is mcp binary plus app CLI, L3 is the egui controller in app.

## What Tier 1 covers

Windows to Windows over LAN and Tailscale: Ed25519 device keys, fingerprint words,
SPAKE2 PAKE, QUIC with pinned raw keys, WGC capture with DXGI fallback, one hardware
encoder plus VP9 software fallback, view/control and view-only, host-enforced
permissions, clipboard text and images, OTP and permanent password, address book,
WOL, screenshot, audit log, CLI verbs with `--json`, MCP read tools.
