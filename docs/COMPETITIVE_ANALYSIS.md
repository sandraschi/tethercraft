# Competitive Analysis: Why TetherCraft Exists

Remote desktop software has two chronic, well-documented failures:

1. **Consecutive money-grabs** by commercial apps that win trust as free or cheap personal tools and then lock users out behind hostile pricing and telemetry.
2. **Tech-support scam abuse**, where legitimate, signed remote-control apps are weaponized to take over elderly relatives' PCs and drain their bank accounts.

TetherCraft is a clean-room, pure-Rust remote desktop (MIT OR Apache-2.0) built to address both problems from the first commit.

---

## 1. The sad story of consecutive remote-control money-grabs

Anyone who has supported family PCs or managed a small homelab over the last 15 years has watched the same bait-and-switch play out over and over:

- **LogMeIn (2014):** Built a huge user base on LogMeIn Free, then killed the free tier with **seven days' notice** and raised commercial subscription prices sharply year after year.
- **TeamViewer:** Won the world as the default free tool for helping parents and friends. Then came aggressive "commercial use suspected" heuristics that locked out genuine personal users (5-minute forced disconnects, cooldown timers, paperwork appeals), alongside high-priced auto-renewing business contracts with notoriously rigid cancellation windows.
- **AnyDesk:** Grew rapidly as the fast, lightweight, fairer alternative to TeamViewer. Once dominant, it followed the same trajectory: nag popups, artificial connection delays on free accounts, shrinking device limits, and enterprise-first upsells.
- **Splashtop, Chrome Remote Desktop, Parsec, and cloud RMMs:** Each solves one slice while trading away local ownership: mandatory vendor cloud accounts, Google ecosystem lock-in, non-commercial restrictions, or per-seat SaaS rent for basic features like an address book or unattended access.

**The structural cause:** When a vendor operates the mandatory rendezvous/relay cloud and controls a closed-source client, the user's machines are rented back to them. Eventually investors or acquirers pull the pricing lever.

---

## 2. The RustDesk story

**RustDesk** broke that cycle and deserves real credit:
- Open-source Rust client and self-hostable rendezvous/relay server (`hbbs` / `hbbr`).
- Fast, modern video codecs and broad platform reach.
- Proved that self-hosted, low-latency remote desktop does not require a commercial cloud middleman.

Why TetherCraft is built as a **clean-room MIT/Apache-2.0 sibling** rather than a fork of RustDesk:

1. **Licensing and Pro split:** RustDesk's client and OSS server are AGPL-3.0, while key management features (address book, audit log, web console, strategies, OIDC/LDAP) live in proprietary **RustDesk Server Pro**. TetherCraft is MIT OR Apache-2.0 with address book, audit log, and relay pairing in the open workspace.
2. **GUI-first vs. unified command bus:** RustDesk was built GUI-first; its CLI covers deployment flags only, and a full CLI has been an open request since 2021. TetherCraft puts the GUI, a complete scriptable CLI, and a confirm-gated MCP server on **one shared command bus** with one host policy engine.
3. **Security architecture from commit one:** Real-world remote-desktop CVE classes (replayable password auth, relay visibility, weak host-side authorization, unsigned config/update paths) are much easier to eliminate in a fresh design: SPAKE2 PAKE, Ed25519 pinned device keys with 8-word TOFU verification, ChaCha20-Poly1305 E2E encryption over relays, and per-message host policy enforcement.
4. **Scam resistance as a core requirement:** RustDesk, like TeamViewer and AnyDesk, still optimizes for low-friction ad-hoc connection by strangers.

*(Clean-room rule: TetherCraft never reads, copies, or translates any RustDesk source code, `.proto` files, assets, or strings.)*

---

## 3. The remote-control scam problem ("granny's PC")

Every day, organized call-center scammers steal billions from older and non-technical people using a simple playbook:

1. A fake bank alert, pop-up warning, or phone call ("Microsoft Support", "Amazon Fraud Department", "your bank's security team") creates urgency and panic.
2. The scammer talks the victim into downloading a **legitimate, code-signed remote-control app** (TeamViewer QuickSupport, AnyDesk, UltraViewer, ConnectWise ScreenConnect, or RustDesk). Because the binary is legitimate, Windows Defender and antivirus tools do not block it.
3. The victim reads out a 9-digit ID and clicks "Accept".
4. Within seconds, the scammer has **full mouse/keyboard control, file transfer, screen blanking ("privacy mode"), and unattended persistence**. They open the victim's browser, watch them log into online banking, blank the screen while initiating wire transfers or crypto purchases, or edit page HTML to fake a "refund overpayment".

