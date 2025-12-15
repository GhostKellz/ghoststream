//! Configuration types for GhostStream

use crate::encode::Codec;
use crate::processing::HdrConfig;
use crate::types::{FrameFormat, Framerate, Resolution};
use serde::{Deserialize, Serialize};

/// Capture configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Target framerate
    pub framerate: Framerate,
    /// Show cursor in capture
    pub show_cursor: bool,
    /// Capture audio (for PipeWire)
    pub capture_audio: bool,
    /// Preferred capture backend
    pub backend: CaptureBackend,
    /// Use DMA-BUF zero-copy if available
    pub prefer_dmabuf: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            framerate: Framerate::FPS_60,
            show_cursor: true,
            capture_audio: false,
            backend: CaptureBackend::Auto,
            prefer_dmabuf: true,
        }
    }
}

impl CaptureConfig {
    pub fn with_fps(mut self, fps: u32) -> Self {
        self.framerate = Framerate::new(fps, 1);
        self
    }

    pub fn with_show_cursor(mut self, show: bool) -> Self {
        self.show_cursor = show;
        self
    }

    pub fn with_backend(mut self, backend: CaptureBackend) -> Self {
        self.backend = backend;
        self
    }
}

/// Capture backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptureBackend {
    /// Auto-detect best backend
    #[default]
    Auto,
    /// xdg-desktop-portal (recommended for Wayland)
    Portal,
    /// Direct PipeWire
    PipeWire,
    /// Wlroots DMA-BUF export (for wlroots compositors)
    WlrExport,
}

/// Encoder configuration
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Video codec
    pub codec: Codec,
    /// Output resolution (None = same as input)
    pub resolution: Option<Resolution>,
    /// Target framerate
    pub framerate: Framerate,
    /// Target bitrate in kbps
    pub bitrate_kbps: u32,
    /// Maximum bitrate in kbps (for VBR)
    pub max_bitrate_kbps: Option<u32>,
    /// Rate control mode
    pub rate_control: RateControl,
    /// Encoder preset (speed vs quality)
    pub preset: EncoderPreset,
    /// Tuning mode
    pub tuning: EncoderTuning,
    /// GOP size (keyframe interval in frames)
    pub gop_size: u32,
    /// B-frames count
    pub b_frames: u32,
    /// Enable lookahead
    pub lookahead: Option<u32>,
    /// Output pixel format
    pub pixel_format: FrameFormat,
    /// Profile (codec-specific)
    pub profile: Option<String>,
    /// Level (codec-specific)
    pub level: Option<String>,
    /// HDR configuration (None for SDR)
    pub hdr: Option<HdrConfig>,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            resolution: None,
            framerate: Framerate::FPS_60,
            bitrate_kbps: 6000,
            max_bitrate_kbps: None,
            rate_control: RateControl::Vbr,
            preset: EncoderPreset::Medium,
            tuning: EncoderTuning::HighQuality,
            gop_size: 120, // 2 seconds at 60fps
            b_frames: 2,
            lookahead: None,
            pixel_format: FrameFormat::Nv12,
            profile: None,
            level: None,
            hdr: None, // SDR by default
        }
    }
}

impl EncoderConfig {
    pub fn with_codec(mut self, codec: Codec) -> Self {
        self.codec = codec;
        self
    }

    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = Some(Resolution::new(width, height));
        self
    }

    pub fn with_bitrate_kbps(mut self, bitrate: u32) -> Self {
        self.bitrate_kbps = bitrate;
        self
    }

    pub fn with_framerate(mut self, fps: u32) -> Self {
        self.framerate = Framerate::new(fps, 1);
        self
    }

    pub fn with_preset(mut self, preset: EncoderPreset) -> Self {
        self.preset = preset;
        self
    }

    pub fn with_rate_control(mut self, rc: RateControl) -> Self {
        self.rate_control = rc;
        self
    }

    pub fn with_gop_size(mut self, gop: u32) -> Self {
        self.gop_size = gop;
        self
    }

    pub fn with_tuning(mut self, tuning: EncoderTuning) -> Self {
        self.tuning = tuning;
        self
    }

    /// Enable HDR10 encoding
    pub fn with_hdr10(mut self) -> Self {
        self.hdr = Some(HdrConfig::hdr10());
        self.pixel_format = FrameFormat::P010;
        self
    }

    /// Enable HDR with custom config
    pub fn with_hdr(mut self, hdr_config: HdrConfig) -> Self {
        if hdr_config.bit_depth >= 10 {
            self.pixel_format = FrameFormat::P010;
        }
        self.hdr = Some(hdr_config);
        self
    }

    /// Enable HLG HDR encoding
    pub fn with_hlg(mut self) -> Self {
        self.hdr = Some(HdrConfig::hlg());
        self.pixel_format = FrameFormat::P010;
        self
    }

    /// Check if HDR is enabled
    pub fn is_hdr(&self) -> bool {
        self.hdr.as_ref().map(|h| h.is_hdr()).unwrap_or(false)
    }

    /// Apply a preset configuration
    pub fn from_preset(preset: Preset) -> Self {
        preset.into()
    }
}

/// Rate control mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RateControl {
    /// Constant bitrate
    Cbr,
    /// Variable bitrate
    #[default]
    Vbr,
    /// Constant QP
    Cqp { qp: u8 },
    /// Constant rate factor (quality-based)
    Crf { crf: u8 },
}

/// Encoder preset (speed vs quality tradeoff)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncoderPreset {
    /// Fastest encoding, lowest quality
    Fastest,
    /// Fast encoding
    Fast,
    /// Balanced
    #[default]
    Medium,
    /// Higher quality, slower
    Slow,
    /// Best quality, slowest
    Slowest,
}

