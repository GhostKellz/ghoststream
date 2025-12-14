//! GhostStream CLI
//!
//! Command-line interface for testing and using GhostStream.

use clap::{Parser, Subcommand, ValueEnum};
use ghoststream::{
    config::{EncoderConfig, Preset},
    encode::{get_info, Codec, EncoderBackend},
    output::{Container, Output},
    PipelineBuilder,
};

/// Encoder backend for CLI
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum Backend {
    /// Auto-select best available (NVENC > Software)
    #[default]
    Auto,
    /// Force NVIDIA NVENC hardware encoding
    Nvenc,
    /// Force CPU software encoding (x264/x265/SVT-AV1)
    Cpu,
}

impl From<Backend> for EncoderBackend {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Auto => EncoderBackend::Auto,
            Backend::Nvenc => EncoderBackend::Nvenc,
            Backend::Cpu => EncoderBackend::Software,
        }
    }
}

#[derive(Parser)]
#[command(name = "ghoststream")]
#[command(about = "NVIDIA GPU Video Engine - Capture, Encode, Stream")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show system information and encoder capabilities
    Info,

    /// Start screen capture and encoding
    Capture {
        /// Output file path (or "camera" for virtual camera)
        #[arg(short, long, default_value = "camera")]
        output: String,

        /// Video codec (h264, hevc, av1)
        #[arg(short, long, default_value = "h264")]
        codec: String,

        /// Bitrate in kbps
        #[arg(short, long, default_value = "6000")]
        bitrate: u32,

        /// Resolution (e.g., 1920x1080)
        #[arg(short, long)]
        resolution: Option<String>,

        /// Framerate
        #[arg(short, long, default_value = "60")]
        fps: u32,

        /// Use preset instead of manual settings
        #[arg(short, long)]
        preset: Option<String>,

        /// Encoder backend (auto, nvenc, cpu)
        #[arg(short, long, value_enum, default_value = "auto")]
        encoder: Backend,
    },

    /// Run encoder benchmark
    Bench {
        /// Codec to benchmark
        #[arg(short, long, default_value = "h264")]
        codec: String,

        /// Number of frames to encode
        #[arg(short, long, default_value = "300")]
        frames: u32,

        /// Encoder backend (auto, nvenc, cpu)
        #[arg(short, long, value_enum, default_value = "auto")]
        encoder: Backend,
    },

    /// List available presets
    Presets,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("ghoststream=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Info => cmd_info(),
        Commands::Capture {
            output,
            codec,
            bitrate,
            resolution,
            fps,
            preset,
            encoder,
        } => cmd_capture(output, codec, bitrate, resolution, fps, preset, encoder).await,
        Commands::Bench { codec, frames, encoder } => cmd_bench(codec, frames, encoder).await,
        Commands::Presets => cmd_presets(),
    }
}

fn cmd_info() -> anyhow::Result<()> {
    println!("GhostStream System Information");
    println!("==============================\n");

    let info = get_info();

    // GPU / NVENC Info
    println!("=== NVIDIA NVENC ===");
    println!(
        "Available: {}",
        if info.nvenc_available { "Yes" } else { "No" }
    );

    if let Some(gpu) = &info.gpu_name {
        println!("GPU: {}", gpu);
    }

    if let Some(driver) = &info.driver_version {
        println!("Driver: {}", driver);
    }

    if info.nvenc_available {
        println!("Codecs:");
        for codec in &info.nvenc_codecs {
            println!("  - {} ({})", codec.display_name(), codec.min_gpu_arch());
        }
        println!(
            "Dual Encoder: {}",
            if info.dual_encoder { "Yes" } else { "No" }
        );
    }

    // CPU / Software Encoder Info
    println!("\n=== CPU Software Encoders ===");
    if let Some(cpu) = &info.cpu {
        if let Some(model) = &cpu.model {
            println!("CPU: {}", model);
        }
        println!("Cores: {}", cpu.cores);
        if cpu.is_amd {
            println!("Type: AMD (optimized)");
        }
    }

    println!("Codecs:");
    println!(
        "  - H.264 (x264): {}",
        if info.software.x264 { "Yes" } else { "No" }
    );
    println!(
        "  - H.265 (x265): {}",
        if info.software.x265 { "Yes" } else { "No" }
    );
    println!(
        "  - AV1 (SVT-AV1): {}",
        if info.software.svtav1 { "Yes" } else { "No" }
    );

    // Summary
    println!("\n=== Summary ===");
    println!("All supported codecs: {:?}", info.supported_codecs());
    println!(
        "AV1 Support: {} ({})",
        if info.av1_support() { "Yes" } else { "No" },
        if info.nvenc_av1 && info.software.svtav1 {
            "NVENC + SVT-AV1"
        } else if info.nvenc_av1 {
            "NVENC"
        } else if info.software.svtav1 {
            "SVT-AV1"
        } else {
            "None"
        }
    );

    Ok(())
}

