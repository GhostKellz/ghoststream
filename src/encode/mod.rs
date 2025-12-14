//! Video encoding module
//!
//! Provides hardware-accelerated encoding via NVENC and software encoding
//! via x264/x265/SVT-AV1 through FFmpeg.

pub mod nvenc;
pub mod software;

use crate::config::EncoderConfig;
use crate::error::Result;
use crate::types::{CodecParams, Frame, Packet};

pub use nvenc::NvencEncoder;
pub use software::{CpuPreset, SoftwareEncoder};

/// Supported video codecs
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Codec {
    /// H.264/AVC - Widest compatibility
    #[default]
    H264,
    /// H.265/HEVC - Better compression
    Hevc,
    /// AV1 - Best compression (RTX 40+ required)
    Av1,
}

impl Codec {
    /// Get FFmpeg encoder name for NVENC
    pub fn nvenc_encoder_name(&self) -> &'static str {
        match self {
            Codec::H264 => "h264_nvenc",
            Codec::Hevc => "hevc_nvenc",
            Codec::Av1 => "av1_nvenc",
        }
    }

    /// Get human-readable name
    pub fn display_name(&self) -> &'static str {
        match self {
            Codec::H264 => "H.264 (AVC)",
            Codec::Hevc => "H.265 (HEVC)",
            Codec::Av1 => "AV1",
        }
    }

    /// Minimum NVIDIA GPU architecture required
    pub fn min_gpu_arch(&self) -> &'static str {
        match self {
            Codec::H264 => "Kepler (GTX 600+)",
            Codec::Hevc => "Maxwell (GTX 900+)",
            Codec::Av1 => "Ada Lovelace (RTX 4000+)",
        }
    }
}

impl std::fmt::Display for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Trait for video encoders
pub trait Encoder {
    /// Initialize the encoder
    fn init(&mut self) -> Result<()>;

    /// Encode a frame
    fn encode(&mut self, frame: &Frame) -> Result<Option<Packet>>;

    /// Flush remaining frames
    fn flush(&mut self) -> Result<Vec<Packet>>;

    /// Get encoder statistics
    fn stats(&self) -> EncoderStats;

    /// Get codec parameters for muxing (extradata, resolution, etc.)
    fn codec_params(&self) -> Option<CodecParams>;

    /// Reconfigure encoder (if supported)
    fn reconfigure(&mut self, config: &EncoderConfig) -> Result<()>;
}

/// Encoder backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncoderBackend {
    /// Automatically select best available (NVENC > Software)
    #[default]
    Auto,
    /// Force NVIDIA NVENC hardware encoding
    Nvenc,
    /// Force CPU software encoding (x264/x265/SVT-AV1)
    Software,
}

/// Create an encoder based on configuration
pub fn create_encoder(config: EncoderConfig) -> Result<Box<dyn Encoder>> {
    create_encoder_with_backend(config, EncoderBackend::Auto)
}

/// Create an encoder with specific backend
pub fn create_encoder_with_backend(
    config: EncoderConfig,
    backend: EncoderBackend,
) -> Result<Box<dyn Encoder>> {
    match backend {
        EncoderBackend::Auto => {
            // Try NVENC first, fall back to software
            if nvenc::is_available() && nvenc::supports_codec(config.codec) {
                tracing::info!("Using NVENC hardware encoder");
                let encoder = NvencEncoder::new(config)?;
                Ok(Box::new(encoder))
            } else if software::is_available(config.codec) {
                tracing::info!("NVENC not available, using software encoder");
                let encoder = SoftwareEncoder::new(config)?;
                Ok(Box::new(encoder))
            } else {
                Err(crate::error::Error::CodecNotSupported(format!(
                    "No encoder available for {}",
                    config.codec.display_name()
                )))
            }
        }
        EncoderBackend::Nvenc => {
            let encoder = NvencEncoder::new(config)?;
            Ok(Box::new(encoder))
        }
        EncoderBackend::Software => {
            let encoder = SoftwareEncoder::new(config)?;
            Ok(Box::new(encoder))
        }
    }
}

/// Encoder statistics
#[derive(Debug, Clone, Default)]
pub struct EncoderStats {
    /// Frames encoded
    pub frames_encoded: u64,
    /// Total bytes output
    pub bytes_output: u64,
    /// Average encoding time in ms
    pub avg_encode_time_ms: f64,
    /// Current bitrate in kbps
    pub current_bitrate_kbps: u64,
    /// Encoder queue depth
    pub queue_depth: u32,
}

/// Information about available encoders
#[derive(Debug, Clone)]
pub struct EncoderInfo {
    /// Is NVENC available?
    pub nvenc_available: bool,
    /// Supported NVENC codecs
    pub nvenc_codecs: Vec<Codec>,
    /// GPU name (if detected)
    pub gpu_name: Option<String>,
    /// Driver version
    pub driver_version: Option<String>,
    /// Supports AV1 (NVENC)?
    pub nvenc_av1: bool,
    /// Supports dual encoder?
    pub dual_encoder: bool,
    /// Software encoder info
    pub software: SoftwareEncoderInfo,
    /// CPU info
    pub cpu: Option<software::CpuInfo>,
}

/// Software encoder availability
#[derive(Debug, Clone, Default)]
pub struct SoftwareEncoderInfo {
    /// x264 available for H.264
    pub x264: bool,
    /// x265 available for HEVC
    pub x265: bool,
    /// SVT-AV1 available for AV1
    pub svtav1: bool,
}

/// Get information about available encoders
pub fn get_info() -> EncoderInfo {
    let nvenc_available = nvenc::is_available();

    let mut nvenc_codecs = Vec::new();
    if nvenc_available {
        nvenc_codecs.push(Codec::H264);

        if nvenc::supports_codec(Codec::Hevc) {
            nvenc_codecs.push(Codec::Hevc);
        }

        if nvenc::supports_codec(Codec::Av1) {
            nvenc_codecs.push(Codec::Av1);
        }
    }

    let software = SoftwareEncoderInfo {
        x264: software::has_x264(),
        x265: software::has_x265(),
        svtav1: software::has_svtav1(),
    };

    let cpu = Some(software::get_cpu_info());

    EncoderInfo {
        nvenc_available,
        nvenc_codecs,
        gpu_name: nvenc::get_gpu_name(),
        driver_version: nvenc::get_driver_version(),
        nvenc_av1: nvenc::supports_codec(Codec::Av1),
        dual_encoder: nvenc::has_dual_encoder(),
        software,
        cpu,
    }
}

// Keep old field names for compatibility
impl EncoderInfo {
    /// Backward compatible: all supported codecs (NVENC + Software)
    pub fn supported_codecs(&self) -> Vec<Codec> {
        let mut codecs = self.nvenc_codecs.clone();
        if self.software.x264 && !codecs.contains(&Codec::H264) {
            codecs.push(Codec::H264);
        }
        if self.software.x265 && !codecs.contains(&Codec::Hevc) {
            codecs.push(Codec::Hevc);
        }
        if self.software.svtav1 && !codecs.contains(&Codec::Av1) {
            codecs.push(Codec::Av1);
        }
        codecs
    }

    /// Backward compatible: AV1 support (any backend)
    pub fn av1_support(&self) -> bool {
        self.nvenc_av1 || self.software.svtav1
    }
}
