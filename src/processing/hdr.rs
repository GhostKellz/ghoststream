//! HDR (High Dynamic Range) support
//!
//! Provides:
//! - HDR10 static metadata
//! - Colorspace handling (BT.2020, BT.709)
//! - P010 (10-bit NV12) format conversion
//! - Optional HDR to SDR tonemapping

use crate::error::{Error, Result};
use crate::types::FrameFormat;

/// HDR transfer function
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransferFunction {
    /// SDR (BT.709 gamma)
    #[default]
    Sdr,
    /// PQ (Perceptual Quantizer) - HDR10, Dolby Vision
    Pq,
    /// HLG (Hybrid Log-Gamma) - BBC/NHK
    Hlg,
}

impl TransferFunction {
    /// FFmpeg color_trc value
    pub fn ffmpeg_trc(&self) -> i32 {
        match self {
            TransferFunction::Sdr => 1,   // AVCOL_TRC_BT709
            TransferFunction::Pq => 16,   // AVCOL_TRC_SMPTE2084
            TransferFunction::Hlg => 18,  // AVCOL_TRC_ARIB_STD_B67
        }
    }
}

/// Color primaries (color gamut)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorPrimaries {
    /// BT.709 (SDR, HD)
    #[default]
    Bt709,
    /// BT.2020 (HDR, UHD)
    Bt2020,
    /// DCI-P3 (Cinema)
    DciP3,
}

impl ColorPrimaries {
    /// FFmpeg color_primaries value
    pub fn ffmpeg_primaries(&self) -> i32 {
        match self {
            ColorPrimaries::Bt709 => 1,   // AVCOL_PRI_BT709
            ColorPrimaries::Bt2020 => 9,  // AVCOL_PRI_BT2020
            ColorPrimaries::DciP3 => 11,  // AVCOL_PRI_SMPTE432 (DCI-P3)
        }
    }
}

/// Color matrix coefficients
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMatrix {
    /// BT.709
    #[default]
    Bt709,
    /// BT.2020 NCL (Non-Constant Luminance)
    Bt2020Ncl,
    /// BT.2020 CL (Constant Luminance)
    Bt2020Cl,
}

impl ColorMatrix {
    /// FFmpeg colorspace value
    pub fn ffmpeg_colorspace(&self) -> i32 {
        match self {
            ColorMatrix::Bt709 => 1,      // AVCOL_SPC_BT709
            ColorMatrix::Bt2020Ncl => 9,  // AVCOL_SPC_BT2020_NCL
            ColorMatrix::Bt2020Cl => 10,  // AVCOL_SPC_BT2020_CL
        }
    }
}

/// HDR10 static metadata (SMPTE ST 2086)
#[derive(Debug, Clone, Default)]
pub struct Hdr10Metadata {
    /// Red primary X (0.0-1.0)
    pub red_primary_x: f32,
    /// Red primary Y (0.0-1.0)
    pub red_primary_y: f32,
    /// Green primary X (0.0-1.0)
    pub green_primary_x: f32,
    /// Green primary Y (0.0-1.0)
    pub green_primary_y: f32,
    /// Blue primary X (0.0-1.0)
    pub blue_primary_x: f32,
    /// Blue primary Y (0.0-1.0)
    pub blue_primary_y: f32,
    /// White point X (0.0-1.0)
    pub white_point_x: f32,
    /// White point Y (0.0-1.0)
    pub white_point_y: f32,
    /// Max luminance in nits (cd/m²)
    pub max_luminance: f32,
    /// Min luminance in nits (cd/m²)
    pub min_luminance: f32,
}

impl Hdr10Metadata {
    /// Create metadata for standard BT.2020 display
    pub fn bt2020_default() -> Self {
        Self {
            // BT.2020 primaries
            red_primary_x: 0.708,
            red_primary_y: 0.292,
            green_primary_x: 0.170,
            green_primary_y: 0.797,
            blue_primary_x: 0.131,
            blue_primary_y: 0.046,
            // D65 white point
            white_point_x: 0.3127,
            white_point_y: 0.3290,
            // Typical HDR display
            max_luminance: 1000.0,
            min_luminance: 0.001,
        }
    }

    /// Create metadata for specific max luminance
    pub fn with_max_luminance(max_nits: f32) -> Self {
        let mut meta = Self::bt2020_default();
        meta.max_luminance = max_nits;
        meta
    }
}

