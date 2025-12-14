//! NVENC hardware encoder via FFmpeg
//!
//! Provides H.264, HEVC, and AV1 encoding using NVIDIA's NVENC.

use crate::config::EncoderConfig;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Frame, FrameFormat, Framerate, Packet, Resolution};

use super::{Codec, Encoder, EncoderStats};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags as ScalerFlags};
use ffmpeg_next::Dictionary;
use std::time::Instant;

/// NVENC encoder using FFmpeg
pub struct NvencEncoder {
    config: EncoderConfig,
    encoder: Option<ffmpeg::encoder::Video>,
    scaler: Option<Scaler>,
    stats: EncoderStats,
    frame_count: u64,
    start_time: Option<Instant>,
    input_resolution: Option<Resolution>,
    time_base: ffmpeg::Rational,
}

impl NvencEncoder {
    /// Create a new NVENC encoder
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::FFmpeg(e.to_string()))?;

        // Verify NVENC is available
        if !is_available() {
            return Err(Error::NvencNotAvailable(
                "NVENC not found. Ensure NVIDIA drivers and FFmpeg with NVENC support are installed.".into(),
            ));
        }

        // Verify codec is supported
        if !supports_codec(config.codec) {
            return Err(Error::CodecNotSupported(format!(
                "{} requires {}",
                config.codec.display_name(),
                config.codec.min_gpu_arch()
            )));
        }

        Ok(Self {
            config,
            encoder: None,
            scaler: None,
            stats: EncoderStats::default(),
            frame_count: 0,
            start_time: None,
            input_resolution: None,
            time_base: ffmpeg::Rational::new(1, 60), // Default, updated on init
        })
    }

    /// Initialize encoder with specific input resolution
    fn init_encoder(&mut self, input_width: u32, input_height: u32) -> Result<()> {
        let encoder_name = self.config.codec.nvenc_encoder_name();

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
        encoder.set_format(Pixel::NV12); // NVENC prefers NV12
        encoder.set_time_base(ffmpeg::Rational::new(1, 1000)); // ms timebase
        self.time_base = ffmpeg::Rational::new(1, 1000);

        // Set framerate
        encoder.set_frame_rate(Some(ffmpeg::Rational::new(60, 1))); // TODO: from config

        // Set GOP size
        encoder.set_gop(self.config.gop_size);

        // Set max B-frames
        encoder.set_max_b_frames(self.config.b_frames as usize);

        // Build encoder options
        let mut opts = Dictionary::new();

        // Preset
        opts.set("preset", self.config.preset.to_nvenc_preset());

        // Tuning
        opts.set("tune", self.config.tuning.to_nvenc_tuning());

        // Rate control
        match self.config.rate_control {
            crate::config::RateControl::Cbr => {
                opts.set("rc", "cbr");
                opts.set("b", &format!("{}k", self.config.bitrate_kbps));
            }
            crate::config::RateControl::Vbr => {
                opts.set("rc", "vbr");
                opts.set("b", &format!("{}k", self.config.bitrate_kbps));
                if let Some(max) = self.config.max_bitrate_kbps {
                    opts.set("maxrate", &format!("{}k", max));
                }
            }
            crate::config::RateControl::Cqp { qp } => {
                opts.set("rc", "constqp");
                opts.set("qp", &qp.to_string());
            }
            crate::config::RateControl::Crf { crf } => {
                opts.set("cq", &crf.to_string());
            }
        }

        // Lookahead
        if let Some(la) = self.config.lookahead {
            opts.set("rc-lookahead", &la.to_string());
        }

        // Low latency options
        if matches!(
            self.config.tuning,
            crate::config::EncoderTuning::LowLatency
                | crate::config::EncoderTuning::UltraLowLatency
        ) {
            opts.set("delay", "0");
            opts.set("zerolatency", "1");
        }

        // Open encoder
        let opened = encoder
            .open_with(opts)
            .map_err(|e| Error::EncoderInit(format!("Failed to open encoder: {}", e)))?;

        self.encoder = Some(opened);
        self.input_resolution = Some(Resolution::new(input_width, input_height));

        // Create scaler if input != output resolution
        if input_width != out_width || input_height != out_height {
            let scaler = Scaler::get(
                Pixel::NV12,
                input_width,
                input_height,
                Pixel::NV12,
                out_width,
                out_height,
                ScalerFlags::BILINEAR,
            )
            .map_err(|e| Error::EncoderInit(format!("Failed to create scaler: {}", e)))?;
            self.scaler = Some(scaler);
        }

        self.start_time = Some(Instant::now());

        tracing::info!(
            "NVENC encoder initialized: {} {}x{} @ {}kbps (preset: {}, tune: {})",
            self.config.codec,
            out_width,
            out_height,
            self.config.bitrate_kbps,
            self.config.preset.to_nvenc_preset(),
            self.config.tuning.to_nvenc_tuning()
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

impl Encoder for NvencEncoder {
    fn init(&mut self) -> Result<()> {
        // Actual initialization happens on first frame when we know the resolution
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

        // Copy frame data
        // For NV12: Y plane is full size, UV plane is half height
        if frame.format == FrameFormat::Nv12 {
            let y_size = (frame.width * frame.height) as usize;
            let uv_size = y_size / 2;

            if frame.data.len() >= y_size + uv_size {
                video_frame.data_mut(0)[..y_size].copy_from_slice(&frame.data[..y_size]);
                video_frame.data_mut(1)[..uv_size]
                    .copy_from_slice(&frame.data[y_size..y_size + uv_size]);
            }
        } else {
            // For other formats, copy to first plane
            let plane_size = video_frame.data(0).len().min(frame.data.len());
            video_frame.data_mut(0)[..plane_size].copy_from_slice(&frame.data[..plane_size]);
        }

        // Set PTS
        video_frame.set_pts(Some(frame.pts));

        // Note: Keyframe insertion is handled by encoder GOP settings
        // frame.is_keyframe is informational for stats/logging

        // Scale if needed
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

                // Update average encode time (exponential moving average)
                let encode_ms = encode_time.as_secs_f64() * 1000.0;
                self.stats.avg_encode_time_ms =
                    self.stats.avg_encode_time_ms * 0.95 + encode_ms * 0.05;

                // Calculate current bitrate
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
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                // No packet available yet, encoder is buffering
                Ok(None)
            }
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

        // Send EOF to encoder
        encoder
            .send_eof()
            .map_err(|e| Error::EncodingFailed(format!("Failed to send EOF: {}", e)))?;

        // Drain remaining packets
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
            "NVENC encoder flushed: {} frames, {} bytes, avg {:.2}ms/frame",
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

        // Get extradata from encoder (SPS/PPS/VPS) via unsafe pointer access
        let extradata = unsafe {
            let ptr = (*encoder.as_ptr()).extradata;
            let size = (*encoder.as_ptr()).extradata_size as usize;
            if !ptr.is_null() && size > 0 {
                std::slice::from_raw_parts(ptr, size).to_vec()
            } else {
                Vec::new()
            }
        };

        // Get resolution
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
            framerate: Framerate::FPS_60, // TODO: from config
            time_base_num: self.time_base.numerator(),
            time_base_den: self.time_base.denominator(),
            bitrate: (self.config.bitrate_kbps as i64) * 1000,
        })
    }

    fn reconfigure(&mut self, config: &EncoderConfig) -> Result<()> {
        // For now, just update the stored config
        // Full reconfiguration would require re-creating the encoder
        self.config = config.clone();
        tracing::info!("Encoder config updated: {}kbps", config.bitrate_kbps);
        Ok(())
    }
}

