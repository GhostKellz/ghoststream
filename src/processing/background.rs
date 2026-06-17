//! AI-powered background removal/blur
//!
//! Provides NVIDIA Broadcast-style background effects:
//! - Background blur (adjustable strength)
//! - Background replacement (virtual backgrounds)
//! - Background removal (transparent/green screen)
//!
//! Uses ONNX Runtime with CUDA/TensorRT acceleration for real-time performance.
//!
//! ## Supported Models
//!
//! - **MediaPipe Selfie Segmentation**: Lightweight, ~5ms on RTX GPU
//! - **RVM (Robust Video Matting)**: Higher quality, ~10ms on RTX GPU
//! - **MODNet**: Good balance of quality and speed
//!
//! ## Model Installation
//!
//! Download models to `~/.local/share/ghoststream/models/`:
//! ```bash
//! mkdir -p ~/.local/share/ghoststream/models
//! # MediaPipe Selfie Segmentation
//! wget -O ~/.local/share/ghoststream/models/selfie_segmentation.onnx \
//!   https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_segmenter/float16/latest/selfie_segmenter.tflite
//! ```

use crate::error::{Error, Result};
use crate::types::{Frame, FrameFormat};

use ort::{
    execution_providers::{CUDAExecutionProvider, TensorRTExecutionProvider},
    inputs,
    session::{builder::GraphOptimizationLevel, Session},
};

use std::path::{Path, PathBuf};

/// Background effect mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundMode {
    /// No effect (passthrough)
    None,
    /// Blur the background
    Blur { strength: u8 },
    /// Replace with solid color
    Color { r: u8, g: u8, b: u8 },
    /// Replace with image
    Image,
    /// Remove background (transparent)
    Remove,
    /// Green screen (chroma key compatible)
    GreenScreen,
}

impl Default for BackgroundMode {
    fn default() -> Self {
        Self::Blur { strength: 15 }
    }
}

/// Segmentation model type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentationModel {
    /// MediaPipe Selfie Segmentation (fastest)
    #[default]
    MediaPipe,
    /// Robust Video Matting (best quality)
    Rvm,
    /// MODNet (balanced)
    ModNet,
    /// Custom ONNX model
    Custom,
}

impl SegmentationModel {
    /// Get the default model filename
    pub fn default_filename(&self) -> &'static str {
        match self {
            Self::MediaPipe => "selfie_segmentation.onnx",
            Self::Rvm => "rvm_mobilenetv3.onnx",
            Self::ModNet => "modnet.onnx",
            Self::Custom => "custom_segmentation.onnx",
        }
    }

    /// Get expected input size (width, height)
    pub fn input_size(&self) -> (u32, u32) {
        match self {
            Self::MediaPipe => (256, 256),
            Self::Rvm => (512, 288),
            Self::ModNet => (512, 512),
            Self::Custom => (256, 256),
        }
    }
}

/// Background removal configuration
#[derive(Debug, Clone)]
pub struct BackgroundConfig {
    /// Effect mode
    pub mode: BackgroundMode,
    /// Model to use
    pub model: SegmentationModel,
    /// Model file path (None = auto-detect)
    pub model_path: Option<PathBuf>,
    /// Use GPU acceleration
    pub use_gpu: bool,
    /// Mask smoothing (0.0-1.0)
    pub smoothing: f32,
    /// Edge feathering (pixels)
    pub feather: u8,
    /// Threshold for person detection (0.0-1.0)
    pub threshold: f32,
    /// Background image path (for Image mode)
    pub background_image: Option<PathBuf>,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            mode: BackgroundMode::Blur { strength: 15 },
            model: SegmentationModel::MediaPipe,
            model_path: None,
            use_gpu: true,
            smoothing: 0.5,
            feather: 3,
            threshold: 0.5,
            background_image: None,
        }
    }
}

impl BackgroundConfig {
    pub fn blur(strength: u8) -> Self {
        Self {
            mode: BackgroundMode::Blur { strength },
            ..Default::default()
        }
    }

    pub fn green_screen() -> Self {
        Self {
            mode: BackgroundMode::GreenScreen,
            ..Default::default()
        }
    }

    pub fn remove() -> Self {
        Self {
            mode: BackgroundMode::Remove,
            ..Default::default()
        }
    }

    pub fn with_model(mut self, model: SegmentationModel) -> Self {
        self.model = model;
        self
    }

    pub fn with_gpu(mut self, use_gpu: bool) -> Self {
        self.use_gpu = use_gpu;
        self
    }
}

