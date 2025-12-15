//! Frame scaling using FFmpeg swscale

use crate::error::{Error, Result};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as SwsContext, Flags as SwsFlags};

/// Scaling algorithm
#[derive(Debug, Clone, Copy, Default)]
pub enum ScaleAlgorithm {
    /// Nearest neighbor (fastest, pixelated)
    Nearest,
    /// Bilinear (fast, smooth)
    #[default]
    Bilinear,
    /// Bicubic (balanced)
    Bicubic,
    /// Lanczos (best quality, slowest)
    Lanczos,
}

impl ScaleAlgorithm {
    /// Convert to FFmpeg swscale flags
    fn to_sws_flags(&self) -> SwsFlags {
        match self {
            ScaleAlgorithm::Nearest => SwsFlags::POINT,
            ScaleAlgorithm::Bilinear => SwsFlags::BILINEAR,
            ScaleAlgorithm::Bicubic => SwsFlags::BICUBIC,
            ScaleAlgorithm::Lanczos => SwsFlags::LANCZOS,
        }
    }
}

/// Frame scaler using FFmpeg swscale
pub struct Scaler {
    algorithm: ScaleAlgorithm,
}

impl Scaler {
    pub fn new(algorithm: ScaleAlgorithm) -> Self {
        // Initialize FFmpeg once
        let _ = ffmpeg::init();
        Self { algorithm }
    }