impl EncoderPreset {
    /// Convert to NVENC preset name
    pub fn to_nvenc_preset(&self) -> &'static str {
        match self {
            EncoderPreset::Fastest => "p1",
            EncoderPreset::Fast => "p3",
            EncoderPreset::Medium => "p4",
            EncoderPreset::Slow => "p5",
            EncoderPreset::Slowest => "p7",
        }
    }
}

/// Encoder tuning mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncoderTuning {
    /// High quality (default)
    #[default]
    HighQuality,
    /// Low latency (streaming)
    LowLatency,
    /// Ultra low latency (real-time)
    UltraLowLatency,
    /// Lossless
    Lossless,
}

impl EncoderTuning {
    pub fn to_nvenc_tuning(&self) -> &'static str {
        match self {
            EncoderTuning::HighQuality => "hq",
            EncoderTuning::LowLatency => "ll",
            EncoderTuning::UltraLowLatency => "ull",
            EncoderTuning::Lossless => "lossless",
        }
    }
}

/// High-level presets for common use cases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    /// Discord 720p30 - Low bandwidth
    Discord720p,
    /// 1080p60 streaming
    Stream1080p60,
    /// 1440p60 high quality
    Quality1440p60,
    /// 1440p120 gaming
    Gaming1440p120,
    /// 4K60 ultra quality
    Ultra4K60,
    /// 4K120 maximum
    Maximum4K120,
    /// Low latency streaming
    LowLatency,
    /// Recording (high quality, large files)
    Recording,
    /// HDR10 4K60 (10-bit P010)
    Hdr10_4K60,
    /// HDR10 1440p60 (10-bit P010)
    Hdr10_1440p60,
}

impl From<Preset> for EncoderConfig {
    fn from(preset: Preset) -> Self {
        match preset {
            Preset::Discord720p => EncoderConfig {
                codec: Codec::H264,
                resolution: Some(Resolution::HD_720P),
                framerate: Framerate::FPS_30,
                bitrate_kbps: 3000,
                preset: EncoderPreset::Fast,
                tuning: EncoderTuning::LowLatency,
                gop_size: 60,
                ..Default::default()
            },
            Preset::Stream1080p60 => EncoderConfig {
                codec: Codec::H264,
                resolution: Some(Resolution::FHD_1080P),
                framerate: Framerate::FPS_60,
                bitrate_kbps: 6000,
                preset: EncoderPreset::Medium,
                tuning: EncoderTuning::HighQuality,
                gop_size: 120,
                ..Default::default()
            },
            Preset::Quality1440p60 => EncoderConfig {
                codec: Codec::Hevc,
                resolution: Some(Resolution::QHD_1440P),
                framerate: Framerate::FPS_60,
                bitrate_kbps: 12000,
                preset: EncoderPreset::Slow,
                tuning: EncoderTuning::HighQuality,
                gop_size: 120,
                ..Default::default()
            },
            Preset::Gaming1440p120 => EncoderConfig {
                codec: Codec::Hevc,
                resolution: Some(Resolution::QHD_1440P),
                framerate: Framerate::FPS_120,
                bitrate_kbps: 15000,
                preset: EncoderPreset::Fast,
                tuning: EncoderTuning::LowLatency,
                gop_size: 240,
                ..Default::default()
            },
            Preset::Ultra4K60 => EncoderConfig {
                codec: Codec::Av1,
                resolution: Some(Resolution::UHD_4K),
                framerate: Framerate::FPS_60,
                bitrate_kbps: 25000,
                preset: EncoderPreset::Slow,
                tuning: EncoderTuning::HighQuality,
                gop_size: 120,
                ..Default::default()
            },
            Preset::Maximum4K120 => EncoderConfig {
                codec: Codec::Av1,
                resolution: Some(Resolution::UHD_4K),
                framerate: Framerate::FPS_120,
                bitrate_kbps: 35000,
                preset: EncoderPreset::Medium,
                tuning: EncoderTuning::HighQuality,
                gop_size: 240,
                ..Default::default()
            },
            Preset::LowLatency => EncoderConfig {
                codec: Codec::H264,
                resolution: None,
                framerate: Framerate::FPS_60,
                bitrate_kbps: 8000,
                preset: EncoderPreset::Fastest,
                tuning: EncoderTuning::UltraLowLatency,
                gop_size: 30,
                b_frames: 0,
                ..Default::default()
            },
            Preset::Recording => EncoderConfig {
                codec: Codec::Hevc,
                resolution: None,
                framerate: Framerate::FPS_60,
                bitrate_kbps: 50000,
                preset: EncoderPreset::Slowest,
                tuning: EncoderTuning::HighQuality,
                gop_size: 300,
                b_frames: 3,
                lookahead: Some(20),
                ..Default::default()
            },
            Preset::Hdr10_4K60 => EncoderConfig {
                codec: Codec::Hevc,
                resolution: Some(Resolution::UHD_4K),
                framerate: Framerate::FPS_60,
                bitrate_kbps: 35000,
                preset: EncoderPreset::Slow,
                tuning: EncoderTuning::HighQuality,
                gop_size: 120,
                pixel_format: FrameFormat::P010,
                hdr: Some(HdrConfig::hdr10()),
                profile: Some("main10".to_string()),
                ..Default::default()
            },
            Preset::Hdr10_1440p60 => EncoderConfig {
                codec: Codec::Hevc,
                resolution: Some(Resolution::QHD_1440P),
                framerate: Framerate::FPS_60,
                bitrate_kbps: 20000,
                preset: EncoderPreset::Slow,
                tuning: EncoderTuning::HighQuality,
                gop_size: 120,
                pixel_format: FrameFormat::P010,
                hdr: Some(HdrConfig::hdr10()),
                profile: Some("main10".to_string()),
                ..Default::default()
            },
        }
    }
}
