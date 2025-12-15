//! Audio/Video Muxer
//!
//! Combines video and audio streams into a single container.

use crate::audio::{AudioParams, AudioPacket};
use crate::encode::Codec;
use crate::error::{Error, Result};
use crate::types::{CodecParams, Packet};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Id as CodecId;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Stream type for muxer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    Video,
    Audio,
}

/// Muxer packet (either video or audio)
#[derive(Debug)]
pub enum MuxerPacket {
    Video(Packet),
    Audio(AudioPacket),
}

impl MuxerPacket {
    pub fn stream_type(&self) -> StreamType {
        match self {
            MuxerPacket::Video(_) => StreamType::Video,
            MuxerPacket::Audio(_) => StreamType::Audio,
        }
    }

    pub fn pts(&self) -> i64 {
        match self {
            MuxerPacket::Video(p) => p.pts,
            MuxerPacket::Audio(p) => p.pts,
        }
    }
}

/// A/V Muxer for file output
pub struct AvMuxer {
    output_ctx: ffmpeg::format::context::Output,
    video_stream_index: usize,
    audio_stream_index: Option<usize>,
    video_time_base: ffmpeg::Rational,
    audio_time_base: Option<ffmpeg::Rational>,
    initialized: bool,
    bytes_written: AtomicU64,
    video_frames: u64,
    audio_frames: u64,
}

impl AvMuxer {
    /// Create a new muxer for a file
    pub fn new(path: impl AsRef<Path>, format: &str) -> Result<Self> {
        ffmpeg::init().map_err(|e| Error::Ffmpeg(e.to_string()))?;

        let path_str = path.as_ref().to_string_lossy();
        let output_ctx = ffmpeg::format::output_as(&*path_str, format)
            .map_err(|e| Error::Muxer(format!("Failed to create output: {}", e)))?;

        Ok(Self {
            output_ctx,
            video_stream_index: 0,
            audio_stream_index: None,
            video_time_base: ffmpeg::Rational::new(1, 1000),
            audio_time_base: None,
            initialized: false,
            bytes_written: AtomicU64::new(0),
            video_frames: 0,
            audio_frames: 0,
        })
    }

