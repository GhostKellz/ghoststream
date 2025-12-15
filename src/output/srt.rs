//! SRT (Secure Reliable Transport) streaming output
//!
//! Low-latency streaming protocol, superior to RTMP for contribution feeds.

use crate::encode::Codec;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Packet};
use std::sync::atomic::{AtomicU64, Ordering};

use super::OutputSink;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Id as CodecId;

/// SRT connection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SrtMode {
    /// Caller mode - initiate connection
    #[default]
    Caller,
    /// Listener mode - wait for connections
    Listener,
    /// Rendezvous mode - peer-to-peer
    Rendezvous,
}

/// SRT streaming output
pub struct SrtOutput {
    url: String,
    latency_ms: u32,
    mode: SrtMode,
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
    // SRT-specific options
    passphrase: Option<String>,
    streamid: Option<String>,
    pbkeylen: Option<u32>,
    max_bandwidth: Option<i64>,
}

impl SrtOutput {
    /// Create a new SRT output
    ///
    /// # Arguments
    /// * `url` - SRT URL (e.g., "srt://host:port")
    /// * `latency_ms` - Target latency in milliseconds (200-8000 typical)
    pub fn new(url: impl Into<String>, latency_ms: u32) -> Self {
        Self {
            url: url.into(),
            latency_ms: latency_ms.clamp(20, 8000),
            mode: SrtMode::Caller,
            initialized: false,
            bytes_written: AtomicU64::new(0),
            output_ctx: None,
            video_stream_index: 0,
            audio_stream_index: None,
            time_base: ffmpeg::Rational::new(1, 1000),
            frame_count: 0,
            connected: false,
            passphrase: None,
            streamid: None,
            pbkeylen: None,
            max_bandwidth: None,
        }
    }

    /// Set connection mode
    pub fn with_mode(mut self, mode: SrtMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set encryption passphrase (10-79 characters)
    pub fn with_passphrase(mut self, passphrase: impl Into<String>) -> Self {
        self.passphrase = Some(passphrase.into());
        self
    }

    /// Set stream ID for multiplexing
    pub fn with_streamid(mut self, streamid: impl Into<String>) -> Self {
        self.streamid = Some(streamid.into());
        self
    }

    /// Set encryption key length (0, 16, 24, or 32)
    pub fn with_key_length(mut self, pbkeylen: u32) -> Self {
        self.pbkeylen = Some(pbkeylen);
        self
    }

    /// Set maximum bandwidth in bytes/sec (-1 for unlimited)
    pub fn with_max_bandwidth(mut self, bandwidth: i64) -> Self {
        self.max_bandwidth = Some(bandwidth);
        self
    }

    /// Get the SRT URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Map codec to FFmpeg codec ID
    fn codec_to_ffmpeg(codec: Codec) -> CodecId {
        match codec {
            Codec::H264 => CodecId::H264,
            Codec::Hevc => CodecId::HEVC,
            Codec::Av1 => CodecId::AV1,
        }
    }

    /// Build SRT URL with options
    fn build_srt_url(&self) -> String {
        let url = self.url.clone();

        // Add query parameters if not already present
        let separator = if url.contains('?') { '&' } else { '?' };
        let mut params = Vec::new();

        // Mode
        let mode_str = match self.mode {
            SrtMode::Caller => "caller",
            SrtMode::Listener => "listener",
            SrtMode::Rendezvous => "rendezvous",
        };
        params.push(format!("mode={}", mode_str));

        // Latency (in microseconds for SRT)
        params.push(format!("latency={}", self.latency_ms * 1000));

        // Passphrase
        if let Some(ref passphrase) = self.passphrase {
            params.push(format!("passphrase={}", passphrase));
        }

        // Stream ID
        if let Some(ref streamid) = self.streamid {
            params.push(format!("streamid={}", streamid));
        }

        // Key length
        if let Some(pbkeylen) = self.pbkeylen {
            params.push(format!("pbkeylen={}", pbkeylen));
        }

        // Max bandwidth
        if let Some(bandwidth) = self.max_bandwidth {
            params.push(format!("maxbw={}", bandwidth));
        }

        // Additional recommended options
        params.push("transtype=live".to_string());

        format!("{}{}{}", url, separator, params.join("&"))
    }

    /// Initialize the SRT connection
    fn init_srt(&mut self, codec_params: &CodecParams) -> Result<()> {
        // Validate URL
        if !self.url.starts_with("srt://") {
            return Err(Error::Srt("URL must start with srt://".into()));
        }

        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::Ffmpeg(e.to_string()))?;

        // Build URL with options
        let full_url = self.build_srt_url();

        // Create output context for MPEG-TS over SRT
        let mut output_ctx = ffmpeg::format::output_as(&full_url, "mpegts")
            .map_err(|e| Error::Srt(format!("Failed to create SRT output: {}", e)))?;

        // Find encoder for codec parameters
        let codec_id = Self::codec_to_ffmpeg(codec_params.codec);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or_else(|| Error::Srt(format!("Codec {:?} not found", codec_id)))?;

        // Add video stream
        let mut stream = output_ctx
            .add_stream(codec)
            .map_err(|e| Error::Srt(format!("Failed to add video stream: {}", e)))?;

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

            // Set extradata
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

        // Write header (this initiates the SRT connection)
        tracing::info!(
            "Connecting via SRT: {} (latency: {}ms, mode: {:?})",
            self.url,
            self.latency_ms,
            self.mode
        );

        output_ctx
            .write_header()
            .map_err(|e| Error::Srt(format!("Failed to connect via SRT: {}", e)))?;

        self.output_ctx = Some(output_ctx);
        self.connected = true;

        tracing::info!(
            "SRT connected: {} ({:?}, {}x{}, latency: {}ms)",
            self.url,
            codec_params.codec,
            codec_params.resolution.width,
            codec_params.resolution.height,
            self.latency_ms,
        );

        Ok(())
    }

