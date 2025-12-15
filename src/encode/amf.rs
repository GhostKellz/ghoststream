//! AMD Advanced Media Framework (AMF) hardware encoder via FFmpeg
//!
//! Provides H.264, HEVC, and AV1 encoding using AMD GPUs (RX 5000+, RX 6000+, RX 7000+).

use crate::config::EncoderConfig;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution};

use super::{Codec, Encoder, EncoderStats};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags as ScalerFlags};
use ffmpeg_next::Dictionary;
use std::time::Instant;

/// AMD AMF encoder using FFmpeg
pub struct AmfEncoder {
    config: EncoderConfig,
    encoder: Option<ffmpeg::encoder::Video>,
    scaler: Option<Scaler>,
    stats: EncoderStats,
    frame_count: u64,
    start_time: Option<Instant>,
    input_resolution: Option<Resolution>,
    time_base: ffmpeg::Rational,
}

impl AmfEncoder {
    /// Create a new AMF encoder
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::Ffmpeg(e.to_string()))?;

        // Verify AMF is available
        if !is_available() {
            return Err(Error::CodecNotSupported(
                "AMD AMF not found. Ensure AMD GPU and FFmpeg with AMF support are installed."
                    .into(),
            ));
        }

        // Verify codec is supported
        if !supports_codec(config.codec) {
            return Err(Error::CodecNotSupported(format!(
                "AMF encoder for {} not available",
                config.codec.display_name()
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
            time_base: ffmpeg::Rational::new(1, 60),
        })
    }

    /// Get AMF encoder name for codec
    fn amf_encoder_name(codec: Codec) -> &'static str {
        match codec {
            Codec::H264 => "h264_amf",
            Codec::Hevc => "hevc_amf",
            Codec::Av1 => "av1_amf",
        }
    }

    /// Initialize encoder with specific input resolution
    fn init_encoder(&mut self, input_width: u32, input_height: u32) -> Result<()> {
        let encoder_name = Self::amf_encoder_name(self.config.codec);

        // Find the encoder
        let codec = ffmpeg::encoder::find_by_name(encoder_name).ok_or_else(|| {
            Error::EncoderInit(format!("AMF encoder {} not found", encoder_name))
        })?;

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
        encoder.set_format(Pixel::NV12); // AMF prefers NV12
        encoder.set_time_base(ffmpeg::Rational::new(1, 1000));
        self.time_base = ffmpeg::Rational::new(1, 1000);

        encoder.set_frame_rate(Some(ffmpeg::Rational::new(
            self.config.framerate.num as i32,
            self.config.framerate.den as i32,
        )));
        encoder.set_gop(self.config.gop_size);
        encoder.set_max_b_frames(self.config.b_frames as usize);

        // Build encoder options
        let mut opts = Dictionary::new();

        // AMF usage mode
        let usage = match self.config.tuning {
            crate::config::EncoderTuning::LowLatency | crate::config::EncoderTuning::UltraLowLatency => "ultralowlatency",
            crate::config::EncoderTuning::HighQuality => "transcoding",
            _ => "transcoding",
        };
        opts.set("usage", usage);

        // AMF quality preset
        let quality = match self.config.preset {
            crate::config::EncoderPreset::Fastest | crate::config::EncoderPreset::Fast => "speed",
            crate::config::EncoderPreset::Medium => "balanced",
            crate::config::EncoderPreset::Slow | crate::config::EncoderPreset::Slowest => "quality",
        };
        opts.set("quality", quality);

        // Rate control
        match self.config.rate_control {
            crate::config::RateControl::Cbr => {
                opts.set("rc", "cbr");
                encoder.set_bit_rate(self.config.bitrate_kbps as usize * 1000);
            }
            crate::config::RateControl::Vbr => {
                opts.set("rc", "vbr_peak");
                encoder.set_bit_rate(self.config.bitrate_kbps as usize * 1000);
                if let Some(max) = self.config.max_bitrate_kbps {
                    opts.set("maxrate", &format!("{}k", max));
                }
            }
            crate::config::RateControl::Cqp { qp } => {
                opts.set("rc", "cqp");
                opts.set("qp_i", &qp.to_string());
                opts.set("qp_p", &qp.to_string());
            }
            crate::config::RateControl::Crf { crf } => {
                opts.set("rc", "vbr_latency");
                opts.set("qp_i", &crf.to_string());
                opts.set("qp_p", &crf.to_string());
            }
        }

        // B-frames
        if self.config.b_frames > 0 {
            opts.set("bf", &self.config.b_frames.to_string());
        }

        // Open encoder
        let opened = encoder
            .open_with(opts)
            .map_err(|e| Error::EncoderInit(format!("Failed to open AMF encoder: {}", e)))?;

        self.encoder = Some(opened);
        self.input_resolution = Some(Resolution::new(input_width, input_height));

        // Create scaler if needed
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
            "AMD AMF encoder initialized: {} {}x{} @ {}kbps (usage: {}, quality: {})",
            self.config.codec,
            out_width,
            out_height,
            self.config.bitrate_kbps,
            usage,
            quality,
        );

        Ok(())
    }

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