    /// Add video stream
    pub fn add_video_stream(&mut self, params: &CodecParams) -> Result<()> {
        let codec_id = Self::video_codec_to_ffmpeg(params.codec);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or_else(|| Error::Muxer(format!("Video codec {:?} not found", codec_id)))?;

        let mut stream = self.output_ctx
            .add_stream(codec)
            .map_err(|e| Error::Muxer(format!("Failed to add video stream: {}", e)))?;

        self.video_stream_index = stream.index();

        // Configure stream parameters
        unsafe {
            let mut stream_params = stream.parameters();
            let codec_ctx = stream_params.as_mut_ptr();

            (*codec_ctx).codec_type = ffmpeg_next::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*codec_ctx).codec_id = codec_id.into();
            (*codec_ctx).width = params.resolution.width as i32;
            (*codec_ctx).height = params.resolution.height as i32;
            (*codec_ctx).format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
            (*codec_ctx).bit_rate = params.bitrate;

            // Set extradata
            if !params.extradata.is_empty() {
                let extradata_size = params.extradata.len();
                let extradata_ptr = ffmpeg_next::ffi::av_malloc(
                    extradata_size + ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                ) as *mut u8;

                if !extradata_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        params.extradata.as_ptr(),
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
        self.video_time_base = ffmpeg::Rational::new(
            params.time_base_num,
            params.time_base_den,
        );
        stream.set_time_base(self.video_time_base);

        // Set framerate
        let fps = params.framerate.num as i32;
        stream.set_rate(ffmpeg::Rational::new(fps, 1));

        tracing::info!(
            "Added video stream: {:?} {}x{} @ {}fps",
            params.codec,
            params.resolution.width,
            params.resolution.height,
            fps
        );

        Ok(())
    }

    /// Add audio stream
    pub fn add_audio_stream(&mut self, params: &AudioParams) -> Result<()> {
        let codec_id = Self::audio_codec_to_ffmpeg(params.codec);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or_else(|| Error::Muxer(format!("Audio codec {:?} not found", codec_id)))?;

        let mut stream = self.output_ctx
            .add_stream(codec)
            .map_err(|e| Error::Muxer(format!("Failed to add audio stream: {}", e)))?;

        self.audio_stream_index = Some(stream.index());

        // Configure stream parameters
        unsafe {
            let mut stream_params = stream.parameters();
            let codec_ctx = stream_params.as_mut_ptr();

            (*codec_ctx).codec_type = ffmpeg_next::ffi::AVMediaType::AVMEDIA_TYPE_AUDIO;
            (*codec_ctx).codec_id = codec_id.into();
            (*codec_ctx).sample_rate = params.sample_rate as i32;
            (*codec_ctx).bit_rate = params.bitrate as i64;

            // Set channel layout using the new API (FFmpeg 7+)
            let ch_layout = &mut (*codec_ctx).ch_layout;
            ffmpeg_next::ffi::av_channel_layout_default(ch_layout, params.channels as i32);

            // Set extradata if available
            if !params.extradata.is_empty() {
                let extradata_size = params.extradata.len();
                let extradata_ptr = ffmpeg_next::ffi::av_malloc(
                    extradata_size + ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                ) as *mut u8;

                if !extradata_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        params.extradata.as_ptr(),
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

        // Set audio time base (1/sample_rate)
        let audio_time_base = ffmpeg::Rational::new(1, params.sample_rate as i32);
        self.audio_time_base = Some(audio_time_base);
        stream.set_time_base(audio_time_base);

        tracing::info!(
            "Added audio stream: {:?} {}Hz {}ch @ {}kbps",
            params.codec,
            params.sample_rate,
            params.channels,
            params.bitrate / 1000
        );

        Ok(())
    }

    /// Start muxing (write header)
    pub fn start(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        self.output_ctx
            .write_header()
            .map_err(|e| Error::Muxer(format!("Failed to write header: {}", e)))?;

        self.initialized = true;
        tracing::info!("Muxer started");

        Ok(())
    }

    /// Write a video packet
    pub fn write_video(&mut self, packet: &Packet) -> Result<()> {
        if !self.initialized {
            return Err(Error::Muxer("Muxer not started".into()));
        }

        let mut pkt = ffmpeg::Packet::copy(&packet.data);
        pkt.set_pts(Some(packet.pts));
        pkt.set_dts(Some(packet.dts));
        pkt.set_duration(packet.duration);
        pkt.set_stream(self.video_stream_index);

        if packet.is_keyframe {
            pkt.set_flags(ffmpeg::codec::packet::Flags::KEY);
        }

        // Rescale timestamps
        let stream = self.output_ctx.stream(self.video_stream_index)
            .ok_or_else(|| Error::Muxer("Video stream not found".into()))?;
        pkt.rescale_ts(self.video_time_base, stream.time_base());

        // Write packet
        pkt.write_interleaved(&mut self.output_ctx)
            .map_err(|e| Error::Muxer(format!("Failed to write video packet: {}", e)))?;

        self.video_frames += 1;
        self.bytes_written.fetch_add(packet.data.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Write an audio packet
    pub fn write_audio(&mut self, packet: &AudioPacket) -> Result<()> {
        if !self.initialized {
            return Err(Error::Muxer("Muxer not started".into()));
        }

        let stream_index = self.audio_stream_index
            .ok_or_else(|| Error::Muxer("No audio stream configured".into()))?;

        let audio_time_base = self.audio_time_base
            .ok_or_else(|| Error::Muxer("Audio time base not set".into()))?;

        let mut pkt = ffmpeg::Packet::copy(&packet.data);
        pkt.set_pts(Some(packet.pts));
        pkt.set_dts(Some(packet.dts));
        pkt.set_duration(packet.duration);
        pkt.set_stream(stream_index);

        // Rescale timestamps
        let stream = self.output_ctx.stream(stream_index)
            .ok_or_else(|| Error::Muxer("Audio stream not found".into()))?;
        pkt.rescale_ts(audio_time_base, stream.time_base());

        // Write packet
        pkt.write_interleaved(&mut self.output_ctx)
            .map_err(|e| Error::Muxer(format!("Failed to write audio packet: {}", e)))?;

        self.audio_frames += 1;
        self.bytes_written.fetch_add(packet.data.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Write a muxer packet (either video or audio)
    pub fn write_packet(&mut self, packet: &MuxerPacket) -> Result<()> {
        match packet {
            MuxerPacket::Video(p) => self.write_video(p),
            MuxerPacket::Audio(p) => self.write_audio(p),
        }
    }

    /// Finish muxing (write trailer)
    pub fn finish(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        self.output_ctx
            .write_trailer()
            .map_err(|e| Error::Muxer(format!("Failed to write trailer: {}", e)))?;

        let bytes = self.bytes_written.load(Ordering::Relaxed);
        tracing::info!(
            "Muxer finished: {} video frames, {} audio frames, {:.2} MB",
            self.video_frames,
            self.audio_frames,
            bytes as f64 / 1_000_000.0
        );

        self.initialized = false;
        Ok(())
    }

    /// Get bytes written
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    /// Check if audio is configured
    pub fn has_audio(&self) -> bool {
        self.audio_stream_index.is_some()
    }

    fn video_codec_to_ffmpeg(codec: Codec) -> CodecId {
        match codec {
            Codec::H264 => CodecId::H264,
            Codec::Hevc => CodecId::HEVC,
            Codec::Av1 => CodecId::AV1,
        }
    }

    fn audio_codec_to_ffmpeg(codec: crate::audio::AudioCodec) -> CodecId {
        match codec {
            crate::audio::AudioCodec::Aac => CodecId::AAC,
            crate::audio::AudioCodec::Opus => CodecId::OPUS,
            crate::audio::AudioCodec::Mp3 => CodecId::MP3,
            crate::audio::AudioCodec::Flac => CodecId::FLAC,
        }
    }
}

impl Drop for AvMuxer {
    fn drop(&mut self) {
        if self.initialized {
            let _ = self.output_ctx.write_trailer();
        }
    }
}
