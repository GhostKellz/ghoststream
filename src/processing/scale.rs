//! Frame scaling

use crate::error::{Error, Result};

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

/// Frame scaler
pub struct Scaler {
    _algorithm: ScaleAlgorithm,
    // FFmpeg SwsContext would go here
}

impl Scaler {
    pub fn new(algorithm: ScaleAlgorithm) -> Self {
        Self {
            _algorithm: algorithm,
        }
    }

    /// Scale a frame
    pub fn scale(
        &self,
        input: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
    ) -> Result<Vec<u8>> {
        scale_frame(input, src_width, src_height, dst_width, dst_height)
    }
}

impl Default for Scaler {
    fn default() -> Self {
        Self::new(ScaleAlgorithm::Bilinear)
    }
}

/// Scale frame data
pub fn scale_frame(
    input: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>> {
    if src_width == dst_width && src_height == dst_height {
        return Ok(input.to_vec());
    }

    // TODO: Use FFmpeg swscale for proper scaling
    //
    // let mut sws = ffmpeg::software::scaling::Context::get(
    //     Pixel::BGRA,
    //     src_width,
    //     src_height,
    //     Pixel::BGRA,
    //     dst_width,
    //     dst_height,
    //     Flags::BILINEAR,
    // )?;
    //
    // let src_frame = ...; // Wrap input
    // let mut dst_frame = ffmpeg::frame::Video::new(Pixel::BGRA, dst_width, dst_height);
    // sws.run(&src_frame, &mut dst_frame)?;
    //
    // return Ok(dst_frame.data(0).to_vec());

    // Placeholder: simple nearest-neighbor scaling
    let src_w = src_width as usize;
    let src_h = src_height as usize;
    let dst_w = dst_width as usize;
    let dst_h = dst_height as usize;
    let bpp = 4; // BGRA

    if input.len() < src_w * src_h * bpp {
        return Err(Error::Scaling("Input buffer too small".into()));
    }

    let mut output = vec![0u8; dst_w * dst_h * bpp];

    for y in 0..dst_h {
        for x in 0..dst_w {
            let src_x = x * src_w / dst_w;
            let src_y = y * src_h / dst_h;

            let src_idx = (src_y * src_w + src_x) * bpp;
            let dst_idx = (y * dst_w + x) * bpp;

            output[dst_idx..dst_idx + bpp].copy_from_slice(&input[src_idx..src_idx + bpp]);
        }
    }

    Ok(output)
}