/// Content Light Level Info (MaxCLL, MaxFALL)
#[derive(Debug, Clone, Default)]
pub struct ContentLightLevel {
    /// Maximum Content Light Level (nits)
    pub max_cll: u16,
    /// Maximum Frame Average Light Level (nits)
    pub max_fall: u16,
}

impl ContentLightLevel {
    pub fn new(max_cll: u16, max_fall: u16) -> Self {
        Self { max_cll, max_fall }
    }

    /// Typical values for HDR content
    pub fn default_hdr() -> Self {
        Self {
            max_cll: 1000,
            max_fall: 400,
        }
    }
}

/// Complete HDR configuration
#[derive(Debug, Clone, Default)]
pub struct HdrConfig {
    /// Transfer function
    pub transfer: TransferFunction,
    /// Color primaries
    pub primaries: ColorPrimaries,
    /// Color matrix
    pub matrix: ColorMatrix,
    /// HDR10 mastering display metadata
    pub hdr10_metadata: Option<Hdr10Metadata>,
    /// Content light level info
    pub content_light: Option<ContentLightLevel>,
    /// Use 10-bit encoding
    pub bit_depth: u8,
}

impl HdrConfig {
    /// Create SDR configuration
    pub fn sdr() -> Self {
        Self {
            transfer: TransferFunction::Sdr,
            primaries: ColorPrimaries::Bt709,
            matrix: ColorMatrix::Bt709,
            hdr10_metadata: None,
            content_light: None,
            bit_depth: 8,
        }
    }

    /// Create HDR10 configuration
    pub fn hdr10() -> Self {
        Self {
            transfer: TransferFunction::Pq,
            primaries: ColorPrimaries::Bt2020,
            matrix: ColorMatrix::Bt2020Ncl,
            hdr10_metadata: Some(Hdr10Metadata::bt2020_default()),
            content_light: Some(ContentLightLevel::default_hdr()),
            bit_depth: 10,
        }
    }

    /// Create HDR10 with custom max luminance
    pub fn hdr10_with_luminance(max_nits: f32, max_cll: u16, max_fall: u16) -> Self {
        Self {
            transfer: TransferFunction::Pq,
            primaries: ColorPrimaries::Bt2020,
            matrix: ColorMatrix::Bt2020Ncl,
            hdr10_metadata: Some(Hdr10Metadata::with_max_luminance(max_nits)),
            content_light: Some(ContentLightLevel::new(max_cll, max_fall)),
            bit_depth: 10,
        }
    }

    /// Create HLG configuration
    pub fn hlg() -> Self {
        Self {
            transfer: TransferFunction::Hlg,
            primaries: ColorPrimaries::Bt2020,
            matrix: ColorMatrix::Bt2020Ncl,
            hdr10_metadata: None,
            content_light: None,
            bit_depth: 10,
        }
    }

    /// Is this an HDR configuration?
    pub fn is_hdr(&self) -> bool {
        self.transfer != TransferFunction::Sdr
    }

    /// Get the appropriate pixel format
    pub fn pixel_format(&self) -> FrameFormat {
        if self.bit_depth >= 10 {
            FrameFormat::P010
        } else {
            FrameFormat::Nv12
        }
    }
}

/// Convert BGRA to P010 (10-bit NV12)
/// P010 format: 16-bit little-endian, with 10 bits of data in the high bits
pub fn bgra_to_p010(input: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let y_size = width * height * 2; // 16-bit per sample
    let uv_size = (width / 2) * (height / 2) * 4; // 16-bit per U and V
    let mut output = vec![0u8; y_size + uv_size];

    // Y plane (16-bit little-endian, 10-bit data in high bits)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let b = input[idx] as f32;
            let g = input[idx + 1] as f32;
            let r = input[idx + 2] as f32;

            // BT.2020 RGB to Y (slightly different coefficients than BT.709)
            let y_val = (0.2627 * r + 0.6780 * g + 0.0593 * b) as u16;
            // Shift to 10-bit in 16-bit container (high 10 bits)
            let y_10bit = (y_val.min(255) as u16) << 6;

            let out_idx = (y * width + x) * 2;
            output[out_idx] = (y_10bit & 0xFF) as u8;
            output[out_idx + 1] = ((y_10bit >> 8) & 0xFF) as u8;
        }
    }

    // UV plane (interleaved, subsampled 2x2, 16-bit each)
    let uv_offset = y_size;
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let idx = (y * width + x) * 4;
            let b = input[idx] as f32;
            let g = input[idx + 1] as f32;
            let r = input[idx + 2] as f32;

            // BT.2020 RGB to U, V
            let u_val = ((-0.1396 * r - 0.3604 * g + 0.5 * b) + 128.0) as u16;
            let v_val = ((0.5 * r - 0.4598 * g - 0.0402 * b) + 128.0) as u16;

            // Shift to 10-bit in 16-bit container
            let u_10bit = (u_val.clamp(0, 255) as u16) << 6;
            let v_10bit = (v_val.clamp(0, 255) as u16) << 6;

            let uv_idx = uv_offset + (y / 2) * width * 2 + x * 2;
            output[uv_idx] = (u_10bit & 0xFF) as u8;
            output[uv_idx + 1] = ((u_10bit >> 8) & 0xFF) as u8;
            output[uv_idx + 2] = (v_10bit & 0xFF) as u8;
            output[uv_idx + 3] = ((v_10bit >> 8) & 0xFF) as u8;
        }
    }

    Ok(output)
}

