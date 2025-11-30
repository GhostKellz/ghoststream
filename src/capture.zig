//! Screen Capture Module
//!
//! Provides screen capture via:
//! - Wayland DMA-BUF (xdg-desktop-portal)
//! - PipeWire screen capture
//! - DRM/KMS direct capture (for compositors)
//!
//! Zero-copy path via DMA-BUF for maximum performance.

const std = @import("std");
const root = @import("root.zig");

// ============================================================================
// Capture Types
// ============================================================================

/// Capture source type
pub const CaptureSource = enum(u8) {
    display, // Full display/monitor
    window, // Specific window
    region, // Screen region
};

/// Capture backend
pub const CaptureBackend = enum(u8) {
    auto, // Auto-detect best backend
    pipewire, // PipeWire screen capture
    portal, // xdg-desktop-portal
    drm, // DRM/KMS direct (compositor only)
    x11, // X11 XShm/XComposite (legacy)
};

/// Pixel format for captured frames
pub const CaptureFormat = enum(u8) {
    bgra, // BGRA 8-bit (most common)
    rgba, // RGBA 8-bit
    rgbx, // RGBX 8-bit (no alpha)
    nv12, // Hardware NV12 (if DMA-BUF)
    unknown,
};

/// Capture configuration
pub const CaptureConfig = struct {
    source: CaptureSource = .display,
    backend: CaptureBackend = .auto,
    display_index: u32 = 0,
    target_fps: u32 = 60,
    capture_cursor: bool = true,
    use_dma_buf: bool = true, // Zero-copy when available
    width: ?u32 = null, // null = native resolution
    height: ?u32 = null,
};

/// Captured frame
pub const CapturedFrame = struct {
    /// Pixel data (CPU memory) or null if DMA-BUF
    data: ?[]u8 = null,
    /// Frame width
    width: u32,
    /// Frame height
    height: u32,
    /// Row stride in bytes
    stride: u32,
    /// Pixel format
    format: CaptureFormat,
    /// Timestamp in microseconds
    timestamp_us: i64,
    /// DMA-BUF file descriptor (if zero-copy)
    dma_buf_fd: ?i32 = null,
    /// DMA-BUF modifier
    dma_buf_modifier: u64 = 0,
    /// DMA-BUF offset
    dma_buf_offset: u32 = 0,
    /// Allocator used for data (if CPU memory)
    allocator: ?std.mem.Allocator = null,

    pub fn deinit(self: *CapturedFrame) void {
        if (self.data) |data| {
            if (self.allocator) |alloc| {
                alloc.free(data);
            }
        }
        if (self.dma_buf_fd) |fd| {
            std.posix.close(fd);
        }
        self.* = undefined;
    }

    pub fn isZeroCopy(self: *const CapturedFrame) bool {
        return self.dma_buf_fd != null;
    }
};

/// Capture statistics
pub const CaptureStats = struct {
    frames_captured: u64 = 0,
    frames_dropped: u64 = 0,
    avg_capture_time_us: f64 = 0,
    last_capture_time_us: u64 = 0,
    zero_copy_frames: u64 = 0,
};

// ============================================================================
// Capture Errors
// ============================================================================

pub const CaptureError = error{
    NotInitialized,
    BackendNotAvailable,
    PermissionDenied,
    NoDisplayFound,
    CaptureFailed,
    DmaBufNotSupported,
    PortalError,
    PipeWireError,
    OutOfMemory,
};

// ============================================================================
// Screen Capture Implementation
// ============================================================================

