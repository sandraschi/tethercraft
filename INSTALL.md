# Installing TetherCraft

TetherCraft is a single-binary pure-Rust remote desktop app (GUI + CLI + host + relay server), with a companion `tethercraft-mcp` binary for AI agent integration.

> **Current release status (2026-11-28):** `v0.9.0` is an unsigned **pre-release for lab use only**. No NSIS installer is published yet; download the plain executable or build from source.

## Option A: Pre-release binary (Windows x64)

1. Download **`tethercraft.exe`** from the [`v0.9.0` pre-release](https://github.com/sandraschi/tethercraft/releases/tag/v0.9.0).
2. Place it in a folder on your machine (for example `C:\Tools\TetherCraft\tethercraft.exe`).
3. Run `tethercraft.exe`. Windows SmartScreen may warn because the pre-release binary is unsigned: choose **More info > Run anyway**.
4. On first launch, complete the mandatory 20-second R1 onboarding check.

## Option B: Build from source (Windows, current `main`)

Prerequisites:
- [Rust toolchain](https://rustup.rs/) 1.78+ (MSVC target on Windows)
- Git

```powershell
git clone https://github.com/sandraschi/tethercraft.git
cd tethercraft
.\start.bat
```

`start.bat` (or `.\start.ps1`) builds the workspace and opens the TetherCraft GUI window. No external codec downloads are needed: `openh264` compiles statically from vendored C++ source.

## Installing the Windows Host Service (optional, admin)

To run a host persistently across reboots (config-driven, grants nothing until an admin edits `%PROGRAMDATA%\TetherCraft\host.json`):

```powershell
tethercraft service install
# Edit %PROGRAMDATA%\TetherCraft\host.json (set password_file, grants, optional relay)
tethercraft service start
```

Manage or remove at any time:

```powershell
tethercraft service status
tethercraft service stop
tethercraft service uninstall
```

There is no silent install and no hidden tray mode.

## MCP Server (optional, for AI agents)

Build and point your MCP client at the stdio server binary:

```powershell
cargo build --release -p tethercraft-mcp
# Binary: target\release\tethercraft-mcp.exe
```

Read tools are open; mutating tools require `confirm=true` and passwords come from `password_file` (never command-line arguments).

## Uninstall

1. If you installed the host service, run `tethercraft service uninstall` from an elevated terminal.
2. Delete `tethercraft.exe` (or the cloned repo folder).
3. Optionally remove local state and audit logs in `%LOCALAPPDATA%\tethercraft\` and `%PROGRAMDATA%\TetherCraft\`.
