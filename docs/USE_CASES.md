# TetherCraft Use Cases (Supported vs. Not Supported)

TetherCraft is deliberately opinionated: it optimizes for people operating their
own machines, supporting family safely, and running audited CLI/MCP automation,
while intentionally refusing patterns that turn remote-control tools into scam
or surveillance weapons.

---

## 1. Use cases we support and design for

### A. Own machines over LAN or Tailscale / WireGuard
- Reach your workstation, home server, or secondary Windows PC without a vendor cloud account, subscription, or commercial-use nag timers.
- Direct TCP with Ed25519 key pinning, SPAKE2 PAKE, and ChaCha20-Poly1305 E2E encryption.
- Persistent WGC 1080p capture + vendored OpenH264 (`ScreenContentRealTime`, 32.5 fps measured on loopback; hardware MFT/NVENC planned next).

### B. Family IT support (anti-scam by default)
- Help parents or relatives without installing a tool that a phone scammer can later hijack.
- First contact starts in the **Stranger** tier (view-only, 30-minute hard cap, no files/terminal/blanking, local recording on).
- Promote a real family member to `TrustedHelper` only outside an active session (in-person token or 24-hour local cooling-off).

### C. Scripted CLI ops and AI-agent automation (MCP)
- Every session verb (`screenshot`, `files`, `sync`, `exec`, `tunnel`, `chat`, `restart`, `shutdown`, `lock`, `batch`, `broadcast-file`, `collect-files`) runs over the same host-enforced command bus with `--json` output.
- `tethercraft-mcp` exposes stateful live sessions to AI agents: read tools are open, every mutating tool requires `confirm=true`, passwords come from `password_file` only, and every call is audited.

### D. Self-hosted NAT traversal (`tethercraft server` / `--via`)
- Run your own single-binary rendezvous/relay server for machines behind NAT.
- Because ChaCha20-Poly1305 E2E encryption runs inside `SessionStream` after SPAKE2, the relay only pipes opaque ciphertext and cannot read or inject frames, keystrokes, or files.

### E. Mobile controller on the go (Tier 5, planned)
- iOS (controller-only via thin SwiftUI + Rust FFI + hardware `VideoToolbox` H.264, see [IOS_PLAN.md](IOS_PLAN.md)) and Android (controller + host).

---

## 2. Legitimate use cases we restrict or intentionally DO NOT support

These use cases are real in corporate IT or gaming, but supporting them in consumer builds conflicts directly with TetherCraft's security and anti-scam invariants (PRD sections 5 and 17):

### A. Onboarding-free "QuickSupport" for strangers (NOT supported in consumer builds)
- **Legitimate use:** A vendor helpdesk wants a customer to run one disposable exe without installing a service, get full mouse/keyboard help for 10 minutes, and delete the file.
- **Why we restrict it:** Instant full control for a first-time caller is the exact mechanic call-center scammers use to take over elderly relatives' PCs and drain bank accounts.
- **TetherCraft's line:** `tethercraft.exe` is already a portable single exe, so no installer is required, **but R1 (20-second onboarding gate) and R2 (Stranger = view-only) are never skipped**. A legitimate helpdesk can still view the screen immediately and guide the user verbally without taking over the mouse.

### B. Covert / stealth monitoring and hidden tray (NOT supported)
- **Legitimate use:** Locked-down kiosks or lab machines where admins do not want users accidentally closing the remote daemon.
- **Why we forbid it:** Hidden agents turn a signed remote-desktop binary into stalkerware or a signed RAT (Remote Access Trojan).
- **TetherCraft's line:** Hard non-goal. Desktop hosts always show a visible, non-dismissable session indicator and write a local audit log.

### C. Silent consumer install and fleet RMM / MDM (NOT supported)
- **Legitimate use:** Corporate IT silently pushing an agent to 500 company laptops via Intune/GPO and managing software inventory/patches.
- **Why we forbid it:** Silent consumer installers are routinely abused by ransomware groups ("Living off the Land" RMM abuse) and stalkerware.
- **TetherCraft's line:** `tethercraft service install` requires local admin elevation and explicit editing of `%PROGRAMDATA%\TetherCraft\host.json`; the default template grants nothing. TetherCraft is a remote-desktop tool, not an RMM/MDM.

### D. Screen blanking ("Privacy Mode") for strangers (RESTRICTED)
- **Legitimate use:** Working remotely on your own office PC in an open-plan office so passers-by cannot read sensitive spreadsheets, or maintaining a public kiosk display.
- **Why we restrict it:** Screen blanking is the #1 banking-scam weapon ("Windows is updating, do not touch anything" while the scammer wires money out of an open browser tab).
- **TetherCraft's line:** Strictly denied for `Stranger` peers (R2). Planned only for pre-enrolled `Managed` / `TrustedHelper` peers (Tier 4), with a local physical escape hotkey that immediately ends the session.

### E. Public SaaS cloud relay operated by the project (NOT supported)
- **Why:** Operating a free public relay is what creates the recurring cloud bill that eventually forces commercial remote-desktop vendors into money-grabs, nag screens, and account lock-in. TetherCraft ships the relay binary (`tethercraft server`) so you run your own or use Tailscale/LAN directly.

### F. 4K120 HDR / competitive game streaming (NOT a target)
- **Why:** Ultra-low-latency game streaming with gamepad passthrough and HDR tone-mapping is already well served by Moonlight/Sunshine and Parsec. TetherCraft optimizes for crisp desktop text, low bandwidth, strong crypto, and CLI/MCP automation.
