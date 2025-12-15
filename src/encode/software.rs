//! Software (CPU) encoder via FFmpeg
//!
//! Provides H.264, HEVC, and AV1 encoding using CPU-based encoders:
//! - libx264 for H.264 (highly optimized, great AMD support)
//! - libx265 for H.265/HEVC (excellent quality)
//! - libsvtav1 for AV1 (best for AMD Zen4/5 with AVX-512)

use crate::config::EncoderConfig;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution};

use super::{Codec, Encoder, EncoderStats};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags as ScalerFlags};
use ffmpeg_next::Dictionary;
use std::time::Instant;

/// CPU encoder preset (affects speed vs quality tradeoff)
#[derive(Debug, Clone, Copy, Default)]
pub enum CpuPreset {
    /// Fastest encoding, lowest quality
    Ultrafast,
    /// Very fast encoding
    Superfast,
    /// Fast encoding
    Veryfast,
    /// Faster than medium
    Faster,
    /// Fast encoding
    Fast,
    /// Balanced speed/quality
    #[default]
    Medium,
    /// Better quality, slower
    Slow,
    /// High quality
    Slower,
    /// Best quality, slowest
    Veryslow,
    /// Maximum quality (not recommended for realtime)
    Placebo,
}

impl CpuPreset {
    /// Get FFmpeg preset string for x264/x265
    pub fn to_x26x_preset(&self) -> &'static str {
        match self {
            CpuPreset::Ultrafast => "ultrafast",
            CpuPreset::Superfast => "superfast",
            CpuPreset::Veryfast => "veryfast",
            CpuPreset::Faster => "faster",
            CpuPreset::Fast => "fast",
            CpuPreset::Medium => "medium",
            CpuPreset::Slow => "slow",
            CpuPreset::Slower => "slower",
            CpuPreset::Veryslow => "veryslow",
            CpuPreset::Placebo => "placebo",
        }
    }

    /// Get SVT-AV1 preset (0-13, 0=slowest/best, 13=fastest)
    pub fn to_svtav1_preset(&self) -> &'static str {
        match self {
            CpuPreset::Ultrafast => "12",
            CpuPreset::Superfast => "11",
            CpuPreset::Veryfast => "10",
            CpuPreset::Faster => "9",
            CpuPreset::Fast => "8",
            CpuPreset::Medium => "6",
            CpuPreset::Slow => "4",
            CpuPreset::Slower => "2",
            CpuPreset::Veryslow => "1",
            CpuPreset::Placebo => "0",
        }
    }

    /// Recommended preset for realtime encoding at given FPS
    pub fn for_realtime(fps: u32, resolution: Resolution) -> Self {
        let pixels_per_second = resolution.pixels() as u64 * fps as u64;

        // Rough thresholds based on modern AMD CPUs
        if pixels_per_second > 500_000_000 {
            // 4K60+
            CpuPreset::Ultrafast
        } else if pixels_per_second > 250_000_000 {
            // 4K30 or 1440p60+
            CpuPreset::Superfast
        } else if pixels_per_second > 125_000_000 {
            // 1080p60+
            CpuPreset::Veryfast
        } else if pixels_per_second > 60_000_000 {
            // 1080p30 or 720p60
            CpuPreset::Faster
        } else {
            CpuPreset::Fast
        }
    }
}

/// Software encoder using FFmpeg CPU codecs
pub struct SoftwareEncoder {
    config: EncoderConfig,
    cpu_preset: CpuPreset,
    encoder: Option<ffmpeg::encoder::Video>,
    scaler: Option<Scaler>,
    stats: EncoderStats,
    frame_count: u64,
    start_time: Option<Instant>,
    input_resolution: Option<Resolution>,
    time_base: ffmpeg::Rational,
    threads: usize,
}

