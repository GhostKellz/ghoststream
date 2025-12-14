//! xdg-desktop-portal screen capture
//!
//! Uses the Portal API for secure screen sharing on Wayland.
//! PipeWire receives the actual video frames.

use crate::config::CaptureConfig;
use crate::error::{Error, Result};
use crate::types::{Frame, FrameFormat, Framerate, Resolution};

use super::Capture;

use pipewire as pw;
use pw::spa::param::video::VideoFormat;
use pw::spa::pod::Pod;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Portal-based screen capture
pub struct PortalCapture {
    config: CaptureConfig,
    active: Arc<AtomicBool>,
    resolution: Option<Resolution>,
    framerate: Option<Framerate>,
    frame_rx: Option<mpsc::Receiver<Frame>>,
    pipewire_thread: Option<std::thread::JoinHandle<()>>,
    frame_count: Arc<AtomicU64>,
    node_id: Option<u32>,
}

impl PortalCapture {
    /// Create a new portal capture session
    pub async fn new(config: CaptureConfig) -> Result<Self> {
        Ok(Self {
            config,
            active: Arc::new(AtomicBool::new(false)),
            resolution: None,
            framerate: None,
            frame_rx: None,
            pipewire_thread: None,
            frame_count: Arc::new(AtomicU64::new(0)),
            node_id: None,
        })
    }

    /// Request screen capture permission from user via portal
    async fn request_permission(&mut self) -> Result<u32> {
        use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
        use ashpd::desktop::PersistMode;

        tracing::info!("Requesting screen capture permission via portal");

        let proxy = Screencast::new()
            .await
            .map_err(|e| Error::Portal(format!("Failed to connect to screencast portal: {}", e)))?;

        // Create session
        let session = proxy
            .create_session()
            .await
            .map_err(|e| Error::Portal(format!("Failed to create session: {}", e)))?;

        // Select sources - allow both monitors and windows
        proxy
            .select_sources(
                &session,
                CursorMode::Embedded, // Include cursor in capture
                SourceType::Monitor | SourceType::Window,
                false, // multiple selection
                None,  // restore_token
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| Error::Portal(format!("Failed to select sources: {}", e)))?;

        // Start the screencast - this shows the portal picker dialog
        // Pass None for window identifier (no parent window)
        let response = proxy
            .start(&session, None)
            .await
            .map_err(|e| Error::Portal(format!("User cancelled or portal error: {}", e)))?;

        // Get the response with the streams
        let streams = response
            .response()
            .map_err(|e| Error::Portal(format!("Failed to get screencast response: {}", e)))?;

        if streams.streams().is_empty() {
            return Err(Error::NoCaptureSource);
        }

        let stream = &streams.streams()[0];
        let node_id = stream.pipe_wire_node_id();

        // Get resolution if available
        if let Some((width, height)) = stream.size() {
            self.resolution = Some(Resolution::new(width as u32, height as u32));
            tracing::info!("Capture source resolution: {}x{}", width, height);
        }

        tracing::info!("Got PipeWire node ID: {}", node_id);

        Ok(node_id)
    }

    /// Start PipeWire stream to receive frames
    fn start_pipewire_stream(&mut self, node_id: u32) -> Result<mpsc::Receiver<Frame>> {
        let (frame_tx, frame_rx) = mpsc::channel::<Frame>(4);
        let active = self.active.clone();
        let frame_count = self.frame_count.clone();
        let target_resolution = self.resolution;
        let target_fps = self.config.framerate.fps();

        // PipeWire needs to run on its own thread with a MainLoop
        let handle = std::thread::spawn(move || {
            if let Err(e) = run_pipewire_capture(
                node_id,
                frame_tx,
                active,
                frame_count,
                target_resolution,
                target_fps,
            ) {
                tracing::error!("PipeWire capture error: {}", e);
            }
        });

        self.pipewire_thread = Some(handle);
        Ok(frame_rx)
    }
}

