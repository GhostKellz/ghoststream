//! Intel Quick Sync Video (QSV) hardware encoder via FFmpeg
//!
//! Provides H.264, HEVC, and AV1 encoding using Intel integrated/discrete GPUs.

use crate::config::EncoderConfig;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution};

use super::{Codec, Encoder, EncoderStats};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags as ScalerFlags};
use ffmpeg_next::Dictionary;
use std::time::Instant;

/// Intel QSV encoder using FFmpeg
pub struct QsvEncoder {
    config: EncoderConfig,
    encoder: Option<ffmpeg::encoder::Video>,
    scaler: Option<Scaler>,
    stats: EncoderStats,
    frame_count: u64,
    start_time: Option<Instant>,
    input_resolution: Option<Resolution>,
    time_base: ffmpeg::Rational,
}

impl QsvEncoder {
    /// Create a new QSV encoder
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::Ffmpeg(e.to_string()))?;

        // Verify QSV is available
        if !is_available() {
            return Err(Error::CodecNotSupported(
                "Intel QSV not found. Ensure Intel GPU and FFmpeg with QSV support are installed."
                    .into(),
            ));
        }

        // Verify codec is supported
        if !supports_codec(config.codec) {
            return Err(Error::CodecNotSupported(format!(
                "QSV encoder for {} not available",
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

    /// Get QSV encoder name for codec
    fn qsv_encoder_name(codec: Codec) -> &'static str {
        match codec {
            Codec::H264 => "h264_qsv",
            Codec::Hevc => "hevc_qsv",
            Codec::Av1 => "av1_qsv",
        }
    }

    /// Initialize encoder with specific input resolution
    fn init_encoder(&mut self, input_width: u32, input_height: u32) -> Result<()> {
        let encoder_name = Self::qsv_encoder_name(self.config.codec);

        // Find the encoder
        let codec = ffmpeg::encoder::find_by_name(encoder_name).ok_or_else(|| {
            Error::EncoderInit(format!("QSV encoder {} not found", encoder_name))
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
        encoder.set_format(Pixel::NV12); // QSV prefers NV12
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

        // QSV preset mapping
        let preset = match self.config.preset {
            crate::config::EncoderPreset::Fastest => "veryfast",
            crate::config::EncoderPreset::Fast => "fast",
            crate::config::EncoderPreset::Medium => "medium",
            crate::config::EncoderPreset::Slow => "slow",
            crate::config::EncoderPreset::Slowest => "veryslow",
        };
        opts.set("preset", preset);

        // Rate control
        match self.config.rate_control {
            crate::config::RateControl::Cbr => {
                opts.set("look_ahead", "0");
                encoder.set_bit_rate(self.config.bitrate_kbps as usize * 1000);
            }
            crate::config::RateControl::Vbr => {
                opts.set("look_ahead", "1");
                encoder.set_bit_rate(self.config.bitrate_kbps as usize * 1000);
                if let Some(max) = self.config.max_bitrate_kbps {
                    opts.set("maxrate", &format!("{}k", max));
                }
            }
            crate::config::RateControl::Cqp { qp } => {
                opts.set("global_quality", &qp.to_string());
            }
            crate::config::RateControl::Crf { crf } => {
                opts.set("global_quality", &crf.to_string());
            }
        }

        // Low latency mode
        if matches!(
            self.config.tuning,
            crate::config::EncoderTuning::LowLatency
                | crate::config::EncoderTuning::UltraLowLatency
        ) {
            opts.set("low_power", "1");
            opts.set("look_ahead", "0");
        }

        // Open encoder
        let opened = encoder
            .open_with(opts)
            .map_err(|e| Error::EncoderInit(format!("Failed to open QSV encoder: {}", e)))?;

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
            "Intel QSV encoder initialized: {} {}x{} @ {}kbps",
            self.config.codec,
            out_width,
            out_height,
            self.config.bitrate_kbps,
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

impl Encoder for QsvEncoder {
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
            "QSV encoder flushed: {} frames, {} bytes",
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
        tracing::info!("QSV encoder config updated: {}kbps", config.bitrate_kbps);
        Ok(())
    }
}

// ============================================================================
// QSV Detection Functions
// ============================================================================

/// Check if Intel QSV is available
pub fn is_available() -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }
    ffmpeg::encoder::find_by_name("h264_qsv").is_some()
}

/// Check if a specific codec is supported
pub fn supports_codec(codec: Codec) -> bool {
    if ffmpeg::init().is_err() {
        return false;
    }
    let encoder_name = QsvEncoder::qsv_encoder_name(codec);
    ffmpeg::encoder::find_by_name(encoder_name).is_some()
}

/// Get Intel GPU info
pub fn get_gpu_info() -> Option<String> {
    // Try vainfo for Intel GPU info
    std::process::Command::new("vainfo")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let output = String::from_utf8_lossy(&o.stdout);
            // Extract driver string
            for line in output.lines() {
                if line.contains("Driver version") || line.contains("Intel") {
                    return Some(line.trim().to_string());
                }
            }
            None
        })
}

/// Get QSV capabilities
pub fn get_capabilities() -> QsvCapabilities {
    QsvCapabilities {
        available: is_available(),
        h264: supports_codec(Codec::H264),
        hevc: supports_codec(Codec::Hevc),
        av1: supports_codec(Codec::Av1),
        gpu_info: get_gpu_info(),
    }
}

/// QSV capabilities
#[derive(Debug, Clone, Default)]
pub struct QsvCapabilities {
    pub available: bool,
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
    pub gpu_info: Option<String>,
}
