//! Audio encoding via FFmpeg
//!
//! Supports AAC and Opus codecs.

use crate::error::{Error, Result};
use super::types::{AudioFrame, AudioPacket, AudioParams, ChannelLayout, SampleFormat};

use ffmpeg_next as ffmpeg;

/// Audio codec
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioCodec {
    /// AAC - Wide compatibility, good for streaming
    #[default]
    Aac,
    /// Opus - Best quality/bitrate, great for voice/music
    Opus,
    /// MP3 - Legacy compatibility
    Mp3,
    /// FLAC - Lossless
    Flac,
}

impl AudioCodec {
    /// FFmpeg encoder name
    pub fn encoder_name(&self) -> &'static str {
        match self {
            AudioCodec::Aac => "aac",
            AudioCodec::Opus => "libopus",
            AudioCodec::Mp3 => "libmp3lame",
            AudioCodec::Flac => "flac",
        }
    }

    /// FFmpeg codec ID
    pub fn codec_id(&self) -> ffmpeg::codec::Id {
        match self {
            AudioCodec::Aac => ffmpeg::codec::Id::AAC,
            AudioCodec::Opus => ffmpeg::codec::Id::OPUS,
            AudioCodec::Mp3 => ffmpeg::codec::Id::MP3,
            AudioCodec::Flac => ffmpeg::codec::Id::FLAC,
        }
    }

    /// Display name
    pub fn display_name(&self) -> &'static str {
        match self {
            AudioCodec::Aac => "AAC",
            AudioCodec::Opus => "Opus",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::Flac => "FLAC",
        }
    }

    /// Recommended bitrate for this codec (for stereo)
    pub fn recommended_bitrate(&self) -> u32 {
        match self {
            AudioCodec::Aac => 192_000,
            AudioCodec::Opus => 128_000,
            AudioCodec::Mp3 => 320_000,
            AudioCodec::Flac => 0, // Lossless
        }
    }
}

/// Audio encoder configuration
#[derive(Debug, Clone)]
pub struct AudioEncoderConfig {
    /// Audio codec
    pub codec: AudioCodec,
    /// Sample rate (default: 48000)
    pub sample_rate: u32,
    /// Channel layout (default: stereo)
    pub channels: ChannelLayout,
    /// Bitrate in bits/sec (default: codec recommended)
    pub bitrate: u32,
    /// Input sample format
    pub input_format: SampleFormat,
}

impl Default for AudioEncoderConfig {
    fn default() -> Self {
        Self {
            codec: AudioCodec::Aac,
            sample_rate: 48000,
            channels: ChannelLayout::Stereo,
            bitrate: 192_000,
            input_format: SampleFormat::F32,
        }
    }
}

impl AudioEncoderConfig {
    pub fn with_codec(mut self, codec: AudioCodec) -> Self {
        self.codec = codec;
        if self.bitrate == 0 {
            self.bitrate = codec.recommended_bitrate();
        }
        self
    }

    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate;
        self
    }

    pub fn with_channels(mut self, channels: ChannelLayout) -> Self {
        self.channels = channels;
        self
    }

    pub fn with_bitrate(mut self, bitrate: u32) -> Self {
        self.bitrate = bitrate;
        self
    }
}

/// Trait for audio encoders
pub trait AudioEncoder: Send {
    /// Initialize the encoder
    fn init(&mut self) -> Result<()>;

    /// Encode audio frame
    fn encode(&mut self, frame: &AudioFrame) -> Result<Option<AudioPacket>>;

    /// Flush remaining frames
    fn flush(&mut self) -> Result<Vec<AudioPacket>>;

    /// Get encoder statistics
    fn stats(&self) -> AudioEncoderStats;

    /// Get codec parameters for muxing
    fn params(&self) -> Option<AudioParams>;
}

/// Audio encoder statistics
#[derive(Debug, Clone, Default)]
pub struct AudioEncoderStats {
    /// Frames encoded
    pub frames_encoded: u64,
    /// Bytes output
    pub bytes_output: u64,
}

/// FFmpeg-based audio encoder
pub struct FfmpegAudioEncoder {
    config: AudioEncoderConfig,
    encoder: Option<ffmpeg::encoder::Audio>,
    stats: AudioEncoderStats,
    pts: i64,
    frame_size: u32,
    initialized: bool,
}

impl FfmpegAudioEncoder {
    pub fn new(config: AudioEncoderConfig) -> Result<Self> {
        Ok(Self {
            config,
            encoder: None,
            stats: AudioEncoderStats::default(),
            pts: 0,
            frame_size: 1024,
            initialized: false,
        })
    }
}

