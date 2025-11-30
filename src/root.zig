//! GhostStream - Core NVIDIA GPU Video Engine
//!
//! A high-performance, low-overhead GPU video pipeline for Linux.
//! Provides frame capture, encoding (NVENC AV1/HEVC/H.264), and real-time streaming.
//!
//! ## Features
//! - NVENC encoding: H.264, HEVC, AV1
//! - Zero-copy DMA-BUF capture
//! - Wayland/PipeWire native capture
//! - C ABI for OBS/FFmpeg integration
//!
//! ## Example
//! ```zig
//! const gs = @import("ghoststream");
//!
//! var encoder = try gs.Encoder.init(.{ .codec = .av1 }, 0);
//! defer encoder.deinit();
//!
//! const packet = try encoder.encodeFrame(frame_data, timing);
//! ```

const std = @import("std");
const builtin = @import("builtin");

// Sub-modules
pub const nvenc = @import("nvenc.zig");
pub const capture = @import("capture.zig");
pub const cabi = @import("cabi.zig");

// ============================================================================
// Public API Types
// ============================================================================

/// Supported video codecs
pub const Codec = enum(u8) {
    h264,
    hevc,
    av1,
};

/// Encoding preset (quality vs speed tradeoff)
pub const Preset = enum(u8) {
    p1_fastest,
    p2_fast,
    p3_medium,
    p4_slow,
    p5_slower,
    p6_slowest,
    p7_quality,
};

/// Rate control mode
pub const RateControl = enum(u8) {
    cbr, // Constant bitrate
    vbr, // Variable bitrate
    cqp, // Constant QP
};

/// Pixel format for input frames
pub const PixelFormat = enum(u8) {
    nv12, // Y plane + interleaved UV (most common for NVENC)
    yuv420p, // Planar YUV 4:2:0
    rgba, // RGBA 8-bit
    bgra, // BGRA 8-bit
    argb10, // 10-bit ARGB (HDR)
};

/// GPU device information
pub const GpuDevice = struct {
    index: u32,
    name: [256]u8,
    name_len: usize,
    compute_capability_major: u32,
    compute_capability_minor: u32,
    vram_mb: u64,
    nvenc_caps: NvencCapabilities,

    pub fn getName(self: *const GpuDevice) []const u8 {
        return self.name[0..self.name_len];
    }
};

/// NVENC hardware capabilities
pub const NvencCapabilities = struct {
    supports_h264: bool,
    supports_hevc: bool,
    supports_av1: bool,
    supports_10bit: bool,
    supports_lookahead: bool,
    max_encode_width: u32,
    max_encode_height: u32,
    dual_encoder: bool, // RTX 40/50 series
    tensor_core_gen: u8,
};

/// Encoder configuration
pub const EncoderConfig = struct {
    codec: Codec = .av1,
    width: u32 = 1920,
    height: u32 = 1080,
    framerate_num: u32 = 60,
    framerate_den: u32 = 1,
    bitrate_kbps: u32 = 20000,
    max_bitrate_kbps: u32 = 40000,
    rate_control: RateControl = .vbr,
    preset: Preset = .p4_slow,
    pixel_format: PixelFormat = .nv12,
    gop_length: u32 = 60, // Keyframe interval (frames)
    bframes: u8 = 2,
    lookahead: u8 = 0, // 0 = disabled, 1-32 = frames
    low_latency: bool = false,

    /// Calculate approximate latency in milliseconds
    pub fn estimatedLatencyMs(self: EncoderConfig) f32 {
        var latency: f32 = 0;
        // Base encode latency
        latency += 1000.0 / @as(f32, @floatFromInt(self.framerate_num / self.framerate_den));
        // B-frame latency
        latency += @as(f32, @floatFromInt(self.bframes)) * (1000.0 / @as(f32, @floatFromInt(self.framerate_num)));
        // Lookahead latency
        if (self.lookahead > 0) {
            latency += @as(f32, @floatFromInt(self.lookahead)) * (1000.0 / @as(f32, @floatFromInt(self.framerate_num)));
        }
        return latency;
    }
};