impl Encoder for AmfEncoder {
    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn encode(&mut self, frame: &Frame) -> Result<Option<Packet>> {
        if self.encoder.is_none() {
            self.init_encoder(frame.width, frame.height)?;
        }

        let encoder = self.encoder.as_mut().unwrap();
        let encode_start = Instant::now();

        let mut video_frame = ffmpeg::frame::Video::new(
            Self::to_ffmpeg_format(frame.format),
            frame.width,
            frame.height,
        );

        // Copy frame data
        if frame.format == FrameFormat::Nv12 {
            let y_size = (frame.width * frame.height) as usize;
            let uv_size = y_size / 2;

            if frame.data.len() >= y_size + uv_size {
                video_frame.data_mut(0)[..y_size].copy_from_slice(&frame.data[..y_size]);
                video_frame.data_mut(1)[..uv_size]
                    .copy_from_slice(&frame.data[y_size..y_size + uv_size]);
            }
        } else {
            let plane_size = video_frame.data(0).len().min(frame.data.len());
            video_frame.data_mut(0)[..plane_size].copy_from_slice(&frame.data[..plane_size]);
        }

        video_frame.set_pts(Some(frame.pts));

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

        encoder
            .send_frame(&frame_to_encode)
            .map_err(|e| Error::EncodingFailed(format!("Failed to send frame: {}", e)))?;

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

                Ok(Some(Packet {
                    data: ffmpeg_packet.data().map(|d| d.to_vec()).unwrap_or_default(),
                    pts: ffmpeg_packet.pts().unwrap_or(0),
                    dts: ffmpeg_packet.dts().unwrap_or(0),
                    duration: ffmpeg_packet.duration(),
                    is_keyframe: ffmpeg_packet.is_key(),
                    flags: 0,
                }))
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
            "AMF encoder flushed: {} frames, {} bytes",
            self.stats.frames_encoded,
            self.stats.bytes_output,
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
        tracing::info!("AMF encoder config updated: {}kbps", config.bitrate_kbps);
        Ok(())
    }
}

// ============================================================================
// AMF Detection Functions
// ============================================================================

/// Check if AMD AMF is available
pub fn is_available() -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }
    ffmpeg::encoder::find_by_name("h264_amf").is_some()
}

/// Check if a specific codec is supported
pub fn supports_codec(codec: Codec) -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }
    let encoder_name = AmfEncoder::amf_encoder_name(codec);
    ffmpeg::encoder::find_by_name(encoder_name).is_some()
}

/// Get AMD GPU info
pub fn get_gpu_info() -> Option<String> {
    // Try to get AMD GPU info via various methods

    // Method 1: Check lspci for AMD GPU
    if let Ok(output) = std::process::Command::new("lspci").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("AMD") && (line.contains("VGA") || line.contains("Display")) {
                    // Extract the GPU name
                    if let Some(idx) = line.find(':') {
                        return Some(line[idx + 1..].trim().to_string());
                    }
                }
            }
        }
    }

    // Method 2: Check for AMD devices in /sys
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path().join("device/vendor");
            if let Ok(vendor) = std::fs::read_to_string(&path) {
                if vendor.trim() == "0x1002" {
                    // AMD vendor ID
                    return Some("AMD GPU detected".to_string());
                }
            }
        }
    }

    None
}

/// Get AMF capabilities
pub fn get_capabilities() -> AmfCapabilities {
    AmfCapabilities {
        available: is_available(),
        h264: supports_codec(Codec::H264),
        hevc: supports_codec(Codec::Hevc),
        av1: supports_codec(Codec::Av1),
        gpu_info: get_gpu_info(),
    }
}

/// AMF capabilities
#[derive(Debug, Clone, Default)]
pub struct AmfCapabilities {
    pub available: bool,
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
    pub gpu_info: Option<String>,
}
