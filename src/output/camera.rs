//! Virtual camera output via PipeWire
//!
//! Creates a PipeWire video source node that appears as a camera
//! in applications like Discord, OBS, and video conferencing tools.
//!
//! Uses RawOutputSink trait since virtual cameras need raw video frames,
//! not encoded packets. Use the capture -> camera pipeline directly
//! for optimal performance (no encoding/decoding overhead).

use crate::error::{Error, Result};
use crate::processing::convert_colorspace;
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution};

use super::{OutputSink, RawOutputSink};

use pipewire as pw;
use pw::spa::param::video::VideoFormat;
use pw::spa::pod::Pod;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::mpsc;

/// Virtual camera output
pub struct VirtualCamera {
    name: String,
    initialized: bool,
    bytes_written: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
    frame_tx: Option<mpsc::Sender<Vec<u8>>>,
    pipewire_thread: Option<std::thread::JoinHandle<()>>,
    width: u32,
    height: u32,
    format: FrameFormat,
}

impl VirtualCamera {
    /// Create a new virtual camera
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            initialized: false,
            bytes_written: Arc::new(AtomicU64::new(0)),
            active: Arc::new(AtomicBool::new(false)),
            frame_tx: None,
            pipewire_thread: None,
            width: 1920,
            height: 1080,
            format: FrameFormat::Bgra,
        }
    }

    /// Set the camera resolution
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set the expected frame format
    pub fn with_format(mut self, format: FrameFormat) -> Self {
        self.format = format;
        self
    }

    /// Start the PipeWire camera thread
    fn start_pipewire_camera(&mut self) -> Result<mpsc::Sender<Vec<u8>>> {
        let (frame_tx, frame_rx) = mpsc::channel::<Vec<u8>>();
        let active = self.active.clone();
        let name = self.name.clone();
        let width = self.width;
        let height = self.height;

        let handle = std::thread::spawn(move || {
            if let Err(e) = run_virtual_camera(name, width, height, frame_rx, active) {
                tracing::error!("Virtual camera error: {}", e);
            }
        });

        self.pipewire_thread = Some(handle);
        Ok(frame_tx)
    }
}

/// RawOutputSink implementation for proper raw frame handling
#[async_trait::async_trait]
impl RawOutputSink for VirtualCamera {
    async fn init_raw(&mut self, resolution: Resolution, format: FrameFormat) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        self.width = resolution.width;
        self.height = resolution.height;
        self.format = format;

        self.active.store(true, Ordering::SeqCst);
        let frame_tx = self.start_pipewire_camera()?;
        self.frame_tx = Some(frame_tx);

        tracing::info!(
            "Virtual camera '{}' initialized ({}x{}, {:?})",
            self.name,
            self.width,
            self.height,
            self.format
        );
        self.initialized = true;
        Ok(())
    }

    async fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        if !self.initialized {
            // Auto-initialize with frame properties
            self.init_raw(
                Resolution::new(frame.width, frame.height),
                frame.format,
            )
            .await?;
        }

        if let Some(ref tx) = self.frame_tx {
            // Convert to BGRA if needed (PipeWire expects BGRx/BGRA)
            let frame_data = if frame.format == FrameFormat::Bgra {
                frame.data.clone()
            } else {
                convert_colorspace(&frame.data, frame.format, FrameFormat::Bgra, frame.width, frame.height)?
            };

            // Send frame data to PipeWire thread
            let _ = tx.send(frame_data);
        }

        self.bytes_written
            .fetch_add(frame.data.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        self.active.store(false, Ordering::SeqCst);
        drop(self.frame_tx.take());

        if let Some(handle) = self.pipewire_thread.take() {
            let _ = handle.join();
        }

        tracing::info!("Virtual camera '{}' stopped", self.name);
        self.initialized = false;
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }
}

/// OutputSink implementation for backward compatibility
///
/// NOTE: Prefer using RawOutputSink with write_frame() for virtual cameras.
/// This implementation assumes packet.data contains raw frame bytes.
#[async_trait::async_trait]
impl OutputSink for VirtualCamera {
    async fn init_with_codec(&mut self, _codec_params: Option<&CodecParams>) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        tracing::warn!(
            "Virtual camera initialized via OutputSink - for best results, use RawOutputSink"
        );

        self.active.store(true, Ordering::SeqCst);
        let frame_tx = self.start_pipewire_camera()?;
        self.frame_tx = Some(frame_tx);

        tracing::info!(
            "Virtual camera '{}' initialized ({}x{})",
            self.name,
            self.width,
            self.height
        );
        self.initialized = true;
        Ok(())
    }

    async fn write(&mut self, packet: &Packet) -> Result<()> {
        if !self.initialized {
            self.init().await?;
        }

        // Pass through raw data - caller must ensure packet.data is raw frame bytes
        if let Some(ref tx) = self.frame_tx {
            let _ = tx.send(packet.data.clone());
        }

        self.bytes_written
            .fetch_add(packet.size() as u64, Ordering::Relaxed);
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        RawOutputSink::finish(self).await
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }
}