impl SoftwareEncoder {
    /// Create a new software encoder
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::FFmpeg(e.to_string()))?;

        // Verify codec is supported
        let encoder_name = Self::get_encoder_name(config.codec);
        if ffmpeg::encoder::find_by_name(encoder_name).is_none() {
            return Err(Error::CodecNotSupported(format!(
                "Software encoder {} not found. Install FFmpeg with {} support.",
                encoder_name,
                config.codec.display_name()
            )));
        }

        // Determine optimal thread count for AMD CPUs
        let threads = Self::optimal_thread_count();

        Ok(Self {
            config,
            cpu_preset: CpuPreset::default(),
            encoder: None,
            scaler: None,
            stats: EncoderStats::default(),
            frame_count: 0,
            start_time: None,
            input_resolution: None,
            time_base: ffmpeg::Rational::new(1, 1000),
            threads,
        })
    }

    /// Create with specific CPU preset
    pub fn with_preset(mut self, preset: CpuPreset) -> Self {
        self.cpu_preset = preset;
        self
    }

    /// Create with automatic preset for realtime encoding
    pub fn with_realtime_preset(mut self, fps: u32, resolution: Resolution) -> Self {
        self.cpu_preset = CpuPreset::for_realtime(fps, resolution);
        self
    }

    /// Get optimal thread count for encoding
    fn optimal_thread_count() -> usize {
        let cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);

        // For encoding, use most cores but leave some for capture/output
        // AMD 7950X3D has 16 cores, 9950X3D has 16 cores
        // Leave 2-4 cores free for system/capture
        cpus.saturating_sub(2).max(4)
    }

    /// Get FFmpeg encoder name for codec
    fn get_encoder_name(codec: Codec) -> &'static str {
        match codec {
            Codec::H264 => "libx264",
            Codec::Hevc => "libx265",
            Codec::Av1 => "libsvtav1",
        }
    }

    /// Initialize encoder with specific input resolution
    fn init_encoder(&mut self, input_width: u32, input_height: u32) -> Result<()> {
        let encoder_name = Self::get_encoder_name(self.config.codec);

        // Find the encoder
        let codec = ffmpeg::encoder::find_by_name(encoder_name)
            .ok_or_else(|| Error::EncoderInit(format!("Encoder {} not found", encoder_name)))?;

        // Determine output resolution
        let (out_width, out_height) = if let Some(res) = self.config.resolution {
            (res.width, res.height)
        } else {
            (input_width, input_height)
        };

        // Create encoder context
        let context = ffmpeg::codec::context::Context::new_with_codec(codec);
        let mut encoder = context
            .encoder()
            .video()
            .map_err(|e| Error::EncoderInit(e.to_string()))?;

        // Set basic parameters
        encoder.set_width(out_width);
        encoder.set_height(out_height);

        // Use YUV420P for software encoders (most compatible)
        encoder.set_format(Pixel::YUV420P);
        encoder.set_time_base(ffmpeg::Rational::new(1, 1000));
        self.time_base = ffmpeg::Rational::new(1, 1000);

        // Set framerate from config
        encoder.set_frame_rate(Some(ffmpeg::Rational::new(
            self.config.framerate.num as i32,
            self.config.framerate.den as i32,
        )));

        // Set GOP size
        encoder.set_gop(self.config.gop_size);

        // Set max B-frames
        encoder.set_max_b_frames(self.config.b_frames as usize);

        // Build encoder options
        let mut opts = Dictionary::new();

        // Set preset
        match self.config.codec {
            Codec::H264 | Codec::Hevc => {
                opts.set("preset", self.cpu_preset.to_x26x_preset());
            }
            Codec::Av1 => {
                opts.set("preset", self.cpu_preset.to_svtav1_preset());
            }
        }

        // Thread count (x265 has issues with >16 frame-threads)
        let thread_count = match self.config.codec {
            Codec::Hevc => self.threads.min(16),
            _ => self.threads,
        };
        opts.set("threads", &thread_count.to_string());

        // Rate control
        match self.config.rate_control {
            crate::config::RateControl::Cbr => {
                match self.config.codec {
                    Codec::H264 | Codec::Hevc => {
                        opts.set("b", &format!("{}k", self.config.bitrate_kbps));
                        // For CBR-like behavior with x264/x265
                        opts.set("maxrate", &format!("{}k", self.config.bitrate_kbps));
                        opts.set("bufsize", &format!("{}k", self.config.bitrate_kbps * 2));
                    }
                    Codec::Av1 => {
                        // SVT-AV1 rate control
                        opts.set("rc", "1"); // VBR mode (CBR not well supported)
                        opts.set("tbr", &format!("{}k", self.config.bitrate_kbps));
                    }
                }
            }
            crate::config::RateControl::Vbr => {
                opts.set("b", &format!("{}k", self.config.bitrate_kbps));
                if let Some(max) = self.config.max_bitrate_kbps {
                    opts.set("maxrate", &format!("{}k", max));
                }
            }
            crate::config::RateControl::Cqp { qp } => {
                match self.config.codec {
                    Codec::H264 => {
                        opts.set("qp", &qp.to_string());
                    }
                    Codec::Hevc => {
                        opts.set("qp", &qp.to_string());
                    }
                    Codec::Av1 => {
                        opts.set("qp", &qp.to_string());
                    }
                }
            }
            crate::config::RateControl::Crf { crf } => {
                opts.set("crf", &crf.to_string());
            }
        }

        // Codec-specific optimizations for AMD
        match self.config.codec {
            Codec::H264 => {
                // x264 AMD optimizations
                opts.set("tune", "zerolatency"); // Low latency for streaming
                // Enable SIMD optimizations (auto-detected, but explicit)
            }
            Codec::Hevc => {
                // x265 AMD optimizations
                // Use x265-params for specific settings
                let x265_params = format!(
                    "log-level=warning:frame-threads={}:lookahead-slices=4:rc-lookahead=20",
                    thread_count.min(8) // x265 frame-threads max is typically 8-16
                );
                opts.set("x265-params", &x265_params);
            }
            Codec::Av1 => {
                // SVT-AV1 is excellent on AMD Zen4/5 with AVX-512
                opts.set("svtav1-params", "tune=0"); // PSNR tuning
                // Enable film grain synthesis for better quality
            }
        }

        // Low latency options
        if matches!(
            self.config.tuning,
            crate::config::EncoderTuning::LowLatency
                | crate::config::EncoderTuning::UltraLowLatency
        ) {
            match self.config.codec {
                Codec::H264 | Codec::Hevc => {
                    opts.set("tune", "zerolatency");
                }
                Codec::Av1 => {
                    // SVT-AV1 low latency
                    opts.set("svtav1-params", "rc=1:pred-struct=1");
                }
            }
        }

        // Open encoder
        let opened = encoder
            .open_with(opts)
            .map_err(|e| Error::EncoderInit(format!("Failed to open encoder: {}", e)))?;

        self.encoder = Some(opened);
        self.input_resolution = Some(Resolution::new(input_width, input_height));

        // Create scaler if input != output resolution or format conversion needed
        let scaler = Scaler::get(
            Pixel::NV12, // Input from capture is typically NV12 or BGRA
            input_width,
            input_height,
            Pixel::YUV420P, // Software encoders prefer YUV420P
            out_width,
            out_height,
            ScalerFlags::BILINEAR,
        )
        .map_err(|e| Error::EncoderInit(format!("Failed to create scaler: {}", e)))?;
        self.scaler = Some(scaler);

        self.start_time = Some(Instant::now());

        tracing::info!(
            "Software encoder initialized: {} {}x{} @ {}kbps (preset: {}, threads: {})",
            Self::get_encoder_name(self.config.codec),
            out_width,
            out_height,
            self.config.bitrate_kbps,
            match self.config.codec {
                Codec::Av1 => self.cpu_preset.to_svtav1_preset(),
                _ => self.cpu_preset.to_x26x_preset(),
            },
            self.threads
        );

        Ok(())
    }

    /// Convert frame format to FFmpeg pixel format
    fn to_ffmpeg_format(format: FrameFormat) -> Pixel {
        match format {
            FrameFormat::Nv12 => Pixel::NV12,
            FrameFormat::Yuv420p => Pixel::YUV420P,
            FrameFormat::Yuv444p => Pixel::YUV444P,
            FrameFormat::Bgra => Pixel::BGRA,
            FrameFormat::Rgba => Pixel::RGBA,
            FrameFormat::Rgb24 => Pixel::RGB24,
            FrameFormat::P010 => Pixel::P010LE,
        }
    }
}

