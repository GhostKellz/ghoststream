//! GhostStream — NVIDIA GPU Video Engine
//!
//! High-performance video capture and encoding library for Linux.
//! Powers GhostCast (recording/streaming) and Nitrogen (Discord companion).
//!
//! # Features
//!
//! - **Capture**: Wayland screen capture via xdg-desktop-portal and PipeWire
//! - **Encode**: Hardware encoding via NVENC (H.264, HEVC, AV1)
//! - **Output**: Virtual camera, file recording, RTMP/SRT streaming
//!
//! # Example
//!
//! ```rust,no_run
//! use ghoststream::{Pipeline, CaptureConfig, EncoderConfig, Codec, Output};
//!
//! #[tokio::main]
//! async fn main() -> ghoststream::Result<()> {
//!     let capture = CaptureConfig::default();
//!     let encoder = EncoderConfig::default()
//!         .with_codec(Codec::H264)
//!         .with_resolution(1920, 1080);
//!     let output = Output::virtual_camera("GhostStream");
//!
//!     let pipeline = Pipeline::new(capture, encoder, output)?;
//!     pipeline.start().await?;
//!     Ok(())
//! }
//! ```

pub mod audio;
pub mod capture;
pub mod config;
pub mod encode;
pub mod error;
pub mod output;
pub mod pipeline;
pub mod processing;
pub mod types;

// Re-exports for convenience
pub use config::{CaptureConfig, EncoderConfig, Preset};
pub use encode::Codec;
pub use error::{Error, Result};
pub use output::{AvMuxer, Container, MuxerPacket, Output, StreamType};
pub use pipeline::{AudioConfig, Pipeline, PipelineBuilder};
pub use processing::{HdrConfig, Hdr10Metadata, ContentLightLevel, TransferFunction, ColorPrimaries};
pub use types::{Frame, FrameFormat, Resolution};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if NVENC is available on this system
pub fn is_nvenc_available() -> bool {
    encode::nvenc::is_available()
}

/// Get information about available encoders
pub fn get_encoder_info() -> encode::EncoderInfo {
    encode::get_info()
}
