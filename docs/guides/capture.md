# Capture Guide

GhostStream supports portal-based Wayland capture, DMA-BUF capture paths, webcam capture, and PipeWire audio capture.

## Portal Capture

Portal capture is the default Wayland-safe path. It delegates screen/window selection to xdg-desktop-portal and receives frames through PipeWire.

## DMA-BUF

DMA-BUF capture is intended for lower-copy video paths when the compositor and driver support it.

## Webcam

Webcam capture uses V4L2 through the `v4l` crate.

## Audio

Audio capture uses PipeWire monitor/input streams. Keep PipeWire and the desktop portal implementation updated when debugging capture failures.