impl Encoder for SoftwareEncoder {
    fn init(&mut self) -> Result<()> {
        // Actual initialization happens on first frame
        Ok(())
    }

    fn encode(&mut self, frame: &Frame) -> Result<Option<Packet>> {
        // Initialize encoder on first frame
        if self.encoder.is_none() {
            self.init_encoder(frame.width, frame.height)?;
        }

        let encoder = self.encoder.as_mut().unwrap();
        let encode_start = Instant::now();

        // Create FFmpeg video frame
        let mut video_frame = ffmpeg::frame::Video::new(
            Self::to_ffmpeg_format(frame.format),
            frame.width,
            frame.height,
        );

        // Copy frame data based on format
        match frame.format {
            FrameFormat::Nv12 => {
                let y_size = (frame.width * frame.height) as usize;
                let uv_size = y_size / 2;
                if frame.data.len() >= y_size + uv_size {
                    video_frame.data_mut(0)[..y_size].copy_from_slice(&frame.data[..y_size]);
                    video_frame.data_mut(1)[..uv_size]
                        .copy_from_slice(&frame.data[y_size..y_size + uv_size]);
                }
            }
            FrameFormat::Yuv420p => {
                let y_size = (frame.width * frame.height) as usize;
                let uv_size = y_size / 4;
                if frame.data.len() >= y_size + uv_size * 2 {
                    video_frame.data_mut(0)[..y_size].copy_from_slice(&frame.data[..y_size]);
                    video_frame.data_mut(1)[..uv_size]
                        .copy_from_slice(&frame.data[y_size..y_size + uv_size]);
                    video_frame.data_mut(2)[..uv_size]
                        .copy_from_slice(&frame.data[y_size + uv_size..y_size + uv_size * 2]);
                }
            }
            _ => {
                let plane_size = video_frame.data(0).len().min(frame.data.len());
                video_frame.data_mut(0)[..plane_size].copy_from_slice(&frame.data[..plane_size]);
            }
        }

        video_frame.set_pts(Some(frame.pts));

        // Scale/convert to YUV420P for encoding
        let frame_to_encode = if let Some(ref mut scaler) = self.scaler {
            let mut scaled = ffmpeg::frame::Video::empty();
            scaler
                .run(&video_frame, &mut scaled)
                .map_err(|e| Error::EncodingFailed(format!("Scaling failed: {}", e)))?;
            scaled.set_pts(video_frame.pts());
            scaled
        } else {
            video_frame
        };

        // Send frame to encoder
        encoder
            .send_frame(&frame_to_encode)
            .map_err(|e| Error::EncodingFailed(format!("Failed to send frame: {}", e)))?;

        // Receive encoded packet
        let mut ffmpeg_packet = ffmpeg::Packet::empty();
        match encoder.receive_packet(&mut ffmpeg_packet) {
            Ok(_) => {
                let encode_time = encode_start.elapsed();

                self.frame_count += 1;
                self.stats.frames_encoded = self.frame_count;
                self.stats.bytes_output += ffmpeg_packet.size() as u64;

                let encode_ms = encode_time.as_secs_f64() * 1000.0;
                self.stats.avg_encode_time_ms =
                    self.stats.avg_encode_time_ms * 0.95 + encode_ms * 0.05;

                if let Some(start) = self.start_time {
                    let elapsed = start.elapsed().as_secs_f64();
                    if elapsed > 0.0 {
                        self.stats.current_bitrate_kbps =
                            ((self.stats.bytes_output as f64 * 8.0) / elapsed / 1000.0) as u64;
                    }
                }

                let packet = Packet {
                    data: ffmpeg_packet.data().map(|d| d.to_vec()).unwrap_or_default(),
                    pts: ffmpeg_packet.pts().unwrap_or(0),
                    dts: ffmpeg_packet.dts().unwrap_or(0),
                    duration: ffmpeg_packet.duration(),
                    is_keyframe: ffmpeg_packet.is_key(),
                    flags: 0,
                };

                Ok(Some(packet))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => Ok(None),
            Err(e) => Err(Error::EncodingFailed(format!(
                "Failed to receive packet: {}",
                e
            ))),
        }
    }

    fn flush(&mut self) -> Result<Vec<Packet>> {
        let encoder = match self.encoder.as_mut() {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        encoder
            .send_eof()
            .map_err(|e| Error::EncodingFailed(format!("Failed to send EOF: {}", e)))?;

        let mut packets = Vec::new();
        loop {
            let mut ffmpeg_packet = ffmpeg::Packet::empty();
            match encoder.receive_packet(&mut ffmpeg_packet) {
                Ok(_) => {
                    self.stats.bytes_output += ffmpeg_packet.size() as u64;
                    packets.push(Packet {
                        data: ffmpeg_packet.data().map(|d| d.to_vec()).unwrap_or_default(),
                        pts: ffmpeg_packet.pts().unwrap_or(0),
                        dts: ffmpeg_packet.dts().unwrap_or(0),
                        duration: ffmpeg_packet.duration(),
                        is_keyframe: ffmpeg_packet.is_key(),
                        flags: 0,
                    });
                }
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => continue,
                Err(e) => {
                    tracing::warn!("Error during flush: {}", e);
                    break;
                }
            }
        }

        tracing::info!(
            "Software encoder flushed: {} frames, {} bytes, avg {:.2}ms/frame",
            self.stats.frames_encoded,
            self.stats.bytes_output,
            self.stats.avg_encode_time_ms
        );

        Ok(packets)
    }

    fn stats(&self) -> EncoderStats {
        self.stats.clone()
    }

    fn codec_params(&self) -> Option<CodecParams> {
        let encoder = self.encoder.as_ref()?;

        let extradata = unsafe {
            let ptr = (*encoder.as_ptr()).extradata;
            let size = (*encoder.as_ptr()).extradata_size as usize;
            if !ptr.is_null() && size > 0 {
                std::slice::from_raw_parts(ptr, size).to_vec()
            } else {
                Vec::new()
            }
        };

        let resolution = if let Some(res) = self.config.resolution {
            res
        } else if let Some(res) = self.input_resolution {
            res
        } else {
            Resolution::new(encoder.width(), encoder.height())
        };

        Some(CodecParams {
            codec: self.config.codec,
            extradata,
            resolution,
            framerate: self.config.framerate,
            time_base_num: self.time_base.numerator(),
            time_base_den: self.time_base.denominator(),
            bitrate: (self.config.bitrate_kbps as i64) * 1000,
        })
    }

    fn reconfigure(&mut self, config: &EncoderConfig) -> Result<()> {
        self.config = config.clone();
        tracing::info!("Software encoder config updated: {}kbps", config.bitrate_kbps);
        Ok(())
    }
}

impl Drop for SoftwareEncoder {
    fn drop(&mut self) {
        if self.encoder.is_some() {
            tracing::debug!("Dropping software encoder");
        }
    }
}

// ============================================================================
// Software Encoder Detection
// ============================================================================

/// Check if software encoder is available for codec
pub fn is_available(codec: Codec) -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }

