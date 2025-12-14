//! Screen capture module
//!
//! Provides screen capture via:
//! - xdg-desktop-portal (recommended for Wayland)
//! - PipeWire direct capture
//! - wlroots DMA-BUF export

mod portal;
mod stream;

pub use portal::PortalCapture;
pub use stream::CaptureStream;

use crate::config::{CaptureBackend, CaptureConfig};
use crate::error::Result;
use crate::types::Frame;

/// Trait for capture sources
#[async_trait::async_trait]
pub trait Capture: Send + Sync {
    /// Start capture session
    async fn start(&mut self) -> Result<()>;

    /// Stop capture session
    async fn stop(&mut self) -> Result<()>;

    /// Get next frame
    async fn next_frame(&mut self) -> Result<Frame>;

    /// Check if capture is active
    fn is_active(&self) -> bool;

    /// Get current resolution
    fn resolution(&self) -> Option<crate::types::Resolution>;

    /// Get current framerate
    fn framerate(&self) -> Option<crate::types::Framerate>;
}

/// Create a capture source based on configuration
pub async fn create_capture(config: CaptureConfig) -> Result<Box<dyn Capture>> {
    let backend = if config.backend == CaptureBackend::Auto {
        detect_best_backend()
    } else {
        config.backend
    };

    match backend {
        CaptureBackend::Auto => unreachable!(),
        CaptureBackend::Portal => {
            let capture = PortalCapture::new(config).await?;
            Ok(Box::new(capture))
        }
        CaptureBackend::PipeWire => {
            // For now, use portal which internally uses PipeWire
            let capture = PortalCapture::new(config).await?;
            Ok(Box::new(capture))
        }
        CaptureBackend::WlrExport => {
            // TODO: Implement wlroots DMA-BUF export
            let capture = PortalCapture::new(config).await?;
            Ok(Box::new(capture))
        }
    }
}

/// Detect the best capture backend for this system
fn detect_best_backend() -> CaptureBackend {
    // Check for Wayland
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return CaptureBackend::Portal;
    }

    // Check XDG session type
    if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
        if session == "wayland" {
            return CaptureBackend::Portal;
        }
    }

    // Default to portal (works on X11 too via xdg-desktop-portal-gtk)
    CaptureBackend::Portal
}

/// Capture source info
#[derive(Debug, Clone)]
pub struct CaptureSourceInfo {
    /// Source ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Source type
    pub source_type: CaptureSourceType,
    /// Resolution if known
    pub resolution: Option<crate::types::Resolution>,
}

/// Type of capture source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSourceType {
    /// Full monitor/display
    Monitor,
    /// Application window
    Window,
    /// Virtual source
    Virtual,
}
