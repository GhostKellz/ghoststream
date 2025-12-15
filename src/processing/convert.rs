//! Colorspace conversion using FFmpeg swscale

use crate::error::{Error, Result};
use crate::types::FrameFormat;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as SwsContext, Flags as SwsFlags};

/// Map FrameFormat to FFmpeg Pixel format
fn format_to_pixel(format: FrameFormat) -> Option<Pixel> {
    match format {
        FrameFormat::Bgra => Some(Pixel::BGRA),
        FrameFormat::Rgba => Some(Pixel::RGBA),
        FrameFormat::Nv12 => Some(Pixel::NV12),
        FrameFormat::P010 => Some(Pixel::P010LE),
        FrameFormat::Yuv420p => Some(Pixel::YUV420P),
        FrameFormat::Yuv444p => Some(Pixel::YUV444P),
        FrameFormat::Rgb24 => Some(Pixel::RGB24),
    }
}

/// Colorspace converter using FFmpeg swscale
pub struct ColorspaceConverter {
    // Cached scaler context (could be extended to cache multiple contexts)
}

impl ColorspaceConverter {
    pub fn new() -> Self {
        // Initialize FFmpeg once
        let _ = ffmpeg::init();
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

/// Convert frame colorspace using FFmpeg swscale
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

    // Simple swaps don't need swscale
    match (src_format, dst_format) {
        (FrameFormat::Bgra, FrameFormat::Rgba) | (FrameFormat::Rgba, FrameFormat::Bgra) => {
            return bgra_rgba_swap(input);
        }
        // P010 conversions use specialized HDR code
        (FrameFormat::Bgra, FrameFormat::P010) => {
            return super::hdr::bgra_to_p010(input, width as usize, height as usize);
        }
        (FrameFormat::Rgba, FrameFormat::P010) => {
            let bgra = bgra_rgba_swap(input)?;
            return super::hdr::bgra_to_p010(&bgra, width as usize, height as usize);
        }
        (FrameFormat::Nv12, FrameFormat::P010) => {
            return super::hdr::nv12_to_p010(input, width as usize, height as usize);
        }
        _ => {}
    }

    // Use FFmpeg swscale for other conversions
    let src_pixel = format_to_pixel(src_format).ok_or_else(|| {
        Error::ColorspaceConversion(format!("Unsupported source format: {:?}", src_format))
    })?;

    let dst_pixel = format_to_pixel(dst_format).ok_or_else(|| {
        Error::ColorspaceConversion(format!("Unsupported destination format: {:?}", dst_format))
    })?;

    convert_with_swscale(input, src_pixel, dst_pixel, width, height)
}

/// Convert using FFmpeg swscale
fn convert_with_swscale(
    input: &[u8],
    src_pixel: Pixel,
    dst_pixel: Pixel,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let _ = ffmpeg::init();

    // Create scaling context (also handles colorspace conversion)
    let mut scaler = SwsContext::get(
        src_pixel,
        width,
        height,
        dst_pixel,
        width,
        height,
        SwsFlags::BILINEAR,
    )
    .map_err(|e| Error::ColorspaceConversion(format!("Failed to create scaler: {}", e)))?;

    // Create source frame
    let mut src_frame = ffmpeg::frame::Video::new(src_pixel, width, height);

    // Copy input data to source frame
    copy_to_frame(input, &mut src_frame, src_pixel, width, height)?;

    // Create destination frame
    let mut dst_frame = ffmpeg::frame::Video::new(dst_pixel, width, height);

    // Run conversion
    scaler
        .run(&src_frame, &mut dst_frame)
        .map_err(|e| Error::ColorspaceConversion(format!("Conversion failed: {}", e)))?;

    // Extract output data
    copy_from_frame(&dst_frame, dst_pixel, width, height)
}

/// Copy input buffer to FFmpeg frame
fn copy_to_frame(
    input: &[u8],
    frame: &mut ffmpeg::frame::Video,
    pixel: Pixel,
    width: u32,
    height: u32,
) -> Result<()> {
    match pixel {
        Pixel::BGRA | Pixel::RGBA => {
            // Packed 4-byte format
            let stride = (width * 4) as usize;
            let frame_stride = frame.stride(0);
            let plane = frame.data_mut(0);

            for y in 0..height as usize {
                let src_start = y * stride;
                let src_end = src_start + stride;
                let dst_start = y * frame_stride;

                if src_end <= input.len() && dst_start + stride <= plane.len() {
                    plane[dst_start..dst_start + stride]
                        .copy_from_slice(&input[src_start..src_end]);
                }
            }
        }
        Pixel::NV12 => {
            // Y plane + interleaved UV plane
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

                    if src_start + copy_len <= input.len() && src_start + copy_len <= y_size {
                        y_plane[dst_start..dst_start + copy_len]
                            .copy_from_slice(&input[src_start..src_start + copy_len]);
                    }
                }
            }

            // Copy UV plane
            {
                let uv_plane = frame.data_mut(1);
                let uv_height = height as usize / 2;
                let uv_width = width as usize;

                for y in 0..uv_height {
                    let src_start = y_size + y * uv_width;
                    let dst_start = y * uv_stride;
                    let copy_len = uv_width;

                    if src_start + copy_len <= input.len() {
                        uv_plane[dst_start..dst_start + copy_len]
                            .copy_from_slice(&input[src_start..src_start + copy_len]);
                    }
                }
            }
        }
        Pixel::YUV420P => {
            // Three separate planes
            let y_size = (width * height) as usize;
            let uv_size = y_size / 4;
            let half_height = height as usize / 2;
            let half_width = width as usize / 2;

            let y_stride = frame.stride(0);
            let u_stride = frame.stride(1);
            let v_stride = frame.stride(2);

            // Y plane
            {
                let y_plane = frame.data_mut(0);
                for y in 0..height as usize {
                    let src_start = y * width as usize;
                    let dst_start = y * y_stride;
                    let copy_len = width as usize;
                    if src_start + copy_len <= y_size {
                        y_plane[dst_start..dst_start + copy_len]
                            .copy_from_slice(&input[src_start..src_start + copy_len]);
                    }
                }
            }

            // U plane
            {
                let u_plane = frame.data_mut(1);
                for y in 0..half_height {
                    let src_start = y_size + y * half_width;
                    let dst_start = y * u_stride;
                    if src_start + half_width <= input.len() {
                        u_plane[dst_start..dst_start + half_width]
                            .copy_from_slice(&input[src_start..src_start + half_width]);
                    }
                }
            }

            // V plane
            {
                let v_plane = frame.data_mut(2);
                for y in 0..half_height {
                    let src_start = y_size + uv_size + y * half_width;
                    let dst_start = y * v_stride;
                    if src_start + half_width <= input.len() {
                        v_plane[dst_start..dst_start + half_width]
                            .copy_from_slice(&input[src_start..src_start + half_width]);
                    }
                }
            }
        }
        _ => {
            // Fallback: copy directly to first plane
            let plane = frame.data_mut(0);
            let copy_len = input.len().min(plane.len());
            plane[..copy_len].copy_from_slice(&input[..copy_len]);
        }
    }

    Ok(())
}