impl AudioEncoder for FfmpegAudioEncoder {
    fn init(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        ffmpeg::init().map_err(|e| Error::Ffmpeg(format!("FFmpeg init failed: {}", e)))?;

        // Find encoder
        let codec = ffmpeg::encoder::find_by_name(self.config.codec.encoder_name())
            .or_else(|| ffmpeg::encoder::find(self.config.codec.codec_id()))
            .ok_or_else(|| Error::CodecNotSupported(
                format!("Audio encoder {} not found", self.config.codec.encoder_name())
            ))?;

        // Create encoder context
        let context = ffmpeg::codec::context::Context::new_with_codec(codec);
        let mut encoder = context
            .encoder()
            .audio()
            .map_err(|e| Error::Ffmpeg(format!("Not an audio encoder: {}", e)))?;

        // Configure encoder via unsafe
        unsafe {
            let ctx = encoder.as_mut_ptr();
            (*ctx).sample_rate = self.config.sample_rate as i32;
            (*ctx).sample_fmt = ffmpeg_next::ffi::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            (*ctx).bit_rate = self.config.bitrate as i64;
            (*ctx).time_base = ffmpeg_next::ffi::AVRational {
                num: 1,
                den: self.config.sample_rate as i32,
            };

            // Set channel layout using the new API (FFmpeg 7+)
            let ch_layout = &mut (*ctx).ch_layout;
            let nb_channels = self.config.channels.channels() as i32;
            ffmpeg_next::ffi::av_channel_layout_default(ch_layout, nb_channels);
        }

        // Open encoder
        let encoder = encoder
            .open()
            .map_err(|e| Error::Ffmpeg(format!("Failed to open audio encoder: {}", e)))?;

        // Get frame size from encoder
        self.frame_size = unsafe { (*encoder.as_ptr()).frame_size as u32 };
        if self.frame_size == 0 {
            self.frame_size = 1024;
        }

        tracing::info!(
            "Audio encoder initialized: {} @ {}Hz, {} channels, {} kbps, frame_size={}",
            self.config.codec.display_name(),
            self.config.sample_rate,
            self.config.channels.channels(),
            self.config.bitrate / 1000,
            self.frame_size
        );

        self.encoder = Some(encoder);
        self.initialized = true;
        Ok(())
    }

    fn encode(&mut self, frame: &AudioFrame) -> Result<Option<AudioPacket>> {
        if !self.initialized {
            self.init()?;
        }

        let encoder = self.encoder.as_mut().ok_or(Error::EncoderNotInitialized)?;

        // Create FFmpeg audio frame
        let mut ff_frame = ffmpeg::frame::Audio::new(
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar),
            frame.samples as usize,
            ffmpeg::channel_layout::ChannelLayout::STEREO,
        );

        ff_frame.set_rate(self.config.sample_rate);
        ff_frame.set_pts(Some(self.pts));
        self.pts += frame.samples as i64;

        // Copy audio data - basic conversion
        let plane = ff_frame.data_mut(0);
        let copy_len = plane.len().min(frame.data.len());
        plane[..copy_len].copy_from_slice(&frame.data[..copy_len]);

        // Send frame to encoder
        encoder
            .send_frame(&ff_frame)
            .map_err(|e| Error::Ffmpeg(format!("Send frame failed: {}", e)))?;

        // Receive encoded packet
        let mut packet = ffmpeg::Packet::empty();
        match encoder.receive_packet(&mut packet) {
            Ok(()) => {
                self.stats.frames_encoded += 1;
                self.stats.bytes_output += packet.size() as u64;

                let audio_packet = AudioPacket {
                    data: packet.data().unwrap_or(&[]).to_vec(),
                    pts: packet.pts().unwrap_or(0),
                    dts: packet.dts().unwrap_or(0),
                    duration: packet.duration(),
                };
                Ok(Some(audio_packet))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => Ok(None),
            Err(e) => Err(Error::Ffmpeg(format!("Receive packet failed: {}", e))),
        }
    }

    fn flush(&mut self) -> Result<Vec<AudioPacket>> {
        let encoder = self.encoder.as_mut().ok_or(Error::EncoderNotInitialized)?;

        encoder
            .send_eof()
            .map_err(|e| Error::Ffmpeg(format!("Send EOF failed: {}", e)))?;

        let mut packets = Vec::new();
        let mut packet = ffmpeg::Packet::empty();

        loop {
            match encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    self.stats.frames_encoded += 1;
                    self.stats.bytes_output += packet.size() as u64;

                    packets.push(AudioPacket {
                        data: packet.data().unwrap_or(&[]).to_vec(),
                        pts: packet.pts().unwrap_or(0),
                        dts: packet.dts().unwrap_or(0),
                        duration: packet.duration(),
                    });
                }
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                Err(e) => {
                    tracing::warn!("Error during flush: {}", e);
                    break;
                }
            }
        }

        Ok(packets)
    }

    fn stats(&self) -> AudioEncoderStats {
        self.stats.clone()
    }

    fn params(&self) -> Option<AudioParams> {
        let encoder = self.encoder.as_ref()?;

        // Get extradata
        let extradata = unsafe {
            let ptr = (*encoder.as_ptr()).extradata;
            let size = (*encoder.as_ptr()).extradata_size as usize;
            if !ptr.is_null() && size > 0 {
                std::slice::from_raw_parts(ptr, size).to_vec()
            } else {
                Vec::new()
            }
        };

        Some(AudioParams {
            codec: self.config.codec,
            sample_rate: self.config.sample_rate,
            channels: self.config.channels.channels(),
            layout: self.config.channels,
            bitrate: self.config.bitrate,
            extradata,
        })
    }
}

/// Check if a codec is available
pub fn is_codec_available(codec: AudioCodec) -> bool {
    ffmpeg::init().ok();
    ffmpeg::encoder::find_by_name(codec.encoder_name()).is_some()
        || ffmpeg::encoder::find(codec.codec_id()).is_some()
}

/// Get available audio codecs
pub fn available_codecs() -> Vec<AudioCodec> {
    let mut codecs = Vec::new();
    for codec in [AudioCodec::Aac, AudioCodec::Opus, AudioCodec::Mp3, AudioCodec::Flac] {
        if is_codec_available(codec) {
            codecs.push(codec);
        }
    }
    codecs
}
