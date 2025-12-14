//! Output module
//!
//! Provides various output destinations:
//! - Virtual camera (PipeWire)
//! - File recording (MKV, MP4, WebM)
//! - Streaming (RTMP, SRT)

mod camera;
mod file;

pub use camera::VirtualCamera;
pub use file::FileOutput;

use crate::error::Result;
use crate::types::{CodecParams, Packet};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Output destination configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    /// Virtual camera (appears in Discord, OBS, etc.)
    VirtualCamera {
        /// Camera name as shown in applications
        name: String,
    },

    /// File recording
    File {
        /// Output file path
        path: PathBuf,
        /// Container format
        container: Container,
    },

    /// RTMP streaming (Twitch, YouTube, etc.)
    Rtmp {
        /// RTMP URL with stream key
        url: String,
    },

    /// SRT streaming (low latency)
    Srt {
        /// SRT URL
        url: String,
        /// Latency in ms
        latency_ms: u32,
    },

    /// Multiple outputs (e.g., record + stream)
    Multiple(Vec<Output>),

    /// Null output (for testing)
    Null,
}

impl Output {
    /// Create a virtual camera output
    pub fn virtual_camera(name: impl Into<String>) -> Self {
        Output::VirtualCamera { name: name.into() }
    }

    /// Create a file output
    pub fn file(path: impl Into<PathBuf>, container: Container) -> Self {
        Output::File {
            path: path.into(),
            container,
        }
    }

    /// Create an RTMP streaming output
    pub fn rtmp(url: impl Into<String>) -> Self {
        Output::Rtmp { url: url.into() }
    }

    /// Create an SRT streaming output
    pub fn srt(url: impl Into<String>, latency_ms: u32) -> Self {
        Output::Srt {
            url: url.into(),
            latency_ms,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Output::VirtualCamera {
            name: "GhostStream Camera".into(),
        }
    }
}

/// Container format for file output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Container {
    /// Matroska (.mkv) - Best for recording
    #[default]
    Matroska,
    /// MP4 (.mp4) - Wide compatibility
    Mp4,
    /// WebM (.webm) - Web-friendly
    WebM,
    /// Transport Stream (.ts) - Streaming-friendly
    Ts,
}

impl Container {
    /// Get file extension
    pub fn extension(&self) -> &'static str {
        match self {
            Container::Matroska => "mkv",
            Container::Mp4 => "mp4",
            Container::WebM => "webm",
            Container::Ts => "ts",
        }
    }

    /// Get FFmpeg format name
    pub fn ffmpeg_format(&self) -> &'static str {
        match self {
            Container::Matroska => "matroska",
            Container::Mp4 => "mp4",
            Container::WebM => "webm",
            Container::Ts => "mpegts",
        }
    }
}

/// Trait for output sinks
#[async_trait::async_trait]
pub trait OutputSink: Send {
    /// Initialize the output with optional codec parameters
    async fn init_with_codec(&mut self, codec_params: Option<&CodecParams>) -> Result<()>;

    /// Initialize without codec params (uses defaults or auto-detection)
    async fn init(&mut self) -> Result<()> {
        self.init_with_codec(None).await
    }

    /// Write an encoded packet
    async fn write(&mut self, packet: &Packet) -> Result<()>;

    /// Flush and finalize
    async fn finish(&mut self) -> Result<()>;

    /// Get bytes written
    fn bytes_written(&self) -> u64;
}

/// Create an output sink from configuration
pub async fn create_output(output: Output) -> Result<Box<dyn OutputSink>> {
    match output {
        Output::VirtualCamera { name } => {
            let camera = VirtualCamera::new(name);
            Ok(Box::new(camera))
        }
        Output::File { path, container } => {
            let file = FileOutput::new(path, container);
            Ok(Box::new(file))
        }
        Output::Rtmp { url: _ } => {
            // TODO: Implement RTMP output
            todo!("RTMP output not yet implemented")
        }
        Output::Srt {
            url: _,
            latency_ms: _,
        } => {
            // TODO: Implement SRT output
            todo!("SRT output not yet implemented")
        }
        Output::Multiple(_outputs) => {
            // TODO: Implement multi-output
            todo!("Multi-output not yet implemented")
        }
        Output::Null => Ok(Box::new(NullOutput::default())),
    }
}

/// Null output (discards all packets)
#[derive(Default)]
struct NullOutput {
    bytes: u64,
}

#[async_trait::async_trait]
impl OutputSink for NullOutput {
    async fn init_with_codec(&mut self, _codec_params: Option<&CodecParams>) -> Result<()> {
        Ok(())
    }

    async fn write(&mut self, packet: &Packet) -> Result<()> {
        self.bytes += packet.size() as u64;
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes
    }
}