/// AI-powered background processor
pub struct BackgroundProcessor {
    config: BackgroundConfig,
    session: Option<Session>,
    /// Cached mask from previous frame (for temporal smoothing)
    prev_mask: Option<Vec<f32>>,
    /// Input dimensions the model expects
    input_width: u32,
    input_height: u32,
    /// Background image data (RGBA)
    background_data: Option<Vec<u8>>,
    /// Frame counter for stats
    frames_processed: u64,
}

impl BackgroundProcessor {
    /// Create a new background processor
    pub fn new(config: BackgroundConfig) -> Result<Self> {
        let (input_width, input_height) = config.model.input_size();

        let mut processor = Self {
            config,
            session: None,
            prev_mask: None,
            input_width,
            input_height,
            background_data: None,
            frames_processed: 0,
        };

        // Load model
        processor.load_model()?;

        // Load background image if specified
        if let Some(path) = processor.config.background_image.clone() {
            processor.load_background_image(&path)?;
        }

        Ok(processor)
    }

    /// Load the ONNX model
    fn load_model(&mut self) -> Result<()> {
        let model_path = self.find_model_path()?;

        tracing::info!("Loading segmentation model: {:?}", model_path);

        // Build session with GPU acceleration if available
        let mut builder = Session::builder()
            .map_err(|e| Error::Internal(format!("Failed to create session builder: {}", e)))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| Error::Internal(format!("Failed to set optimization level: {}", e)))?;

        let session = if self.config.use_gpu {
            // Try TensorRT first (fastest), then CUDA, then CPU
            match builder
                .with_execution_providers([
                    TensorRTExecutionProvider::default()
                        .with_fp16(true)
                        .build(),
                    CUDAExecutionProvider::default().build(),
                ]) {
                Ok(mut gpu_builder) => {
                    gpu_builder
                        .commit_from_file(&model_path)
                        .map_err(|e| Error::Internal(format!("Failed to load model: {}", e)))?
                }
                Err(e) => {
                    tracing::warn!("GPU execution providers failed: {}, falling back to CPU", e);
                    // Create fresh builder for CPU fallback
                    Session::builder()
                        .map_err(|e| Error::Internal(format!("Failed to create session builder: {}", e)))?
                        .with_optimization_level(GraphOptimizationLevel::Level3)
                        .map_err(|e| Error::Internal(format!("Failed to set optimization level: {}", e)))?
                        .commit_from_file(&model_path)
                        .map_err(|e| Error::Internal(format!("Failed to load model: {}", e)))?
                }
            }
        } else {
            builder
                .commit_from_file(&model_path)
                .map_err(|e| Error::Internal(format!("Failed to load model: {}", e)))?
        };

        tracing::info!(
            "Model loaded successfully (GPU: {})",
            self.config.use_gpu
        );

