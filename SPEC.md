# 🧬 GhostStream — Core Nvidia GPU Video Engine (Zig)

**Status:** Experimental • Prototype • NVENC/AV1-focused  
**Languages:** Zig (core), optional C ABI, Rust bindings  
**Purpose:** Provide a high-performance, low-overhead GPU video pipeline for Linux, enabling next-gen encoding, capture, and real-time streaming.

---

## 🎯 Overview

**GhostStream** is a next-generation GPU video engine built in **Zig** for maximum performance and direct low-level control.  
It powers the *recording, capture, encoding, and real-time streaming* pipeline used by **GhostCast**.

GhostStream is designed as a **cross-platform, vendor-agnostic**, forward-compatible replacement for legacy encoding stacks (NVENC-only, OBS plugin sprawl, X11-bound recorders, etc.).

The project centers on:

- A **fast, low-latency** GPU capture + encode pipeline  
- A **clean, stable C ABI** that any language can wrap  
- Future support for **AMD AMF**, **Intel QuickSync**, **Vulkan**, and **PipeWire DMA-BUF**  
- Modern formats: **AV1**, **HEVC**, **H.264**, **HDR10**, **High refresh streaming**

---

## 🏗 Architecture

```text
┌────────────────────────────────────────────┐
│                Client Apps                 │
│      (GhostCast, CLIs, Rust, Python)       │
└────────────────────────────────────────────┘
                    │ C ABI
                    ▼
┌────────────────────────────────────────────┐
│               GhostStream Core             │
│               (Zig Library)                │
├────────────────────────────────────────────┤
│ GPU Capture Layer   │ Wayland / DRM / PipeWire
│ Encoder Backend      │ NVENC / (Future AMF/QSV)
│ Format Pipeline      │ AV1, H.264, HEVC
│ Buffer Engine        │ Zero-copy GPU memory
│ Timing & Sync        │ FPS, VSync, frame pacing
└────────────────────────────────────────────┘

# 🧬 GhostStream — Core Capabilities Overview

GhostStream is loaded by libraries or applications at runtime, providing:

- Frame capture  
- Frame encode  
- Bitrate control  
- Hardware optimizations  
- Predictable memory behavior and zero-copy pipelines  

---

## 🔥 Core Features

### **1. GPU Capture**
- Wayland DMA-BUF capture  
- PipeWire native capture (fallback)  
- Vulkan capture (planned)  
- Frame pacing + timestamping  
- Zero-copy pass-through  

---

### **2. GPU Encoding**
**NVENC (primary target)**  
- AV1, HEVC, H.264  
- Dual encoder support (RTX 40/50 series)  
- Low-latency mode  
- CBR / VBR / CQP modes  

**Future Backends:**  
- AMD AMF  
- Intel QuickSync  

---

### **3. Processing Pipeline**
- YUV / NV12 / RGBA conversions  
- Tone mapping  
- HDR → SDR (planned)  
- Scaling & sharpening (CUDA or Vulkan compute modules)  
- Async frame queues  

---

### **4. API Surface**
Exposed through:

- **`ghoststream.h`** → C ABI  
- **`ghoststream.zig`** → Zig-native interface  
- **`ghoststream-rs`** → Safe Rust wrapper  

---

## 📦 Outputs

GhostStream can produce:

- Encoded frame packets (Annex-B, AVCC, etc.)  
- Raw bitstreams (file-safe output)  
- Circular buffer for replay-style recording  
- Frame metadata: timestamps, durations, dropped frames, GPU timing  

---

## 🔐 Security Model

- No kernel modules  
- No kernel hooks  
- No anti-cheat footprint  
- Pure user-space GPU interaction  
- Zero privileged operations required  

GhostStream is safe for Linux gaming ecosystems, free from anti-cheat–related risks.

---

## 🛣 Roadmap Summary

- **v0.1** — NVENC + AV1 core working  
- **v0.2** — Frame capture + encode + Rust wrapper  
- **v0.3** — Pipeline processing + dual encoders  
- **v0.4** — HDR and Vulkan capture  
- **v1.0** — Stable ABI + GhostCast integration  

