//! Video processing module
//!
//! Provides frame processing capabilities:
//! - Resolution scaling
//! - Colorspace conversion
//! - HDR to SDR tonemapping

mod convert;
mod scale;

pub use convert::ColorspaceConverter;
pub use scale::Scaler;

use crate::error::Result;
use crate::types::{Frame, FrameFormat, Resolution};

/// Process a frame (scale, convert, etc.)
pub fn process_frame(
    frame: &Frame,
    target_resolution: Option<Resolution>,
    target_format: Option<FrameFormat>,
) -> Result<Frame> {
    let mut result = frame.data.clone();
    let mut width = frame.width;
    let mut height = frame.height;
    let mut format = frame.format;

    // Scale if needed
    if let Some(res) = target_resolution {
        if res.width != frame.width || res.height != frame.height {
            result = scale::scale_frame(&result, frame.width, frame.height, res.width, res.height)?;
            width = res.width;
            height = res.height;
        }
    }

    // Convert colorspace if needed
    if let Some(fmt) = target_format {
        if fmt != frame.format {
            result = convert::convert_colorspace(&result, format, fmt, width, height)?;
            format = fmt;
        }
    }

    Ok(Frame {
        data: result,
        width,
        height,
        stride: width * 4, // Adjust based on format
        format,
        pts: frame.pts,
        duration: frame.duration,
        is_keyframe: frame.is_keyframe,
        dmabuf_fd: None, // Processing breaks zero-copy
    })
}
