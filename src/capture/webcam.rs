//! V4L2 Webcam capture module
//!
//! Provides capture from USB webcams (Elgato Facecam, Logitech, etc.)
//! using the Video4Linux2 (V4L2) API.

use crate::config::WebcamConfig;
use crate::error::{Error, Result};
use crate::types::{Frame, FrameFormat, Framerate, Resolution};

#[allow(unused_imports)]
use FrameFormat::*;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;
use v4l::video::Capture;
use v4l::FourCC;

/// Webcam capture source
pub struct WebcamCapture {
    config: WebcamConfig,
    device: Option<Device>,
    stream: Option<MmapStream<'static>>,
    active: Arc<AtomicBool>,
    resolution: Option<Resolution>,
    framerate: Option<Framerate>,
    format: FrameFormat,
    frame_count: u64,
}

impl WebcamCapture {
    /// Create a new webcam capture
    pub fn new(config: WebcamConfig) -> Result<Self> {
        Ok(Self {
            config,
            device: None,
            stream: None,
            active: Arc::new(AtomicBool::new(false)),
            resolution: None,
            framerate: None,
            format: FrameFormat::Yuyv,
            frame_count: 0,
        })
    }

    /// List available webcam devices
    pub fn list_devices() -> Result<Vec<WebcamDeviceInfo>> {
        let mut devices = Vec::new();

        // Scan /dev/video* devices
        for i in 0..16 {
            let path = format!("/dev/video{}", i);
            if Path::new(&path).exists() {
                if let Ok(device) = Device::new(i) {
                    // Check if it's a capture device
                    if let Ok(caps) = device.query_caps() {
                        if caps.capabilities.contains(v4l::capability::Flags::VIDEO_CAPTURE) {
                            let name = caps.card.clone();
                            let driver = caps.driver.clone();

                            // Get supported formats
                            let formats = device
                                .enum_formats()
                                .map(|fmts| {
                                    fmts.into_iter()
                                        .map(|f| f.fourcc.to_string())
                                        .collect()
                                })
                                .unwrap_or_default();

                            devices.push(WebcamDeviceInfo {
                                path: path.clone(),
                                name,
                                driver,
                                formats,
                            });
                        }
                    }
                }
            }
        }

        Ok(devices)
    }

    /// Auto-detect the best webcam device
    fn auto_detect_device() -> Result<String> {
        let devices = Self::list_devices()?;

        // Prefer known good webcams
        let preferred = ["Elgato", "Facecam", "Logitech", "C920", "C922", "BRIO"];

        for pref in &preferred {
            if let Some(dev) = devices.iter().find(|d| d.name.contains(pref)) {
                tracing::info!("Auto-detected webcam: {} ({})", dev.name, dev.path);
                return Ok(dev.path.clone());
            }
        }

        // Fall back to first available device
        devices
            .first()
            .map(|d| d.path.clone())
            .ok_or_else(|| Error::Capture("No webcam devices found".into()))
    }

    /// Convert V4L2 FourCC to our FrameFormat
    fn fourcc_to_format(fourcc: FourCC) -> FrameFormat {
        match fourcc.str().unwrap_or_default() {
            "YUYV" => FrameFormat::Yuyv,
            "MJPG" => FrameFormat::Mjpeg,
            "NV12" => FrameFormat::Nv12,
            "RGB3" | "RGB" => FrameFormat::Rgb,
            "BGR3" | "BGR" => FrameFormat::Bgr,
            "RGBP" | "RGBA" => FrameFormat::Rgba,
            "BA24" | "BGRA" => FrameFormat::Bgra,
            _ => FrameFormat::Yuyv, // Default fallback
        }
    }

    /// Convert our FrameFormat to V4L2 FourCC
    fn format_to_fourcc(format: FrameFormat) -> FourCC {
        match format {
            FrameFormat::Yuyv => FourCC::new(b"YUYV"),
            FrameFormat::Mjpeg => FourCC::new(b"MJPG"),
            FrameFormat::Nv12 => FourCC::new(b"NV12"),
            FrameFormat::Rgb => FourCC::new(b"RGB3"),
            FrameFormat::Bgr => FourCC::new(b"BGR3"),
            FrameFormat::Rgba => FourCC::new(b"RGBP"),
            FrameFormat::Bgra => FourCC::new(b"BA24"),
            _ => FourCC::new(b"YUYV"),
        }
    }
}