/// Copy data from FFmpeg frame to output buffer
fn copy_from_frame(
    frame: &ffmpeg::frame::Video,
    pixel: Pixel,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    match pixel {
        Pixel::BGRA | Pixel::RGBA => {
            // Packed 4-byte format
            let stride = (width * 4) as usize;
            let mut output = vec![0u8; stride * height as usize];
            let plane = frame.data(0);
            let frame_stride = frame.stride(0);

            for y in 0..height as usize {
                let src_start = y * frame_stride;
                let dst_start = y * stride;
                output[dst_start..dst_start + stride]
                    .copy_from_slice(&plane[src_start..src_start + stride]);
            }

            Ok(output)
        }
        Pixel::NV12 => {
            // Y plane + interleaved UV plane
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
        Pixel::YUV420P => {
            // Three separate planes
            let y_size = (width * height) as usize;
            let uv_size = y_size / 4;
            let mut output = vec![0u8; y_size + uv_size * 2];

            // Y plane
            let y_plane = frame.data(0);
            let y_stride = frame.stride(0);
            for y in 0..height as usize {
                let src_start = y * y_stride;
                let dst_start = y * width as usize;
                output[dst_start..dst_start + width as usize]
                    .copy_from_slice(&y_plane[src_start..src_start + width as usize]);
            }

            // U plane
            let u_plane = frame.data(1);
            let u_stride = frame.stride(1);
            let half_height = height as usize / 2;
            let half_width = width as usize / 2;
            for y in 0..half_height {
                let src_start = y * u_stride;
                let dst_start = y_size + y * half_width;
                output[dst_start..dst_start + half_width]
                    .copy_from_slice(&u_plane[src_start..src_start + half_width]);
            }

            // V plane
            let v_plane = frame.data(2);
            let v_stride = frame.stride(2);
            for y in 0..half_height {
                let src_start = y * v_stride;
                let dst_start = y_size + uv_size + y * half_width;
                output[dst_start..dst_start + half_width]
                    .copy_from_slice(&v_plane[src_start..src_start + half_width]);
            }

            Ok(output)
        }
        _ => {
            // Fallback: copy from first plane
            let plane = frame.data(0);
            Ok(plane.to_vec())
        }
    }
}

/// BGRA <-> RGBA swap (just swap R and B channels)
fn bgra_rgba_swap(input: &[u8]) -> Result<Vec<u8>> {
    let mut output = input.to_vec();
    for chunk in output.chunks_exact_mut(4) {
        chunk.swap(0, 2); // Swap B and R
    }
    Ok(output)
}
