//! Common types used throughout GhostStream

use serde::{Deserialize, Serialize};

/// Video resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    // Common resolutions
    pub const SD_480P: Self = Self::new(854, 480);
    pub const HD_720P: Self = Self::new(1280, 720);
    pub const FHD_1080P: Self = Self::new(1920, 1080);
    pub const QHD_1440P: Self = Self::new(2560, 1440);
    pub const UHD_4K: Self = Self::new(3840, 2160);
    pub const UHD_8K: Self = Self::new(7680, 4320);

    /// Calculate total pixels
    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Calculate aspect ratio
    pub fn aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self::FHD_1080P
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Frame format / pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FrameFormat {
    /// NV12 - NVIDIA preferred format (Y plane + interleaved UV)
    Nv12,
    /// YUV420P - Planar YUV 4:2:0
    Yuv420p,
    /// YUV444P - Planar YUV 4:4:4
    Yuv444p,
    /// BGRA - 32-bit BGRA (common for desktop capture)
    Bgra,
    /// RGBA - 32-bit RGBA
    Rgba,
    /// RGB24 - 24-bit RGB
    Rgb24,
    /// P010 - 10-bit NV12 (HDR)
    P010,
}

impl FrameFormat {
    /// Bytes per pixel (approximate for planar formats)
    pub fn bytes_per_pixel(&self) -> f32 {
        match self {
            FrameFormat::Nv12 | FrameFormat::Yuv420p => 1.5,
            FrameFormat::Yuv444p => 3.0,
            FrameFormat::Bgra | FrameFormat::Rgba => 4.0,
            FrameFormat::Rgb24 => 3.0,
            FrameFormat::P010 => 3.0, // 10-bit = 1.5 * 2
        }
    }

    /// Is this a hardware-friendly format for NVENC?
    pub fn is_nvenc_native(&self) -> bool {
        matches!(self, FrameFormat::Nv12 | FrameFormat::P010)
    }
}

impl Default for FrameFormat {
    fn default() -> Self {
        FrameFormat::Nv12
    }
}

/// A video frame
#[derive(Debug)]
pub struct Frame {
    /// Raw frame data
    pub data: Vec<u8>,
    /// Frame width
    pub width: u32,
    /// Frame height
    pub height: u32,
    /// Row stride in bytes
    pub stride: u32,
    /// Pixel format
    pub format: FrameFormat,
    /// Presentation timestamp in microseconds
    pub pts: i64,
    /// Duration in microseconds
    pub duration: i64,
    /// Is this a keyframe?
    pub is_keyframe: bool,
    /// DMA-BUF file descriptor (for zero-copy)
    pub dmabuf_fd: Option<i32>,
}

impl Frame {
    /// Create a new frame with allocated buffer
    pub fn new(width: u32, height: u32, format: FrameFormat) -> Self {
        let size = (width as f32 * height as f32 * format.bytes_per_pixel()) as usize;
        Self {
            data: vec![0u8; size],
            width,
            height,
            stride: width * 4, // Default stride, adjust based on format
            format,
            pts: 0,
            duration: 0,
            is_keyframe: false,
            dmabuf_fd: None,
        }
    }

    /// Create a frame from existing data
    pub fn from_data(
        data: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
        format: FrameFormat,
    ) -> Self {
        Self {
            data,
            width,
            height,
            stride,
            format,
            pts: 0,
            duration: 0,
            is_keyframe: false,
            dmabuf_fd: None,
        }
    }

    /// Get resolution
    pub fn resolution(&self) -> Resolution {
        Resolution::new(self.width, self.height)
    }

    /// Is this a zero-copy frame (DMA-BUF)?
    pub fn is_zero_copy(&self) -> bool {
        self.dmabuf_fd.is_some()
    }

    /// Calculate frame size in bytes
    pub fn size_bytes(&self) -> usize {
        self.data.len()
    }
}

/// Encoded packet (output from encoder)
#[derive(Debug)]
pub struct Packet {
    /// Encoded data
    pub data: Vec<u8>,
    /// Presentation timestamp
    pub pts: i64,
    /// Decode timestamp
    pub dts: i64,
    /// Duration
    pub duration: i64,
    /// Is this a keyframe?
    pub is_keyframe: bool,
    /// Codec-specific flags
    pub flags: u32,
}

impl Packet {
    pub fn new(data: Vec<u8>, pts: i64, dts: i64, is_keyframe: bool) -> Self {
        Self {
            data,
            pts,
            dts,
            duration: 0,
            is_keyframe,
            flags: 0,
        }
    }

    /// Size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Framerate representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Framerate {
    pub num: u32,
    pub den: u32,
}

impl Framerate {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    // Common framerates
    pub const FPS_24: Self = Self::new(24, 1);
    pub const FPS_30: Self = Self::new(30, 1);
    pub const FPS_60: Self = Self::new(60, 1);
    pub const FPS_120: Self = Self::new(120, 1);
    pub const FPS_144: Self = Self::new(144, 1);
    pub const FPS_240: Self = Self::new(240, 1);

    /// Get framerate as f64
    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Get framerate as integer fps (numerator when den=1)
    pub fn fps(&self) -> u32 {
        if self.den == 0 {
            self.num
        } else {
            self.num / self.den
        }
    }

    /// Frame duration in microseconds
    pub fn frame_duration_us(&self) -> i64 {
        (1_000_000 * self.den as i64) / self.num as i64
    }
}

impl Default for Framerate {
    fn default() -> Self {
        Self::FPS_60
    }
}

impl std::fmt::Display for Framerate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.den == 1 {
            write!(f, "{} fps", self.num)
        } else {
            write!(f, "{:.2} fps", self.as_f64())
        }
    }
}

/// Codec parameters for muxing
#[derive(Debug, Clone)]
pub struct CodecParams {
    /// Codec type
    pub codec: crate::encode::Codec,
    /// Extradata (SPS/PPS for H.264, VPS/SPS/PPS for HEVC, etc.)
    pub extradata: Vec<u8>,
    /// Video resolution
    pub resolution: Resolution,
    /// Framerate
    pub framerate: Framerate,
    /// Timebase numerator
    pub time_base_num: i32,
    /// Timebase denominator
    pub time_base_den: i32,
    /// Bitrate in bits/sec
    pub bitrate: i64,
}

impl Default for CodecParams {
    fn default() -> Self {
        Self {
            codec: crate::encode::Codec::H264,
            extradata: Vec::new(),
            resolution: Resolution::FHD_1080P,
            framerate: Framerate::FPS_60,
            time_base_num: 1,
            time_base_den: 1000,
            bitrate: 6_000_000,
        }
    }
}

/// Statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Frames captured
    pub frames_captured: u64,
    /// Frames encoded
    pub frames_encoded: u64,
    /// Frames dropped
    pub frames_dropped: u64,
    /// Current encoding FPS
    pub encoding_fps: f64,
    /// Average encoding latency in ms
    pub avg_encode_latency_ms: f64,
    /// Current bitrate in kbps
    pub current_bitrate_kbps: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// GPU encoder utilization (0-100)
    pub gpu_encoder_util: u8,
}
