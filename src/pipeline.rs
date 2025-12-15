//! Video processing pipeline
//!
//! Connects capture → process → encode → output
//! Supports both video-only and A/V pipelines.

use crate::audio::{self, AudioCapture, AudioEncoder};
use crate::capture;
use crate::config::{CaptureConfig, EncoderConfig};
use crate::encode;
use crate::error::{Error, Result};
use crate::output::{self, AvMuxer, Output, OutputSink};
use crate::processing;
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution, Stats};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Audio configuration for pipeline
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Enable audio capture
    pub enabled: bool,
    /// Audio source
    pub source: audio::AudioSource,
    /// Audio codec
    pub codec: audio::AudioCodec,
    /// Sample rate
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u32,
    /// Bitrate in bps
    pub bitrate: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source: audio::AudioSource::Desktop,
            codec: audio::AudioCodec::Aac,
            sample_rate: 48000,
            channels: 2,
            bitrate: 192000,
        }
    }
}

/// Video processing pipeline
pub struct Pipeline {
    capture_config: CaptureConfig,
    encoder_config: EncoderConfig,
    #[allow(dead_code)] // Used when audio is enabled
    audio_config: AudioConfig,
    output_config: Output,
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
    // Audio components (initialized when audio_config.enabled)
    #[allow(dead_code)] // Will be used in start() for A/V pipeline
    audio_running: Arc<AtomicBool>,
}

impl Pipeline {
    /// Create a new pipeline (video only)
    pub fn new(capture: CaptureConfig, encoder: EncoderConfig, output: Output) -> Result<Self> {
        Self::new_with_audio(capture, encoder, AudioConfig::default(), output)
    }

