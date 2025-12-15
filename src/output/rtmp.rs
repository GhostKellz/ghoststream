//! RTMP streaming output
//!
//! Streams video to RTMP servers (Twitch, YouTube, Facebook, etc.)

use crate::encode::Codec;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Packet};
use std::sync::atomic::{AtomicU64, Ordering};

use super::OutputSink;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Id as CodecId;

/// RTMP streaming output
pub struct RtmpOutput {
    url: String,
    initialized: bool,
    bytes_written: AtomicU64,
    // FFmpeg muxer
    output_ctx: Option<ffmpeg::format::context::Output>,
    video_stream_index: usize,
    #[allow(dead_code)] // Reserved for A/V muxing
    audio_stream_index: Option<usize>,
    time_base: ffmpeg::Rational,
    frame_count: u64,
    // Connection state
    connected: bool,
    reconnect_attempts: u32,
    max_reconnect_attempts: u32,
}

impl RtmpOutput {
    /// Create a new RTMP output
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            initialized: false,
            bytes_written: AtomicU64::new(0),
            output_ctx: None,
            video_stream_index: 0,
            audio_stream_index: None,
            time_base: ffmpeg::Rational::new(1, 1000),
            frame_count: 0,
            connected: false,
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        }
    }

    /// Set maximum reconnection attempts
    pub fn with_max_reconnects(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }

    /// Get the RTMP URL (without stream key for privacy)
    pub fn url_masked(&self) -> String {
        // Mask the stream key portion for logging
        if let Some(idx) = self.url.rfind('/') {
            format!("{}/*****", &self.url[..idx])
        } else {
            "rtmp://*****".to_string()
        }
    }

    /// Map codec to FFmpeg codec ID
    fn codec_to_ffmpeg(codec: Codec) -> CodecId {
        match codec {
            Codec::H264 => CodecId::H264,
            Codec::Hevc => CodecId::HEVC,
            Codec::Av1 => CodecId::AV1,
        }
    }

    /// Initialize the RTMP connection
    fn init_rtmp(&mut self, codec_params: &CodecParams) -> Result<()> {
        // Validate URL
        if !self.url.starts_with("rtmp://") && !self.url.starts_with("rtmps://") {
            return Err(Error::Rtmp("URL must start with rtmp:// or rtmps://".into()));
        }

        // RTMP only supports H.264 and AAC natively
        // HEVC/AV1 require enhanced RTMP (not widely supported)
        if codec_params.codec != Codec::H264 {
            tracing::warn!(
                "RTMP typically only supports H.264. {} may not work with all servers.",
                codec_params.codec.display_name()
            );
        }

        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::Ffmpeg(e.to_string()))?;

        // Set RTMP-specific options
        let mut options = ffmpeg::Dictionary::new();
        options.set("flvflags", "no_duration_filesize");
        options.set("rtmp_live", "live");

        // Create output context for RTMP (FLV format)
        let mut output_ctx = ffmpeg::format::output_as_with(&self.url, "flv", options)
            .map_err(|e| Error::Rtmp(format!("Failed to create RTMP output: {}", e)))?;

        // Find encoder for codec parameters
        let codec_id = Self::codec_to_ffmpeg(codec_params.codec);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or_else(|| Error::Rtmp(format!("Codec {:?} not found", codec_id)))?;

        // Add video stream
        let mut stream = output_ctx
            .add_stream(codec)
            .map_err(|e| Error::Rtmp(format!("Failed to add video stream: {}", e)))?;

        self.video_stream_index = stream.index();

        // Configure stream parameters
        unsafe {
            let mut params = stream.parameters();
            let codec_ctx = params.as_mut_ptr();

            (*codec_ctx).codec_type = ffmpeg_next::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*codec_ctx).codec_id = codec_id.into();
            (*codec_ctx).width = codec_params.resolution.width as i32;
            (*codec_ctx).height = codec_params.resolution.height as i32;
            (*codec_ctx).format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
            (*codec_ctx).bit_rate = codec_params.bitrate;

            // Set extradata (SPS/PPS for H.264)
            if !codec_params.extradata.is_empty() {
                let extradata_size = codec_params.extradata.len();
                let extradata_ptr = ffmpeg_next::ffi::av_malloc(
                    extradata_size + ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                ) as *mut u8;

                if !extradata_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        codec_params.extradata.as_ptr(),
                        extradata_ptr,
                        extradata_size,
                    );
                    std::ptr::write_bytes(
                        extradata_ptr.add(extradata_size),
                        0,
                        ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                    );
                    (*codec_ctx).extradata = extradata_ptr;
                    (*codec_ctx).extradata_size = extradata_size as i32;
                }
            }
        }

        // Set time base
        self.time_base = ffmpeg::Rational::new(
            codec_params.time_base_num,
            codec_params.time_base_den,
        );
        stream.set_time_base(self.time_base);

        // Set framerate
        let fps = codec_params.framerate.num as i32;
        stream.set_rate(ffmpeg::Rational::new(fps, 1));

        // Write header (this initiates the RTMP connection)
        tracing::info!("Connecting to RTMP server: {}", self.url_masked());

        output_ctx
            .write_header()
            .map_err(|e| Error::Rtmp(format!("Failed to connect to RTMP server: {}", e)))?;

        self.output_ctx = Some(output_ctx);
        self.connected = true;

        tracing::info!(
            "RTMP connected: {} ({:?}, {}x{})",
            self.url_masked(),
            codec_params.codec,
            codec_params.resolution.width,
            codec_params.resolution.height,
        );

        Ok(())
    }

    /// Initialize with default codec params
    fn init_default(&mut self) -> Result<()> {
        let default_params = CodecParams::default();
        self.init_rtmp(&default_params)
    }

    /// Attempt to reconnect
    #[allow(dead_code)] // Will be used for automatic reconnection
    fn try_reconnect(&mut self) -> Result<()> {
        if self.reconnect_attempts >= self.max_reconnect_attempts {
            return Err(Error::Rtmp(format!(
                "Max reconnection attempts ({}) exceeded",
                self.max_reconnect_attempts
            )));
        }

        self.reconnect_attempts += 1;
        tracing::warn!(
            "RTMP connection lost, attempting reconnect ({}/{})",
            self.reconnect_attempts,
            self.max_reconnect_attempts
        );

        // Reset state
        self.output_ctx = None;
        self.connected = false;
        self.initialized = false;

        // Try to reconnect
        self.init_default()
    }
}

