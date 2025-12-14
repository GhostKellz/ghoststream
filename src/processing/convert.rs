//! Colorspace conversion

use crate::error::{Error, Result};
use crate::types::FrameFormat;

/// Colorspace converter
pub struct ColorspaceConverter {
    // FFmpeg SwsContext for conversion
}

impl ColorspaceConverter {
    pub fn new() -> Self {
        Self {}
    }

    /// Convert frame colorspace
    pub fn convert(
        &self,
        input: &[u8],
        src_format: FrameFormat,
        dst_format: FrameFormat,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        convert_colorspace(input, src_format, dst_format, width, height)
    }
}

impl Default for ColorspaceConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert frame colorspace
pub fn convert_colorspace(
    input: &[u8],
    src_format: FrameFormat,
    dst_format: FrameFormat,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    if src_format == dst_format {
        return Ok(input.to_vec());
    }

    // TODO: Use FFmpeg swscale for proper conversion
    //
    // let src_pix = format_to_ffmpeg(src_format);
    // let dst_pix = format_to_ffmpeg(dst_format);
    //
    // let mut sws = ffmpeg::software::scaling::Context::get(
    //     src_pix, width, height,
    //     dst_pix, width, height,
    //     Flags::BILINEAR,
    // )?;

    let w = width as usize;
    let h = height as usize;

    match (src_format, dst_format) {
        (FrameFormat::Bgra, FrameFormat::Nv12) => bgra_to_nv12(input, w, h),
        (FrameFormat::Rgba, FrameFormat::Nv12) => rgba_to_nv12(input, w, h),
        (FrameFormat::Bgra, FrameFormat::Rgba) => bgra_to_rgba(input),
        (FrameFormat::Rgba, FrameFormat::Bgra) => bgra_to_rgba(input), // Same swap
        _ => Err(Error::ColorspaceConversion(format!(
            "Unsupported conversion: {:?} -> {:?}",
            src_format, dst_format
        ))),
    }
}

/// BGRA to RGBA (or vice versa - just swap R and B)
fn bgra_to_rgba(input: &[u8]) -> Result<Vec<u8>> {
    let mut output = input.to_vec();
    for chunk in output.chunks_exact_mut(4) {
        chunk.swap(0, 2); // Swap B and R
    }
    Ok(output)
}

/// BGRA to NV12 conversion
fn bgra_to_nv12(input: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let y_size = width * height;
    let uv_size = (width / 2) * (height / 2) * 2;
    let mut output = vec![0u8; y_size + uv_size];

    // Y plane
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let b = input[idx] as f32;
            let g = input[idx + 1] as f32;
            let r = input[idx + 2] as f32;

            // BT.601 RGB to Y
            let y_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
            output[y * width + x] = y_val;
        }
    }

    // UV plane (interleaved, subsampled 2x2)
    let uv_offset = y_size;
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let idx = (y * width + x) * 4;
            let b = input[idx] as f32;
            let g = input[idx + 1] as f32;
            let r = input[idx + 2] as f32;

            // BT.601 RGB to U, V
            let u = ((-0.169 * r - 0.331 * g + 0.5 * b) + 128.0) as u8;
            let v = ((0.5 * r - 0.419 * g - 0.081 * b) + 128.0) as u8;

            let uv_idx = uv_offset + (y / 2) * width + x;
            output[uv_idx] = u;
            output[uv_idx + 1] = v;
        }
    }

    Ok(output)
}

/// RGBA to NV12 conversion
fn rgba_to_nv12(input: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    // Convert RGBA to BGRA first, then to NV12
    let bgra = bgra_to_rgba(input)?; // Swap R and B
    bgra_to_nv12(&bgra, width, height)
}
