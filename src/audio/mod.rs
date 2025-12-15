//! Audio capture and encoding module
//!
//! Provides:
//! - PipeWire audio capture (desktop/application audio)
//! - FFmpeg audio encoding (AAC, Opus)

mod capture;
mod encode;
mod types;

pub use capture::{AudioCapture, AudioCaptureConfig, AudioSource, PipeWireAudioCapture};
pub use encode::{
    available_codecs, is_codec_available, AudioCodec, AudioEncoder, AudioEncoderConfig,
    FfmpegAudioEncoder,
};
pub use types::{AudioFrame, AudioPacket, AudioParams, ChannelLayout, SampleFormat};

use crate::error::Result;

/// Create audio capture from config
pub async fn create_audio_capture(config: AudioCaptureConfig) -> Result<Box<dyn AudioCapture>> {
    let capture = capture::PipeWireAudioCapture::new(config)?;
    Ok(Box::new(capture))
}

/// Create audio encoder from config
pub fn create_audio_encoder(config: AudioEncoderConfig) -> Result<Box<dyn AudioEncoder>> {
    let encoder = encode::FfmpegAudioEncoder::new(config)?;
    Ok(Box::new(encoder))
}

/// Check if audio capture is available
pub fn is_audio_available() -> bool {
    // Check for PipeWire
    std::process::Command::new("pw-cli")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get available audio sources
pub fn list_audio_sources() -> Vec<AudioSource> {
    capture::list_sources()
}
