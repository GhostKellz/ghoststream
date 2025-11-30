//! GhostStream C ABI Exports
//!
//! This module provides the C-compatible API for ghoststream.h.
//! Used by OBS, FFmpeg, GStreamer, and other applications.

const std = @import("std");
const root = @import("root.zig");
const nvenc = @import("nvenc.zig");

// ============================================================================
// Types matching ghoststream.h
// ============================================================================

pub const GhostStreamError = enum(c_int) {
    ok = 0,
    not_initialized = -1,
    nvenc_not_available = -2,
    cuda_init_failed = -3,
    encoder_create_failed = -4,
    invalid_config = -5,
    encode_failed = -6,
    out_of_memory = -7,
    device_not_found = -8,
    unsupported_codec = -9,
    unsupported_resolution = -10,
    capture_failed = -11,
};

pub const GpuInfo = extern struct {
    index: u32,
    name: [256]u8,
    compute_major: u32,
    compute_minor: u32,
    vram_mb: u64,
    supports_h264: bool,
    supports_hevc: bool,
    supports_av1: bool,
    supports_10bit: bool,
    dual_encoder: bool,
    max_width: u32,
    max_height: u32,
};

pub const EncoderConfig = extern struct {
    codec: u32,
    width: u32,
    height: u32,
    framerate_num: u32,
    framerate_den: u32,
    bitrate_kbps: u32,
    max_bitrate_kbps: u32,
    rc_mode: u32,
    preset: u32,
    pixel_format: u32,
    gop_length: u32,
    bframes: u8,
    lookahead: u8,
    low_latency: bool,
    gpu_index: u32,
};

pub const FrameTiming = extern struct {
    pts: i64,
    dts: i64,
    duration: i64,
    is_keyframe: bool,
};

pub const Packet = extern struct {
    data: ?[*]u8,
    size: usize,
    timing: FrameTiming,
    codec: u32,
    is_config: bool,
};

pub const EncoderStats = extern struct {
    frames_encoded: u64,
    frames_dropped: u64,
    avg_encode_time_ms: f64,
    avg_bitrate_kbps: f64,
    bytes_encoded: u64,
};

// ============================================================================
// Global State
// ============================================================================

var g_initialized: bool = false;
var g_allocator: std.mem.Allocator = std.heap.c_allocator;

// ============================================================================
// Library Initialization
// ============================================================================

export fn ghoststream_init() GhostStreamError {
    if (g_initialized) return .ok;

    // Try to load NVENC
    var loader = nvenc.getLoader();
    loader.load() catch |err| {
        return switch (err) {
            nvenc.NvencError.LibraryNotFound => .nvenc_not_available,
            nvenc.NvencError.SymbolNotFound => .nvenc_not_available,
            nvenc.NvencError.InitFailed => .cuda_init_failed,
            else => .nvenc_not_available,
        };
    };

    g_initialized = true;
    return .ok;
}

export fn ghoststream_deinit() void {
    if (!g_initialized) return;

    var loader = nvenc.getLoader();
    loader.unload();

    g_initialized = false;
}

export fn ghoststream_get_version() [*:0]const u8 {
    return "0.1.0";
}

export fn ghoststream_nvenc_available() bool {
    return nvenc.NvencLoader.isAvailable();
}

// ============================================================================
// GPU Detection
// ============================================================================

export fn ghoststream_get_gpu_count() u32 {
    if (!g_initialized) return 0;
    // TODO: Enumerate CUDA devices
    return 1; // Stub
}

export fn ghoststream_get_gpu_info(index: u32, info: ?*GpuInfo) GhostStreamError {
    if (!g_initialized) return .not_initialized;
    if (info == null) return .invalid_config;

    const out = info.?;

    // TODO: Get real GPU info via CUDA
    out.index = index;
    const name = "NVIDIA GeForce RTX (GhostStream)";
    @memcpy(out.name[0..name.len], name);
    out.name[name.len] = 0;
    out.compute_major = 8;
    out.compute_minor = 9;
    out.vram_mb = 12288;
    out.supports_h264 = true;
    out.supports_hevc = true;
    out.supports_av1 = true;
    out.supports_10bit = true;
    out.dual_encoder = true;
    out.max_width = 8192;
    out.max_height = 8192;

    return .ok;
}

// ============================================================================
// Encoder API
// ============================================================================

const EncoderHandle = struct {
    encoder: root.Encoder,
    allocator: std.mem.Allocator,
};

export fn ghoststream_encoder_config_default(config: ?*EncoderConfig) void {
    if (config == null) return;

    const out = config.?;
    out.codec = 2; // AV1
    out.width = 1920;
    out.height = 1080;
    out.framerate_num = 60;
    out.framerate_den = 1;
    out.bitrate_kbps = 20000;
    out.max_bitrate_kbps = 40000;
    out.rc_mode = 1; // VBR
    out.preset = 4; // P4
    out.pixel_format = 0; // NV12
    out.gop_length = 60;
    out.bframes = 2;
    out.lookahead = 0;
    out.low_latency = false;
    out.gpu_index = 0;
}