async fn cmd_capture(
    output: String,
    codec: String,
    bitrate: u32,
    resolution: Option<String>,
    fps: u32,
    preset: Option<String>,
    backend: Backend,
) -> anyhow::Result<()> {
    println!("Starting capture...\n");
    let _encoder_backend: EncoderBackend = backend.into();

    // Parse codec
    let codec = match codec.to_lowercase().as_str() {
        "h264" | "avc" => Codec::H264,
        "h265" | "hevc" => Codec::Hevc,
        "av1" => Codec::Av1,
        _ => {
            eprintln!("Unknown codec: {}. Using H.264.", codec);
            Codec::H264
        }
    };

    // Build pipeline
    let mut builder = PipelineBuilder::new()
        .codec(codec)
        .bitrate(bitrate)
        .fps(fps);

    // Apply preset if specified
    if let Some(preset_name) = preset {
        let preset = match preset_name.to_lowercase().as_str() {
            "discord" | "discord720p" => Preset::Discord720p,
            "stream" | "stream1080p60" => Preset::Stream1080p60,
            "quality" | "quality1440p60" => Preset::Quality1440p60,
            "gaming" | "gaming1440p120" => Preset::Gaming1440p120,
            "4k" | "ultra4k60" => Preset::Ultra4K60,
            "max" | "maximum4k120" => Preset::Maximum4K120,
            "lowlatency" => Preset::LowLatency,
            "recording" => Preset::Recording,
            _ => {
                eprintln!(
                    "Unknown preset: {}. Use 'ghoststream presets' to see available.",
                    preset_name
                );
                return Ok(());
            }
        };
        builder = builder.preset(preset);
    }

    // Parse resolution
    if let Some(res) = resolution {
        let parts: Vec<&str> = res.split('x').collect();
        if parts.len() == 2 {
            if let (Ok(w), Ok(h)) = (parts[0].parse(), parts[1].parse()) {
                builder = builder.resolution(w, h);
            }
        }
    }

    // Set output
    let output = if output == "camera" {
        Output::virtual_camera("GhostStream Camera")
    } else if output.ends_with(".mkv") {
        Output::file(&output, Container::Matroska)
    } else if output.ends_with(".mp4") {
        Output::file(&output, Container::Mp4)
    } else if output.ends_with(".webm") {
        Output::file(&output, Container::WebM)
    } else if output.starts_with("rtmp://") {
        Output::rtmp(&output)
    } else {
        Output::file(&output, Container::Matroska)
    };

    builder = builder.output(output);

    let pipeline = builder.build()?;

    println!("Configuration:");
    println!("  Codec: {}", codec);
    println!("  Bitrate: {} kbps", bitrate);
    println!("  FPS: {}", fps);
    println!();

    // Start pipeline
    pipeline.start().await?;

    println!("Capture started. Press Ctrl+C to stop.\n");

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;

    println!("\nStopping...");
    pipeline.stop().await?;

    let stats = pipeline.stats().await;
    println!("\nStatistics:");
    println!("  Frames captured: {}", stats.frames_captured);
    println!("  Frames encoded: {}", stats.frames_encoded);
    println!("  Bytes written: {}", stats.bytes_written);

    Ok(())
}

