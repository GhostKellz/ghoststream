# Encoding Guide

GhostStream exposes hardware and software encoder backends behind a common API.

## Hardware Backends

- NVIDIA NVENC for H.264, HEVC, and AV1 on supported GPUs
- Intel QSV
- AMD AMF

## Software Backends

Software encoding targets x264, x265, and SVT-AV1 through FFmpeg where available.

## Selection

Use automatic backend selection for normal operation. Force a backend only when testing hardware support or validating a known deployment profile.
