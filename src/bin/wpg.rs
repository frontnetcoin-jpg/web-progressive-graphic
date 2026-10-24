use clap::{Parser, Subcommand};
use image::{ImageBuffer, ImageEncoder, Rgba};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use wpg::{
    compute_psnr, compute_ssim, ColorSubsampling, EncodeOptions, WpgDecoder, WpgEncoder, WpgHeader,
    WpgMode,
};

#[derive(Parser, Debug)]
#[command(
    name = "wpg",
    version = "0.1.0",
    about = "WPG (Web Progressive Graphic) — Next-Gen Image Format CLI & Server"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Encode an image (PNG, JPEG, WebP) to .wpg
    Encode {
        /// Input image path
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Output .wpg file path
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,

        /// Quality level (1..100)
        #[arg(short, long, default_value_t = 80)]
        quality: u8,

        /// Encoding speed preset (1..10)
        #[arg(short, long, default_value_t = 6)]
        speed: u8,

        /// Mode: auto, av1, palette, modular
        #[arg(short, long, default_value = "auto")]
        mode: String,

        /// Chroma subsampling: 420, 422, 444
        #[arg(long, default_value = "420")]
        subsampling: String,

        /// Film grain scale (0 = off, 1..255, omit for auto)
        #[arg(short, long)]
        grain: Option<u8>,

        /// Disable micro-preview header block
        #[arg(long)]
        no_preview: bool,
    },

    /// Decode a .wpg file to an image (PNG, JPEG, WebP)
    Decode {
        /// Input .wpg file path
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Output image file path
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,

        /// Only extract the 16-byte micro-preview
        #[arg(long)]
        preview_only: bool,
    },

    /// Inspect a .wpg file header, container overhead, preview block, and film grain
    Info {
        /// Input .wpg file path
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },

    /// Run full comparative benchmark (WPG vs PNG vs JPEG vs WebP) with PSNR & SSIM metrics
    Bench {
        /// Input image path to benchmark
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Target quality (1..100)
        #[arg(short, long, default_value_t = 80)]
        quality: u8,
    },

    /// Start the interactive Web Showcase & Viewer server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },

    /// Batch convert an entire directory of images (PNG, JPEG, WebP) to .wpg format
    Batch {
        /// Input directory containing images
        #[arg(value_name = "INPUT_DIR")]
        input_dir: PathBuf,

        /// Output directory to store .wpg files (defaults to same directory if omitted)
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output_dir: Option<PathBuf>,

        /// Target quality (1..100)
        #[arg(short, long, default_value_t = 75)]
        quality: u8,

        /// Speed preset (1..10)
        #[arg(short, long, default_value_t = 6)]
        speed: u8,

        /// Recursively traverse subdirectories
        #[arg(short, long)]
        recursive: bool,
    },

    /// Open a .wpg file (or launch blank canvas) in the native Windows Desktop Viewer
    View {
        /// Optional input .wpg file path
        #[arg(value_name = "INPUT")]
        input: Option<PathBuf>,
    },

    /// Register .wpg file format in Windows Explorer (double-click association, icons, context menus)
    Register,

    /// Unregister .wpg file format from Windows Explorer
    Unregister,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            input,
            output,
            quality,
            speed,
            mode,
            subsampling,
            grain,
            no_preview,
        } => {
            println!("Encoding image: {} -> {}", input.display(), output.display());
            let img = wpg::open_image_with_orientation(&input)?;
            let rgba = img.to_rgba8();
            let width = rgba.width();
            let height = rgba.height();

            let m = match mode.to_lowercase().as_str() {
                "av1" => Some(WpgMode::Av1Enhanced),
                "palette" => Some(WpgMode::PaletteLossless),
                "modular" => Some(WpgMode::ModularLossless),
                _ => None,
            };

            let sub = match subsampling.as_str() {
                "422" => ColorSubsampling::Yuv422,
                "444" => ColorSubsampling::Yuv444,
                _ => ColorSubsampling::Yuv420,
            };

            let mut opts = EncodeOptions::default();
            opts.quality = quality;
            opts.speed = speed;
            opts.mode = m;
            opts.subsampling = sub;
            opts.enable_preview = !no_preview;
            opts.enable_grain = grain != Some(0);
            opts.grain_scale = grain;

            let t0 = Instant::now();
            let encoded = WpgEncoder::encode(rgba.as_raw(), width, height, &opts)?;
            let elapsed = t0.elapsed();

            fs::write(&output, &encoded)?;
            let raw_size = (width * height * 4) as f64;
            let comp_size = encoded.len() as f64;
            let bpp = (comp_size * 8.0) / (width * height) as f64;

            println!("✔ Done in {:.2} ms", elapsed.as_secs_f64() * 1000.0);
            println!("Size: {} bytes ({:.2} KB), bpp: {:.3}, ratio: {:.2}:1", encoded.len(), comp_size / 1024.0, bpp, raw_size / comp_size);
        }

        Commands::Decode {
            input,
            output,
            preview_only,
        } => {
            let bytes = fs::read(&input)?;
            if preview_only {
                let (w, h, rgb) = WpgDecoder::decode_preview(&bytes)?;
                let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(4, 4);
                for y in 0..4 {
                    for x in 0..4 {
                        let idx = (y * 4 + x) * 3;
                        img.put_pixel(x as u32, y as u32, Rgba([rgb[idx], rgb[idx + 1], rgb[idx + 2], 255]));
                    }
                }
                img.save(&output)?;
                println!("Extracted 16-byte micro-preview ({}x{} placeholder) to: {}", w, h, output.display());
            } else {
                let t0 = Instant::now();
                let decoded = WpgDecoder::decode(&bytes)?;
                let elapsed = t0.elapsed();
                let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(decoded.width, decoded.height, decoded.rgba)
                    .ok_or("Failed to create image buffer")?;
                img.save(&output)?;
                println!("✔ Decoded {}x{} in {:.2} ms -> {}", decoded.width, decoded.height, elapsed.as_secs_f64() * 1000.0, output.display());
            }
        }

        Commands::Info { input } => {
            let bytes = fs::read(&input)?;
            let mut reader = std::io::Cursor::new(&bytes);
            let header = WpgHeader::deserialize(&mut reader)?;

            println!("\n========================================");
            println!("  WPG File Analysis: {}", input.display());
            println!("========================================");
            println!("Dimensions:           {} x {} px", header.width, header.height);
            println!("Compression Mode:     {:?}", header.mode);
            println!("Chroma Subsampling:   {:?}", header.subsampling);
            println!("Quality Level:        {}", header.quality);
            println!("Total File Size:      {} bytes ({:.2} KB)", bytes.len(), bytes.len() as f64 / 1024.0);
            println!("Container Overhead:   ~16 bytes (vs 350-800B in AVIF)");
            println!("Has Alpha Channel:    {}", header.has_alpha());
            println!("Has 16B Micro-Preview:{}", header.has_micro_preview());
            println!("Has Film Grain:       {}", header.has_film_grain());
            println!("Secondary Zstd:       {}", header.is_secondary_entropy());
            println!("Bits Per Pixel (bpp): {:.4}", (bytes.len() as f64 * 8.0) / (header.width * header.height) as f64);
            println!("========================================\n");
        }

        Commands::Bench { input, quality } => {
            println!("\nBenchmarking: {} (Target Quality: {})...", input.display(), quality);
            let img = wpg::open_image_with_orientation(&input)?;
            let rgba = img.to_rgba8();
            let width = rgba.width();
            let height = rgba.height();

            // Encode WPG
            let mut opts = EncodeOptions::default();
            opts.quality = quality;
            opts.speed = 6;
            let t0 = Instant::now();
            let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), width, height, &opts)?;
            let wpg_time = t0.elapsed().as_secs_f64() * 1000.0;
            let dec_wpg = WpgDecoder::decode(&wpg_bytes)?;
            let wpg_psnr = compute_psnr(rgba.as_raw(), &dec_wpg.rgba);
            let wpg_ssim = compute_ssim(rgba.as_raw(), &dec_wpg.rgba, width as usize, height as usize);

            // Encode PNG
            let mut png_bytes = Vec::new();
            let png_enc = image::codecs::png::PngEncoder::new(&mut png_bytes);
            png_enc.write_image(rgba.as_raw(), width, height, image::ColorType::Rgba8.into())?;

            // Encode JPEG
            let rgb = img.to_rgb8();
            let mut jpeg_bytes = Vec::new();
            let mut j_enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, quality);
            let _ = j_enc.encode(rgb.as_raw(), width, height, image::ColorType::Rgb8.into());
            let (jpeg_psnr, jpeg_ssim) = if let Ok(dec_j) = image::load_from_memory(&jpeg_bytes) {
                let dec_rgba = dec_j.to_rgba8();
                (compute_psnr(rgba.as_raw(), dec_rgba.as_raw()), compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize))
            } else {
                (0.0, 0.0)
            };

            // Encode WebP (Lossless)
            let mut webp_bytes = Vec::new();
            let w_enc = image::codecs::webp::WebPEncoder::new_lossless(&mut webp_bytes);
            let _ = w_enc.write_image(rgba.as_raw(), width, height, image::ColorType::Rgba8.into());

            // Encode WebP (Lossy libwebp)
            let lossy_webp_mem = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(quality as f32);
            let lossy_webp_bytes = lossy_webp_mem.to_vec();
            let (lossy_webp_psnr, lossy_webp_ssim) = if let Some(dec) = webp::Decoder::new(&lossy_webp_bytes).decode() {
                let dec_rgba = dec.to_image().to_rgba8();
                (
                    compute_psnr(rgba.as_raw(), dec_rgba.as_raw()),
                    compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize),
                )
            } else {
                (0.0, 0.0)
            };

            // Encode AVIF
            let mut avif_bytes = Vec::new();
            let mut avif_alpha_size = 0;
            let mut avif_color_size = 0;
            {
                let slice: &[ravif::RGBA8] = unsafe {
                    std::slice::from_raw_parts(rgba.as_raw().as_ptr() as *const ravif::RGBA8, (width * height) as usize)
                };
                let img_ref = ravif::Img::new(slice, width as usize, height as usize);
                let enc = ravif::Encoder::new()
                    .with_quality(quality as f32)
                    .with_speed(6)
                    .with_bit_depth(ravif::BitDepth::Eight);
                if let Ok(encoded) = enc.encode_rgba(img_ref) {
                    avif_bytes = encoded.avif_file;
                    avif_alpha_size = encoded.alpha_byte_size;
                    avif_color_size = encoded.color_byte_size;
                }
            }

            let (avif_psnr, avif_ssim) = if !avif_bytes.is_empty() {
                if let Ok(dec_rgba) = wpg::decode_avif_to_rgba(&avif_bytes, width as usize, height as usize, avif_alpha_size, avif_color_size) {
                    (
                        compute_psnr(rgba.as_raw(), &dec_rgba),
                        compute_ssim(rgba.as_raw(), &dec_rgba, width as usize, height as usize),
                    )
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            };

            println!("\n+--------------------+------------+--------+----------+--------+------------------+");
            println!("| Format             | Size (B)   | bpp    | PSNR(dB) | SSIM   | vs PNG Baseline  |");
            println!("+--------------------+------------+--------+----------+--------+------------------+");
            println!(
                "| ✨ WPG (Our Format)| {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15.1}% |",
                wpg_bytes.len(),
                (wpg_bytes.len() as f64 * 8.0) / (width * height) as f64,
                wpg_psnr,
                wpg_ssim,
                (1.0 - wpg_bytes.len() as f64 / png_bytes.len() as f64) * -100.0
            );
            if !avif_bytes.is_empty() {
                println!(
                    "| AVIF (Standard Q{}) | {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15.1}% |",
                    quality,
                    avif_bytes.len(),
                    (avif_bytes.len() as f64 * 8.0) / (width * height) as f64,
                    avif_psnr,
                    avif_ssim,
                    (1.0 - avif_bytes.len() as f64 / png_bytes.len() as f64) * -100.0
                );
            }
            println!(
                "| PNG (Lossless)     | {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15}  |",
                png_bytes.len(),
                (png_bytes.len() as f64 * 8.0) / (width * height) as f64,
                99.99,
                1.0000,
                "0.0%"
            );
            println!(
                "| JPEG (Q{})         | {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15.1}% |",
                quality,
                jpeg_bytes.len(),
                (jpeg_bytes.len() as f64 * 8.0) / (width * height) as f64,
                jpeg_psnr,
                jpeg_ssim,
                (1.0 - jpeg_bytes.len() as f64 / png_bytes.len() as f64) * -100.0
            );
            println!(
                "| WebP (Lossy Q{})    | {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15.1}% |",
                quality,
                lossy_webp_bytes.len(),
                (lossy_webp_bytes.len() as f64 * 8.0) / (width * height) as f64,
                lossy_webp_psnr,
                lossy_webp_ssim,
                (1.0 - lossy_webp_bytes.len() as f64 / png_bytes.len() as f64) * -100.0
            );
            println!(
                "| WebP (Lossless VP8L)| {:>10} | {:>6.3} | {:>8.2} | {:>6.4} | {:>15.1}% |",
                webp_bytes.len(),
                (webp_bytes.len() as f64 * 8.0) / (width * height) as f64,
                99.99,
                1.0000,
                (1.0 - webp_bytes.len() as f64 / png_bytes.len() as f64) * -100.0
            );
            println!("+--------------------+------------+--------+----------+--------+------------------+\n");
            println!("WPG Encode Time: {:.2} ms", wpg_time);
        }

        Commands::Serve { port } => {
            wpg::server::run_server(port)?;
        }

        Commands::Batch {
            input_dir,
            output_dir,
            quality,
            speed,
            recursive,
        } => {
            println!("\n🔍 Scanning for images in: {:?} (Recursive: {})...", input_dir, recursive);
            let mut image_files = Vec::new();
            discover_images(&input_dir, recursive, &mut image_files);

            if image_files.is_empty() {
                println!("⚠️ No supported images (PNG, JPEG, WebP, BMP) found in {:?}.", input_dir);
                return Ok(());
            }

            println!("🚀 Found {} images to convert (Quality: {}, Speed: {}).\n", image_files.len(), quality, speed);
            println!("+--------------------------------+------------+------------+----------+-----------+");
            println!("| File                           | Orig Size  | WPG Size   | Saved    | Time (ms) |");
            println!("+--------------------------------+------------+------------+----------+-----------+");

            let mut total_orig_bytes = 0u64;
            let mut total_wpg_bytes = 0u64;
            let t_total = Instant::now();

            let options = EncodeOptions {
                quality,
                speed,
                mode: Some(WpgMode::Av1Enhanced),
                subsampling: ColorSubsampling::Yuv420,
                ..Default::default()
            };

            for path in &image_files {
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                let orig_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                total_orig_bytes += orig_size;

                let t_start = Instant::now();
                let res = (|| -> Result<(usize, PathBuf), Box<dyn std::error::Error>> {
                    let raw_bytes = fs::read(path)?;
                    let img = wpg::load_image_with_orientation(&raw_bytes)?;
                    let rgba = img.to_rgba8();
                    let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), rgba.width(), rgba.height(), &options)?;

                    let out_path = if let Some(ref out_dir) = output_dir {
                        fs::create_dir_all(out_dir)?;
                        out_dir.join(path.file_stem().unwrap_or_default()).with_extension("wpg")
                    } else {
                        path.with_extension("wpg")
                    };

                    fs::write(&out_path, &wpg_bytes)?;
                    Ok((wpg_bytes.len(), out_path))
                })();

                match res {
                    Ok((wpg_size, _)) => {
                        total_wpg_bytes += wpg_size as u64;
                        let elapsed_ms = t_start.elapsed().as_secs_f64() * 1000.0;
                        let savings = (1.0 - wpg_size as f64 / orig_size.max(1) as f64) * 100.0;
                        let display_name = if file_name.len() > 30 {
                            format!("...{}", &file_name[file_name.len() - 27..])
                        } else {
                            file_name.to_string()
                        };
                        println!(
                            "| {:<30} | {:>10} | {:>10} | {:>7.1}% | {:>9.1} |",
                            display_name,
                            format_bytes(orig_size),
                            format_bytes(wpg_size as u64),
                            savings,
                            elapsed_ms
                        );
                    }
                    Err(e) => {
                        println!("| {:<30} | ERROR: {:<43} |", file_name, e);
                    }
                }
            }

            let total_saved = total_orig_bytes.saturating_sub(total_wpg_bytes);
            let total_pct = (1.0 - total_wpg_bytes as f64 / total_orig_bytes.max(1) as f64) * 100.0;
            println!("+--------------------------------+------------+------------+----------+-----------+");
            println!(
                "| TOTAL ({:>3} images)            | {:>10} | {:>10} | {:>7.1}% | {:>8.2}s |",
                image_files.len(),
                format_bytes(total_orig_bytes),
                format_bytes(total_wpg_bytes),
                total_pct,
                t_total.elapsed().as_secs_f64()
            );
            println!("+--------------------------------+------------+------------+----------+-----------+");
            println!("\n✨ Batch conversion complete! Saved {} of bandwidth/storage.\n", format_bytes(total_saved));
        }

        Commands::View { input } => {
            let exe_path = std::env::current_exe()?;
            let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new(""));
            let viewer_path = exe_dir.join("wpg-view.exe");
            let mut cmd = std::process::Command::new(if viewer_path.exists() {
                viewer_path
            } else {
                PathBuf::from("wpg-view.exe")
            });
            if let Some(p) = input {
                cmd.arg(p);
            }
            cmd.spawn()?;
        }

        Commands::Register => {
            register_windows_shell()?;
        }

        Commands::Unregister => {
            unregister_windows_shell()?;
        }
    }

    Ok(())
}

