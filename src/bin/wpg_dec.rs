use clap::Parser;
use image::{ImageBuffer, Rgba};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use wpg::WpgDecoder;

#[derive(Parser, Debug)]
#[command(name = "wpg-dec", about = "WPG (Web Progressive Graphic) Decoder")]
struct Args {
    /// Input .wpg file path
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output image path (.png, .jpg, .webp)
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Only extract and render the 16-byte micro-preview
    #[arg(long)]
    preview_only: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("WPG Decoder v0.1.0");
    println!("Reading input file: {}", args.input.display());

    let bytes = fs::read(&args.input)?;

    if args.preview_only {
        println!("Extracting embedded 16-byte micro-preview...");
        let t0 = Instant::now();
        let (w, h, preview_rgb) = WpgDecoder::decode_preview(&bytes)?;
        let elapsed = t0.elapsed();

        println!("Micro-preview extracted in {:.3} ms! Dimensions: {}x{}", elapsed.as_secs_f64() * 1000.0, w, h);

        // Render 4x4 preview to 64x64 PNG for visual inspection
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let idx = (y * 4 + x) * 3;
                img.put_pixel(x as u32, y as u32, Rgba([preview_rgb[idx], preview_rgb[idx + 1], preview_rgb[idx + 2], 255]));
            }
        }
        img.save(&args.output)?;
        println!("Saved micro-preview to: {}", args.output.display());
        return Ok(());
    }

    let t0 = Instant::now();
    let decoded = WpgDecoder::decode(&bytes)?;
    let elapsed = t0.elapsed();

    println!("\nDecoded successfully in {:.2} ms!", elapsed.as_secs_f64() * 1000.0);
    println!("Dimensions: {}x{}", decoded.width, decoded.height);
    println!("Mode: {:?}", decoded.header.mode);
    println!("Subsampling: {:?}", decoded.header.subsampling);
    println!("Has Micro-Preview: {}", decoded.preview.is_some());
    println!("Has Film Grain: {}", decoded.grain.is_some());

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(decoded.width, decoded.height, decoded.rgba)
        .ok_or("Failed to construct image buffer from decoded RGBA")?;

    img.save(&args.output)?;
    println!("Saved decoded image to: {}", args.output.display());

    Ok(())
}