**Why mainstream remote-desktop tools fail to stop this:**
- Their enterprise customers demand **zero-friction ad-hoc support** ("QuickSupport" single-exe, instant full control on first click, screen blanking available to the first-time caller).
- A tiny warning dialog that the caller can talk the victim through clicking in 2 seconds ("just click Allow, that's normal") does not work under social engineering.

---

## 4. What TetherCraft tries to mitigate (R1-R7 in code)

TetherCraft cannot make social engineering impossible, but it refuses to be an easy weapon for a stranger on the phone. Our anti-scam rules (PRD section 17) are enforced in host code, not hidden in a settings checkbox:

| Rule | Threat it targets | Mitigation in TetherCraft |
|---|---|---|
| **R1: Non-skippable onboarding gate** | Victim pressured on the phone to install and immediately accept a stranger | First launch shows a full-screen question with a **20-second mandatory wait**: *"Did someone call, email, text or show a pop-up telling you to install this?"* Answering **Yes** or **Not sure** locks the PC into **24-hour outgoing-only mode** (all incoming connections refused, persistent banner telling the user to hang up). |
| **R2: Stranger tier by default** | First-time caller immediately taking over mouse, files, or screen | Every new peer starts as **Stranger**: view-only, no file transfer, no terminal, no screen blanking/privacy mode, no input blocking, no unattended access, hard 30-minute session cap, local recording forced on. Enforced per-message on the host. |
| **R3: No in-session trust promotion** | Scammer talking the victim into clicking "Grant full access" mid-call | Promoting a peer to `TrustedHelper` (for genuine family IT support) cannot be done from inside an active session. Requires an in-person token or a 24-hour local cooling-off started outside any session. |
| **R4: Scam Shield** | Scammer viewing or operating online banking / password managers | Banking, payment, and password-manager windows are shielded (blurred and input-blocked) during Stranger sessions. |
| **R5: Risk scoring + local interstitial** | Classic scammer script behaviors during a session | Suspicious sequences trigger an input freeze and a local capture-excluded warning that the remote caller cannot see or dismiss. |
| **R6: One-click evidence + block** | Victim realizing afterward they were scammed | One action exports an audit/evidence bundle and permanently blocks the caller's Ed25519 device key. |
| **R7: No QuickSupport, no stealth** | Disposable scammer-friendly executables and hidden persistence | Consumer builds always run R1. No onboarding-free QuickSupport exe, no silent install, no hidden tray, no stealth screen-hiding. |

---

## 5. Landscape comparison

| Capability | TeamViewer | AnyDesk | LogMeIn | RustDesk (OSS / Pro) | TetherCraft |
|---|---|---|---|---|---|
| License | Proprietary | Proprietary | Proprietary | AGPL-3.0 / Proprietary Pro | MIT OR Apache-2.0 |
| Self-hosted relay | No | Enterprise only | No | Yes (`hbbs`/`hbbr`) | Yes (`tethercraft server`) |
| Money-grab / lock-out risk | High (proven) | High (proven) | High (proven) | Low (OSS) / Pro paywall | None (local + open) |
| E2E encryption over relay | Vendor-controlled | Vendor-controlled | Vendor-controlled | Yes | Yes (SPAKE2 + ChaCha20-Poly1305) |
| Full CLI + MCP automation bus | No | Minimal CLI | No | Deployment CLI only | Yes (shared host policy bus) |
| Anti-scam defaults (R1-R7) | Weak (QuickSupport) | Weak | Weak | Weak (ad-hoc full control) | Built into core + host policy |
| Maturity (2026-11) | Mature | Mature | Mature | Mature | Pre-release (`v0.9.0`, Windows Tier 1-3 active) |

---

## 6. QuickSupport, stealth, and privacy mode: legitimate uses vs. abuse

These features have real enterprise and power-user value, which is why commercial tools ship them. TetherCraft restricts or forbids them in consumer builds because **the exact same mechanics are what scammers, stalkerware, and ransomware operators exploit**:

