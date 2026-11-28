# TetherCraft completion and gap-closing plan

Date: 2026-11-28. Basis: HANDOVER.md (agy, 2026-11-28 10:32), opencode session log
(2026-11-24 to 26), PRD (mcd projects/remotecraft/PRD.md), and a code audit of
`main` at 836675f. Everything marked "verified" was checked in the tree or by running it
on Goliath today; anything else is marked as an estimate.

## 1. Verdict

TetherCraft is a strong Storytold beta candidate: a clear, high-recognition category,
a real differentiator (scam resistance by default, one CLI/MCP/GUI command bus, MIT/Apache),
and an honest, well-measured engineering base. It is not beta-ready today. The headline
feature, "granny-proof by default", is mostly domain logic and tests that are not wired
into live sessions, CI has been red for five pushes, and the only published binary
predates the E2E encryption the README advertises.

Realistic distance to a credible beta submission: about 8 to 10 working days with
AI-assisted development (Phases A to C below), the one external dependency being a
second physical PC for the network run.

## 2. Verified state

Works live (verified: `cargo test --workspace` 130 passed, `cargo fmt --check` clean;
benchmarks from docs/BENCH.md not re-run today):

- SPAKE2 handshake, Ed25519 pinned keys with 8-word TOFU, ChaCha20-Poly1305 session
  encryption inside `SessionStream`, relay that pipes opaque ciphertext.
- Persistent WGC capture plus OpenH264: 1080p at 32.5 fps loopback, 485 B/frame average.
- Input injection, jailed file manager, exec (30 s kill), power, TCP tunnels, chat,
  folder sync planner, relay `--via`, Windows SCM service, stateful MCP, egui Board /
  Session / Help, R1 gate in the window.
- Linux musl probe controller against a Windows host (cross-OS proven in a VM).

Built as domain logic only, not wired into live sessions (verified by grep):

| Item | Evidence |
|---|---|
| R1 outgoing-only on the host | `ServeConfig.outgoing_only` is honoured at serve.rs:349 but the CLI host (main.rs ~1589) and the service (service.rs ~220) hardcode `false`; the onboarding answer never reaches the host |
| R3 trust promotion | `book promote` (main.rs ~927) calls `set_tier("trusted_helper")` directly; `core::trust` cooling and in-person logic is not used by it |
| R4 scam shield | `host/shield.rs` is only declared in host lib.rs; serve.rs never calls it |
| R5 risk scoring, interstitial | `core/risk.rs` unused by any session code; no `SetWindowDisplayAffinity` / capture exclusion anywhere |
| R6 evidence and block | store has a blocklist table; no one-click flow, no bundle export in a session path |
| Forced local recording for strangers | `Recorder` (codec/rec.rs) is not used by host or serve |
| Visible host indicator, End session, hotkey | no tray, border or host UI exists (PRD section 5 and R2 call this mandatory) |
| Local "accept by click" | none; a correct password proof admits the peer |
| Clipboard | permission mapping only; no transport |
| Audio, multi-monitor, PTY terminal, SAS, UAC, logon screen | none or `NotImplemented` (`list_monitors`, terminal PTY) |
| QUIC | quinn/rustls are declared in Cargo.toml but not used; transport is TCP plus app-layer AEAD |

Process and release findings (verified):

- CI: last five pushes to main all failed. Cause in the latest log: `field frame_no is never
  read` under `-D warnings` on ubuntu (tethercraft-capture/src/lib.rs:92). Windows leg was
  not inspected.
- Release: v0.9.0 asset was uploaded 2026-11-26, E2E landed 2026-11-28. The README says
  v0.9.0 and describes E2E. The published exe is unsigned and has no SHA256SUMS asset;
  dist/ holds a newer build, setup.exe and portable zip that were never published.
- Licence hygiene: `MIT OR Apache-2.0` is claimed but there is no LICENSE-APACHE file.
  docs/assets/tethercraft-story.jpg (920 KB) exists while ATTRIBUTION.md says "no binary
  asset files". `unsafe_code` is `warn`, not `forbid`. No packaging/env.sh, no screenshots.
  The MCP binary has no `--automation-read-root` / `--automation-write-root`
  (craft rule 18). AGENTS.md "This repository" still says "Tier 0 + Tier 1 ... No relay
  server". Cargo.lock is modified and uncommitted.
- Crypto review items: session keys come from a hand-rolled SHA-256 derivation
  (net/session.rs:188), not HKDF with labels and a transcript hash; no rekeying; no
  fuzz harnesses (malformed-input unit tests stand in); PRD requires external review
  before a public release. THREAT_MODEL item 3 still describes QUIC.
- H.264 patent coverage: Cisco pays the MPEG LA royalties only for the OpenH264 binaries it
  distributes; projects that compile the source themselves carry that obligation (Cisco FAQ
  wording, repeated in the Fedora and Tor discussions). The `openh264` crate defaults to
  the `source` feature, which compiles vendored C++ into our exe (the opencode handover
  confirms: no download happens). That contradicts the O3 licence verdict "Cisco prebuilt
  only for H.264" and PRD D4. The crate's `libloading` feature is the Cisco-binary route,
  and the crate docs say that binary must be downloaded at install time. Not legal advice;
  it matters for anything distributed, which a Storytold beta is.
