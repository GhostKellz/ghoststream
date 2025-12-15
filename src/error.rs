//! Error types for GhostStream

use thiserror::Error;

/// Result type alias for GhostStream operations
pub type Result<T> = std::result::Result<T, Error>;

/// GhostStream error type
#[derive(Error, Debug)]
pub enum Error {
    // Capture errors
    #[error("Portal error: {0}")]
    Portal(String),

    #[error("PipeWire error: {0}")]
    PipeWire(String),

    #[error("No capture source selected")]
    NoCaptureSource,

    #[error("Capture permission denied")]
    CapturePermissionDenied,

    // Encoder errors
    #[error("NVENC not available: {0}")]
    NvencNotAvailable(String),

    #[error("Codec not supported: {0}")]
    CodecNotSupported(String),

    #[error("Encoder initialization failed: {0}")]
    EncoderInit(String),

    #[error("Encoding failed: {0}")]
    EncodingFailed(String),

    #[error("Invalid encoder configuration: {0}")]
    InvalidEncoderConfig(String),

    // Output errors
    #[error("Output initialization failed: {0}")]
    OutputInit(String),

    #[error("Failed to create virtual camera: {0}")]
    VirtualCamera(String),

    #[error("File output error: {0}")]
    FileOutput(String),

    #[error("Streaming error: {0}")]
    Streaming(String),

    // Pipeline errors
    #[error("Pipeline not started")]
    PipelineNotStarted,

    #[error("Pipeline already running")]
    PipelineAlreadyRunning,

    #[error("Pipeline error: {0}")]
    Pipeline(String),

    // Processing errors
    #[error("Scaling error: {0}")]
    Scaling(String),

    #[error("Colorspace conversion error: {0}")]
    ColorspaceConversion(String),

    // FFmpeg errors
    #[error("FFmpeg error: {0}")]
    FFmpeg(String),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),

    // Audio errors
    #[error("Audio capture error: {0}")]
    AudioCapture(String),

    #[error("Audio encoder error: {0}")]
    AudioEncoder(String),

    #[error("Capture not started")]
    CaptureNotStarted,

    #[error("Capture ended")]
    CaptureEnded,

    #[error("Encoder not initialized")]
    EncoderNotInitialized,

    #[error("Timeout: {0}")]
    Timeout(String),

    // Streaming errors
    #[error("RTMP error: {0}")]
    Rtmp(String),

    #[error("SRT error: {0}")]
    Srt(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    // Muxer errors
    #[error("Muxer error: {0}")]
    Muxer(String),

    // General errors
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Error::EncodingFailed(_) | Error::Streaming(_) | Error::Pipeline(_)
        )
    }

    /// Check if this is a hardware/driver issue
    pub fn is_hardware_issue(&self) -> bool {
        matches!(
            self,
            Error::NvencNotAvailable(_) | Error::CodecNotSupported(_)
        )
    }
}