/// Frame timing information
pub const FrameTiming = struct {
    pts: i64, // Presentation timestamp (microseconds)
    dts: i64, // Decode timestamp (microseconds)
    duration: i64, // Frame duration (microseconds)
    is_keyframe: bool,
};

/// Encoded packet output
pub const EncodedPacket = struct {
    data: []const u8,
    timing: FrameTiming,
    codec: Codec,
    is_sps_pps: bool, // Contains codec config data
};

/// Capture source type
pub const CaptureSource = enum(u8) {
    display, // Full display capture
    window, // Specific window
    region, // Screen region
    dma_buf, // DMA-BUF from Wayland/PipeWire
};

/// Capture configuration
pub const CaptureConfig = struct {
    source: CaptureSource = .display,
    display_index: u32 = 0,
    target_fps: u32 = 60,
    capture_cursor: bool = true,
    use_dma_buf: bool = true, // Zero-copy when available
};

// ============================================================================
// Error Handling
// ============================================================================

pub const Error = error{
    NvencNotAvailable,
    CudaInitFailed,
    EncoderCreateFailed,
    EncoderConfigFailed,
    EncodeFrameFailed,
    InvalidConfig,
    OutOfMemory,
    DeviceNotFound,
    CaptureInitFailed,
    CaptureFailed,
    DmaBufNotSupported,
    UnsupportedCodec,
    UnsupportedResolution,
};

// ============================================================================
// Core Encoder Interface
// ============================================================================

/// NVENC encoder instance
pub const Encoder = struct {
    config: EncoderConfig,
    device_index: u32,
    initialized: bool,
    frame_count: u64,

    // Internal NVENC handles (opaque pointers for C interop)
    nvenc_handle: ?*anyopaque,
    cuda_context: ?*anyopaque,

    const Self = @This();

    /// Create a new encoder with the specified configuration
    pub fn init(config: EncoderConfig, device_index: u32) Error!Self {
        var encoder = Self{
            .config = config,
            .device_index = device_index,
            .initialized = false,
            .frame_count = 0,
            .nvenc_handle = null,
            .cuda_context = null,
        };

        // TODO: Initialize CUDA context
        // TODO: Load NVENC API
        // TODO: Create encoder session

        encoder.initialized = true;
        return encoder;
    }

    /// Destroy the encoder and release resources
    pub fn deinit(self: *Self) void {
        if (self.initialized) {
            // TODO: Destroy NVENC session
            // TODO: Release CUDA context
            self.initialized = false;
        }
    }

    /// Encode a single frame
    pub fn encodeFrame(
        self: *Self,
        frame_data: []const u8,
        timing: FrameTiming,
    ) Error!?EncodedPacket {
        if (!self.initialized) {
            return Error.EncoderCreateFailed;
        }

        // TODO: Actual NVENC encoding
        // 1. Upload frame to GPU (or use DMA-BUF for zero-copy)
        // 2. Submit to NVENC
        // 3. Retrieve encoded bitstream

        self.frame_count += 1;

        // Placeholder return
        _ = frame_data;
        _ = timing;
        return null;
    }

    /// Flush the encoder (get remaining frames)
    pub fn flush(self: *Self) Error!?EncodedPacket {
        if (!self.initialized) {
            return Error.EncoderCreateFailed;
        }
        // TODO: Flush NVENC
        return null;
    }

    /// Get encoder statistics
    pub fn getStats(self: *const Self) EncoderStats {
        return EncoderStats{
            .frames_encoded = self.frame_count,
            .config = self.config,
        };
    }
};

/// Encoder performance statistics
pub const EncoderStats = struct {
    frames_encoded: u64,
    frames_dropped: u64 = 0,
    avg_encode_time_us: f64 = 0,
    avg_bitrate_kbps: f64 = 0,
    config: EncoderConfig,
};