    /// Create a new pipeline with audio
    pub fn new_with_audio(
        capture: CaptureConfig,
        encoder: EncoderConfig,
        audio: AudioConfig,
        output: Output,
    ) -> Result<Self> {
        Ok(Self {
            capture_config: capture,
            encoder_config: encoder,
            audio_config: audio,
            output_config: output,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(Stats::default())),
            audio_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create pipeline from preset
    pub fn from_preset(preset: crate::config::Preset, output: Output) -> Result<Self> {
        let encoder_config = EncoderConfig::from_preset(preset);
        Self::new(CaptureConfig::default(), encoder_config, output)
    }

    /// Create pipeline from preset with audio
    pub fn from_preset_with_audio(
        preset: crate::config::Preset,
        audio: AudioConfig,
        output: Output,
    ) -> Result<Self> {
        let encoder_config = EncoderConfig::from_preset(preset);
        Self::new_with_audio(CaptureConfig::default(), encoder_config, audio, output)
    }

    /// Start the pipeline
    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(Error::PipelineAlreadyRunning);
        }

        self.running.store(true, Ordering::SeqCst);
        let audio_enabled = self.audio_config.enabled;
        tracing::info!(
            "Pipeline starting (audio: {})",
            if audio_enabled { "enabled" } else { "disabled" }
        );

        // Clone configs for use in tasks
        let capture_config = self.capture_config.clone();
        let encoder_config = self.encoder_config.clone();
        let audio_config = self.audio_config.clone();
        let output_config = self.output_config.clone();
        let running = self.running.clone();
        let audio_running = self.audio_running.clone();
        let stats = self.stats.clone();

        // Determine processing needs
        let target_resolution = encoder_config.resolution;
        let target_format = if encoder_config.pixel_format.is_nvenc_native() {
            Some(encoder_config.pixel_format)
        } else {
            Some(FrameFormat::Nv12)
        };

        // Create channels for frame/packet communication
        let (frame_tx, frame_rx) = crossbeam_channel::bounded::<Frame>(4);
        let (packet_tx, mut packet_rx) = tokio::sync::mpsc::channel::<Packet>(8);

        // Audio channels (only used if audio enabled)
        let (audio_packet_tx, mut audio_packet_rx) =
            tokio::sync::mpsc::channel::<audio::AudioPacket>(16);

        // Channel for codec params (sent after first frame is encoded)
        let (codec_params_tx, codec_params_rx) =
            tokio::sync::oneshot::channel::<Option<CodecParams>>();

        // Channel for audio params (sent after audio encoder is initialized)
        let (audio_params_tx, audio_params_rx) =
            tokio::sync::oneshot::channel::<Option<audio::AudioParams>>();

        // Spawn audio capture and encoder thread if enabled
        if audio_enabled {
            audio_running.store(true, Ordering::SeqCst);
            let audio_running_clone = audio_running.clone();
            let audio_config_clone = audio_config.clone();

            std::thread::spawn(move || {
                if let Err(e) = run_audio_pipeline(
                    audio_config_clone,
                    audio_running_clone,
                    audio_packet_tx,
                    audio_params_tx,
                ) {
                    tracing::error!("Audio pipeline error: {}", e);
                }
            });
        } else {
            // Send None if audio not enabled
            let _ = audio_params_tx.send(None);
        }

        // Spawn video encoder thread (blocking, non-Send encoder lives here)
        let encoder_running = running.clone();
        std::thread::spawn(move || {
            // Create encoder in this thread
            let mut encoder = match encode::create_encoder(encoder_config) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Failed to create encoder: {}", e);
                    let _ = codec_params_tx.send(None);
                    return;
                }
            };

            if let Err(e) = encoder.init() {
                tracing::error!("Failed to initialize encoder: {}", e);
                let _ = codec_params_tx.send(None);
                return;
            }

            tracing::info!("Encoder thread started");

            // Track if we've sent codec params
            let mut codec_params_sent = false;
            let mut codec_params_tx = Some(codec_params_tx);

            // Process frames until shutdown
            while encoder_running.load(Ordering::SeqCst) {
                match frame_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(frame) => {
                        // Process frame (scale/convert if needed)
                        let processed = match processing::process_frame(
                            &frame,
                            target_resolution,
                            target_format,
                        ) {
                            Ok(f) => f,
                            Err(e) => {
                                tracing::error!("Processing error: {}", e);
                                continue;
                            }
                        };

                        // Encode
                        match encoder.encode(&processed) {
                            Ok(Some(packet)) => {
                                // Send codec params after first successful encode
                                if !codec_params_sent {
                                    if let Some(tx) = codec_params_tx.take() {
                                        let params = encoder.codec_params();
                                        let _ = tx.send(params);
                                        codec_params_sent = true;
                                    }
                                }

                                if packet_tx.blocking_send(packet).is_err() {
                                    tracing::debug!("Output channel closed");
                                    break;
                                }
                            }
                            Ok(None) => {} // Buffered
                            Err(e) => {
                                tracing::error!("Encode error: {}", e);
                            }
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }

            // If we never sent codec params, send None now
            if let Some(tx) = codec_params_tx.take() {
                let _ = tx.send(None);
            }

            // Flush encoder
            tracing::debug!("Flushing encoder");
            if let Ok(packets) = encoder.flush() {
                for packet in packets {
                    let _ = packet_tx.blocking_send(packet);
                }
            }

            tracing::info!("Encoder thread stopped");
        });

        // Spawn capture + output task (async)
        tokio::spawn(async move {
            // Create capture
            let mut capture = match capture::create_capture(capture_config).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create capture: {}", e);
                    return;
                }
            };

            // Start capture
            if let Err(e) = capture.start().await {
                tracing::error!("Failed to start capture: {}", e);
                return;
            }

            tracing::info!("Capture started, waiting for codec params from encoder");

            // Wait for video codec params
            let video_params = match codec_params_rx.await {
                Ok(Some(params)) => {
                    tracing::info!(
                        "Received video codec params: {:?} {}x{}",
                        params.codec,
                        params.resolution.width,
                        params.resolution.height
                    );
                    Some(params)
                }
                Ok(None) => {
                    tracing::warn!("No video codec params available");
                    None
                }
                Err(_) => {
                    tracing::warn!("Video codec params channel closed");
                    None
                }
            };

            // Wait for audio params if audio enabled
            let audio_params = match audio_params_rx.await {
                Ok(params) => {
                    if let Some(ref p) = params {
                        tracing::info!(
                            "Received audio params: {:?} {}Hz {}ch",
                            p.codec,
                            p.sample_rate,
                            p.channels
                        );
                    }
                    params
                }
                Err(_) => {
                    tracing::warn!("Audio params channel closed");
                    None
                }
            };

            // Determine output type based on config and audio availability
            let use_av_muxer = audio_enabled && audio_params.is_some();

            // Output handler - either video-only OutputSink or A/V AvMuxer
            enum OutputHandler {
                VideoOnly(Box<dyn OutputSink>),
                AudioVideo(AvMuxer),
            }

            let mut output_handler = match (&output_config, use_av_muxer) {
                (Output::File { path, container }, true) => {
                    // Use AvMuxer for file output with audio
                    let mut muxer = match AvMuxer::new(path, container.ffmpeg_format()) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::error!("Failed to create A/V muxer: {}", e);
                            return;
                        }
                    };

                    // Add video stream
                    if let Some(ref params) = video_params {
                        if let Err(e) = muxer.add_video_stream(params) {
                            tracing::error!("Failed to add video stream: {}", e);
                            return;
                        }
                    }

                    // Add audio stream
                    if let Some(ref params) = audio_params {
                        if let Err(e) = muxer.add_audio_stream(params) {
                            tracing::error!("Failed to add audio stream: {}", e);
                            return;
                        }
                    }

                    // Start muxer (write header)
                    if let Err(e) = muxer.start() {
                        tracing::error!("Failed to start muxer: {}", e);
                        return;
                    }

                    tracing::info!("A/V muxer initialized for {}", path.display());
                    OutputHandler::AudioVideo(muxer)
                }
                _ => {
                    // Use standard OutputSink for video-only or non-file outputs
                    let mut output = match output::create_output(output_config).await {
                        Ok(o) => o,
                        Err(e) => {
                            tracing::error!("Failed to create output: {}", e);
                            return;
                        }
                    };

                    // Initialize with video codec params
                    if let Err(e) = output.init_with_codec(video_params.as_ref()).await {
                        tracing::error!("Failed to init output: {}", e);
                        return;
                    }

                    OutputHandler::VideoOnly(output)
                }
            };

            tracing::info!("Output initialized, entering main loop");

            // Main loop: capture frames, send to encoder, receive packets, write output
            loop {
                tokio::select! {
                    // Check for shutdown
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)), if !running.load(Ordering::SeqCst) => {
                        break;
                    }

                    // Capture next frame
                    frame_result = capture.next_frame() => {
                        match frame_result {
                            Ok(frame) => {
                                {
                                    let mut s = stats.lock().await;
                                    s.frames_captured += 1;
                                }

                                // Send to encoder thread
                                if frame_tx.send(frame).is_err() {
                                    tracing::debug!("Encoder channel closed");
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!("Capture error: {}", e);
                                // Continue trying
                            }
                        }
                    }

                    // Receive encoded video packets
                    Some(packet) = packet_rx.recv() => {
                        {
                            let mut s = stats.lock().await;
                            s.frames_encoded += 1;
                            s.bytes_written += packet.size() as u64;
                        }

                        match &mut output_handler {
                            OutputHandler::VideoOnly(output) => {
                                if let Err(e) = output.write(&packet).await {
                                    tracing::error!("Output error: {}", e);
                                }
                            }
                            OutputHandler::AudioVideo(muxer) => {
                                if let Err(e) = muxer.write_video(&packet) {
                                    tracing::error!("Muxer video write error: {}", e);
                                }
                            }
                        }
                    }

                    // Receive encoded audio packets (only when using A/V muxer)
                    Some(audio_packet) = audio_packet_rx.recv() => {
                        if let OutputHandler::AudioVideo(muxer) = &mut output_handler {
                            if let Err(e) = muxer.write_audio(&audio_packet) {
                                tracing::error!("Muxer audio write error: {}", e);
                            }
                        }
                    }
                }
            }

            // Cleanup
            tracing::info!("Pipeline stopping");
            let _ = capture.stop().await;

            // Drain remaining video packets
            while let Ok(packet) = packet_rx.try_recv() {
                match &mut output_handler {
                    OutputHandler::VideoOnly(output) => {
                        let _ = output.write(&packet).await;
                    }
                    OutputHandler::AudioVideo(muxer) => {
                        let _ = muxer.write_video(&packet);
                    }
                }
            }

            // Drain remaining audio packets
            while let Ok(audio_packet) = audio_packet_rx.try_recv() {
                if let OutputHandler::AudioVideo(muxer) = &mut output_handler {
                    let _ = muxer.write_audio(&audio_packet);
                }
            }

            // Finish output
            match output_handler {
                OutputHandler::VideoOnly(mut output) => {
                    let _ = output.finish().await;
                }
                OutputHandler::AudioVideo(mut muxer) => {
                    let _ = muxer.finish();
                }
            }
        });

        Ok(())
    }

    /// Stop the pipeline
    pub async fn stop(&self) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(false, Ordering::SeqCst);
        tracing::info!("Pipeline stop requested");

        // Give time for cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        Ok(())
    }

    /// Check if pipeline is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get current statistics
    pub async fn stats(&self) -> Stats {
        self.stats.lock().await.clone()
    }

    /// Update encoder configuration (runtime reconfiguration)
    pub async fn reconfigure_encoder(&mut self, config: EncoderConfig) -> Result<()> {
        self.encoder_config = config;
        // Note: Actual reconfiguration would need to communicate with running encoder
        Ok(())
    }
}

