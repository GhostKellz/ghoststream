# GhostStream — NVIDIA GPU Video Engine

**Status:** Active Development
**Language:** Rust
**Purpose:** High-performance GPU video capture, encoding, and streaming library for Linux

---

## Overview

**GhostStream** is a Rust library providing GPU-accelerated video capture and encoding for Linux. It powers the video pipeline for **GhostCast** (recording/streaming) and **Nitrogen** (Discord companion).

Built on:
- **NVENC** via FFmpeg for hardware encoding (H.264, HEVC, AV1)
- **PipeWire** for Wayland screen capture
- **xdg-desktop-portal** for secure screen sharing

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Client Applications                     │
│              (GhostCast, Nitrogen, PhantomLink)              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    ghoststream (Rust)                        │
├─────────────────────────────────────────────────────────────┤
│  capture/                                                    │
│    ├── portal.rs      xdg-desktop-portal (ashpd)            │
│    ├── pipewire.rs    PipeWire screencast stream            │
│    └── dmabuf.rs      Zero-copy DMA-BUF handling            │
│                                                              │
│  encode/                                                     │
│    ├── nvenc.rs       NVENC encoder (ffmpeg-next)           │
│    ├── h264.rs        H.264/AVC profiles                    │
│    ├── hevc.rs        HEVC/H.265 profiles                   │
│    └── av1.rs         AV1 (RTX 40/50 series)                │
│                                                              │
│  output/                                                     │
│    ├── camera.rs      Virtual camera (PipeWire node)        │
│    ├── file.rs        MKV/MP4/WebM muxing                   │
│    └── stream.rs      RTMP/SRT streaming                    │
│                                                              │
│  processing/                                                 │
│    ├── scale.rs       Resolution scaling                    │
│    ├── convert.rs     Colorspace conversion                 │
│    └── tonemap.rs     HDR → SDR tonemapping                 │
│                                                              │
│  pipeline.rs          Capture → Process → Encode → Output   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     NVIDIA RTX GPU                           │
│                NVENC  │  CUDA  │  DMA-BUF                   │
└─────────────────────────────────────────────────────────────┘
```

---

## Core Features

### 1. Screen Capture

| Feature | Implementation |
|---------|----------------|
| Wayland screencast | xdg-desktop-portal via `ashpd` |
| PipeWire streams | `pipewire-rs` for video nodes |
| Zero-copy capture | DMA-BUF pass-through to NVENC |
| Window/monitor selection | Portal picker dialog |
| High refresh support | Up to 240Hz capture |

### 2. Hardware Encoding (NVENC)

| Codec | Support | Min GPU |
|-------|---------|---------|
| H.264/AVC | Full | GTX 600+ |
| HEVC/H.265 | Full | GTX 900+ |
| AV1 | Full | RTX 4000+ |

**Encoder Features:**
- Rate control: CBR, VBR, CQP, CRF
- B-frames and lookahead
- Low-latency mode (streaming)
- Dual encoder (RTX 40/50 for 8K or 2x streams)
- 10-bit HDR encoding

### 3. Output Modes

| Mode | Use Case |
|------|----------|
| Virtual Camera | Discord, OBS, video calls |
| File Recording | MKV, MP4, WebM containers |
| RTMP Stream | Twitch, YouTube, Kick |
| SRT Stream | Low-latency secure streaming |
| Raw Packets | Custom integrations |

### 4. Processing Pipeline

- **Scaling:** Lanczos, bilinear, nearest-neighbor
- **Colorspace:** NV12, YUV420, RGB conversion
- **HDR → SDR:** Tonemapping for SDR displays
- **Frame pacing:** VSync-aware timing

---

## Presets

```rust
pub enum Preset {
    /// 720p30 - Low bandwidth (3 Mbps)
    Discord720p,
    /// 1080p60 - Standard streaming (6 Mbps)
    Stream1080p60,
    /// 1440p60 - High quality (12 Mbps)
    Quality1440p60,
    /// 1440p120 - High refresh gaming (15 Mbps)
    Gaming1440p120,
    /// 4K60 - Ultra quality (25 Mbps)
    Ultra4K60,
    /// 4K120 - Maximum (35 Mbps, RTX 40/50)
    Maximum4K120,
}
```

---

## API Surface

### Basic Usage

```rust
use ghoststream::{Pipeline, CaptureConfig, EncoderConfig, Codec, Output};

