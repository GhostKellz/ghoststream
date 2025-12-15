//! Output module
//!
//! Provides various output destinations:
//! - Virtual camera (PipeWire)
//! - File recording (MKV, MP4, WebM)
//! - Streaming (RTMP, SRT)
//! - A/V Muxing

mod camera;
mod file;
mod muxer;
mod rtmp;
mod srt;

pub use camera::VirtualCamera;
pub use file::FileOutput;
pub use muxer::{AvMuxer, MuxerPacket, StreamType};
pub use rtmp::{RtmpOutput, RtmpService};
pub use srt::{SrtMode, SrtOutput, SrtStats};

use crate::error::Result;
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution};
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

    /// Create a multi-output (record + stream, etc.)
    pub fn multiple(outputs: Vec<Output>) -> Self {
        Output::Multiple(outputs)
    }

    /// Combine two outputs
    pub fn and(self, other: Output) -> Self {
        match self {
            Output::Multiple(mut outputs) => {
                outputs.push(other);
                Output::Multiple(outputs)
            }
            _ => Output::Multiple(vec![self, other]),
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

/// Trait for output sinks (encoded packets)
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

/// Trait for raw frame output sinks (uncompressed frames)
///
/// Use this for outputs that need raw video frames instead of encoded packets:
/// - Virtual cameras (PipeWire)
/// - Frame grabbers
/// - Preview displays
#[async_trait::async_trait]
pub trait RawOutputSink: Send {
    /// Initialize with frame format and resolution
    async fn init_raw(&mut self, resolution: Resolution, format: FrameFormat) -> Result<()>;

    /// Write a raw frame
    async fn write_frame(&mut self, frame: &Frame) -> Result<()>;

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
        Output::Rtmp { url } => {
            let rtmp = RtmpOutput::new(url);
            Ok(Box::new(rtmp))
        }
        Output::Srt { url, latency_ms } => {
            let srt = SrtOutput::new(url, latency_ms);
            Ok(Box::new(srt))
        }
        Output::Multiple(outputs) => {
            let multi = MultiOutput::new(outputs).await?;
            Ok(Box::new(multi))
        }
        Output::Null => Ok(Box::new(NullOutput::default())),
    }
}

/// Multi-output that writes to multiple destinations simultaneously
pub struct MultiOutput {
    outputs: Vec<Box<dyn OutputSink>>,
}

impl MultiOutput {
    /// Create a multi-output from a list of output configs
    pub async fn new(configs: Vec<Output>) -> Result<Self> {
        let mut outputs = Vec::with_capacity(configs.len());

        for config in configs {
            // Create each output directly to avoid async recursion
            let output: Box<dyn OutputSink> = match config {
                Output::VirtualCamera { name } => Box::new(VirtualCamera::new(name)),
                Output::File { path, container } => Box::new(FileOutput::new(path, container)),
                Output::Rtmp { url } => Box::new(RtmpOutput::new(url)),
                Output::Srt { url, latency_ms } => Box::new(SrtOutput::new(url, latency_ms)),
                Output::Multiple(_) => {
                    tracing::warn!("Nested multi-output not supported, skipping");
                    continue;
                }
                Output::Null => Box::new(NullOutput::default()),
            };
            outputs.push(output);
        }

        if outputs.is_empty() {
            return Err(crate::error::Error::OutputInit(
                "No valid outputs in multi-output".into(),
            ));
        }

        tracing::info!("Multi-output created with {} destinations", outputs.len());
        Ok(Self { outputs })
    }
}

#[async_trait::async_trait]
impl OutputSink for MultiOutput {
    async fn init_with_codec(&mut self, codec_params: Option<&CodecParams>) -> Result<()> {
        let mut errors = Vec::new();

        for (i, output) in self.outputs.iter_mut().enumerate() {
            if let Err(e) = output.init_with_codec(codec_params).await {
                tracing::error!("Failed to init output {}: {}", i, e);
                errors.push(e);
            }
        }

        // Return first error if all failed
        if errors.len() == self.outputs.len() && !errors.is_empty() {
            return Err(errors.remove(0));
        }

        Ok(())
    }

    async fn write(&mut self, packet: &Packet) -> Result<()> {
        // Write to all outputs, continuing even if some fail
        for (i, output) in self.outputs.iter_mut().enumerate() {
            if let Err(e) = output.write(packet).await {
                tracing::error!("Output {} write error: {}", i, e);
                // Continue writing to other outputs
            }
        }
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        for (i, output) in self.outputs.iter_mut().enumerate() {
            if let Err(e) = output.finish().await {
                tracing::error!("Failed to finish output {}: {}", i, e);
                errors.push(e);
            }
        }

        // Return first error if any
        if let Some(e) = errors.into_iter().next() {
            return Err(e);
        }

        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        // Return max bytes across all outputs (they should all be roughly the same)
        self.outputs.iter().map(|o| o.bytes_written()).max().unwrap_or(0)
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
