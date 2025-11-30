//! GhostStream CLI - NVENC Video Encoding Engine
//!
//! Command-line interface for testing and using GhostStream.

const std = @import("std");
const gs = @import("ghoststream");

pub fn main() !void {
    const allocator = std.heap.page_allocator;

    var args = try std.process.argsWithAllocator(allocator);
    defer args.deinit();

    // Skip program name
    _ = args.skip();

    const command = args.next() orelse {
        printUsage();
        return;
    };

    if (std.mem.eql(u8, command, "version")) {
        printVersion();
    } else if (std.mem.eql(u8, command, "info")) {
        try printSystemInfo(allocator);
    } else if (std.mem.eql(u8, command, "test")) {
        try runEncoderTest(allocator);
    } else if (std.mem.eql(u8, command, "help")) {
        printUsage();
    } else {
        std.debug.print("Unknown command: {s}\n", .{command});
        printUsage();
    }
}

fn printVersion() void {
    std.debug.print(
        \\GhostStream {s}
        \\
        \\NVIDIA GPU Video Engine for Linux
        \\Codecs: AV1, HEVC, H.264 (via NVENC)
        \\
    , .{gs.getVersion()});
}

fn printUsage() void {
    std.debug.print(
        \\GhostStream - NVIDIA GPU Video Engine
        \\
        \\Usage: ghoststream <command> [options]
        \\
        \\Commands:
        \\  version    Show version information
        \\  info       Display GPU and NVENC capabilities
        \\  test       Run encoder test
        \\  help       Show this help message
        \\
        \\Examples:
        \\  ghoststream info
        \\  ghoststream test
        \\
    , .{});
}

fn printSystemInfo(allocator: std.mem.Allocator) !void {
    std.debug.print("GhostStream System Information\n", .{});
    std.debug.print("==============================\n\n", .{});

    // Check NVENC availability
    const nvenc_available = gs.isNvencAvailable();
    std.debug.print("NVENC Available: {s}\n\n", .{if (nvenc_available) "Yes" else "No"});

    // Detect GPUs
    const gpus = try gs.detectGpus(allocator);
    defer allocator.free(gpus);

    std.debug.print("Detected GPUs: {d}\n\n", .{gpus.len});

    for (gpus) |gpu| {
        std.debug.print("GPU {d}: {s}\n", .{ gpu.index, gpu.getName() });
        std.debug.print("  Compute Capability: {d}.{d}\n", .{ gpu.compute_capability_major, gpu.compute_capability_minor });
        std.debug.print("  VRAM: {d} MB\n", .{gpu.vram_mb});
        std.debug.print("  NVENC Capabilities:\n", .{});
        std.debug.print("    H.264:      {s}\n", .{if (gpu.nvenc_caps.supports_h264) "Yes" else "No"});
        std.debug.print("    HEVC:       {s}\n", .{if (gpu.nvenc_caps.supports_hevc) "Yes" else "No"});
        std.debug.print("    AV1:        {s}\n", .{if (gpu.nvenc_caps.supports_av1) "Yes" else "No"});
        std.debug.print("    10-bit:     {s}\n", .{if (gpu.nvenc_caps.supports_10bit) "Yes" else "No"});
        std.debug.print("    Lookahead:  {s}\n", .{if (gpu.nvenc_caps.supports_lookahead) "Yes" else "No"});
        std.debug.print("    Dual Enc:   {s}\n", .{if (gpu.nvenc_caps.dual_encoder) "Yes" else "No"});
        std.debug.print("    Max Res:    {d}x{d}\n", .{ gpu.nvenc_caps.max_encode_width, gpu.nvenc_caps.max_encode_height });
        std.debug.print("\n", .{});
    }
}

fn runEncoderTest(allocator: std.mem.Allocator) !void {
    std.debug.print("GhostStream Encoder Test\n", .{});
    std.debug.print("========================\n\n", .{});

    // Create encoder config
    const config = gs.EncoderConfig{
        .codec = .av1,
        .width = 1920,
        .height = 1080,
        .framerate_num = 60,
        .framerate_den = 1,
        .bitrate_kbps = 20000,
        .rate_control = .vbr,
        .preset = .p4_slow,
    };

    std.debug.print("Configuration:\n", .{});
    std.debug.print("  Codec:      AV1\n", .{});
    std.debug.print("  Resolution: {d}x{d}\n", .{ config.width, config.height });
    std.debug.print("  Framerate:  {d} fps\n", .{config.framerate_num / config.framerate_den});
    std.debug.print("  Bitrate:    {d} kbps\n", .{config.bitrate_kbps});
    std.debug.print("  Est. Latency: {d:.1} ms\n\n", .{config.estimatedLatencyMs()});

    // Initialize encoder
    std.debug.print("Initializing encoder...\n", .{});
    var encoder = try gs.Encoder.init(config, 0);
    defer encoder.deinit();

    std.debug.print("Encoder initialized successfully!\n\n", .{});

    // Simulate encoding a few frames
    std.debug.print("Simulating encode (10 frames)...\n", .{});

    const frame_data = try allocator.alloc(u8, 1920 * 1080 * 3 / 2); // NV12 size
    defer allocator.free(frame_data);

    var i: u32 = 0;
    while (i < 10) : (i += 1) {
        const timing = gs.FrameTiming{
            .pts = @as(i64, @intCast(i)) * 16667, // ~60fps in microseconds
            .dts = @as(i64, @intCast(i)) * 16667,
            .duration = 16667,
            .is_keyframe = (i == 0),
        };

        _ = try encoder.encodeFrame(frame_data, timing);
    }

    const stats = encoder.getStats();
    std.debug.print("Encoded {d} frames\n\n", .{stats.frames_encoded});

    std.debug.print("Test completed successfully!\n", .{});
    std.debug.print("Note: Actual NVENC encoding is TODO - this is a stub test.\n", .{});
}

test "main sanity" {
    // Basic test that imports work
    try std.testing.expect(gs.isNvencAvailable());
}