// Configure capture
let capture = CaptureConfig::default()
    .with_fps(60)
    .with_show_cursor(true);

// Configure encoder
let encoder = EncoderConfig::default()
    .with_codec(Codec::AV1)
    .with_resolution(1920, 1080)
    .with_bitrate_kbps(8000)
    .with_preset(EncoderPreset::Quality);

// Configure output
let output = Output::VirtualCamera {
    name: "GhostStream Camera".into()
};

// Create and run pipeline
let pipeline = Pipeline::new(capture, encoder, output)?;
pipeline.start().await?;
```

### File Recording

```rust
let output = Output::File {
    path: "/home/user/recording.mkv".into(),
    container: Container::Matroska,
};
```

### Streaming

```rust
let output = Output::Rtmp {
    url: "rtmp://live.twitch.tv/app/your_stream_key".into(),
};
```

---

## Dependencies

```toml
[dependencies]
# Async runtime
tokio = { version = "1.41", features = ["full"] }

# Screen capture
ashpd = "0.10"                    # xdg-desktop-portal
pipewire = "0.8"                  # PipeWire
libspa = "0.8"

# Video encoding
ffmpeg-next = "7.0"               # FFmpeg with NVENC

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Config
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
```

---

## Hardware Requirements

| Tier | GPU | Codecs | Max Resolution |
|------|-----|--------|----------------|
| Minimum | GTX 1050 | H.264 | 1080p60 |
| Recommended | RTX 3060 | H.264, HEVC | 4K60 |
| Optimal | RTX 4070+ | H.264, HEVC, AV1 | 4K120 |
| Maximum | RTX 5090 | All + Dual Encoder | 8K60 / 4K240 |

**Driver Requirements:**
- NVIDIA 525+ (NVENC support)
- NVIDIA 545+ (AV1 encoding)

---

## Security Model

- Runs in user space only
- No kernel modules or hooks
- No root/sudo required
- Portal-based capture (user approval)
- Anti-cheat safe (no game injection)

---

## Integration

### With GhostWave (Audio)

```rust
use ghoststream::Pipeline;
use ghostwave_core::GhostWaveProcessor;

// Video pipeline
let video = Pipeline::new(capture, encoder, output)?;

// Audio processor (RTX denoising)
let audio = GhostWaveProcessor::new(audio_config)?;

// Sync A/V
video.set_audio_source(audio.output_node())?;
```

### With GhostCast

GhostCast uses GhostStream as its video backend:
- Recording (file output)
- Replay buffer (circular buffer)
- Streaming (RTMP/SRT)

### With Nitrogen

Nitrogen uses GhostStream for Discord:
- Virtual camera output
- Optimized for low latency
- AV1 when available

---

## Roadmap

### v0.1.0 — Foundation
- [ ] Library structure and types
- [ ] PipeWire capture via portal
- [ ] NVENC encoding (H.264)
- [ ] Virtual camera output
- [ ] Basic CLI for testing

### v0.2.0 — Codecs
- [ ] HEVC encoding
- [ ] AV1 encoding (RTX 40+)
- [ ] File output (MKV/MP4)
- [ ] Preset system

### v0.3.0 — Streaming
- [ ] RTMP output
- [ ] SRT output
- [ ] Bitrate adaptation

### v0.4.0 — Processing
- [ ] Resolution scaling
- [ ] HDR → SDR tonemapping
- [ ] Frame interpolation

### v1.0.0 — Production
- [ ] Stable API
- [ ] Full documentation
- [ ] GhostCast integration
- [ ] Nitrogen integration

---

## Reference

Archive contains reference implementations:
- `archive/ghoststream-zig/` — Original Zig prototype
- `archive/nvenc/` — NVIDIA Video Codec SDK
- `archive/ffmpeg-nvenc/` — FFmpeg NVENC examples
- `archive/open-gpu-kernel-modules/` — NVIDIA open kernel modules

---

## License

MIT OR Apache-2.0