    /// Scale a frame (assumes BGRA format)
    pub fn scale(
        &self,
        input: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<Vec<u8>> {
        scale_frame_with_algorithm(
            input,
            src_width,
            src_height,
            dst_width,
            dst_height,
            self.algorithm,
        )
    }
}

impl Default for Scaler {
    fn default() -> Self {
        Self::new(ScaleAlgorithm::Bilinear)
    }
}

/// Scale frame data using FFmpeg swscale (assumes BGRA format)
pub fn scale_frame(
    input: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>> {
    scale_frame_with_algorithm(
        input,
        src_width,
        src_height,
        dst_width,
        dst_height,
        ScaleAlgorithm::Bilinear,
    )
}

/// Scale frame data with specific algorithm
pub fn scale_frame_with_algorithm(
    input: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    algorithm: ScaleAlgorithm,
) -> Result<Vec<u8>> {
    // No scaling needed
    if src_width == dst_width && src_height == dst_height {
        return Ok(input.to_vec());
    }

    let _ = ffmpeg::init();

    let pixel_format = Pixel::BGRA;
    let bpp = 4; // Bytes per pixel for BGRA

    // Validate input size
    let expected_size = (src_width * src_height * bpp) as usize;
    if input.len() < expected_size {
        return Err(Error::Scaling(format!(
            "Input buffer too small: {} < {} ({}x{}x{})",
            input.len(),
            expected_size,
            src_width,
            src_height,
            bpp
        )));
    }

    // Create scaling context
    let mut scaler = SwsContext::get(
        pixel_format,
        src_width,
        src_height,
        pixel_format,
        dst_width,
        dst_height,
        algorithm.to_sws_flags(),
    )
    .map_err(|e| Error::Scaling(format!("Failed to create scaler: {}", e)))?;

    // Create source frame and copy input data
    let mut src_frame = ffmpeg::frame::Video::new(pixel_format, src_width, src_height);
    copy_bgra_to_frame(input, &mut src_frame, src_width, src_height);

    // Create destination frame
    let mut dst_frame = ffmpeg::frame::Video::new(pixel_format, dst_width, dst_height);

    // Run scaling
    scaler
        .run(&src_frame, &mut dst_frame)
        .map_err(|e| Error::Scaling(format!("Scaling failed: {}", e)))?;

    // Extract output data
    copy_bgra_from_frame(&dst_frame, dst_width, dst_height)
}

/// Copy BGRA data to FFmpeg frame
fn copy_bgra_to_frame(input: &[u8], frame: &mut ffmpeg::frame::Video, width: u32, height: u32) {
    let stride = (width * 4) as usize;
    let frame_stride = frame.stride(0);
    let plane = frame.data_mut(0);

    for y in 0..height as usize {
        let src_start = y * stride;
        let dst_start = y * frame_stride;
        let copy_len = stride.min(frame_stride);

        if src_start + copy_len <= input.len() && dst_start + copy_len <= plane.len() {
            plane[dst_start..dst_start + copy_len]
                .copy_from_slice(&input[src_start..src_start + copy_len]);
        }
    }
}

/// Copy BGRA data from FFmpeg frame
fn copy_bgra_from_frame(
    frame: &ffmpeg::frame::Video,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let stride = (width * 4) as usize;
    let mut output = vec![0u8; stride * height as usize];
    let plane = frame.data(0);
    let frame_stride = frame.stride(0);

    for y in 0..height as usize {
        let src_start = y * frame_stride;
        let dst_start = y * stride;

        if src_start + stride <= plane.len() && dst_start + stride <= output.len() {
            output[dst_start..dst_start + stride]
                .copy_from_slice(&plane[src_start..src_start + stride]);
        }
    }

    Ok(output)
}

/// Scale NV12 frame data
pub fn scale_nv12(
    input: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>> {
    if src_width == dst_width && src_height == dst_height {
        return Ok(input.to_vec());
    }

    let _ = ffmpeg::init();

    let pixel_format = Pixel::NV12;

    // Create scaling context
    let mut scaler = SwsContext::get(
        pixel_format,
        src_width,
        src_height,
        pixel_format,
        dst_width,
        dst_height,
        SwsFlags::BILINEAR,
    )
    .map_err(|e| Error::Scaling(format!("Failed to create NV12 scaler: {}", e)))?;

    // Create source frame
    let mut src_frame = ffmpeg::frame::Video::new(pixel_format, src_width, src_height);
    copy_nv12_to_frame(input, &mut src_frame, src_width, src_height);

    // Create destination frame
    let mut dst_frame = ffmpeg::frame::Video::new(pixel_format, dst_width, dst_height);

    // Run scaling
    scaler
        .run(&src_frame, &mut dst_frame)
        .map_err(|e| Error::Scaling(format!("NV12 scaling failed: {}", e)))?;

    // Extract output
    copy_nv12_from_frame(&dst_frame, dst_width, dst_height)
}

/// Copy NV12 data to FFmpeg frame
fn copy_nv12_to_frame(input: &[u8], frame: &mut ffmpeg::frame::Video, width: u32, height: u32) {
    let y_size = (width * height) as usize;
    let y_stride = frame.stride(0);
    let uv_stride = frame.stride(1);

    // Copy Y plane
    {
        let y_plane = frame.data_mut(0);
        for y in 0..height as usize {
            let src_start = y * width as usize;
            let dst_start = y * y_stride;
            let copy_len = width as usize;
            if src_start + copy_len <= y_size && dst_start + copy_len <= y_plane.len() {
                y_plane[dst_start..dst_start + copy_len]
                    .copy_from_slice(&input[src_start..src_start + copy_len]);
            }
        }
    }

    // Copy UV plane
    {
        let uv_plane = frame.data_mut(1);
        let uv_height = height as usize / 2;
        for y in 0..uv_height {
            let src_start = y_size + y * width as usize;
            let dst_start = y * uv_stride;
            let copy_len = width as usize;
            if src_start + copy_len <= input.len() && dst_start + copy_len <= uv_plane.len() {
                uv_plane[dst_start..dst_start + copy_len]
                    .copy_from_slice(&input[src_start..src_start + copy_len]);
            }
        }
    }
}

/// Copy NV12 data from FFmpeg frame
fn copy_nv12_from_frame(
    frame: &ffmpeg::frame::Video,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let y_size = (width * height) as usize;
    let uv_size = y_size / 2;
    let mut output = vec![0u8; y_size + uv_size];

    // Copy Y plane
    let y_plane = frame.data(0);
    let y_stride = frame.stride(0);
    for y in 0..height as usize {
        let src_start = y * y_stride;
        let dst_start = y * width as usize;
        output[dst_start..dst_start + width as usize]
            .copy_from_slice(&y_plane[src_start..src_start + width as usize]);
    }

    // Copy UV plane
    let uv_plane = frame.data(1);
    let uv_stride = frame.stride(1);
    let uv_height = height as usize / 2;
    for y in 0..uv_height {
        let src_start = y * uv_stride;
        let dst_start = y_size + y * width as usize;
        output[dst_start..dst_start + width as usize]
            .copy_from_slice(&uv_plane[src_start..src_start + width as usize]);
    }

    Ok(output)
}