        self.session = Some(session);
        Ok(())
    }

    /// Find the model file path
    fn find_model_path(&self) -> Result<PathBuf> {
        // Check explicit path first
        if let Some(ref path) = self.config.model_path {
            if path.exists() {
                return Ok(path.clone());
            }
            return Err(Error::Internal(format!(
                "Model not found: {:?}",
                path
            )));
        }

        // Check standard locations
        let filename = self.config.model.default_filename();
        let search_paths = [
            // User local
            dirs::data_local_dir()
                .map(|p| p.join("ghoststream/models").join(filename)),
            // System
            Some(PathBuf::from("/usr/share/ghoststream/models").join(filename)),
            // Current directory
            Some(PathBuf::from("models").join(filename)),
        ];

        for path in search_paths.into_iter().flatten() {
            if path.exists() {
                return Ok(path);
            }
        }

        Err(Error::Internal(format!(
            "Model '{}' not found. Download it to ~/.local/share/ghoststream/models/",
            filename
        )))
    }

    /// Load background image for replacement mode
    fn load_background_image(&mut self, path: &Path) -> Result<()> {
        let img = image::open(path)
            .map_err(|e| Error::Internal(format!("Failed to load background image: {}", e)))?
            .to_rgba8();

        self.background_data = Some(img.into_raw());
        tracing::info!("Loaded background image: {:?}", path);
        Ok(())
    }

    /// Process a frame with background effects
    pub fn process(&mut self, frame: &Frame) -> Result<Frame> {
        // Skip if no effect
        if self.config.mode == BackgroundMode::None {
            return Ok(Frame {
                data: frame.data.clone(),
                ..*frame
            });
        }

        // Convert frame to RGB for model input
        let rgb_data = self.frame_to_rgb(frame)?;

        // Resize to model input size
        let resized = self.resize_for_model(&rgb_data, frame.width, frame.height)?;

        // Run inference to get segmentation mask
        let mask = {
            let session = self.session.as_mut().ok_or_else(|| {
                Error::Internal("Model not loaded".into())
            })?;
            run_inference(session, &resized, self.input_width, self.input_height)?
        };

        // Resize mask back to frame size
        let full_mask = self.resize_mask(&mask, frame.width, frame.height)?;

        // Apply temporal smoothing
        let smoothed_mask = self.smooth_mask(&full_mask);

        // Apply effect based on mode
        let result = self.apply_effect(frame, &smoothed_mask)?;

        self.frames_processed += 1;

        Ok(Frame {
            data: result,
            ..*frame
        })
    }

    /// Convert frame to RGB format for model input
    fn frame_to_rgb(&self, frame: &Frame) -> Result<Vec<u8>> {
        match frame.format {
            FrameFormat::Rgb => Ok(frame.data.clone()),
            FrameFormat::Rgba => {
                // Strip alpha channel
                let mut rgb = Vec::with_capacity(frame.data.len() * 3 / 4);
                for chunk in frame.data.chunks(4) {
                    rgb.extend_from_slice(&chunk[..3]);
                }
                Ok(rgb)
            }
            FrameFormat::Bgra => {
                // Convert BGRA to RGB
                let mut rgb = Vec::with_capacity(frame.data.len() * 3 / 4);
                for chunk in frame.data.chunks(4) {
                    rgb.push(chunk[2]); // R
                    rgb.push(chunk[1]); // G
                    rgb.push(chunk[0]); // B
                }
                Ok(rgb)
            }
            FrameFormat::Bgr => {
                // Convert BGR to RGB
                let mut rgb = Vec::with_capacity(frame.data.len());
                for chunk in frame.data.chunks(3) {
                    rgb.push(chunk[2]); // R
                    rgb.push(chunk[1]); // G
                    rgb.push(chunk[0]); // B
                }
                Ok(rgb)
            }
            _ => Err(Error::Internal(format!(
                "Unsupported frame format for background removal: {:?}",
                frame.format
            ))),
        }
    }

    /// Resize image for model input
    fn resize_for_model(&self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<f32>> {
        use image::{ImageBuffer, Rgb};

        let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_raw(width, height, rgb.to_vec())
            .ok_or_else(|| Error::Internal("Invalid image dimensions".into()))?;

        let resized = image::imageops::resize(
            &img,
            self.input_width,
            self.input_height,
            image::imageops::FilterType::Triangle,
        );

        // Normalize to [0, 1] and convert to NCHW format
        let mut tensor = vec![0.0f32; (self.input_width * self.input_height * 3) as usize];
        let hw = (self.input_width * self.input_height) as usize;

        for (i, pixel) in resized.pixels().enumerate() {
            tensor[i] = pixel[0] as f32 / 255.0;           // R channel
            tensor[hw + i] = pixel[1] as f32 / 255.0;      // G channel
            tensor[hw * 2 + i] = pixel[2] as f32 / 255.0;  // B channel
        }

        Ok(tensor)
    }

    /// Resize mask to frame dimensions
    fn resize_mask(&self, mask: &[f32], width: u32, height: u32) -> Result<Vec<f32>> {
        use image::{ImageBuffer, Luma};

        // Convert mask to grayscale image
        let mask_u8: Vec<u8> = mask.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect();

        let mask_img: ImageBuffer<Luma<u8>, _> =
            ImageBuffer::from_raw(self.input_width, self.input_height, mask_u8)
                .ok_or_else(|| Error::Internal("Invalid mask dimensions".into()))?;

        let resized = image::imageops::resize(
            &mask_img,
            width,
            height,
            image::imageops::FilterType::Triangle,
        );

        Ok(resized.pixels().map(|p| p[0] as f32 / 255.0).collect())
    }

    /// Apply temporal smoothing to mask
    fn smooth_mask(&mut self, mask: &[f32]) -> Vec<f32> {
        let smoothing = self.config.smoothing;

        if smoothing <= 0.0 || self.prev_mask.is_none() {
            self.prev_mask = Some(mask.to_vec());
            return mask.to_vec();
        }

        let prev = self.prev_mask.as_ref().unwrap();
        if prev.len() != mask.len() {
            self.prev_mask = Some(mask.to_vec());
            return mask.to_vec();
        }

        // Exponential moving average
        let smoothed: Vec<f32> = mask
            .iter()
            .zip(prev.iter())
            .map(|(&curr, &prev)| prev * smoothing + curr * (1.0 - smoothing))
            .collect();

        self.prev_mask = Some(smoothed.clone());
        smoothed
    }

    /// Apply the configured effect using the mask
    fn apply_effect(&self, frame: &Frame, mask: &[f32]) -> Result<Vec<u8>> {
        match self.config.mode {
            BackgroundMode::None => Ok(frame.data.clone()),
            BackgroundMode::Blur { strength } => self.apply_blur(frame, mask, strength),
            BackgroundMode::Color { r, g, b } => self.apply_color(frame, mask, r, g, b),
            BackgroundMode::Image => self.apply_image(frame, mask),
            BackgroundMode::Remove => self.apply_remove(frame, mask),
            BackgroundMode::GreenScreen => self.apply_green_screen(frame, mask),
        }
    }

    /// Apply background blur
    fn apply_blur(&self, frame: &Frame, mask: &[f32], strength: u8) -> Result<Vec<u8>> {
        // Create blurred version of frame
        let blurred = self.blur_frame(frame, strength)?;

        // Blend based on mask
        self.blend_frames(&frame.data, &blurred, mask, frame.format)
    }

    /// Blur a frame using box blur
    fn blur_frame(&self, frame: &Frame, strength: u8) -> Result<Vec<u8>> {
        use image::{ImageBuffer, Rgba};

        let bytes_per_pixel = match frame.format {
            FrameFormat::Rgba | FrameFormat::Bgra => 4,
            FrameFormat::Rgb | FrameFormat::Bgr => 3,
            _ => return Err(Error::Internal("Unsupported format for blur".into())),
        };

        if bytes_per_pixel == 4 {
            let img: ImageBuffer<Rgba<u8>, _> =
                ImageBuffer::from_raw(frame.width, frame.height, frame.data.clone())
                    .ok_or_else(|| Error::Internal("Invalid frame dimensions".into()))?;

            let blurred = image::imageops::blur(&img, strength as f32);
            Ok(blurred.into_raw())
        } else {
            use image::Rgb;
            let img: ImageBuffer<Rgb<u8>, _> =
                ImageBuffer::from_raw(frame.width, frame.height, frame.data.clone())
                    .ok_or_else(|| Error::Internal("Invalid frame dimensions".into()))?;

            let blurred = image::imageops::blur(&img, strength as f32);
            Ok(blurred.into_raw())
        }
    }

    /// Blend two frames based on mask
    fn blend_frames(
        &self,
        fg: &[u8],
        bg: &[u8],
        mask: &[f32],
        format: FrameFormat,
    ) -> Result<Vec<u8>> {
        let bytes_per_pixel = match format {
            FrameFormat::Rgba | FrameFormat::Bgra => 4,
            FrameFormat::Rgb | FrameFormat::Bgr => 3,
            _ => return Err(Error::Internal("Unsupported format for blend".into())),
        };

        let mut result = vec![0u8; fg.len()];
        let threshold = self.config.threshold;

        for (i, &alpha) in mask.iter().enumerate() {
            let alpha = if alpha > threshold { alpha } else { 0.0 };
            let start = i * bytes_per_pixel;
            let end = start + bytes_per_pixel;

            if start >= fg.len() || end > fg.len() {
                break;
            }

            for j in 0..bytes_per_pixel {
                let fg_val = fg[start + j] as f32;
                let bg_val = bg[start + j] as f32;
                result[start + j] = (fg_val * alpha + bg_val * (1.0 - alpha)) as u8;
            }
        }

        Ok(result)
    }

    /// Replace background with solid color
    fn apply_color(&self, frame: &Frame, mask: &[f32], r: u8, g: u8, b: u8) -> Result<Vec<u8>> {
        let bytes_per_pixel = match frame.format {
            FrameFormat::Rgba | FrameFormat::Bgra => 4,
            FrameFormat::Rgb | FrameFormat::Bgr => 3,
            _ => return Err(Error::Internal("Unsupported format".into())),
        };

        let mut result = frame.data.clone();
        let threshold = self.config.threshold;

        // Determine color order based on format
        let (c0, c1, c2) = match frame.format {
            FrameFormat::Bgra | FrameFormat::Bgr => (b, g, r),
            _ => (r, g, b),
        };

        for (i, &alpha) in mask.iter().enumerate() {
            let start = i * bytes_per_pixel;
            if start + 2 >= result.len() {
                break;
            }

            if alpha < threshold {
                result[start] = c0;
                result[start + 1] = c1;
                result[start + 2] = c2;
                if bytes_per_pixel == 4 {
                    result[start + 3] = 255;
                }
            }
        }

        Ok(result)
    }

    /// Replace background with image
    fn apply_image(&self, frame: &Frame, mask: &[f32]) -> Result<Vec<u8>> {
        let bg = self.background_data.as_ref().ok_or_else(|| {
            Error::Internal("No background image loaded".into())
        })?;

        // Resize background to frame size if needed
        // For now, assume same size or tile/stretch
        self.blend_frames(&frame.data, bg, mask, frame.format)
    }

    /// Remove background (make transparent)
    fn apply_remove(&self, frame: &Frame, mask: &[f32]) -> Result<Vec<u8>> {
        // Ensure RGBA output
        let mut result = match frame.format {
            FrameFormat::Rgba => frame.data.clone(),
            FrameFormat::Bgra => {
                // Convert BGRA to RGBA
                let mut rgba = frame.data.clone();
                for chunk in rgba.chunks_mut(4) {
                    chunk.swap(0, 2);
                }
                rgba
            }
            FrameFormat::Rgb => {
                // Add alpha channel
                let mut rgba = Vec::with_capacity(frame.data.len() * 4 / 3);
                for chunk in frame.data.chunks(3) {
                    rgba.extend_from_slice(chunk);
                    rgba.push(255);
                }
                rgba
            }
            _ => return Err(Error::Internal("Unsupported format for remove".into())),
        };

        let threshold = self.config.threshold;

        // Set alpha based on mask
        for (i, &alpha) in mask.iter().enumerate() {
            let alpha_idx = i * 4 + 3;
            if alpha_idx < result.len() {
                result[alpha_idx] = if alpha > threshold {
                    255
                } else {
                    0
                };
            }
        }

        Ok(result)
    }

    /// Apply green screen effect
    fn apply_green_screen(&self, frame: &Frame, mask: &[f32]) -> Result<Vec<u8>> {
        // Green screen: (0, 177, 64) - chroma key green
        self.apply_color(frame, mask, 0, 177, 64)
    }

    /// Get number of frames processed
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed
    }

    /// Check if GPU is being used
    pub fn is_gpu_enabled(&self) -> bool {
        self.config.use_gpu && self.session.is_some()
    }

    /// Update configuration
    pub fn set_mode(&mut self, mode: BackgroundMode) {
        self.config.mode = mode;
    }

    /// Set blur strength (for Blur mode)
    pub fn set_blur_strength(&mut self, strength: u8) {
        if let BackgroundMode::Blur { .. } = self.config.mode {
            self.config.mode = BackgroundMode::Blur { strength };
        }
    }

    /// Set mask smoothing factor
    pub fn set_smoothing(&mut self, smoothing: f32) {
        self.config.smoothing = smoothing.clamp(0.0, 1.0);
    }

    /// Set detection threshold
    pub fn set_threshold(&mut self, threshold: f32) {
        self.config.threshold = threshold.clamp(0.0, 1.0);
    }
}

/// Run inference to get segmentation mask (standalone to avoid borrow issues)
fn run_inference(
    session: &mut Session,
    input: &[f32],
    input_width: u32,
    input_height: u32,
) -> Result<Vec<f32>> {
    use ort::value::Tensor;

    let input_shape = [1usize, 3, input_height as usize, input_width as usize];

    // Create tensor from input data
    let input_tensor = Tensor::from_array((input_shape, input.to_vec()))
        .map_err(|e| Error::Internal(format!("Failed to create input tensor: {}", e)))?;

    let outputs = session
        .run(inputs!["input" => input_tensor])
        .map_err(|e| Error::Internal(format!("Inference failed: {}", e)))?;

    // Extract mask from output - get first output
    let first = outputs
        .iter()
        .next()
        .ok_or_else(|| Error::Internal("No output from model".into()))?;

    let (_shape, data) = first
        .1
        .try_extract_tensor::<f32>()
        .map_err(|e| Error::Internal(format!("Failed to extract output: {}", e)))?;

    Ok(data.to_vec())
}
