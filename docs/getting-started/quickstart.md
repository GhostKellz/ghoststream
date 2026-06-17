# Quickstart

## Requirements

- Linux with Wayland
- PipeWire and xdg-desktop-portal
- FFmpeg development libraries
- Rust stable

For NVIDIA NVENC, install NVIDIA drivers and FFmpeg with NVENC support.

## Build

```bash
cargo build --release
```

## Run

```bash
ghoststream info
ghoststream capture --output camera
ghoststream capture --output recording.mkv --codec hevc --bitrate 8000
```

## Use as a Library

```toml
[dependencies]
ghoststream = { git = "https://github.com/ghostkellz/ghoststream" }
```