| Feature | Legitimate use | Abuse vector | TetherCraft stance |
|---|---|---|---|
| **QuickSupport (single-exe ad-hoc)** | Customer runs one portable exe for a 10-minute vendor fix without installing a background service or needing admin rights. | Scammer talks a victim into running it and gets full mouse, keyboard, file, and blanking control in under 20 seconds. | `tethercraft.exe` is already a portable single exe, **but R1 (20 s gate) and R2 (Stranger = view-only) are never skipped**. Helpdesks can still view and guide verbally. |
| **Screen blanking (Privacy Mode)** | Working remotely on your own office PC in an open-plan office so passers-by cannot read your screen; kiosk maintenance. | **#1 banking-scam weapon:** Scammer blanks granny's screen ("Windows is updating") while wiring money from her open bank tab. | **Blocked for Strangers (R2).** Planned only for pre-enrolled `Managed`/`TrustedHelper` peers (Tier 4), with a local physical escape key. |
| **Silent / unattended install** | Corporate IT pushing an agent to 500 company laptops via Intune/GPO. | Signed RMM-as-malware (ransomware persistence) and stalkerware. | **No silent consumer install.** `tethercraft service install` requires local admin and explicit config editing; default grants nothing. |
| **Hidden tray / no indicator** | Locked-down kiosks where users should not close the daemon. | Covert surveillance (stalkerware). | **Hard non-goal (PRD sec 5).** Active desktop sessions always show a visible indicator and write a local audit log. |

See [USE_CASES.md](USE_CASES.md) for the full supported vs. unsupported use-case breakdown.

---

## 7. Feature gaps vs. competitors: what deserves a clean-room filch

What RustDesk, AnyDesk, TeamViewer, LogMeIn, Splashtop, and Parsec have that TetherCraft does **not** yet support, ranked by whether we should build a clean-room version:

### High-value features worth building (clean-room)

1. **Hardware H.264 / H.265 / AV1 via OS APIs (Windows Media Foundation MFT, Apple VideoToolbox):** Automatically routes to NVENC (NVIDIA), AMF (AMD), and Quick Sync (Intel) with software OpenH264 fallback, plus adaptive bitrate/resolution. *(RustDesk, AnyDesk, Parsec)*
2. **Multi-monitor switcher + "show all" + remember last monitor per peer:** Essential for multi-screen workstations. *(RustDesk, AnyDesk, TeamViewer)*
3. **Virtual display driver for headless Windows/Linux hosts:** Lets headless homelab boxes stream at arbitrary resolution without an HDMI dummy plug. *(RustDesk, Parsec)*
4. **Windows Ctrl+Alt+Del (SAS) + logon-screen / UAC secure-desktop access via the Windows service:** Needed for real Windows admin sessions. *(TeamViewer, AnyDesk, RustDesk)*
5. **Live remote system audio (Opus):** Hearing alerts, media, or calls from the remote host. *(RustDesk, AnyDesk, Splashtop, Parsec)*
6. **Clipboard file copy/paste + drag-and-drop onto the session canvas:** Faster day-to-day file moves than opening a file browser. *(AnyDesk, TeamViewer, RustDesk)*
7. **Session whiteboard / laser-pointer annotation overlay:** Huge win for **view-only family tech support** - point to the exact button on grandma's screen without taking over her mouse. *(AnyDesk, TeamViewer)*
8. **Remote cursor rendering + "show local cursor" + relative mouse mode:** Fixes cursor lag and enables 3D/CAD/phone-trackpad workflows. *(AnyDesk, RustDesk, Parsec)*
9. **Session tabs / multiple simultaneous sessions in one window:** Operate two machines side by side. *(AnyDesk, RustDesk)*
10. **UDP hole punching (direct P2P NAT traversal) + WebSocket/proxy fallback:** Cuts latency and relay bandwidth when both sides are behind NAT. *(RustDesk, AnyDesk)*
11. **LAN auto-discovery (mDNS / local broadcast):** Zero-config discovery of your own machines on the same Wi-Fi/LAN. *(RustDesk, AnyDesk)*

### Features we intentionally skip (anti-features / abuse vectors)

- Onboarding-free QuickSupport full-control executable (R7).
- Covert/stealth mode, hidden tray, silent consumer install (PRD sec 5).
- Built-in RMM/MDM software inventory and policy push (PRD sec 5).
- Mandatory vendor cloud account or SaaS licensing server.
- 4K120 HDR competitive game streaming (well served by Moonlight/Sunshine and Parsec).
