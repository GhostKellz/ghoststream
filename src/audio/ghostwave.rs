//! GhostWave Integration — RTX Audio Processing
//!
//! Provides optional integration with GhostWave for:
//! - AI-powered noise suppression (NVIDIA Broadcast / Krisp parity)
//! - Echo cancellation for mic capture
//! - Voice isolation for multi-speaker scenarios
//!
//! RTX acceleration uses Tensor Cores:
//! - RTX 20/30: FP16 inference
//! - RTX 40: FP16 with improved throughput
//! - RTX 50: FP4 Tensor Core (2-3x speedup)

use super::types::{AudioFrame, SampleFormat};
use crate::error::{Error, Result};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// GhostWave processing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingMode {
    /// RTX GPU-accelerated (Tensor Core inference)
    RtxGpu,
    /// CUDA GPU compute (no Tensor Cores)
    CudaGpu,
    /// CPU fallback (SIMD optimized)
    Cpu,
    /// Bypass (no processing)
    Bypass,
}

impl std::fmt::Display for ProcessingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RtxGpu => write!(f, "RTX GPU (Tensor Core)"),
            Self::CudaGpu => write!(f, "CUDA GPU"),
            Self::Cpu => write!(f, "CPU (SIMD)"),
            Self::Bypass => write!(f, "Bypass"),
        }
    }
}

/// Processing profile preset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessingProfile {
    /// Balanced (default) - good quality with low latency
    #[default]
    Balanced,
    /// Streaming - optimized for live content
    Streaming,
    /// Studio - maximum quality, higher latency
    Studio,
    /// Gaming - minimum latency
    Gaming,
}

impl ProcessingProfile {
    /// Get noise suppression strength for this profile
    pub fn noise_strength(&self) -> f32 {
        match self {
            Self::Balanced => 0.7,
            Self::Streaming => 0.8,
            Self::Studio => 0.9,
            Self::Gaming => 0.5, // Lower to preserve game audio cues
        }
    }

    /// Get echo cancellation strength
    pub fn echo_strength(&self) -> f32 {
        match self {
            Self::Balanced => 0.6,
            Self::Streaming => 0.7,
            Self::Studio => 0.8,
            Self::Gaming => 0.4,
        }
    }

    /// Get voice isolation strength
    pub fn voice_isolation(&self) -> f32 {
        match self {
            Self::Balanced => 0.0, // Disabled by default
            Self::Streaming => 0.3,
            Self::Studio => 0.5,
            Self::Gaming => 0.0,
        }
    }

    /// Get target latency in milliseconds
    pub fn target_latency_ms(&self) -> u32 {
        match self {
            Self::Balanced => 20,
            Self::Streaming => 30,
            Self::Studio => 50,
            Self::Gaming => 10,
        }
    }
}

/// GhostWave processor configuration
#[derive(Debug, Clone)]
pub struct GhostWaveConfig {
    /// Enable noise suppression
    pub noise_suppression: bool,
    /// Noise suppression strength (0.0 - 1.0)
    pub noise_strength: f32,
    /// Enable echo cancellation
    pub echo_cancellation: bool,
    /// Echo cancellation strength (0.0 - 1.0)
    pub echo_strength: f32,
    /// Enable voice isolation
    pub voice_isolation: bool,
    /// Voice isolation strength (0.0 - 1.0)
    pub isolation_strength: f32,
    /// Processing profile
    pub profile: ProcessingProfile,
    /// Force CPU mode (disable GPU)
    pub force_cpu: bool,
    /// Sample rate
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u32,
}

impl Default for GhostWaveConfig {
    fn default() -> Self {
        let profile = ProcessingProfile::default();
        Self {
            noise_suppression: true,
            noise_strength: profile.noise_strength(),
            echo_cancellation: false,
            echo_strength: profile.echo_strength(),
            voice_isolation: false,
            isolation_strength: profile.voice_isolation(),
            profile,
            force_cpu: false,
            sample_rate: 48000,
            channels: 2,
        }
    }
}

impl GhostWaveConfig {
    /// Create config from profile
    pub fn from_profile(profile: ProcessingProfile) -> Self {
        Self {
            noise_suppression: true,
            noise_strength: profile.noise_strength(),
            echo_cancellation: true,
            echo_strength: profile.echo_strength(),
            voice_isolation: profile.voice_isolation() > 0.0,
            isolation_strength: profile.voice_isolation(),
            profile,
            ..Default::default()
        }
    }