    /// Initialize with default codec params
    fn init_default(&mut self) -> Result<()> {
        let default_params = CodecParams::default();
        self.init_srt(&default_params)
    }
}

#[async_trait::async_trait]
impl OutputSink for SrtOutput {
    async fn init_with_codec(&mut self, codec_params: Option<&CodecParams>) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        match codec_params {
            Some(params) => self.init_srt(params)?,
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
            Error::Srt("SRT output not initialized".into())
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
            Error::Srt("Video stream not found".into())
        })?;

        pkt.rescale_ts(self.time_base, stream.time_base());

        // Write packet
        pkt.write_interleaved(output_ctx)
            .map_err(|e| Error::Srt(format!("Write failed: {}", e)))?;

        self.frame_count += 1;
        self.bytes_written
            .fetch_add(packet.size() as u64, Ordering::Relaxed);

        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        if let Some(ref mut output_ctx) = self.output_ctx {
            output_ctx
                .write_trailer()
                .map_err(|e| Error::Srt(format!("Failed to write trailer: {}", e)))?;
        }

        let bytes = self.bytes_written.load(Ordering::Relaxed);
        tracing::info!(
            "SRT stream ended: {} ({} frames, {} bytes, {:.2} MB)",
            self.url,
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

impl Drop for SrtOutput {
    fn drop(&mut self) {
        if self.initialized {
            if let Some(ref mut output_ctx) = self.output_ctx {
                let _ = output_ctx.write_trailer();
            }
        }
    }
}

/// SRT connection statistics (if available)
#[derive(Debug, Clone, Default)]
pub struct SrtStats {
    /// Round-trip time in milliseconds
    pub rtt_ms: f64,
    /// Packet loss percentage
    pub packet_loss_percent: f64,
    /// Available bandwidth estimate in Mbps
    pub bandwidth_mbps: f64,
    /// Send buffer level in bytes
    pub send_buffer_bytes: u64,
    /// Packets retransmitted
    pub packets_retransmitted: u64,
}