/// Builder for pipeline configuration
pub struct PipelineBuilder {
    capture: CaptureConfig,
    encoder: EncoderConfig,
    audio: AudioConfig,
    output: Output,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self {
            capture: CaptureConfig::default(),
            encoder: EncoderConfig::default(),
            audio: AudioConfig::default(),
            output: Output::default(),
        }
    }

    pub fn capture(mut self, config: CaptureConfig) -> Self {
        self.capture = config;
        self
    }

    pub fn encoder(mut self, config: EncoderConfig) -> Self {
        self.encoder = config;
        self
    }

    pub fn output(mut self, output: Output) -> Self {
        self.output = output;
        self
    }

    pub fn preset(mut self, preset: crate::config::Preset) -> Self {
        self.encoder = EncoderConfig::from_preset(preset);
        self
    }

    pub fn codec(mut self, codec: crate::encode::Codec) -> Self {
        self.encoder.codec = codec;
        self
    }

    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.encoder.resolution = Some(Resolution::new(width, height));
        self
    }

    pub fn bitrate(mut self, kbps: u32) -> Self {
        self.encoder.bitrate_kbps = kbps;
        self
    }

    pub fn fps(mut self, fps: u32) -> Self {
        self.capture.framerate = crate::types::Framerate::new(fps, 1);
        self
    }

    /// Enable audio capture with default settings
    pub fn with_audio(mut self) -> Self {
        self.audio.enabled = true;
        self
    }

    /// Configure audio settings
    pub fn audio(mut self, config: AudioConfig) -> Self {
        self.audio = config;
        self
    }

    /// Set audio source
    pub fn audio_source(mut self, source: audio::AudioSource) -> Self {
        self.audio.enabled = true;
        self.audio.source = source;
        self
    }

    /// Set audio codec
    pub fn audio_codec(mut self, codec: audio::AudioCodec) -> Self {
        self.audio.enabled = true;
        self.audio.codec = codec;
        self
    }

    /// Set audio bitrate in kbps
    pub fn audio_bitrate(mut self, kbps: u32) -> Self {
        self.audio.enabled = true;
        self.audio.bitrate = kbps * 1000;
        self
    }

    pub fn build(self) -> Result<Pipeline> {
        Pipeline::new_with_audio(self.capture, self.encoder, self.audio, self.output)
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the audio capture and encoding pipeline
fn run_audio_pipeline(
    config: AudioConfig,
    running: Arc<AtomicBool>,
    packet_tx: tokio::sync::mpsc::Sender<audio::AudioPacket>,
    params_tx: tokio::sync::oneshot::Sender<Option<audio::AudioParams>>,
) -> Result<()> {
    tracing::info!(
        "Audio pipeline starting: {:?} @ {}Hz, {} channels, {}kbps",
        config.codec,
        config.sample_rate,
        config.channels,
        config.bitrate / 1000
    );

    // Create audio capture config
    let capture_config = audio::AudioCaptureConfig {
        source: config.source.clone(),
        sample_rate: config.sample_rate,
        channels: audio::ChannelLayout::Stereo, // Default to stereo
        format: audio::SampleFormat::F32,
        buffer_size: 1024,
    };

    // Create audio encoder config
    let encoder_config = audio::AudioEncoderConfig {
        codec: config.codec,
        sample_rate: config.sample_rate,
        channels: audio::ChannelLayout::Stereo,
        bitrate: config.bitrate,
        input_format: audio::SampleFormat::F32,
    };

    // Create capture and encoder
    let mut capture = audio::PipeWireAudioCapture::new(capture_config)?;
    let mut encoder = audio::FfmpegAudioEncoder::new(encoder_config)?;

    // Initialize encoder
    encoder.init()?;

    // Send audio params for muxer setup
    let audio_params = encoder.params();
    let _ = params_tx.send(audio_params);

    // Start capture (blocking call in this thread context)
    // We need to use a runtime for the async start
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Internal(format!("Failed to create audio runtime: {}", e)))?;

    rt.block_on(async {
        capture.start().await?;
        Ok::<_, Error>(())
    })?;

    tracing::info!("Audio capture started");

    // Process audio frames until shutdown
    while running.load(Ordering::SeqCst) {
        // Get next audio frame (with timeout)
        let frame = rt.block_on(async {
            tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                capture.next_frame(),
            )
            .await
        });

        match frame {
            Ok(Ok(audio_frame)) => {
                // Encode the audio frame
                match encoder.encode(&audio_frame) {
                    Ok(Some(packet)) => {
                        if packet_tx.blocking_send(packet).is_err() {
                            tracing::debug!("Audio packet channel closed");
                            break;
                        }
                    }
                    Ok(None) => {} // Buffered
                    Err(e) => {
                        tracing::error!("Audio encode error: {}", e);
                    }
                }
            }
            Ok(Err(Error::Timeout(_))) => continue,
            Ok(Err(e)) => {
                tracing::error!("Audio capture error: {}", e);
            }
            Err(_) => continue, // Timeout from tokio::time::timeout
        }
    }

    // Flush encoder
    tracing::debug!("Flushing audio encoder");
    if let Ok(packets) = encoder.flush() {
        for packet in packets {
            let _ = packet_tx.blocking_send(packet);
        }
    }

    // Stop capture
    rt.block_on(async {
        let _ = capture.stop().await;
    });

    tracing::info!("Audio pipeline stopped");
    Ok(())
}