async fn cmd_bench(codec: String, frames: u32, backend: Backend) -> anyhow::Result<()> {
    println!("GhostStream Encoder Benchmark");
    println!("=============================\n");

    let codec = match codec.to_lowercase().as_str() {
        "h264" | "avc" => Codec::H264,
        "h265" | "hevc" => Codec::Hevc,
        "av1" => Codec::Av1,
        _ => Codec::H264,
    };

    let encoder_backend: EncoderBackend = backend.into();

    let backend_name = match encoder_backend {
        EncoderBackend::Auto => "Auto",
        EncoderBackend::Nvenc => "NVENC",
        EncoderBackend::Software => "Software (CPU)",
    };

    println!("Codec: {}", codec);
    println!("Backend: {}", backend_name);
    println!("Frames: {}", frames);
    println!("Resolution: 1920x1080");
    println!();

    println!("Running benchmark...\n");

    let config = EncoderConfig::default()
        .with_codec(codec)
        .with_resolution(1920, 1080)
        .with_bitrate_kbps(10000);

    let mut encoder = ghoststream::encode::create_encoder_with_backend(config, encoder_backend)?;
    encoder.init()?;

    let start = std::time::Instant::now();

    for i in 0..frames {
        let mut f =
            ghoststream::types::Frame::new(1920, 1080, ghoststream::types::FrameFormat::Nv12);
        f.pts = i as i64 * 16667; // ~60fps
        let _ = encoder.encode(&f)?;
    }

    let _ = encoder.flush()?;
    let elapsed = start.elapsed();

    let fps = frames as f64 / elapsed.as_secs_f64();
    let ms_per_frame = elapsed.as_millis() as f64 / frames as f64;

    println!("Results:");
    println!("  Total time: {:.2}s", elapsed.as_secs_f64());
    println!("  Encoding FPS: {:.1}", fps);
    println!("  ms/frame: {:.2}", ms_per_frame);
    println!(
        "  Realtime capable (60fps): {}",
        if fps >= 60.0 { "Yes" } else { "No" }
    );
    println!(
        "  Realtime capable (120fps): {}",
        if fps >= 120.0 { "Yes" } else { "No" }
    );

    let stats = encoder.stats();
    println!("\nEncoder Stats:");
    println!("  Frames encoded: {}", stats.frames_encoded);
    println!("  Bytes output: {}", stats.bytes_output);
    println!(
        "  Avg bitrate: {:.0} kbps",
        stats.bytes_output as f64 * 8.0 / elapsed.as_secs_f64() / 1000.0
    );

    Ok(())
}

fn cmd_presets() -> anyhow::Result<()> {
    println!("Available Presets");
    println!("=================\n");

    let presets = [
        (
            "discord",
            "Discord720p",
            "720p30, H.264, 3 Mbps, low latency",
        ),
        (
            "stream",
            "Stream1080p60",
            "1080p60, H.264, 6 Mbps, balanced",
        ),
        (
            "quality",
            "Quality1440p60",
            "1440p60, HEVC, 12 Mbps, high quality",
        ),
        (
            "gaming",
            "Gaming1440p120",
            "1440p120, HEVC, 15 Mbps, low latency",
        ),
        ("4k", "Ultra4K60", "4K60, AV1, 25 Mbps, high quality"),
        ("max", "Maximum4K120", "4K120, AV1, 35 Mbps, RTX 40/50"),
        (
            "lowlatency",
            "LowLatency",
            "Native res, H.264, 8 Mbps, ultra low latency",
        ),
        (
            "recording",
            "Recording",
            "Native res, HEVC, 50 Mbps, max quality",
        ),
    ];

    for (name, full_name, description) in presets {
        println!("  {:<12} ({}) - {}", name, full_name, description);
    }

    println!("\nUsage: ghoststream capture --preset <name>");

    Ok(())
}