pub const ScreenCapture = struct {
    config: CaptureConfig,
    backend: CaptureBackend,
    initialized: bool = false,
    stats: CaptureStats = .{},

    // Backend-specific handles
    pipewire_stream: ?*anyopaque = null,
    portal_session: ?*anyopaque = null,
    drm_fd: ?i32 = null,

    // Frame info
    frame_width: u32 = 0,
    frame_height: u32 = 0,
    frame_format: CaptureFormat = .unknown,

    const Self = @This();

    /// Create a new screen capture instance
    pub fn init(config: CaptureConfig) CaptureError!Self {
        var capture = Self{
            .config = config,
            .backend = config.backend,
        };

        // Auto-detect backend if needed
        if (capture.backend == .auto) {
            capture.backend = detectBestBackend();
        }

        // Initialize selected backend
        switch (capture.backend) {
            .pipewire => try capture.initPipeWire(),
            .portal => try capture.initPortal(),
            .drm => try capture.initDrm(),
            .x11 => try capture.initX11(),
            .auto => unreachable,
        }

        capture.initialized = true;
        return capture;
    }

    /// Shutdown capture
    pub fn deinit(self: *Self) void {
        if (!self.initialized) return;

        switch (self.backend) {
            .pipewire => self.deinitPipeWire(),
            .portal => self.deinitPortal(),
            .drm => self.deinitDrm(),
            .x11 => self.deinitX11(),
            .auto => {},
        }

        self.initialized = false;
    }

    /// Capture a single frame
    pub fn captureFrame(self: *Self, allocator: std.mem.Allocator) CaptureError!CapturedFrame {
        if (!self.initialized) return CaptureError.NotInitialized;

        const start_time = std.time.microTimestamp();

        const frame = switch (self.backend) {
            .pipewire => try self.capturePipeWire(allocator),
            .portal => try self.capturePortal(allocator),
            .drm => try self.captureDrm(allocator),
            .x11 => try self.captureX11(allocator),
            .auto => unreachable,
        };

        // Update stats
        const capture_time = @as(u64, @intCast(std.time.microTimestamp() - start_time));
        self.stats.frames_captured += 1;
        self.stats.last_capture_time_us = capture_time;
        self.stats.avg_capture_time_us = self.stats.avg_capture_time_us * 0.95 + @as(f64, @floatFromInt(capture_time)) * 0.05;
        if (frame.isZeroCopy()) {
            self.stats.zero_copy_frames += 1;
        }

        return frame;
    }

    /// Get capture statistics
    pub fn getStats(self: *const Self) CaptureStats {
        return self.stats;
    }

    // ========================================================================
    // Backend Detection
    // ========================================================================

    fn detectBestBackend() CaptureBackend {
        // Check environment for Wayland
        const wayland_display = std.posix.getenv("WAYLAND_DISPLAY");
        const xdg_session = std.posix.getenv("XDG_SESSION_TYPE");

        if (wayland_display != null or (xdg_session != null and std.mem.eql(u8, xdg_session.?, "wayland"))) {
            // Try PipeWire first (best for Wayland)
            if (isPipeWireAvailable()) {
                return .pipewire;
            }
            // Fall back to portal
            return .portal;
        }

        // X11 fallback
        if (std.posix.getenv("DISPLAY") != null) {
            return .x11;
        }

        // Last resort: DRM
        return .drm;
    }

    fn isPipeWireAvailable() bool {
        // Check if PipeWire is running
        const runtime_dir = std.posix.getenv("XDG_RUNTIME_DIR") orelse return false;

        var buf: [256]u8 = undefined;
        const socket_path = std.fmt.bufPrint(&buf, "{s}/pipewire-0", .{runtime_dir}) catch return false;

        const stat_result = std.fs.cwd().statFile(socket_path);
        return stat_result != error.FileNotFound;
    }

    // ========================================================================
    // PipeWire Backend
    // ========================================================================

    fn initPipeWire(self: *Self) CaptureError!void {
        // TODO: Initialize PipeWire screen capture
        // 1. pw_init()
        // 2. Connect to PipeWire
        // 3. Create screen capture stream
        // 4. Start stream
        _ = self;
    }

    fn deinitPipeWire(self: *Self) void {
        _ = self;
    }

    fn capturePipeWire(self: *Self, allocator: std.mem.Allocator) CaptureError!CapturedFrame {
        // TODO: Actual PipeWire capture
        // For now, return a mock frame
        _ = self;

        const width: u32 = 1920;
        const height: u32 = 1080;
        const stride = width * 4;
        const size = stride * height;

        const data = allocator.alloc(u8, size) catch return CaptureError.OutOfMemory;
        @memset(data, 0);

        return CapturedFrame{
            .data = data,
            .width = width,
            .height = height,
            .stride = stride,
            .format = .bgra,
            .timestamp_us = std.time.microTimestamp(),
            .allocator = allocator,
        };
    }

    // ========================================================================
    // Portal Backend (xdg-desktop-portal)
    // ========================================================================

    fn initPortal(self: *Self) CaptureError!void {
        // TODO: Initialize xdg-desktop-portal screen capture
        // 1. Connect to D-Bus
        // 2. Request screen capture via org.freedesktop.portal.ScreenCast
        // 3. Open PipeWire stream
        _ = self;
    }

    fn deinitPortal(self: *Self) void {
        _ = self;
    }

    fn capturePortal(self: *Self, allocator: std.mem.Allocator) CaptureError!CapturedFrame {
        // Delegate to PipeWire (portal uses PipeWire for actual capture)
        return self.capturePipeWire(allocator);
    }

    // ========================================================================
    // DRM Backend (direct kernel capture)
    // ========================================================================

    fn initDrm(self: *Self) CaptureError!void {
        // TODO: Initialize DRM capture
        // 1. Open /dev/dri/card0
        // 2. Set up DRM framebuffer capture
        _ = self;
    }

    fn deinitDrm(self: *Self) void {
        if (self.drm_fd) |fd| {
            std.posix.close(fd);
            self.drm_fd = null;
        }
    }

    fn captureDrm(self: *Self, allocator: std.mem.Allocator) CaptureError!CapturedFrame {
        _ = self;

        // Mock frame
        const width: u32 = 1920;
        const height: u32 = 1080;
        const stride = width * 4;
        const size = stride * height;

        const data = allocator.alloc(u8, size) catch return CaptureError.OutOfMemory;
        @memset(data, 0);

        return CapturedFrame{
            .data = data,
            .width = width,
            .height = height,
            .stride = stride,
            .format = .bgra,
            .timestamp_us = std.time.microTimestamp(),
            .allocator = allocator,
        };
    }

    // ========================================================================
    // X11 Backend (legacy)
    // ========================================================================

    fn initX11(self: *Self) CaptureError!void {
        // TODO: Initialize X11 capture
        _ = self;
    }

    fn deinitX11(self: *Self) void {
        _ = self;
    }

    fn captureX11(self: *Self, allocator: std.mem.Allocator) CaptureError!CapturedFrame {
        _ = self;

        // Mock frame
        const width: u32 = 1920;
        const height: u32 = 1080;
        const stride = width * 4;
        const size = stride * height;

        const data = allocator.alloc(u8, size) catch return CaptureError.OutOfMemory;
        @memset(data, 0);

        return CapturedFrame{
            .data = data,
            .width = width,
            .height = height,
            .stride = stride,
            .format = .bgra,
            .timestamp_us = std.time.microTimestamp(),
            .allocator = allocator,
        };
    }
};

