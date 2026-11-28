# TetherCraft Game Streaming (Under Development / TODO)

> **Status: UNDER DEVELOPMENT / TODO (Tier 4/5).**
> Not yet implemented. Today TetherCraft delivers 32.5 fps real 1080p desktop
> streaming over persistent WGC + vendored OpenH264 (`docs/BENCH.md`). This
> document specifies the low-latency game-streaming mode that RustDesk does not
> have, built on top of our upcoming hardware-encode and QUIC/UDP upgrades.

---

## Why add game streaming to TetherCraft?

RustDesk is great for office desktops, but it does **not** do low-latency game
streaming (Sunshine/Moonlight territory). Yet once a remote desktop has GPU
hardware encoding, QUIC/UDP transport, relative mouse input, and live system
audio, **~85% of a Sunshine/Moonlight-class game streamer is already built**.

Adding a dedicated **Performance / Gaming Mode** means one pure-Rust tool on
Goliath (RTX 4090) handles:
- Dev work from a Viennese Kaffeehaus (with full CLI + MCP automation)
- Safe family IT support (with R1-R7 anti-scam defaults)
- Low-latency 60-120 fps game streaming to a laptop, handheld, or iPad/iPhone

---

## Planned pipeline (TODO checklist)

### 1. Zero-copy GPU capture and hardware encode (Tier 4)
- [ ] **Windows Media Foundation H.264 / H.265 MFT** in `tethercraft-codec` (vendor-agnostic OS API: automatically routes to **NVENC** on NVIDIA, **AMF** on AMD, **Quick Sync** on Intel, with automatic OpenH264 fallback).
- [ ] **Zero-copy D3D11 -> NVENC / MFT path:** Keep the WGC `ID3D11Texture2D` on the GPU instead of copying 8.3 MB BGRA frames to CPU RAM per frame. Target encode time: **1-2 ms** at 1080p/1440p/4K.
- [ ] **Adaptive bitrate & quality presets:** Desktop preset (crisp text, low idle bandwidth) vs. Gaming preset (constant high fps, 15-50 Mbps target on LAN/Tailscale, low-latency VBV/CBR).
- [ ] Optional **HEVC 10-bit / HDR10** tone-mapping and **AV1** on RTX 40-series / modern GPUs.

### 2. Low-latency QUIC / UDP media transport (Tier 4)
- [ ] Move video and audio frames off TCP onto **QUIC (`quinn`, already in `Cargo.toml`)** using unreliable datagrams or per-frame unidirectional streams so a single dropped Wi-Fi packet never stalls the stream (eliminating TCP head-of-line blocking).
- [ ] Keep control, files, exec, and tunnels on reliable E2E-encrypted streams.
- [ ] Preserve full ChaCha20-Poly1305 / TLS 1.3 pinned-key E2E security.

### 3. Live system audio (Tier 4)
- [ ] **WASAPI loopback** capture on Windows host -> **Opus** low-latency audio encoding (10-20 ms frames, 48 kHz stereo) -> synchronized playback on desktop and iOS controllers.

### 4. Gaming input (Tier 4/5)
- [ ] **Relative mouse mode** (`PointerLock` / raw mouse delta events) for 3D and FPS games.
- [ ] **Gamepad passthrough:** Virtual Xbox controller on Windows host (ViGEmBus) driven by local USB/Bluetooth controllers on desktop and `GCController` on iOS/iPadOS.
- [ ] **High-refresh pacing:** 60 / 90 / 120 fps target grid matching the client display.

### 5. Safety invariant stays intact
- Gaming Mode is available only for **pre-enrolled `Managed` or `TrustedHelper` peers** (never `Stranger` sessions). R1-R7 anti-scam rules and visible host session indicators remain non-negotiable.