#[async_trait::async_trait]
impl Capture for PortalCapture {
    async fn start(&mut self) -> Result<()> {
        if self.active.load(Ordering::SeqCst) {
            return Err(Error::Pipeline("Capture already active".into()));
        }

        // Request permission via portal - this shows the picker dialog
        let node_id = self.request_permission().await?;
        self.node_id = Some(node_id);

        // Start PipeWire stream
        self.active.store(true, Ordering::SeqCst);
        let frame_rx = self.start_pipewire_stream(node_id)?;
        self.frame_rx = Some(frame_rx);
        self.framerate = Some(self.config.framerate);

        tracing::info!("Portal capture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.active.store(false, Ordering::SeqCst);

        // Wait for PipeWire thread to finish
        if let Some(handle) = self.pipewire_thread.take() {
            let _ = handle.join();
        }

        tracing::info!("Portal capture stopped");
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Frame> {
        if !self.active.load(Ordering::SeqCst) {
            return Err(Error::PipelineNotStarted);
        }

        let rx = self
            .frame_rx
            .as_mut()
            .ok_or_else(|| Error::Pipeline("Frame receiver not initialized".into()))?;

        // Wait for next frame with timeout
        match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
            Ok(Some(frame)) => Ok(frame),
            Ok(None) => Err(Error::Pipeline("Frame channel closed".into())),
            Err(_) => {
                // Timeout - return an empty frame to keep pipeline moving
                let resolution = self.resolution.unwrap_or(Resolution::FHD_1080P);
                let mut frame = Frame::new(resolution.width, resolution.height, FrameFormat::Bgra);
                frame.pts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_micros() as i64;
                Ok(frame)
            }
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn resolution(&self) -> Option<Resolution> {
        self.resolution
    }

    fn framerate(&self) -> Option<Framerate> {
        self.framerate
    }
}

impl Drop for PortalCapture {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(handle) = self.pipewire_thread.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// PipeWire Capture Implementation
// ============================================================================

/// User data for stream callbacks
struct CaptureState {
    frame_tx: mpsc::Sender<Frame>,
    frame_count: Arc<AtomicU64>,
    format: pw::spa::param::video::VideoInfoRaw,
}

/// Run PipeWire capture loop - based on pipewire-rs streams.rs example
fn run_pipewire_capture(
    node_id: u32,
    frame_tx: mpsc::Sender<Frame>,
    active: Arc<AtomicBool>,
    frame_count: Arc<AtomicU64>,
    target_resolution: Option<Resolution>,
    target_fps: u32,
) -> Result<()> {
    tracing::info!("Starting PipeWire capture for node {}", node_id);

    // Initialize PipeWire
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None)
        .map_err(|e| Error::PipeWire(format!("Failed to create main loop: {:?}", e)))?;

    let context = pw::context::Context::new(&mainloop)
        .map_err(|e| Error::PipeWire(format!("Failed to create context: {:?}", e)))?;

    let core = context
        .connect(None)
        .map_err(|e| Error::PipeWire(format!("Failed to connect to PipeWire: {:?}", e)))?;

    // Create stream with properties
    let stream = pw::stream::Stream::new(
        &core,
        "ghoststream-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| Error::PipeWire(format!("Failed to create stream: {:?}", e)))?;

    // Capture state for callbacks
    let state = CaptureState {
        frame_tx,
        frame_count,
        format: Default::default(),
    };

    // Clone for use in main loop check
    let active_clone = active.clone();
    let mainloop_weak = mainloop.downgrade();

    // Set up stream listener with callbacks
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_, _, old, new| {
            tracing::debug!("Stream state changed: {:?} -> {:?}", old, new);

            // Stop mainloop on error or if we're done
            if matches!(new, pw::stream::StreamState::Error(_)) {
                if let Some(mainloop) = mainloop_weak.upgrade() {
                    mainloop.quit();
                }
            }
        })
        .param_changed(|_, state, id, param| {
            // Only handle Format params
            let Some(param) = param else { return };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            // Parse media type/subtype
            let (media_type, media_subtype) =
                match pw::spa::param::format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };

            // Only handle raw video
            if media_type != pw::spa::param::format::MediaType::Video
                || media_subtype != pw::spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            // Parse video format info
            if let Err(e) = state.format.parse(param) {
                tracing::warn!("Failed to parse video format: {:?}", e);
                return;
            }

            tracing::info!(
                "Video format negotiated: {:?} {}x{} @ {}/{}fps",
                state.format.format(),
                state.format.size().width,
                state.format.size().height,
                state.format.framerate().num,
                state.format.framerate().denom,
            );
        })
        .process(|stream, state| {
            // Dequeue buffer from stream
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let data = &mut datas[0];
            let chunk = data.chunk();
            let size = chunk.size() as usize;
            let offset = chunk.offset() as usize;

            if size == 0 {
                return;
            }

            // Get the actual frame data
            let Some(slice) = data.data() else {
                return;
            };

            // Determine resolution from negotiated format
            let width = state.format.size().width;
            let height = state.format.size().height;

            // Map PipeWire format to our format
            let frame_format = match state.format.format() {
                VideoFormat::BGRx | VideoFormat::BGRA => FrameFormat::Bgra,
                VideoFormat::RGBx | VideoFormat::RGBA => FrameFormat::Rgba,
                VideoFormat::RGB => FrameFormat::Rgb24,
                VideoFormat::NV12 => FrameFormat::Nv12,
                VideoFormat::I420 => FrameFormat::Yuv420p,
                _ => {
                    tracing::warn!("Unsupported video format: {:?}", state.format.format());
                    FrameFormat::Bgra // Fallback
                }
            };

            // Create frame and copy data
            let mut frame = Frame::new(width, height, frame_format);

            // Calculate expected size based on format
            let expected_size = frame.data.len();
            let copy_size = size
                .min(expected_size)
                .min(slice.len().saturating_sub(offset));

            if copy_size > 0 && offset < slice.len() {
                frame.data[..copy_size].copy_from_slice(&slice[offset..offset + copy_size]);
            }

            // Set timestamp
            frame.pts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as i64;

            state.frame_count.fetch_add(1, Ordering::Relaxed);

            // Send frame (non-blocking, drop if channel full)
            let _ = state.frame_tx.try_send(frame);
        })
        .register()
        .map_err(|e| Error::PipeWire(format!("Failed to register stream listener: {:?}", e)))?;

    // Build format parameters using the pod macros
    let resolution = target_resolution.unwrap_or(Resolution::FHD_1080P);

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
        // Preferred formats - BGRx is common for screen capture
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
            VideoFormat::NV12
        ),
        // Resolution range
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: resolution.width,
                height: resolution.height,
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1,
            },
            pw::spa::utils::Rectangle {
                width: 7680, // 8K max
                height: 4320,
            }
        ),
        // Framerate range
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction {
                num: target_fps,
                denom: 1
            },
            pw::spa::utils::Fraction { num: 1, denom: 1 },
            pw::spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );

    // Serialize the pod object
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| Error::PipeWire(format!("Failed to serialize format params: {:?}", e)))?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values)
        .ok_or_else(|| Error::PipeWire("Failed to create pod from bytes".into()))?];

    // Connect to the screencast node
    stream
        .connect(
            pw::spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|e| {
            Error::PipeWire(format!(
                "Failed to connect stream to node {}: {:?}",
                node_id, e
            ))
        })?;

    tracing::info!("PipeWire stream connected to node {}", node_id);

    // Run main loop until stopped
    while active_clone.load(Ordering::SeqCst) {
        // Iterate with timeout to check active flag periodically
        mainloop.loop_().iterate(std::time::Duration::from_millis(16)); // ~60fps check rate
    }

    tracing::info!("PipeWire capture loop ended");
    Ok(())
}
