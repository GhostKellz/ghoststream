//! PipeWire audio capture
//!
//! Captures audio from desktop (monitor) or application sources.

use crate::error::{Error, Result};
use super::types::{AudioFrame, ChannelLayout, SampleFormat};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Audio source type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    /// Capture all desktop audio (monitor)
    Desktop,
    /// Capture specific application by name
    Application(String),
    /// Capture from specific PipeWire node ID
    NodeId(u32),
    /// Default input device (microphone)
    DefaultInput,
    /// Default output device (speakers/headphones monitor)
    DefaultOutput,
}

impl Default for AudioSource {
    fn default() -> Self {
        AudioSource::Desktop
    }
}

/// Audio capture configuration
#[derive(Debug, Clone)]
pub struct AudioCaptureConfig {
    /// Audio source to capture
    pub source: AudioSource,
    /// Sample rate (default: 48000)
    pub sample_rate: u32,
    /// Channel layout (default: stereo)
    pub channels: ChannelLayout,
    /// Sample format (default: F32)
    pub format: SampleFormat,
    /// Buffer size in samples (default: 1024)
    pub buffer_size: u32,
}

impl Default for AudioCaptureConfig {
    fn default() -> Self {
        Self {
            source: AudioSource::Desktop,
            sample_rate: 48000,
            channels: ChannelLayout::Stereo,
            format: SampleFormat::F32,
            buffer_size: 1024,
        }
    }
}

/// Trait for audio capture implementations
#[async_trait::async_trait]
pub trait AudioCapture: Send + Sync {
    /// Start capturing audio
    async fn start(&mut self) -> Result<()>;

    /// Stop capturing audio
    async fn stop(&mut self) -> Result<()>;

    /// Get next audio frame
    async fn next_frame(&mut self) -> Result<AudioFrame>;

    /// Check if capture is active
    fn is_active(&self) -> bool;

    /// Get current sample rate
    fn sample_rate(&self) -> u32;

    /// Get number of channels
    fn channels(&self) -> u32;
}

/// PipeWire-based audio capture
pub struct PipeWireAudioCapture {
    config: AudioCaptureConfig,
    running: Arc<AtomicBool>,
    frame_rx: Option<crossbeam_channel::Receiver<AudioFrame>>,
    _thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl PipeWireAudioCapture {
    /// Create new PipeWire audio capture
    pub fn new(config: AudioCaptureConfig) -> Result<Self> {
        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            frame_rx: None,
            _thread_handle: None,
        })
    }
}

#[async_trait::async_trait]
impl AudioCapture for PipeWireAudioCapture {
    async fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);

        let (frame_tx, frame_rx) = crossbeam_channel::bounded::<AudioFrame>(16);
        self.frame_rx = Some(frame_rx);

        let config = self.config.clone();
        let running = self.running.clone();

        // Spawn PipeWire capture thread
        let handle = std::thread::spawn(move || {
            if let Err(e) = run_pipewire_capture(config, running.clone(), frame_tx) {
                tracing::error!("PipeWire audio capture error: {}", e);
                running.store(false, Ordering::SeqCst);
            }
        });

        self._thread_handle = Some(handle);
        tracing::info!("PipeWire audio capture started");

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        tracing::info!("PipeWire audio capture stopped");
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<AudioFrame> {
        let rx = self.frame_rx.as_ref().ok_or(Error::CaptureNotStarted)?;

        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(frame) => Ok(frame),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                Err(Error::Timeout("Audio frame timeout".into()))
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(Error::CaptureEnded)
            }
        }
    }

    fn is_active(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    fn channels(&self) -> u32 {
        self.config.channels.channels()
    }
}

