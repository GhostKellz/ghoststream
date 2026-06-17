//! Audio types

use serde::{Deserialize, Serialize};

/// Audio sample format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SampleFormat {
    /// 16-bit signed integer
    #[default]
    S16,
    /// 32-bit signed integer
    S32,
    /// 32-bit float
    F32,
    /// Planar 16-bit signed integer
    S16P,
    /// Planar 32-bit float
    F32P,
}

impl SampleFormat {
    /// Bytes per sample
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            SampleFormat::S16 | SampleFormat::S16P => 2,
            SampleFormat::S32 | SampleFormat::F32 | SampleFormat::F32P => 4,
        }
    }

    /// Is this a planar format?
    pub fn is_planar(&self) -> bool {
        matches!(self, SampleFormat::S16P | SampleFormat::F32P)
    }

    /// FFmpeg format string
    pub fn ffmpeg_format(&self) -> &'static str {
        match self {
            SampleFormat::S16 => "s16",
            SampleFormat::S32 => "s32",
            SampleFormat::F32 => "flt",
            SampleFormat::S16P => "s16p",
            SampleFormat::F32P => "fltp",
        }
    }
}

/// Audio channel layout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ChannelLayout {
    /// Mono (1 channel)
    Mono,
    /// Stereo (2 channels)
    #[default]
    Stereo,
    /// 5.1 surround (6 channels)
    Surround51,
    /// 7.1 surround (8 channels)
    Surround71,
}

impl ChannelLayout {
    /// Number of channels
    pub fn channels(&self) -> u32 {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
            ChannelLayout::Surround51 => 6,
            ChannelLayout::Surround71 => 8,
        }
    }

    /// FFmpeg channel layout string
    pub fn ffmpeg_layout(&self) -> &'static str {
        match self {
            ChannelLayout::Mono => "mono",
            ChannelLayout::Stereo => "stereo",
            ChannelLayout::Surround51 => "5.1",
            ChannelLayout::Surround71 => "7.1",
        }
    }
}

/// Audio frame containing samples
#[derive(Debug)]
pub struct AudioFrame {
    /// Sample data
    pub data: Vec<u8>,
    /// Number of samples per channel
    pub samples: u32,
    /// Sample format
    pub format: SampleFormat,
    /// Number of channels
    pub channels: u32,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Presentation timestamp in microseconds
    pub pts: i64,
    /// Duration in microseconds
    pub duration: i64,
}

impl AudioFrame {
    /// Create a new audio frame
    pub fn new(samples: u32, channels: u32, format: SampleFormat, sample_rate: u32) -> Self {
        let size = samples as usize * channels as usize * format.bytes_per_sample();
        Self {
            data: vec![0u8; size],
            samples,
            format,
            channels,
            sample_rate,
            pts: 0,
            duration: 0,
        }
    }

    /// Create from existing data
    pub fn from_data(
        data: Vec<u8>,
        samples: u32,
        channels: u32,
        format: SampleFormat,
        sample_rate: u32,
    ) -> Self {
        Self {
            data,
            samples,
            format,
            channels,
            sample_rate,
            pts: 0,
            duration: 0,
        }
    }

    /// Calculate duration in microseconds based on sample count and rate
    pub fn calculated_duration_us(&self) -> i64 {
        (self.samples as i64 * 1_000_000) / self.sample_rate as i64
    }

    /// Size in bytes
    pub fn size_bytes(&self) -> usize {
        self.data.len()
    }

    /// Convert to f32 slice (for processing)
    ///
    /// Returns borrowed slice if data is already F32 format,
    /// otherwise converts and returns owned Vec as slice.
    pub fn as_f32_slice(&self) -> crate::Result<std::borrow::Cow<'_, [f32]>> {
        match self.format {
            SampleFormat::F32 => {
                // Safety: we know the data is f32 samples
                let ptr = self.data.as_ptr() as *const f32;
                let len = self.data.len() / 4;
                Ok(std::borrow::Cow::Borrowed(unsafe {
                    std::slice::from_raw_parts(ptr, len)
                }))
            }
            SampleFormat::S16 => {
                // Convert S16 to F32
                let samples = self.data.len() / 2;
                let mut f32_data = Vec::with_capacity(samples);
                for chunk in self.data.chunks_exact(2) {
                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                    f32_data.push(sample as f32 / 32768.0);
                }
                Ok(std::borrow::Cow::Owned(f32_data))
            }
            SampleFormat::S32 => {
                // Convert S32 to F32
                let samples = self.data.len() / 4;
                let mut f32_data = Vec::with_capacity(samples);
                for chunk in self.data.chunks_exact(4) {
                    let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    f32_data.push(sample as f32 / 2147483648.0);
                }
                Ok(std::borrow::Cow::Owned(f32_data))
            }
            _ => Err(crate::Error::AudioProcessing(format!(
                "Cannot convert {:?} to f32 slice",
                self.format
            ))),
        }
    }
}

/// Audio codec parameters for muxing
#[derive(Debug, Clone)]
pub struct AudioParams {
    /// Audio codec
    pub codec: super::AudioCodec,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u32,
    /// Channel layout
    pub layout: ChannelLayout,
    /// Bitrate in bits/sec
    pub bitrate: u32,
    /// Extradata (codec-specific headers)
    pub extradata: Vec<u8>,
}

impl Default for AudioParams {
    fn default() -> Self {
        Self {
            codec: super::AudioCodec::Aac,
            sample_rate: 48000,
            channels: 2,
            layout: ChannelLayout::Stereo,
            bitrate: 192_000,
            extradata: Vec::new(),
        }
    }
}

/// Encoded audio packet
#[derive(Debug)]
pub struct AudioPacket {
    /// Encoded data
    pub data: Vec<u8>,
    /// Presentation timestamp
    pub pts: i64,
    /// Decode timestamp
    pub dts: i64,
    /// Duration
    pub duration: i64,
}

impl AudioPacket {
    pub fn new(data: Vec<u8>, pts: i64, dts: i64) -> Self {
        Self {
            data,
            pts,
            dts,
            duration: 0,
        }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}
