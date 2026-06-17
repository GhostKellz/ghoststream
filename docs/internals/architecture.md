# Architecture

GhostStream is organized as a pipeline:

```text
Capture -> Processing -> Encoding -> Output
```

## Capture

Capture implementations receive frames from xdg-desktop-portal/PipeWire, DMA-BUF paths, webcams, or audio sources.

## Processing

Processing covers scaling, colorspace conversion, HDR transforms, and background effects.

## Encoding

Encoder backends wrap NVENC, QSV, AMF, and software encoders through FFmpeg and backend-specific adapters.

## Output

Output implementations write to virtual cameras, files, RTMP endpoints, and SRT endpoints.