/// Run PipeWire capture loop
fn run_pipewire_capture(
    config: AudioCaptureConfig,
    running: Arc<AtomicBool>,
    frame_tx: crossbeam_channel::Sender<AudioFrame>,
) -> Result<()> {
    use pipewire as pw;

    // Initialize PipeWire
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None)
        .map_err(|e| Error::PipeWire(format!("Failed to create mainloop: {}", e)))?;

    let context = pw::context::Context::new(&mainloop)
        .map_err(|e| Error::PipeWire(format!("Failed to create context: {}", e)))?;

    let core = context
        .connect(None)
        .map_err(|e| Error::PipeWire(format!("Failed to connect: {}", e)))?;

    // Create stream properties
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::STREAM_CAPTURE_SINK => "true",
    };

    // Create stream
    let stream = pw::stream::Stream::new(&core, "ghoststream-audio", props)
        .map_err(|e| Error::PipeWire(format!("Failed to create stream: {}", e)))?;

    // Set up stream listener
    let config_clone = config.clone();
    let running_clone = running.clone();
    let frame_tx_clone = frame_tx.clone();
    let mut pts: i64 = 0;

    let _listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let data = &mut datas[0];
                let chunk = data.chunk();
                let offset = chunk.offset() as usize;
                let size = chunk.size() as usize;

                if let Some(slice) = data.data() {
                    if offset + size <= slice.len() {
                        let audio_data = slice[offset..offset + size].to_vec();
                        let samples = size as u32
                            / (config_clone.channels.channels()
                                * config_clone.format.bytes_per_sample() as u32);

                        let mut frame = AudioFrame::from_data(
                            audio_data,
                            samples,
                            config_clone.channels.channels(),
                            config_clone.format,
                            config_clone.sample_rate,
                        );
                        frame.pts = pts;
                        frame.duration = frame.calculated_duration_us();
                        pts += frame.duration;

                        let _ = frame_tx_clone.try_send(frame);
                    }
                }
            }
        })
        .register()
        .map_err(|e| Error::PipeWire(format!("Failed to register listener: {}", e)))?;

    // Build audio format
    let sample_rate = config.sample_rate;
    let channels = config.channels.channels();
    let format = match config.format {
        SampleFormat::S16 | SampleFormat::S16P => libspa_sys::SPA_AUDIO_FORMAT_S16,
        SampleFormat::S32 => libspa_sys::SPA_AUDIO_FORMAT_S32,
        SampleFormat::F32 | SampleFormat::F32P => libspa_sys::SPA_AUDIO_FORMAT_F32,
    };

    // Create audio info
    let mut audio_info = libspa_sys::spa_audio_info_raw {
        format,
        flags: 0,
        rate: sample_rate,
        channels,
        position: [0; 64],
    };

    // Set channel positions for stereo
    if channels >= 2 {
        audio_info.position[0] = libspa_sys::SPA_AUDIO_CHANNEL_FL;
        audio_info.position[1] = libspa_sys::SPA_AUDIO_CHANNEL_FR;
    }

    // Build format pod
    let mut buffer = vec![0u8; 1024];
    let pod = unsafe {
        let builder = libspa_sys::spa_pod_builder {
            data: buffer.as_mut_ptr() as *mut _,
            size: buffer.len() as u32,
            _padding: 0,
            state: libspa_sys::spa_pod_builder_state {
                offset: 0,
                flags: 0,
                frame: std::ptr::null_mut(),
            },
            callbacks: libspa_sys::spa_callbacks {
                funcs: std::ptr::null(),
                data: std::ptr::null_mut(),
            },
        };

        let pod_ptr = libspa_sys::spa_format_audio_raw_build(
            &builder as *const _ as *mut _,
            libspa_sys::SPA_PARAM_EnumFormat,
            &mut audio_info,
        );

        if pod_ptr.is_null() {
            return Err(Error::PipeWire("Failed to build audio format pod".into()));
        }

        libspa::pod::Pod::from_raw(pod_ptr)
    };

    // Connect stream
    let flags = pw::stream::StreamFlags::AUTOCONNECT
        | pw::stream::StreamFlags::MAP_BUFFERS
        | pw::stream::StreamFlags::RT_PROCESS;

    stream
        .connect(libspa::utils::Direction::Input, None, flags, &mut [pod])
        .map_err(|e| Error::PipeWire(format!("Failed to connect stream: {}", e)))?;

    tracing::info!("PipeWire audio stream connected");

    // Run main loop until stopped
    let weak_mainloop = mainloop.downgrade();
    while running.load(Ordering::SeqCst) {
        // Process events with timeout
        if let Some(ml) = weak_mainloop.upgrade() {
            ml.loop_().iterate(std::time::Duration::from_millis(10));
        } else {
            break;
        }
    }

    Ok(())
}

/// List available audio sources
pub fn list_sources() -> Vec<AudioSource> {
    vec![
        AudioSource::Desktop,
        AudioSource::DefaultInput,
        AudioSource::DefaultOutput,
    ]
}