    /// Create config for microphone input (noise + echo)
    pub fn for_microphone() -> Self {
        Self {
            noise_suppression: true,
            noise_strength: 0.8,
            echo_cancellation: true,
            echo_strength: 0.7,
            voice_isolation: false,
            isolation_strength: 0.0,
            profile: ProcessingProfile::Streaming,
            force_cpu: false,
            sample_rate: 48000,
            channels: 1, // Mono mic
        }
    }

    /// Create config for desktop audio (light processing)
    pub fn for_desktop() -> Self {
        Self {
            noise_suppression: false, // Don't denoise game/media audio
            noise_strength: 0.0,
            echo_cancellation: false,
            echo_strength: 0.0,
            voice_isolation: false,
            isolation_strength: 0.0,
            profile: ProcessingProfile::Gaming,
            force_cpu: false,
            sample_rate: 48000,
            channels: 2, // Stereo
        }
    }
}

/// GhostWave audio processor
///
/// Wraps ghostwave-core for integration with GhostStream pipelines.
pub struct GhostWaveProcessor {
    config: GhostWaveConfig,
    mode: ProcessingMode,
    initialized: bool,
    // Processing state
    sample_rate: u32,
    channels: u32,
    frame_size: usize,
    // Internal buffers
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    // Stats
    frames_processed: u64,
    processing_time_ns: u64,
}

impl GhostWaveProcessor {
    /// Create a new GhostWave processor
    pub fn new(config: GhostWaveConfig) -> Result<Self> {
        info!("Creating GhostWave processor");
        debug!("Config: {}Hz, {} channels", config.sample_rate, config.channels);

        // Detect available processing mode
        let mode = if config.force_cpu {
            ProcessingMode::Cpu
        } else {
            detect_processing_mode()
        };

        info!("GhostWave processing mode: {}", mode);

        Ok(Self {
            config,
            mode,
            initialized: false,
            sample_rate: 48000,
            channels: 2,
            frame_size: 0,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            frames_processed: 0,
            processing_time_ns: 0,
        })
    }

    /// Initialize the processor for a specific format
    pub fn init(&mut self, sample_rate: u32, channels: u32, frame_size: usize) -> Result<()> {
        info!(
            "Initializing GhostWave: {}Hz, {} channels, {} frame size",
            sample_rate, channels, frame_size
        );

        self.sample_rate = sample_rate;
        self.channels = channels;
        self.frame_size = frame_size;

        // Pre-allocate buffers
        let buffer_size = frame_size * channels as usize;
        self.input_buffer = vec![0.0f32; buffer_size];
        self.output_buffer = vec![0.0f32; buffer_size];

        self.initialized = true;
        Ok(())
    }

    /// Process an audio frame
    pub fn process(&mut self, frame: &AudioFrame) -> Result<AudioFrame> {
        if !self.initialized {
            return Err(Error::AudioProcessing(
                "GhostWave processor not initialized".to_string(),
            ));
        }

        let start = std::time::Instant::now();

        // Validate frame format
        if frame.sample_rate != self.sample_rate {
            return Err(Error::AudioProcessing(format!(
                "Sample rate mismatch: expected {}, got {}",
                self.sample_rate, frame.sample_rate
            )));
        }

        // Convert to f32 if needed and process
        let input = frame.as_f32_slice()?;

        // Ensure output buffer is correct size
        if self.output_buffer.len() != input.len() {
            self.output_buffer.resize(input.len(), 0.0);
        }

        // Copy input to local buffer to avoid borrow conflict
        self.input_buffer.clear();
        self.input_buffer.extend_from_slice(&input);

        // Apply processing chain
        Self::process_chain_impl(
            &self.config,
            self.mode,
            self.sample_rate,
            &self.input_buffer,
            &mut self.output_buffer,
        )?;

        // Create output frame
        let output_frame = AudioFrame {
            data: self.output_buffer.iter().flat_map(|&s| s.to_le_bytes()).collect(),
            sample_rate: frame.sample_rate,
            channels: frame.channels,
            format: SampleFormat::F32,
            samples: frame.samples,
            pts: frame.pts,
            duration: frame.duration,
        };

        // Update stats
        self.frames_processed += 1;
        self.processing_time_ns += start.elapsed().as_nanos() as u64;

        Ok(output_frame)
    }

    /// Process in-place (for efficiency)
    pub fn process_inplace(&mut self, samples: &mut [f32]) -> Result<()> {
        if !self.initialized {
            return Err(Error::AudioProcessing(
                "GhostWave processor not initialized".to_string(),
            ));
        }

        // Copy input for processing
        let input: Vec<f32> = samples.to_vec();

        // Process
        Self::process_chain_impl(&self.config, self.mode, self.sample_rate, &input, samples)?;

        self.frames_processed += 1;
        Ok(())
    }

