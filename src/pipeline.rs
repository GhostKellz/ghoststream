//! Video processing pipeline
//!
//! Connects capture → process → encode → output

use crate::capture;
use crate::config::{CaptureConfig, EncoderConfig};
use crate::encode;
use crate::error::{Error, Result};
use crate::output::{self, Output};
use crate::processing;
use crate::types::{CodecParams, Frame, FrameFormat, Packet, Resolution, Stats};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Video processing pipeline
pub struct Pipeline {
    capture_config: CaptureConfig,
    encoder_config: EncoderConfig,
    output_config: Output,
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
}

impl Pipeline {
    /// Create a new pipeline
    pub fn new(capture: CaptureConfig, encoder: EncoderConfig, output: Output) -> Result<Self> {
        Ok(Self {
            capture_config: capture,
            encoder_config: encoder,
            output_config: output,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(Stats::default())),
        })
    }

    /// Create pipeline from preset
    pub fn from_preset(preset: crate::config::Preset, output: Output) -> Result<Self> {
        let encoder_config = EncoderConfig::from_preset(preset);
        Self::new(CaptureConfig::default(), encoder_config, output)
    }

    /// Start the pipeline
    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(Error::PipelineAlreadyRunning);
        }

        self.running.store(true, Ordering::SeqCst);
        tracing::info!("Pipeline starting");

        // Clone configs for use in tasks
        let capture_config = self.capture_config.clone();
        let encoder_config = self.encoder_config.clone();
        let output_config = self.output_config.clone();
        let running = self.running.clone();
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

        // Channel for codec params (sent after first frame is encoded)
        let (codec_params_tx, codec_params_rx) = tokio::sync::oneshot::channel::<Option<CodecParams>>();

        // Spawn encoder thread (blocking, non-Send encoder lives here)
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
            // Create capture and output
            let mut capture = match capture::create_capture(capture_config).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create capture: {}", e);
                    return;
                }
            };

            let mut output = match output::create_output(output_config).await {
                Ok(o) => o,
                Err(e) => {
                    tracing::error!("Failed to create output: {}", e);
                    return;
                }
            };

            // Start capture
            if let Err(e) = capture.start().await {
                tracing::error!("Failed to start capture: {}", e);
                return;
            }

            tracing::info!("Capture started, waiting for codec params from encoder");

            // Track if output is initialized (we need codec params from encoder first)
            let mut output_initialized = false;
            let mut codec_params_rx = Some(codec_params_rx);

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

                    // Receive encoded packets
                    Some(packet) = packet_rx.recv() => {
                        // Initialize output with codec params if not done yet
                        if !output_initialized {
                            // Try to get codec params (non-blocking check)
                            if let Some(rx) = codec_params_rx.take() {
                                match rx.await {
                                    Ok(Some(params)) => {
                                        tracing::info!("Received codec params: {:?} {}x{}",
                                            params.codec,
                                            params.resolution.width,
                                            params.resolution.height
                                        );
                                        if let Err(e) = output.init_with_codec(Some(&params)).await {
                                            tracing::error!("Failed to init output with codec params: {}", e);
                                            // Fall back to default init
                                            if let Err(e) = output.init().await {
                                                tracing::error!("Failed to init output: {}", e);
                                                return;
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        tracing::warn!("No codec params available, using default init");
                                        if let Err(e) = output.init().await {
                                            tracing::error!("Failed to init output: {}", e);
                                            return;
                                        }
                                    }
                                    Err(_) => {
                                        tracing::warn!("Codec params channel closed, using default init");
                                        if let Err(e) = output.init().await {
                                            tracing::error!("Failed to init output: {}", e);
                                            return;
                                        }
                                    }
                                }
                            }
                            output_initialized = true;
                            tracing::info!("Output initialized, entering main loop");
                        }

                        {
                            let mut s = stats.lock().await;
                            s.frames_encoded += 1;
                            s.bytes_written += packet.size() as u64;
                        }

                        if let Err(e) = output.write(&packet).await {
                            tracing::error!("Output error: {}", e);
                        }
                    }
                }
            }

            // Cleanup
            tracing::info!("Pipeline stopping");
            let _ = capture.stop().await;

            // Drain remaining packets
            while let Ok(packet) = packet_rx.try_recv() {
                let _ = output.write(&packet).await;
            }

            let _ = output.finish().await;
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
    output: Output,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self {
            capture: CaptureConfig::default(),
            encoder: EncoderConfig::default(),
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

    pub fn build(self) -> Result<Pipeline> {
        Pipeline::new(self.capture, self.encoder, self.output)
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