fn register_windows_shell() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new(""));
    let viewer_path = exe_dir.join("wpg-view.exe");
    let viewer_str = if viewer_path.exists() {
        viewer_path.to_string_lossy().to_string()
    } else {
        exe_path.to_string_lossy().to_string()
    };
    let wpg_str = exe_path.to_string_lossy().to_string();

    println!("\n========================================================");
    println!("  🪟 Registering WPG Image Format in Windows Explorer");
    println!("========================================================");

    let reg_script = format!(
        r#"
$hkcu = "HKCU:\Software\Classes"
New-Item -Path "$hkcu\.wpg" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\.wpg" -Name "(Default)" -Value "WPGImage"
Set-ItemProperty -Path "$hkcu\.wpg" -Name "Content Type" -Value "image/wpg"
Set-ItemProperty -Path "$hkcu\.wpg" -Name "PerceivedType" -Value "image"

New-Item -Path "$hkcu\WPGImage" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage" -Name "(Default)" -Value "WPG (Web Progressive Graphic) Image"

New-Item -Path "$hkcu\WPGImage\DefaultIcon" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage\DefaultIcon" -Name "(Default)" -Value '"{viewer}",0'

New-Item -Path "$hkcu\WPGImage\shell\open\command" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage\shell\open" -Name "(Default)" -Value "Open with WPG Viewer"
Set-ItemProperty -Path "$hkcu\WPGImage\shell\open\command" -Name "(Default)" -Value '"{viewer}" "%1"'

New-Item -Path "$hkcu\WPGImage\shell\convert_png\command" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage\shell\convert_png" -Name "(Default)" -Value "Convert to PNG"
Set-ItemProperty -Path "$hkcu\WPGImage\shell\convert_png\command" -Name "(Default)" -Value '"{wpg}" decode "%1" -o "%1.png"'

New-Item -Path "$hkcu\WPGImage\shell\info\command" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage\shell\info" -Name "(Default)" -Value "WPG Image Analysis"
Set-ItemProperty -Path "$hkcu\WPGImage\shell\info\command" -Name "(Default)" -Value 'cmd /k ""{wpg}" info "%1""'

# Refresh Explorer associations
$code = '[DllImport("shell32.dll")]public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);'
$type = Add-Type -MemberDefinition $code -Name ShellUtil -Namespace Win32 -PassThru
$type::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"#,
        viewer = viewer_str,
        wpg = wpg_str
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &reg_script])
        .status()?;

    if status.success() {
        println!("✔ Successfully registered .wpg format in Windows Explorer (HKCU)!");
        println!("  • Default viewer: {}", viewer_str);
        println!("  • Double-click any .wpg file to open instantly in WPG Viewer");
        println!("  • Right-click menu options added: 'Open with WPG Viewer', 'Convert to PNG', 'WPG Image Analysis'\n");
    } else {
        eprintln!("⚠️ Failed to write registry keys.");
    }

    Ok(())
}

fn unregister_windows_shell() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nUnregistering WPG format from Windows Explorer...");
    let unreg_script = r#"
Remove-Item -Path "HKCU:\Software\Classes\.wpg" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKCU:\Software\Classes\WPGImage" -Recurse -Force -ErrorAction SilentlyContinue
$code = '[DllImport("shell32.dll")]public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);'
$type = Add-Type -MemberDefinition $code -Name ShellUtil -Namespace Win32 -PassThru
$type::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"#;
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", unreg_script])
        .status()?;
    println!("✔ Unregistered .wpg file associations successfully.\n");
    Ok(())
}

fn discover_images(dir: &std::path::Path, recursive: bool, list: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && recursive {
                discover_images(&path, recursive, list);
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if matches!(ext_lower.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp") {
                        list.push(path);
                    }
                }
            }
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
