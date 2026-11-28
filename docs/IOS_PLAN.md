# TetherCraft iOS Client Plan (Tier 5)

Status: **Planned** (groundwork exists in `crates/tethercraft-core/src/mobile.rs`; implementation starts in Tier 5 after desktop hardware encoding and two-machine LAN/Tailscale benchmarks).

## 1. Scope and hard OS constraints

- **Controller-only:** Apple forbids arbitrary remote input injection and background capture of other apps on iOS. `tethercraft-core::mobile::Role::Controller` enforces this rule (`can_host(Role::Controller)` always returns `Denied`).
- **Target use case:** Control your Windows, Linux, or macOS hosts from an iPhone or iPad over LAN, Tailscale/WireGuard, or a self-hosted `tethercraft server` relay (`--via`), with full SPAKE2 + ChaCha20-Poly1305 E2E encryption and TOFU key pinning.

## 2. Architecture decision (PRD D5): Thin SwiftUI shell + Rust core FFI

Use a **native SwiftUI app shell** linked against a **Rust static library (`tethercraft-ffi`)**, rather than running `egui`/`eframe` on iOS:

| Concern | Thin SwiftUI + Rust FFI | egui on iOS |
|---|---|---|
| H.264 decode & battery | Hardware `VideoToolbox` (`VTDecompressionSession` + `AVSampleBufferDisplayLayer`), 60 fps, cool battery | Software OpenH264 on CPU (hot, battery drain) |
| iOS keyboard, Face ID, Keychain | Native, zero friction | Fragile / manual bridges |
| iPadOS trackpad & hardware keyboard | Native `UIPointerInteraction` + `UIKey` | Limited |
| App Store review & HIG | Native look, safe areas, Dynamic Type | Non-native feel |
| Code reuse | 100% of crypto, protocol, relay, store, policy stays in Rust | Same |

### Crate boundary (already mostly clean today)

- **Shared Rust core for iOS (`aarch64-apple-ios` / `aarch64-apple-ios-sim`):**
  - `tethercraft-core` (permissions, tiers, `mobile` gesture/pacing policy, audit)
  - `tethercraft-proto` (wire envelopes)
  - `tethercraft-net` with `default-features = false` (Ed25519, SPAKE2, ChaCha20-Poly1305 E2E, relay client; omits vendored `openh264` C++ since iOS uses `VideoToolbox`)
  - `tethercraft-input` (input event types)
  - `tethercraft-store` (SQLite address book + pinned keys + audit)
- **New bridge crate (Phase 5A):** `crates/tethercraft-ffi` (`crate-type = ["staticlib"]`, `uniffi` or minimal C ABI via `cbindgen`) compiled into `TetherCraftCore.xcframework`.
- **Excluded on iOS:** `tethercraft-capture`, `tethercraft-host`, `tethercraft-codec` (egui/openh264), `tethercraft-app` (desktop binary), `tethercraft-mcp`.

## 3. Video decode path (Annex-B -> VideoToolbox)

1. Host streams standard Annex-B H.264 NAL units (`MessageKind::FrameH264`) inside ChaCha20-Poly1305 E2E envelopes.
2. `tethercraft-ffi` decrypts and passes raw `(width, height, annex_b_bytes)` to the Swift callback without CPU decoding.
3. Swift extracts SPS/PPS (`nal_type` 7 and 8) to build `CMVideoFormatDescriptionCreateFromH264ParameterSets`, converts start codes (`00 00 00 01`) to 4-byte AVCC big-endian length prefixes, wraps in `CMSampleBuffer`, and enqueues directly to `AVSampleBufferDisplayLayer` (or `VTDecompressionSession` -> Metal texture for pinch-zoom/overlay).

## 4. Touch, input, and iOS lifecycle

- **Gesture map:** Follows `tethercraft-core::mobile::TouchGesture` (tap = left-click, long-press = right-click, two-finger tap = middle-click, pan = scroll/drag, pinch = local viewport zoom).
- **Virtual trackpad mode:** Optional relative-pointer mode for precise clicking on dense 1080p/4K Windows desktops from a phone screen.
- **iPadOS hardware keyboard & pointer:** Direct `UIKey` -> Windows VK mapping and hover/scroll forwarding.
- **Security & Keychain:**
  - Controller Ed25519 identity key stored in iOS Keychain (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`).
  - Optional Face ID / Touch ID gate for saved host passwords (never stored in `UserDefaults`).
  - Full in-app TOFU banner showing the 8 fingerprint words on first contact; key mismatch fails closed.
- **Backgrounding & network handoff:** On `scenePhase == .background`, send `SessionEnd` cleanly (iOS suspends sockets ~30 s after backgrounding); offer one-tap reconnect with pinned key when returning to foreground.
- **Battery & bandwidth policy:** Wire `tethercraft-core::mobile::battery_fps_cap` and `frame_interval_ms` so the host can adapt target fps when the phone is on battery or cellular.

## 5. Build, CI, and distribution

- **Cross-check on Windows/Linux CI:** `cargo check -p tethercraft-ffi --target aarch64-apple-ios` (verifies zero desktop crate leakage into the mobile core).
- **Signing & packaging:** Requires macOS + Xcode (local Mac or `macos-latest` GitHub Actions runner) to build `TetherCraftCore.xcframework` and archive the SwiftUI app.
- **Distribution:** TestFlight (internal/lab testing, requires Apple Developer membership) -> App Store. Our R1-R7 anti-scam design and controller-only scope align well with App Store review guidelines.

## 6. Phased implementation order

| Phase | Where | Deliverables |
|---|---|---|
| **5A: Rust mobile FFI prep** | Windows (Goliath) | Raw `FrameH264` NAL passthrough in `tethercraft-net`, `crates/tethercraft-ffi` staticlib (`book_*`, `connect`, `send_input`, `disconnect`, file/chat RPC), `aarch64-apple-ios` check green |
| **5B: SwiftUI + VideoToolbox MVP** | macOS + Xcode | `ios/TetherCraft/` SwiftUI app (Board, Connect modal, TOFU banner, Session view), `VideoToolbox` Annex-B->AVCC hardware decoder, live E2E H.264 stream from Goliath |
| **5C: Input, Keychain & TestFlight** | macOS + iPhone/iPad | Touch gestures + virtual trackpad mode, iPadOS keyboard/pointer, Keychain + Face ID, background lifecycle, TestFlight archive |