impl Drop for VirtualCamera {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        drop(self.frame_tx.take());
        if let Some(handle) = self.pipewire_thread.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// PipeWire Virtual Camera Implementation
// ============================================================================

/// Run PipeWire virtual camera output
fn run_virtual_camera(
    name: String,
    width: u32,
    height: u32,
    frame_rx: mpsc::Receiver<Vec<u8>>,
    active: Arc<AtomicBool>,
) -> Result<()> {
    tracing::info!(
        "Starting PipeWire virtual camera '{}' ({}x{})",
        name,
        width,
        height
    );

    // Initialize PipeWire
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None)
        .map_err(|e| Error::PipeWire(format!("Failed to create main loop: {:?}", e)))?;

    let context = pw::context::Context::new(&mainloop)
        .map_err(|e| Error::PipeWire(format!("Failed to create context: {:?}", e)))?;

    let core = context
        .connect(None)
        .map_err(|e| Error::PipeWire(format!("Failed to connect to PipeWire: {:?}", e)))?;

    // Create stream as a video source (camera)
    let stream = pw::stream::Stream::new(
        &core,
        &name,
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_CLASS => "Video/Source",
            *pw::keys::MEDIA_ROLE => "Camera",
            *pw::keys::NODE_NAME => name.as_str(),
            *pw::keys::NODE_DESCRIPTION => "GhostStream Virtual Camera",
        },
    )
    .map_err(|e| Error::PipeWire(format!("Failed to create stream: {:?}", e)))?;

    // State for callbacks
    struct CameraState {
        frame_rx: mpsc::Receiver<Vec<u8>>,
        frame_size: usize,
        stride: u32,
    }

    let stride = width * 4; // BGRx = 4 bytes per pixel
    let frame_size = (height * stride) as usize;
    let state = CameraState {
        frame_rx,
        frame_size,
        stride,
    };

    let active_clone = active.clone();
    let mainloop_weak = mainloop.downgrade();

    // Set up stream listener
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_, _, old, new| {
            tracing::debug!("Camera stream state: {:?} -> {:?}", old, new);

            if matches!(new, pw::stream::StreamState::Error(_)) {
                if let Some(mainloop) = mainloop_weak.upgrade() {
                    mainloop.quit();
                }
            }
        })
        .process(|stream, state| {
            // Get buffer to fill
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let data = &mut datas[0];

            // Try to get next frame from channel
            if let Ok(frame_data) = state.frame_rx.try_recv() {
                // Write frame data to buffer
                if let Some(slice) = data.data() {
                    let copy_size = frame_data.len().min(slice.len()).min(state.frame_size);

                    // Safety: we're writing to the PipeWire buffer
                    unsafe {
                        let dst = slice.as_ptr() as *mut u8;
                        std::ptr::copy_nonoverlapping(frame_data.as_ptr(), dst, copy_size);
                    }

                    // Set chunk metadata
                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = state.stride as i32;
                    *chunk.size_mut() = state.frame_size as u32;
                }
            } else {
                // No frame available, output black frame
                if let Some(slice) = data.data() {
                    let clear_size = slice.len().min(state.frame_size);

                    unsafe {
                        let dst = slice.as_ptr() as *mut u8;
                        std::ptr::write_bytes(dst, 0, clear_size);
                    }

                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = state.stride as i32;
                    *chunk.size_mut() = state.frame_size as u32;
                }
            }
        })
        .register()
        .map_err(|e| Error::PipeWire(format!("Failed to register listener: {:?}", e)))?;

    // Build format parameters - output BGRx which is common for cameras
    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Rectangle,
            pw::spa::utils::Rectangle { width, height }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Fraction,
            pw::spa::utils::Fraction { num: 60, denom: 1 }
        ),
    );

    // Serialize format
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| Error::PipeWire(format!("Failed to serialize format: {:?}", e)))?
    .0
    .into_inner();

    let mut params =
        [Pod::from_bytes(&values).ok_or_else(|| Error::PipeWire("Invalid pod".into()))?];

    // Connect stream as OUTPUT (we're producing video)
    stream
        .connect(
            pw::spa::utils::Direction::Output,
            None, // No specific target, let applications connect
            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::DRIVER,
            &mut params,
        )
        .map_err(|e| Error::PipeWire(format!("Failed to connect camera stream: {:?}", e)))?;

    tracing::info!("Virtual camera '{}' ready", name);

    // Run main loop
    while active_clone.load(Ordering::SeqCst) {
        mainloop.loop_().iterate(std::time::Duration::from_millis(16));
    }

    tracing::info!("Virtual camera stopped");
    Ok(())
}