export fn ghoststream_encoder_create(
    config: ?*const EncoderConfig,
    encoder_out: ?*?*EncoderHandle,
) GhostStreamError {
    if (!g_initialized) return .not_initialized;
    if (config == null or encoder_out == null) return .invalid_config;

    const cfg = config.?;

    // Convert C config to Zig config
    const zig_config = root.EncoderConfig{
        .codec = @enumFromInt(cfg.codec),
        .width = cfg.width,
        .height = cfg.height,
        .framerate_num = cfg.framerate_num,
        .framerate_den = cfg.framerate_den,
        .bitrate_kbps = cfg.bitrate_kbps,
        .max_bitrate_kbps = cfg.max_bitrate_kbps,
        .rate_control = @enumFromInt(cfg.rc_mode),
        .preset = @enumFromInt(cfg.preset),
        .pixel_format = @enumFromInt(cfg.pixel_format),
        .gop_length = cfg.gop_length,
        .bframes = cfg.bframes,
        .lookahead = cfg.lookahead,
        .low_latency = cfg.low_latency,
    };

    // Create encoder
    const encoder = root.Encoder.init(zig_config, cfg.gpu_index) catch {
        return .encoder_create_failed;
    };

    // Allocate handle
    const handle = g_allocator.create(EncoderHandle) catch {
        return .out_of_memory;
    };

    handle.* = .{
        .encoder = encoder,
        .allocator = g_allocator,
    };

    encoder_out.?.* = handle;
    return .ok;
}

export fn ghoststream_encoder_destroy(encoder: ?*EncoderHandle) void {
    if (encoder == null) return;

    var handle = encoder.?;
    handle.encoder.deinit();
    handle.allocator.destroy(handle);
}

export fn ghoststream_encode_frame(
    encoder: ?*EncoderHandle,
    frame_data: ?[*]const u8,
    frame_size: usize,
    timing: ?*const FrameTiming,
    packet: ?*Packet,
) GhostStreamError {
    if (!g_initialized) return .not_initialized;
    if (encoder == null or frame_data == null or timing == null or packet == null) {
        return .invalid_config;
    }

    const handle = encoder.?;
    const t = timing.?;

    const zig_timing = root.FrameTiming{
        .pts = t.pts,
        .dts = t.dts,
        .duration = t.duration,
        .is_keyframe = t.is_keyframe,
    };

    // Encode
    const result = handle.encoder.encodeFrame(
        frame_data.?[0..frame_size],
        zig_timing,
    ) catch {
        return .encode_failed;
    };

    // Fill output packet
    const out = packet.?;
    if (result) |encoded| {
        // Allocate and copy data
        const data = g_allocator.alloc(u8, encoded.data.len) catch {
            return .out_of_memory;
        };
        @memcpy(data, encoded.data);

        out.data = data.ptr;
        out.size = data.len;
        out.timing = .{
            .pts = encoded.timing.pts,
            .dts = encoded.timing.dts,
            .duration = encoded.timing.duration,
            .is_keyframe = encoded.timing.is_keyframe,
        };
        out.codec = @intFromEnum(encoded.codec);
        out.is_config = encoded.is_sps_pps;
    } else {
        out.data = null;
        out.size = 0;
    }

    return .ok;
}

export fn ghoststream_encoder_flush(
    encoder: ?*EncoderHandle,
    packet: ?*Packet,
) GhostStreamError {
    if (!g_initialized) return .not_initialized;
    if (encoder == null or packet == null) return .invalid_config;

    const handle = encoder.?;
    const out = packet.?;

    const result = handle.encoder.flush() catch {
        return .encode_failed;
    };

    if (result) |encoded| {
        const data = g_allocator.alloc(u8, encoded.data.len) catch {
            return .out_of_memory;
        };
        @memcpy(data, encoded.data);

        out.data = data.ptr;
        out.size = data.len;
        out.codec = @intFromEnum(encoded.codec);
    } else {
        out.data = null;
        out.size = 0;
    }

    return .ok;
}

export fn ghoststream_encoder_get_stats(
    encoder: ?*EncoderHandle,
    stats: ?*EncoderStats,
) void {
    if (encoder == null or stats == null) return;

    const handle = encoder.?;
    const zig_stats = handle.encoder.getStats();
    const out = stats.?;

    out.frames_encoded = zig_stats.frames_encoded;
    out.frames_dropped = zig_stats.frames_dropped;
    out.avg_encode_time_ms = zig_stats.avg_encode_time_us;
    out.avg_bitrate_kbps = zig_stats.avg_bitrate_kbps;
    out.bytes_encoded = 0; // TODO
}

export fn ghoststream_packet_free(packet: ?*Packet) void {
    if (packet == null) return;

    const p = packet.?;
    if (p.data) |data| {
        g_allocator.free(data[0..p.size]);
        p.data = null;
        p.size = 0;
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

export fn ghoststream_error_string(err: GhostStreamError) [*:0]const u8 {
    return switch (err) {
        .ok => "Success",
        .not_initialized => "GhostStream not initialized",
        .nvenc_not_available => "NVENC not available",
        .cuda_init_failed => "CUDA initialization failed",
        .encoder_create_failed => "Failed to create encoder",
        .invalid_config => "Invalid configuration",
        .encode_failed => "Encoding failed",
        .out_of_memory => "Out of memory",
        .device_not_found => "GPU device not found",
        .unsupported_codec => "Unsupported codec",
        .unsupported_resolution => "Unsupported resolution",
        .capture_failed => "Screen capture failed",
    };
}

export fn ghoststream_frame_buffer_size(
    width: u32,
    height: u32,
    format: u32,
) usize {
    const pixels = @as(usize, width) * @as(usize, height);
    return switch (format) {
        0 => pixels + pixels / 2, // NV12: Y + UV
        1 => pixels + pixels / 2, // YUV420P
        2, 3 => pixels * 4, // RGBA/BGRA
        4 => pixels * 4, // ARGB10
        5 => pixels + pixels / 2, // P010 (10-bit NV12, but stored as 16-bit)
        else => pixels * 4,
    };
}
