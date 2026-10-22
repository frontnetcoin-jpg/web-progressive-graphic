use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use wpg::{ColorSubsampling, EncodeOptions, WpgEncoder, WpgMode};

#[derive(Parser, Debug)]
#[command(name = "wpg-enc", about = "WPG (Web Progressive Graphic) Encoder")]
struct Args {
    /// Input image path (.png, .jpg, .jpeg, .webp)
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output .wpg file path
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Compression quality (1..100)
    #[arg(short, long, default_value_t = 80)]
    quality: u8,

    /// Encoding speed preset (1..10, 10=fastest, 1=slowest/highest compression)
    #[arg(short, long, default_value_t = 6)]
    speed: u8,

    /// Mode: auto, av1, palette, modular
    #[arg(short, long, default_value = "auto")]
    mode: String,

    /// Chroma subsampling: 420, 422, 444
    #[arg(long, default_value = "420")]
    subsampling: String,

    /// Film grain scale (0 = off, 1..255, omit for auto-detection)
    #[arg(short, long)]
    grain: Option<u8>,

    /// Disable embedded 16-byte micro-preview
    #[arg(long)]
    no_preview: bool,

    /// Disable secondary entropy super-compression
    #[arg(long)]
    no_zstd: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("WPG Encoder v0.1.0");
    println!("Reading input image: {}", args.input.display());

    let img = wpg::open_image_with_orientation(&args.input)?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();

    let mode = match args.mode.to_lowercase().as_str() {
        "av1" => Some(WpgMode::Av1Enhanced),
        "palette" => Some(WpgMode::PaletteLossless),
        "modular" => Some(WpgMode::ModularLossless),
        _ => None, // Auto
    };

    let subsampling = match args.subsampling.as_str() {
        "422" => ColorSubsampling::Yuv422,
        "444" => ColorSubsampling::Yuv444,
        _ => ColorSubsampling::Yuv420,
    };

    let mut opts = EncodeOptions::default();
    opts.quality = args.quality;
    opts.speed = args.speed;
    opts.mode = mode;
    opts.subsampling = subsampling;
    opts.enable_preview = !args.no_preview;
    opts.enable_grain = args.grain != Some(0);
    opts.grain_scale = args.grain;
    opts.secondary_entropy = !args.no_zstd;

    println!("Encoding {}x{} pixels (Quality: {}, Speed: {})...", width, height, opts.quality, opts.speed);

    let t0 = Instant::now();
    let encoded = WpgEncoder::encode(rgba.as_raw(), width, height, &opts)?;
    let elapsed = t0.elapsed();

    fs::write(&args.output, &encoded)?;

    let orig_raw_size = (width * height * 4) as f64;
    let comp_size = encoded.len() as f64;
    let bpp = (comp_size * 8.0) / (width * height) as f64;
    let ratio = orig_raw_size / comp_size;

    println!("\nEncoded successfully in {:.2} ms", elapsed.as_secs_f64() * 1000.0);
    println!("Output: {}", args.output.display());
    println!("File Size: {} bytes ({:.2} KB)", encoded.len(), comp_size / 1024.0);
    println!("Bits Per Pixel (bpp): {:.4}", bpp);
    println!("Compression Ratio: {:.2}:1 ({:.1}% reduction from raw RGBA)", ratio, (1.0 - comp_size / orig_raw_size) * 100.0);

    Ok(())
}