    let encoder_name = SoftwareEncoder::get_encoder_name(codec);
    ffmpeg::encoder::find_by_name(encoder_name).is_some()
}

/// Check for x264 support
pub fn has_x264() -> bool {
    is_available(Codec::H264)
}

/// Check for x265 support
pub fn has_x265() -> bool {
    is_available(Codec::Hevc)
}

/// Check for SVT-AV1 support
pub fn has_svtav1() -> bool {
    is_available(Codec::Av1)
}

/// Get CPU information for encoding
pub fn get_cpu_info() -> CpuInfo {
    let cpus = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4);

    // Try to detect AMD Zen architecture
    let (is_amd, model) = detect_amd_cpu();

    CpuInfo {
        cores: cpus,
        is_amd,
        model,
        has_avx2: cfg!(target_feature = "avx2"),
        has_avx512: cfg!(target_feature = "avx512f"),
        x264_available: has_x264(),
        x265_available: has_x265(),
        svtav1_available: has_svtav1(),
    }
}

/// Try to detect AMD CPU
fn detect_amd_cpu() -> (bool, Option<String>) {
    // Try to read /proc/cpuinfo on Linux
    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        let is_amd = cpuinfo.contains("AMD");
        let model = cpuinfo
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string());
        return (is_amd, model);
    }
    (false, None)
}

/// CPU information
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub cores: usize,
    pub is_amd: bool,
    pub model: Option<String>,
    pub has_avx2: bool,
    pub has_avx512: bool,
    pub x264_available: bool,
    pub x265_available: bool,
    pub svtav1_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_detection() {
        let info = get_cpu_info();
        println!("CPU Info: {:?}", info);
    }

    #[test]
    fn test_software_encoder_creation() {
        if !has_x264() {
            println!("x264 not available, skipping test");
            return;
        }

        let config = EncoderConfig::default();
        let encoder = SoftwareEncoder::new(config);
        assert!(encoder.is_ok());
    }
}