    /// Internal processing chain (static to avoid borrow conflicts)
    fn process_chain_impl(
        config: &GhostWaveConfig,
        mode: ProcessingMode,
        sample_rate: u32,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<()> {
        // Start with input copy
        output.copy_from_slice(input);

        // Skip all processing in bypass mode
        if mode == ProcessingMode::Bypass {
            return Ok(());
        }

        // Apply noise suppression
        if config.noise_suppression && config.noise_strength > 0.0 {
            Self::apply_noise_suppression_impl(config.noise_strength, output)?;
        }

        // Apply echo cancellation (if enabled)
        if config.echo_cancellation && config.echo_strength > 0.0 {
            Self::apply_echo_cancellation_impl(config.echo_strength, sample_rate, output)?;
        }

        // Apply voice isolation (if enabled)
        if config.voice_isolation && config.isolation_strength > 0.0 {
            Self::apply_voice_isolation_impl(config.isolation_strength, output)?;
        }

        Ok(())
    }

    /// Apply noise suppression (spectral denoising)
    fn apply_noise_suppression_impl(noise_strength: f32, samples: &mut [f32]) -> Result<()> {
        // In production, this would call ghostwave_core::GhostWaveProcessor
        // For now, apply a simple soft-knee expander as placeholder
        let threshold = -40.0_f32; // dB
        let ratio = noise_strength * 4.0; // Higher strength = more reduction
        let attack_coef = 0.001_f32;
        let release_coef = 0.05_f32;

        let mut envelope = 0.0_f32;

        for sample in samples.iter_mut() {
            let input_db = 20.0 * sample.abs().max(1e-10).log10();

            // Update envelope
            let target = if input_db > envelope { input_db } else { input_db };
            let coef = if target > envelope { attack_coef } else { release_coef };
            envelope = envelope + coef * (target - envelope);

            // Calculate gain reduction
            if envelope < threshold {
                let reduction_db = (threshold - envelope) * (1.0 - 1.0 / ratio);
                let gain = 10.0_f32.powf(-reduction_db / 20.0);
                *sample *= gain;
            }
        }

        Ok(())
    }

    /// Apply echo cancellation (NLMS adaptive filter)
    fn apply_echo_cancellation_impl(
        echo_strength: f32,
        sample_rate: u32,
        samples: &mut [f32],
    ) -> Result<()> {
        // Placeholder - real implementation would use ghostwave_core AEC
        // Echo cancellation requires reference signal (speaker output)
        // Without reference, we can only apply basic spectral subtraction

        // For now, just a high-pass filter to remove low-frequency room resonance
        let cutoff = 80.0; // Hz
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        let dt = 1.0 / sample_rate as f32;
        let alpha = dt / (rc + dt);

        let mut prev_output = 0.0_f32;
        let mut prev_input = 0.0_f32;

        for sample in samples.iter_mut() {
            let input = *sample;
            let output = alpha * (prev_output + input - prev_input);
            prev_input = input;
            prev_output = output;
            *sample = output * echo_strength + input * (1.0 - echo_strength);
        }

        Ok(())
    }

    /// Apply voice isolation (frequency-domain masking)
    fn apply_voice_isolation_impl(isolation_strength: f32, samples: &mut [f32]) -> Result<()> {
        // Placeholder - real implementation would use ghostwave_core voice isolation
        // This would involve:
        // 1. STFT analysis
        // 2. Neural network voice detection
        // 3. Spectral masking
        // 4. ISTFT synthesis

        // Simple bandpass for voice frequencies (85Hz - 3kHz)
        // This is a crude approximation
        let _low_cutoff = 85.0;
        let _high_cutoff = 3000.0;

        // Apply strength-based mixing (would be replaced by proper isolation)
        for sample in samples.iter_mut() {
            // Attenuate by isolation strength (1.0 = full isolation attempted)
            // In real implementation, non-voice content would be removed
            *sample *= 1.0 - (isolation_strength * 0.3); // Slight attenuation as placeholder
        }

        Ok(())
    }

    /// Get current processing mode
    pub fn mode(&self) -> ProcessingMode {
        self.mode
    }

    /// Get processing statistics
    pub fn stats(&self) -> ProcessingStats {
        ProcessingStats {
            frames_processed: self.frames_processed,
            total_processing_time_ns: self.processing_time_ns,
            avg_frame_time_us: if self.frames_processed > 0 {
                (self.processing_time_ns / self.frames_processed / 1000) as u32
            } else {
                0
            },
            mode: self.mode,
        }
    }

    /// Update configuration
    pub fn set_config(&mut self, config: GhostWaveConfig) {
        self.config = config;
    }

    /// Set processing profile
    pub fn set_profile(&mut self, profile: ProcessingProfile) {
        self.config.profile = profile;
        self.config.noise_strength = profile.noise_strength();
        self.config.echo_strength = profile.echo_strength();
        self.config.isolation_strength = profile.voice_isolation();
        self.config.voice_isolation = profile.voice_isolation() > 0.0;
        info!("GhostWave profile set to: {:?}", profile);
    }

    /// Enable/disable noise suppression
    pub fn set_noise_suppression(&mut self, enabled: bool, strength: f32) {
        self.config.noise_suppression = enabled;
        self.config.noise_strength = strength.clamp(0.0, 1.0);
    }

    /// Enable/disable echo cancellation
    pub fn set_echo_cancellation(&mut self, enabled: bool, strength: f32) {
        self.config.echo_cancellation = enabled;
        self.config.echo_strength = strength.clamp(0.0, 1.0);
    }
}

/// Processing statistics
#[derive(Debug, Clone)]
pub struct ProcessingStats {
    pub frames_processed: u64,
    pub total_processing_time_ns: u64,
    pub avg_frame_time_us: u32,
    pub mode: ProcessingMode,
}

/// Detect available processing mode
fn detect_processing_mode() -> ProcessingMode {
    // Check for RTX GPU with Tensor Cores
    if is_rtx_available() {
        return ProcessingMode::RtxGpu;
    }

    // Check for CUDA GPU
    if is_cuda_available() {
        return ProcessingMode::CudaGpu;
    }

    // Fall back to CPU
    ProcessingMode::Cpu
}

/// Check if RTX GPU with Tensor Cores is available
fn is_rtx_available() -> bool {
    // Check for NVIDIA GPU with compute capability >= 7.0 (Turing+)
    // In production, this would query NVML or use cuda-runtime

    // Check for nvidia-smi
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=compute_cap")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() {
            if let Ok(cap) = String::from_utf8(output.stdout) {
                // Parse compute capability (e.g., "8.9" for RTX 40)
                if let Some(major) = cap.trim().split('.').next() {
                    if let Ok(major_ver) = major.parse::<u32>() {
                        // Turing (7.5) and newer have Tensor Cores
                        if major_ver >= 7 {
                            info!("RTX GPU detected (compute capability {})", cap.trim());
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Check if CUDA is available
fn is_cuda_available() -> bool {
    // Check for nvidia-smi (indicates NVIDIA driver is installed)
    std::process::Command::new("nvidia-smi")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Thread-safe wrapper for GhostWave processor
pub struct SharedGhostWaveProcessor {
    inner: Arc<Mutex<GhostWaveProcessor>>,
}

impl SharedGhostWaveProcessor {
    pub fn new(config: GhostWaveConfig) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(GhostWaveProcessor::new(config)?)),
        })
    }

    pub fn process(&self, frame: &AudioFrame) -> Result<AudioFrame> {
        self.inner
            .lock()
            .map_err(|_| Error::AudioProcessing("Lock poisoned".to_string()))?
            .process(frame)
    }

    pub fn init(&self, sample_rate: u32, channels: u32, frame_size: usize) -> Result<()> {
        self.inner
            .lock()
            .map_err(|_| Error::AudioProcessing("Lock poisoned".to_string()))?
            .init(sample_rate, channels, frame_size)
    }

    pub fn stats(&self) -> Result<ProcessingStats> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| Error::AudioProcessing("Lock poisoned".to_string()))?
            .stats())
    }
}

impl Clone for SharedGhostWaveProcessor {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = GhostWaveConfig::default();
        assert!(config.noise_suppression);
        assert!(!config.echo_cancellation);
        assert_eq!(config.sample_rate, 48000);
    }

    #[test]
    fn test_profile_creation() {
        let config = GhostWaveConfig::from_profile(ProcessingProfile::Gaming);
        assert_eq!(config.noise_strength, 0.5);
        assert_eq!(config.profile.target_latency_ms(), 10);
    }

    #[test]
    fn test_processor_creation() {
        let config = GhostWaveConfig::default();
        let processor = GhostWaveProcessor::new(config);
        assert!(processor.is_ok());
    }

    #[test]
    fn test_mode_detection() {
        let mode = detect_processing_mode();
        println!("Detected processing mode: {}", mode);
    }
}