#[async_trait::async_trait]
impl OutputSink for RtmpOutput {
    async fn init_with_codec(&mut self, codec_params: Option<&CodecParams>) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        match codec_params {
            Some(params) => self.init_rtmp(params)?,
            None => self.init_default()?,
        }

        self.initialized = true;
        Ok(())
    }

    async fn write(&mut self, packet: &Packet) -> Result<()> {
        if !self.initialized {
            self.init_with_codec(None).await?;
        }

        let output_ctx = self.output_ctx.as_mut().ok_or_else(|| {
            Error::Rtmp("RTMP output not initialized".into())
        })?;

        // Create FFmpeg packet
        let mut pkt = ffmpeg::Packet::copy(&packet.data);

        pkt.set_pts(Some(packet.pts));
        pkt.set_dts(Some(packet.dts));
        pkt.set_duration(packet.duration);
        pkt.set_stream(self.video_stream_index);

        if packet.is_keyframe {
            pkt.set_flags(ffmpeg::codec::packet::Flags::KEY);
        }

        // Rescale timestamps
        let stream = output_ctx.stream(self.video_stream_index).ok_or_else(|| {
            Error::Rtmp("Video stream not found".into())
        })?;

        pkt.rescale_ts(self.time_base, stream.time_base());

        // Write packet
        match pkt.write_interleaved(output_ctx) {
            Ok(()) => {
                self.frame_count += 1;
                self.bytes_written
                    .fetch_add(packet.size() as u64, Ordering::Relaxed);
                self.reconnect_attempts = 0; // Reset on successful write
                Ok(())
            }
            Err(e) => {
                tracing::error!("RTMP write error: {}", e);
                // Try to reconnect on network errors
                // Note: In production, you'd want more sophisticated error handling
                Err(Error::Rtmp(format!("Write failed: {}", e)))
            }
        }
    }

    async fn finish(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        if let Some(ref mut output_ctx) = self.output_ctx {
            output_ctx
                .write_trailer()
                .map_err(|e| Error::Rtmp(format!("Failed to write trailer: {}", e)))?;
        }

        let bytes = self.bytes_written.load(Ordering::Relaxed);
        tracing::info!(
            "RTMP stream ended: {} ({} frames, {} bytes, {:.2} MB)",
            self.url_masked(),
            self.frame_count,
            bytes,
            bytes as f64 / 1_000_000.0
        );

        self.output_ctx = None;
        self.initialized = false;
        self.connected = false;

        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }
}

impl Drop for RtmpOutput {
    fn drop(&mut self) {
        if self.initialized {
            if let Some(ref mut output_ctx) = self.output_ctx {
                let _ = output_ctx.write_trailer();
            }
        }
    }
}

/// Common RTMP streaming services
pub struct RtmpService;

impl RtmpService {
    /// Twitch ingest URL (replace STREAM_KEY)
    pub fn twitch(ingest_server: &str, stream_key: &str) -> String {
        format!("rtmp://{}/app/{}", ingest_server, stream_key)
    }

    /// YouTube Live URL
    pub fn youtube(stream_key: &str) -> String {
        format!("rtmp://a.rtmp.youtube.com/live2/{}", stream_key)
    }

    /// Facebook Live URL
    pub fn facebook(stream_key: &str) -> String {
        format!("rtmps://live-api-s.facebook.com:443/rtmp/{}", stream_key)
    }

    /// Kick ingest URL
    pub fn kick(ingest_url: &str, stream_key: &str) -> String {
        format!("{}/{}", ingest_url, stream_key)
    }

    /// Common Twitch ingest servers
    pub fn twitch_ingest_servers() -> &'static [(&'static str, &'static str)] {
        &[
            ("Auto", "live.twitch.tv"),
            ("US West", "sea.contribute.live-video.net"),
            ("US East", "iad05.contribute.live-video.net"),
            ("EU West", "ams03.contribute.live-video.net"),
            ("EU Central", "fra05.contribute.live-video.net"),
            ("Asia", "tyo05.contribute.live-video.net"),
            ("Australia", "syd02.contribute.live-video.net"),
        ]
    }
}