#[async_trait::async_trait]
impl super::Capture for WebcamCapture {
    async fn start(&mut self) -> Result<()> {
        if self.active.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Get device path
        let device_path = match &self.config.device {
            Some(path) => path.clone(),
            None => Self::auto_detect_device()?,
        };

        tracing::info!("Opening webcam: {}", device_path);

        // Extract device index from path
        let device_index: usize = device_path
            .trim_start_matches("/dev/video")
            .parse()
            .map_err(|_| Error::Capture(format!("Invalid device path: {}", device_path)))?;

        // Open device
        let device = Device::new(device_index)
            .map_err(|e| Error::Capture(format!("Failed to open webcam: {}", e)))?;

        // Query capabilities
        let caps = device
            .query_caps()
            .map_err(|e| Error::Capture(format!("Failed to query webcam caps: {}", e)))?;

        tracing::info!("Webcam: {} (driver: {})", caps.card, caps.driver);

        // Set format
        let mut fmt = device
            .format()
            .map_err(|e| Error::Capture(format!("Failed to get format: {}", e)))?;

        // Apply resolution if specified
        if let Some(res) = &self.config.resolution {
            fmt.width = res.width;
            fmt.height = res.height;
        }

        // Apply pixel format if specified
        if let Some(pix_fmt) = &self.config.format {
            fmt.fourcc = Self::format_to_fourcc(*pix_fmt);
        }

        // Try to set the format
        let fmt = device
            .set_format(&fmt)
            .map_err(|e| Error::Capture(format!("Failed to set format: {}", e)))?;

        self.resolution = Some(Resolution::new(fmt.width, fmt.height));
        self.format = Self::fourcc_to_format(fmt.fourcc);

        tracing::info!(
            "Webcam format: {}x{} {:?}",
            fmt.width,
            fmt.height,
            self.format
        );

        // Set framerate
        if let Ok(mut params) = device.params() {
            params.interval = v4l::Fraction::new(1, self.config.framerate.num);
            let _ = device.set_params(&params);

            // Read back actual framerate
            if let Ok(params) = device.params() {
                self.framerate = Some(Framerate::new(
                    params.interval.denominator,
                    params.interval.numerator,
                ));
                tracing::info!("Webcam framerate: {:?}", self.framerate);
            }
        }

        // Create memory-mapped stream
        // We need to use unsafe to create a 'static stream from the device
        // This is safe because we ensure the device outlives the stream
        let stream = unsafe {
            let device_ptr = &device as *const Device;
            MmapStream::with_buffers(&*device_ptr, Type::VideoCapture, 4)
                .map_err(|e| Error::Capture(format!("Failed to create stream: {}", e)))?
        };

        self.device = Some(device);
        // Store stream - we use transmute to extend lifetime (device is stored in self)
        self.stream = Some(unsafe { std::mem::transmute(stream) });
        self.active.store(true, Ordering::SeqCst);

        tracing::info!("Webcam capture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        self.stream = None;
        self.device = None;
        tracing::info!("Webcam capture stopped");
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Frame> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| Error::Capture("Webcam not started".into()))?;

        // Capture frame
        let (buf, meta) = stream
            .next()
            .map_err(|e| Error::Capture(format!("Failed to capture frame: {}", e)))?;

        let resolution = self
            .resolution
            .ok_or_else(|| Error::Capture("Resolution not set".into()))?;

        self.frame_count += 1;

        // Calculate stride based on format
        let stride = match self.format {
            FrameFormat::Yuyv | FrameFormat::Uyvy => resolution.width * 2,
            FrameFormat::Rgb | FrameFormat::Bgr => resolution.width * 3,
            FrameFormat::Rgba | FrameFormat::Bgra => resolution.width * 4,
            _ => resolution.width * 2, // Default for YUV formats
        };

        // Calculate duration based on framerate
        let duration = self.framerate
            .map(|fr| fr.frame_duration_us())
            .unwrap_or(33333); // Default to ~30fps

        // Convert to Frame
        let frame = Frame {
            data: buf.to_vec(),
            width: resolution.width,
            height: resolution.height,
            stride,
            format: self.format,
            pts: meta.timestamp.sec as i64 * 1_000_000 + meta.timestamp.usec as i64,
            duration,
            is_keyframe: true, // Every webcam frame is a keyframe (no inter-frame compression)
            dmabuf_fd: None,
        };

        Ok(frame)
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn resolution(&self) -> Option<Resolution> {
        self.resolution
    }

    fn framerate(&self) -> Option<Framerate> {
        self.framerate
    }
}

impl Drop for WebcamCapture {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        self.stream = None;
        self.device = None;
    }
}

/// Information about a webcam device
#[derive(Debug, Clone)]
pub struct WebcamDeviceInfo {
    /// Device path (e.g., "/dev/video0")
    pub path: String,
    /// Device name (e.g., "Elgato Facecam Pro")
    pub name: String,
    /// Driver name
    pub driver: String,
    /// Supported pixel formats
    pub formats: Vec<String>,
}

impl std::fmt::Display for WebcamDeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.path, self.name, self.driver)
    }
}