impl Drop for NvencEncoder {
    fn drop(&mut self) {
        if self.encoder.is_some() {
            tracing::debug!("Dropping NVENC encoder");
        }
    }
}

// ============================================================================
// NVENC Detection Functions
// ============================================================================

/// Check if NVENC is available on this system
pub fn is_available() -> bool {
    // Try to initialize FFmpeg
    if ffmpeg::init().is_err() {
        return false;
    }

    // Check if h264_nvenc encoder exists
    ffmpeg::encoder::find_by_name("h264_nvenc").is_some()
}

/// Check if a specific codec is supported
pub fn supports_codec(codec: Codec) -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }

    let encoder_name = codec.nvenc_encoder_name();
    ffmpeg::encoder::find_by_name(encoder_name).is_some()
}

/// Check for AV1 encoding support (RTX 4000+)
pub fn supports_av1() -> bool {
    supports_codec(Codec::Av1)
}

/// Check for dual encoder support (RTX 40/50)
pub fn has_dual_encoder() -> bool {
    // Check GPU name for RTX 40/50 series
    if let Some(gpu) = get_gpu_name() {
        return gpu.contains("RTX 40") || gpu.contains("RTX 50");
    }
    false
}

/// Get GPU name via nvidia-smi
pub fn get_gpu_name() -> Option<String> {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get NVIDIA driver version
pub fn get_driver_version() -> Option<String> {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=driver_version", "--format=csv,noheader"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get detailed NVENC capabilities
pub fn get_capabilities() -> NvencCapabilities {
    let mut caps = NvencCapabilities::default();

    caps.available = is_available();
    caps.h264 = supports_codec(Codec::H264);
    caps.hevc = supports_codec(Codec::Hevc);
    caps.av1 = supports_codec(Codec::Av1);
    caps.dual_encoder = has_dual_encoder();
    caps.gpu_name = get_gpu_name();
    caps.driver_version = get_driver_version();

    caps
}

/// NVENC capabilities
#[derive(Debug, Clone, Default)]
pub struct NvencCapabilities {
    pub available: bool,
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
    pub dual_encoder: bool,
    pub gpu_name: Option<String>,
    pub driver_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvenc_detection() {
        let caps = get_capabilities();
        println!("NVENC Capabilities: {:?}", caps);
    }

    #[test]
    fn test_encoder_creation() {
        if !is_available() {
            println!("NVENC not available, skipping test");
            return;
        }

        let config = EncoderConfig::default();
        let encoder = NvencEncoder::new(config);
        assert!(encoder.is_ok());
    }
}