// ============================================================================
// DMA-BUF Helpers
// ============================================================================

/// DMA-BUF frame info
pub const DmaBufInfo = struct {
    fd: i32,
    width: u32,
    height: u32,
    stride: u32,
    format: u32, // DRM_FORMAT_*
    modifier: u64,
    offset: u32,
};

/// Import DMA-BUF to CUDA for NVENC
pub fn importDmaBufToCuda(info: DmaBufInfo) CaptureError!*anyopaque {
    // TODO: Use cuGraphicsEGLRegisterImage or cuImportExternalMemory
    // to import DMA-BUF into CUDA for zero-copy NVENC input
    _ = info;
    return CaptureError.DmaBufNotSupported;
}

// ============================================================================
// Tests
// ============================================================================

test "capture config defaults" {
    const config = CaptureConfig{};
    try std.testing.expectEqual(CaptureSource.display, config.source);
    try std.testing.expectEqual(CaptureBackend.auto, config.backend);
    try std.testing.expectEqual(@as(u32, 60), config.target_fps);
}

test "backend detection" {
    const backend = ScreenCapture.detectBestBackend();
    try std.testing.expect(backend != .auto);
}

test "captured frame deinit" {
    var frame = CapturedFrame{
        .width = 100,
        .height = 100,
        .stride = 400,
        .format = .bgra,
        .timestamp_us = 0,
    };
    frame.deinit();
}