// ============================================================================
// Capture Interface (Wayland/DMA-BUF)
// ============================================================================

/// Frame capture instance
pub const Capture = struct {
    config: CaptureConfig,
    initialized: bool,
    frame_count: u64,

    const Self = @This();

    pub fn init(config: CaptureConfig) Error!Self {
        return Self{
            .config = config,
            .initialized = true,
            .frame_count = 0,
        };
    }

    pub fn deinit(self: *Self) void {
        self.initialized = false;
    }

    /// Capture a single frame
    pub fn captureFrame(self: *Self, allocator: std.mem.Allocator) Error!CapturedFrame {
        if (!self.initialized) {
            return Error.CaptureInitFailed;
        }

        self.frame_count += 1;

        // TODO: Actual capture via Wayland portal or PipeWire
        // For now, return a placeholder
        const data = try allocator.alloc(u8, 1920 * 1080 * 4);
        return CapturedFrame{
            .data = data,
            .width = 1920,
            .height = 1080,
            .stride = 1920 * 4,
            .format = .rgba,
            .timestamp_us = std.time.microTimestamp(),
        };
    }
};

/// Captured frame data
pub const CapturedFrame = struct {
    data: []u8,
    width: u32,
    height: u32,
    stride: u32,
    format: PixelFormat,
    timestamp_us: i64,
    dma_buf_fd: ?i32 = null, // For zero-copy

    pub fn deinit(self: *CapturedFrame, allocator: std.mem.Allocator) void {
        allocator.free(self.data);
    }
};

// ============================================================================
// GPU Detection
// ============================================================================

/// Detect available NVIDIA GPUs with NVENC support
pub fn detectGpus(allocator: std.mem.Allocator) Error![]GpuDevice {
    // TODO: Use CUDA API to enumerate devices
    // For now, return a mock device for development

    var devices = try allocator.alloc(GpuDevice, 1);
    var name: [256]u8 = undefined;
    const mock_name = "NVIDIA GeForce RTX 5090 (Mock)";
    @memcpy(name[0..mock_name.len], mock_name);

    devices[0] = GpuDevice{
        .index = 0,
        .name = name,
        .name_len = mock_name.len,
        .compute_capability_major = 10,
        .compute_capability_minor = 0,
        .vram_mb = 32768,
        .nvenc_caps = NvencCapabilities{
            .supports_h264 = true,
            .supports_hevc = true,
            .supports_av1 = true,
            .supports_10bit = true,
            .supports_lookahead = true,
            .max_encode_width = 8192,
            .max_encode_height = 8192,
            .dual_encoder = true,
            .tensor_core_gen = 5,
        },
    };

    return devices;
}

/// Check if NVENC is available on the system
pub fn isNvencAvailable() bool {
    // TODO: Actually check for libnvidia-encode.so
    return true;
}

// ============================================================================
// Version Information
// ============================================================================

pub const version = struct {
    pub const major = 0;
    pub const minor = 1;
    pub const patch = 0;
    pub const string = "0.1.0";
};

/// Get GhostStream version string
pub fn getVersion() []const u8 {
    return version.string;
}

// ============================================================================
// Tests
// ============================================================================

test "encoder config latency estimation" {
    const config = EncoderConfig{
        .framerate_num = 60,
        .framerate_den = 1,
        .bframes = 2,
        .lookahead = 0,
    };

    const latency = config.estimatedLatencyMs();
    try std.testing.expect(latency > 0);
    try std.testing.expect(latency < 100); // Should be reasonable
}

test "detect gpus" {
    const allocator = std.testing.allocator;
    const gpus = try detectGpus(allocator);
    defer allocator.free(gpus);

    try std.testing.expect(gpus.len > 0);
    try std.testing.expect(gpus[0].nvenc_caps.supports_av1);
}

test "encoder init and deinit" {
    const config = EncoderConfig{};
    var encoder = try Encoder.init(config, 0);
    defer encoder.deinit();

    try std.testing.expect(encoder.initialized);
}
