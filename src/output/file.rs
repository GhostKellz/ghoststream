//! File output (recording)
//!
//! Writes encoded video to MKV, MP4, WebM, or TS files using FFmpeg muxer.

use crate::encode::Codec;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Packet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Container, OutputSink};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Id as CodecId;

/// File output for recording
pub struct FileOutput {
    path: PathBuf,
    container: Container,
    initialized: bool,
    bytes_written: AtomicU64,
    // FFmpeg muxer
    output_ctx: Option<ffmpeg::format::context::Output>,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    frame_count: u64,
}

impl FileOutput {
    /// Create a new file output
    pub fn new(path: impl Into<PathBuf>, container: Container) -> Self {
        Self {
            path: path.into(),
            container,
            initialized: false,
            bytes_written: AtomicU64::new(0),
            output_ctx: None,
            stream_index: 0,
            time_base: ffmpeg::Rational::new(1, 1000),
            frame_count: 0,
        }
    }

    /// Get the output path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Map GhostStream codec to FFmpeg codec ID
    fn codec_to_ffmpeg(codec: Codec) -> CodecId {
        match codec {
            Codec::H264 => CodecId::H264,
            Codec::Hevc => CodecId::HEVC,
            Codec::Av1 => CodecId::AV1,
        }
    }

    /// Initialize the muxer with codec parameters
    fn init_muxer(&mut self, codec_params: &CodecParams) -> Result<()> {
        // Initialize FFmpeg
        ffmpeg::init().map_err(|e| Error::FFmpeg(e.to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::FileOutput(format!("Failed to create directory: {}", e)))?;
            }
        }

        // Create output context with format hint
        let mut output_ctx = ffmpeg::format::output_as(
            &self.path,
            self.container.ffmpeg_format(),
        )
        .map_err(|e| Error::FileOutput(format!("Failed to create output context: {}", e)))?;

        // Find encoder for codec parameters
        let codec_id = Self::codec_to_ffmpeg(codec_params.codec);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or_else(|| Error::FileOutput(format!("Codec {:?} not found", codec_id)))?;

        // Add video stream
        let mut stream = output_ctx.add_stream(codec)
            .map_err(|e| Error::FileOutput(format!("Failed to add stream: {}", e)))?;

        self.stream_index = stream.index();

        // Set stream parameters
        let mut params = stream.parameters();

        // Configure stream parameters via codec context
        unsafe {
            let codec_ctx = params.as_mut_ptr();

            // Set codec type
            (*codec_ctx).codec_type = ffmpeg_next::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*codec_ctx).codec_id = codec_id.into();

            // Set resolution
            (*codec_ctx).width = codec_params.resolution.width as i32;
            (*codec_ctx).height = codec_params.resolution.height as i32;

            // Set pixel format (NV12 for NVENC)
            (*codec_ctx).format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_NV12 as i32;

            // Set bitrate
            (*codec_ctx).bit_rate = codec_params.bitrate;

            // Set extradata if present
            if !codec_params.extradata.is_empty() {
                // Allocate extradata buffer
                let extradata_size = codec_params.extradata.len();
                let extradata_ptr = ffmpeg_next::ffi::av_malloc(
                    extradata_size + ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize
                ) as *mut u8;

                if !extradata_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        codec_params.extradata.as_ptr(),
                        extradata_ptr,
                        extradata_size,
                    );
                    // Zero the padding
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

        // Write header
        output_ctx.write_header()
            .map_err(|e| Error::FileOutput(format!("Failed to write header: {}", e)))?;

        self.output_ctx = Some(output_ctx);

        tracing::info!(
            "File output initialized: {} ({}, {:?}, {}x{})",
            self.path.display(),
            self.container.extension(),
            codec_params.codec,
            codec_params.resolution.width,
            codec_params.resolution.height,
        );

        Ok(())
    }

    /// Initialize with default codec params (H.264 1080p)
    fn init_default(&mut self) -> Result<()> {
        let default_params = CodecParams::default();
        self.init_muxer(&default_params)
    }
}

#[async_trait::async_trait]
impl OutputSink for FileOutput {
    async fn init_with_codec(&mut self, codec_params: Option<&CodecParams>) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        match codec_params {
            Some(params) => self.init_muxer(params)?,
            None => self.init_default()?,
        }

        self.initialized = true;
        Ok(())
    }

    async fn write(&mut self, packet: &Packet) -> Result<()> {
        if !self.initialized {
            // Try default initialization - real usage should call init_with_codec first
            self.init_with_codec(None).await?;
        }

        let output_ctx = self.output_ctx.as_mut()
            .ok_or_else(|| Error::FileOutput("Output not initialized".into()))?;

        // Create FFmpeg packet
        let mut pkt = ffmpeg::Packet::copy(&packet.data);

        // Set packet properties
        pkt.set_pts(Some(packet.pts));
        pkt.set_dts(Some(packet.dts));
        pkt.set_duration(packet.duration);
        pkt.set_stream(self.stream_index);

        if packet.is_keyframe {
            pkt.set_flags(ffmpeg::codec::packet::Flags::KEY);
        }

        // Rescale timestamps to stream time base
        let stream = output_ctx.stream(self.stream_index)
            .ok_or_else(|| Error::FileOutput("Stream not found".into()))?;

        pkt.rescale_ts(self.time_base, stream.time_base());

        // Write packet (interleaved for proper ordering)
        pkt.write_interleaved(output_ctx)
            .map_err(|e| Error::FileOutput(format!("Failed to write packet: {}", e)))?;

        self.frame_count += 1;
        self.bytes_written.fetch_add(packet.size() as u64, Ordering::Relaxed);

        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        if let Some(ref mut output_ctx) = self.output_ctx {
            // Write trailer
            output_ctx.write_trailer()
                .map_err(|e| Error::FileOutput(format!("Failed to write trailer: {}", e)))?;
        }

        let bytes = self.bytes_written.load(Ordering::Relaxed);
        tracing::info!(
            "File output finished: {} ({} frames, {} bytes, {:.2} MB)",
            self.path.display(),
            self.frame_count,
            bytes,
            bytes as f64 / 1_000_000.0
        );

        self.output_ctx = None;
        self.initialized = false;
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }
}

impl Drop for FileOutput {
    fn drop(&mut self) {
        // Write trailer if still open
        if self.initialized {
            if let Some(ref mut output_ctx) = self.output_ctx {
                let _ = output_ctx.write_trailer();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_extensions() {
        assert_eq!(Container::Matroska.extension(), "mkv");
        assert_eq!(Container::Mp4.extension(), "mp4");
        assert_eq!(Container::WebM.extension(), "webm");
        assert_eq!(Container::Ts.extension(), "ts");
    }

    #[test]
    fn test_container_ffmpeg_formats() {
        assert_eq!(Container::Matroska.ffmpeg_format(), "matroska");
        assert_eq!(Container::Mp4.ffmpeg_format(), "mp4");
        assert_eq!(Container::WebM.ffmpeg_format(), "webm");
        assert_eq!(Container::Ts.ffmpeg_format(), "mpegts");
    }
}