- Docs honesty: the README matrix uses "Partial" for both "live" and "domain model
  only", which hides the table above. The README headline says RustDesk parity while
  D1 (RustDesk interop) has no spec: `projects/remotecraft/spec/` holds only LEGAL_NOTES.md.

## 2b. Handover reconciliation

The root HANDOVER.md was first written by opencode-musespark at about 03:10 today (commit
b884f84, a "problem report for agy") and rewritten by agy at 10:32 (commit 44a6a61, still
carrying opencode's older log below). Checked against git history and the code:

- Persistent WGC capture, exposed encoder config (bitrate, ScreenContentRealTime), and the
  deadline-pacing incident (root cause: serve loop dropped encoded NALs when two timers
  beat; fixed with a bounded sync_channel(2) and a regression test): all closed in dd9575d.
- GUI dial and TOFU notice, relay E2E: closed in 44a6a61.
- Still open from that report: hardware encoder, two-machine run, the start.ps1 Floor check.
- Not in the report and found today: the H.264 patent coverage point above. The report
  correctly says no codec shopping is needed technically; the licence route is the issue.
- The session log (mcd operations/session-log/2026-11-27.md) has the same three entries.

## 3. Storytold beta bar (from mcd BETA_CANDIDATES.md, filter of 2026-11-21)

| Filter | Status |
|---|---|
| Recognizable category, broad appeal | Strong. Remote desktop replacement with a family-safety angle |
| Zero collision with Storytold apps | Not checked against their current list; do before submitting. Name already cleared of the RemoteCraft clash |
| Standalone fresh-clone build, Win + macOS + Linux CI, SHA256SUMS | No sibling path deps found. CI red. Host is Windows-only |
| craft-contrib compliance | Gaps: LICENSE-APACHE, forbid(unsafe_code), packaging/env.sh, xtask ci coverage, screenshots, MCP roots, naming scrub, accessibility, i18n |

Two risks specific to this app, not to the checklist:

1. wincraft was put in Tier C as "Windows-only by definition". TetherCraft's host is
   Windows-only today. Mitigation: pitch it as cross-platform controller (Linux and macOS
   build and CI green) with a Windows host in beta and a Linux X11 host on the roadmap.
2. Remote-control software is abuse-adjacent. Maintainers may want the security review
   and the R1-R7 wiring demonstrated, not just documented. Phase A is therefore also the
   acceptance case, not only engineering hygiene.

## 4. Plan

Day counts are AI-assisted estimates. Commit rules stay: max 10 files per commit, LF,
ASCII, no em dash, single trunk.

### Phase A: make the claims true (4 days)

A1. CI green (0.5 day). Fix the dead-code error with a `cfg(windows)` or a real use of
`frame_no`; confirm the Windows leg; add `cargo deny check` and an `xtask ci` call.

A2. Host-side safety wiring (2.5 days), each with a scripted "scammer controller" scenario
test as the PRD requires:
- R1: host and service read the persisted onboarding answer and lockout window; refuse
  incoming when locked. Remove the two hardcoded `false`.
- Visible host indicator: a user-session process that shows an always-on-top border or
  tray state, the live session list, an End session button and a global hotkey. This
  forces the service design question below (session 0 versus user session).
- Local accept prompt for every new peer, capture-excluded.
- R2 completion: forced local `.tetherrec` recording for stranger sessions.
- R3: route `book promote` and the GUI through `core::trust` (24 h cooling or in-person
  token); direct promote only in a test build.
- R4/R5/R6: call `shield` from the capture path, feed risk signals from window titles and
  input events, show the interstitial with `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
  so the remote side cannot see it, input freeze until local answer, "This was a scam"
  button producing the evidence bundle and a block.
- Optional siren on the R1 gate (Sandra's idea, default on in consumer build).

A3. Service model decision (0.5 day): a SYSTEM service cannot show UI or capture the
interactive desktop from session 0. I found no session-helper code (`WTSGetActiveConsoleSessionId`
or `CreateProcessAsUser` do not appear). Confirm by running the service build on a second
session, then implement the SYSTEM-service plus per-session helper split from PRD section
10. This is also the prerequisite for UAC, logon screen and SAS later.

A4. Truthful docs (0.5 day): split README matrix into Live / Domain only / Planned; fix
THREAT_MODEL item 3 (TCP plus AEAD now, QUIC deferred); amend PRD D2; rewrite AGENTS.md
"This repository"; say plainly "native protocol only, RustDesk interop is not in beta".

### Phase B: beta hygiene (2 to 3 days)

- Add LICENSE-APACHE; set `unsafe_code = "forbid"` and isolate any unavoidable Windows
  FFI behind named modules per craft rule 13; resolve the story.jpg provenance (record
  author and licence in ATTRIBUTION.md, or drop it); run `cargo xtask assets`.
- MCP: add `--automation-read-root` and `--automation-write-root`, refuse to start without
  them, jail file access (rule 18). Keep the existing confirm gates.
- packaging/env.sh, dark and light screenshots with accessibility tree reads, keyboard
  navigation and egui accesskit pass (rule 21), translation keys (rule 16).
- Naming scrub per naming.md: user-facing "RustDesk-style", competitor product names in
  UI text; keep competitor names in the analysis docs only if the standard allows it.
- CI: ubuntu, windows, macos matrix for controller crates; host crates gated by cfg.
  Prefer craft-common-actions for workflows as a separate PR (rule 17).
- Reusable pieces to consider moving to craft-libs later: H.264 wrapper, `.tetherrec`
  format, session AEAD. Not a beta blocker.
- H.264 licence route (decide first, small change): on Windows prefer the OS Media Foundation
  encoder and decoder (see Phase C); keep OpenH264 only through the crate's `libloading`
  feature with Cisco's prebuilt binary fetched at install or first run, pinned by SHA-256;
  controllers on Linux and macOS use the same fetched binary. Royalty-free VP9/AV1 software
  is the longer-term default (PRD section 10). Update O3 and PRD D4 to match. The musl probe
  stays raw-only and is unaffected.
- Release: bump to 0.10.0, tag, publish plain exes plus SHA256SUMS.txt (no zip as the
  only asset; the portable zip can ship beside them). Code-sign if a certificate is
  available, otherwise say "unsigned" on the release page and in R7 docs.
- Check the current Storytold app list for collisions; fill the 4-field form at
  getartcraft.com/beta; write docs/STORYTOLD_BETA_TODO.md like the Tier A repos have.

### Phase C: real-world proof and hardening (3 to 4 days)

- Two-machine run over LAN and Tailscale, glass-to-glass latency recorded in BENCH.md.
  Unblock options: second physical PC, one interactive login on `win11-automated`, or a
  borrowed machine. This is the only item that needs something outside the repo.
- Crypto: HKDF with domain-separated labels and a transcript hash, optional rekey, add
  cargo-fuzz targets for proto decode, session envelope and file path validation, run
  them in CI for a bounded time. Then request an external review; label the beta
  "not externally reviewed" until it is done.
- Hardware encoder through Media Foundation (NVENC / AMF / Quick Sync routing), which also
  answers the H.264 licence question on Windows; OpenH264 stays as the libloading fallback.
  Measure at 1080p60 and 4K.
- Daily-driver gaps: clipboard text and images, multi-monitor switching, ConPTY
  terminal, UAC and Ctrl+Alt+Del through the helper.
- Crates.io sweep before building anything (per your rule). Candidates to verify, not
  yet checked: `hkdf`, `arboard`, `portable-pty` or a ConPTY binding, `opus`, `cpal`,
  `ashpd` (Wayland portals), `xcap` or similar for Linux capture, `quinn` already present.
  Each needs a licence check against the craft-contrib allowlist.

### Phase D: parity and platforms (7 to 10 days, after beta acceptance)

Audio (WASAPI loopback plus Opus), clipboard files, UDP hole punching and a QUIC media
path, LAN discovery, config import/export, signed auto-update download, Linux X11 host
and controller packaging, macOS controller, then Wayland portals.

### Phase E: RustDesk interoperability (about 6 days, blocked)

Blocked on the legal check and a spec written by a separate team. Keep out of beta and
out of the README headline until a spec exists. Compat peers stay stranger-tier with
in-person promotion only (R8).

### Phase F: later tiers

iOS controller (docs/IOS_PLAN.md; note craft rule 11 prefers Rust-only apps, so state the
departure reason), Android, game streaming (needs the QUIC datagram path, hardware
encode, WASAPI, relative mouse, ViGEm), web client, Pro-class features.

## 5. Decisions needed from Sandra

1. Beta pitch: "scam-resistant, scriptable, self-hosted remote desktop (Windows host)"
   instead of "RustDesk parity". Recommended; parity is a long road and D1 has no spec.
2. Service model: accept the SYSTEM service plus user-session helper split (Phase A3).
3. Transport: confirm TCP plus AEAD for beta and QUIC only when game streaming starts.
4. Release signing: is a code-signing certificate worth buying for the beta, or ship
   unsigned with clear wording.
5. Second machine for the network run, and who reviews the crypto externally.
6. Whether the comic image stays (needs provenance) or goes.
7. H.264 route: Media Foundation plus Cisco-binary fallback (recommended), or royalty-free
   VP9/AV1 first. A lawyer check is still advisable before wide distribution.

## 6. Exit criteria for "submit to Storytold"

- CI green on Windows, Linux and macOS; 130+ tests plus scammer-scenario tests.
- R1 to R7 each demonstrated by a scenario test against a live session, not only unit tests.
- Host shows a visible indicator and End session; recording on for strangers.
- Two-machine bench recorded; v0.10.0 published with SHA256SUMS.
- craft-contrib checklist complete (licences, forbid unsafe, MCP roots, env.sh,
  screenshots, naming, accessibility); README matrix split into Live / Domain / Planned.