/// Convert NV12 (8-bit) to P010 (10-bit)
pub fn nv12_to_p010(input: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let y_size_8bit = width * height;
    let uv_size_8bit = (width / 2) * (height / 2) * 2;

    if input.len() < y_size_8bit + uv_size_8bit {
        return Err(Error::ColorspaceConversion(
            "Input buffer too small for NV12".into()
        ));
    }

    let y_size_10bit = width * height * 2;
    let uv_size_10bit = (width / 2) * (height / 2) * 4;
    let mut output = vec![0u8; y_size_10bit + uv_size_10bit];

    // Convert Y plane (8-bit to 10-bit in 16-bit container)
    for i in 0..y_size_8bit {
        let y_10bit = (input[i] as u16) << 6;
        let out_idx = i * 2;
        output[out_idx] = (y_10bit & 0xFF) as u8;
        output[out_idx + 1] = ((y_10bit >> 8) & 0xFF) as u8;
    }

    // Convert UV plane (8-bit interleaved to 10-bit interleaved)
    let uv_in_offset = y_size_8bit;
    let uv_out_offset = y_size_10bit;
    for i in 0..uv_size_8bit {
        let uv_10bit = (input[uv_in_offset + i] as u16) << 6;
        let out_idx = uv_out_offset + i * 2;
        output[out_idx] = (uv_10bit & 0xFF) as u8;
        output[out_idx + 1] = ((uv_10bit >> 8) & 0xFF) as u8;
    }

    Ok(output)
}

/// Apply PQ (SMPTE ST 2084) transfer function
/// Converts linear light to PQ encoded value
pub fn linear_to_pq(linear: f32) -> f32 {
    const M1: f32 = 0.1593017578125; // 2610/16384
    const M2: f32 = 78.84375; // 2523/32 * 128
    const C1: f32 = 0.8359375; // 3424/4096
    const C2: f32 = 18.8515625; // 2413/128
    const C3: f32 = 18.6875; // 2392/128

    let y = (linear / 10000.0).max(0.0);
    let y_m1 = y.powf(M1);
    ((C1 + C2 * y_m1) / (1.0 + C3 * y_m1)).powf(M2)
}

/// Apply inverse PQ transfer function
/// Converts PQ encoded value to linear light
pub fn pq_to_linear(pq: f32) -> f32 {
    const M1: f32 = 0.1593017578125;
    const M2: f32 = 78.84375;
    const C1: f32 = 0.8359375;
    const C2: f32 = 18.8515625;
    const C3: f32 = 18.6875;

    let e_inv_m2 = pq.max(0.0).powf(1.0 / M2);
    let num = (e_inv_m2 - C1).max(0.0);
    let den = C2 - C3 * e_inv_m2;
    10000.0 * (num / den).powf(1.0 / M1)
}

/// Simple HDR to SDR tonemapping (Reinhard)
pub fn tonemap_reinhard(hdr_linear: f32, max_luminance: f32) -> f32 {
    let scaled = hdr_linear / max_luminance;
    scaled / (1.0 + scaled)
}

/// ACES filmic tonemapping
pub fn tonemap_aces(x: f32) -> f32 {
    const A: f32 = 2.51;
    const B: f32 = 0.03;
    const C: f32 = 2.43;
    const D: f32 = 0.59;
    const E: f32 = 0.14;

    ((x * (A * x + B)) / (x * (C * x + D) + E)).clamp(0.0, 1.0)
}
