use clap::{Parser, Subcommand};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use wpg::{
    compute_psnr, compute_ssim, decode_avif_to_rgba, EncodeOptions, WpgDecoder, WpgEncoder,
};

#[derive(Parser, Debug)]
#[command(
    name = "wpg-lab",
    version = "2.0.0",
    about = "WPG Compression Laboratory & Experimental Suite"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run an experimental Rate-Distortion sweep on a single image and export JSON
    Run {
        /// Image path (defaults to laboratory/datasets/camera_photo.jpg)
        #[arg(value_name = "IMAGE")]
        image: Option<PathBuf>,

        /// Quality points to sweep (comma-separated, default: 40,60,70,75,80,83,85,90)
        #[arg(short, long, default_value = "40,60,70,75,80,83,85,90")]
        qualities: String,

        /// Encoding speed preset (1..10, default 6)
        #[arg(short, long, default_value_t = 6)]
        speed: u8,

        /// Output directory for run JSON (default: laboratory/runs)
        #[arg(short, long, default_value = "laboratory/runs")]
        output_dir: PathBuf,
    },

    /// Run full benchmark sweep on all dataset images in laboratory/datasets/
    All {
        /// Quality points to sweep
        #[arg(short, long, default_value = "40,60,70,75,80,83,85,90")]
        qualities: String,

        /// Speed preset
        #[arg(short, long, default_value_t = 6)]
        speed: u8,
    },

    /// Launch the interactive Laboratory Playground web server
    Serve {
        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub size_bytes: usize,
    pub bpp: f64,
    pub ratio: f64,
    pub psnr_db: f64,
    pub ssim: f64,
    pub enc_ms: f64,
    pub dec_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPoint {
    pub quality: u8,
    pub wpg: MetricPoint,
    pub avif: MetricPoint,
    pub webp: MetricPoint,
    pub jpeg: MetricPoint,
    pub wpg_vs_avif_size_pct: f64,
    pub wpg_vs_avif_psnr_diff: f64,
    pub wpg_vs_webp_size_pct: f64,
    pub wpg_vs_webp_psnr_diff: f64,
    pub wpg_beats_avif_both: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRunReport {
    pub experiment_name: String,
    pub timestamp: String,
    pub image_name: String,
    pub width: u32,
    pub height: u32,
    pub total_pixels: usize,
    pub source_bytes: usize,
    pub png_lossless_bytes: usize,
    pub speed: u8,
    pub points: Vec<SweepPoint>,
    pub summary: ExperimentSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentSummary {
    pub total_tested: usize,
    pub wpg_smaller_count: usize,
    pub wpg_higher_psnr_count: usize,
    pub wpg_beats_avif_both_count: usize,
    pub avg_size_savings_vs_avif_pct: f64,
    pub avg_psnr_gain_vs_avif_db: f64,
    pub max_size_savings_pct: f64,
    pub max_psnr_gain_db: f64,
}

fn parse_qualities(s: &str) -> Vec<u8> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<u8>().ok())
        .filter(|&q| q >= 1 && q <= 100)
        .collect()
}

fn run_sweep(image_path: &Path, qualities: &[u8], speed: u8, out_dir: &Path) -> Result<ExperimentRunReport, Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                    WPG EXPERIMENTAL LABORATORY — RATE-DISTORTION SWEEP                     ║");
    println!("╚════════════════════════════════════════════════════════════════════════════════════════════╝");
    println!("Testing Asset: {} (Speed: {})", image_path.display(), speed);

    let img = wpg::open_image_with_orientation(image_path)?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let total_pixels = (width as usize * height as usize).max(1);
    let raw_rgb_bytes = total_pixels as f64 * 3.0;
    let source_bytes = fs::metadata(image_path).map(|m| m.len() as usize).unwrap_or(1);

    // PNG Lossless reference
    let mut png_bytes = Vec::new();
    let png_enc = image::codecs::png::PngEncoder::new(&mut png_bytes);
    png_enc.write_image(rgba.as_raw(), width, height, image::ColorType::Rgba8.into())?;

    let mut points = Vec::new();

    println!("\n+-----+-------------------------------+-------------------------------+-------------------------------+-----------------------+--------------------+");
    println!("|     |          WPG (Next-Gen)       |          AVIF (rav1e)         |          WebP (libwebp)       |          JPEG         | Head-to-Head Win   |");
    println!("|  Q  | Size(B)    bpp   PSNR    SSIM | Size(B)    bpp   PSNR    SSIM | Size(B)    bpp   PSNR    SSIM | Size(B)    bpp   PSNR | Size vs AVIF | PSNR|");
    println!("+-----+-------------------------------+-------------------------------+-------------------------------+-----------------------+--------------------+");

    for &q in qualities {
        // --- 1. WPG ---
        let mut wpg_opts = EncodeOptions::default();
        wpg_opts.quality = q;
        wpg_opts.speed = speed;
        let t0 = Instant::now();
        let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), width, height, &wpg_opts)?;
        let wpg_enc_time = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = Instant::now();
        let dec_wpg = WpgDecoder::decode(&wpg_bytes)?;
        let wpg_dec_time = t1.elapsed().as_secs_f64() * 1000.0;
        let wpg_psnr = compute_psnr(rgba.as_raw(), &dec_wpg.rgba);
        let wpg_ssim = compute_ssim(rgba.as_raw(), &dec_wpg.rgba, width as usize, height as usize);
        let wpg_bpp = (wpg_bytes.len() as f64 * 8.0) / total_pixels as f64;
        let wpg_ratio = raw_rgb_bytes / wpg_bytes.len().max(1) as f64;

        // --- 2. AVIF ---
        let mut avif_bytes = Vec::new();
        let mut avif_enc_time = 0.0;
        let mut avif_alpha_size = 0;
        let mut avif_color_size = 0;
        {
            let t0 = Instant::now();
            let slice: &[ravif::RGBA8] = unsafe {
                std::slice::from_raw_parts(rgba.as_raw().as_ptr() as *const ravif::RGBA8, total_pixels)
            };
            let img_ref = ravif::Img::new(slice, width as usize, height as usize);
            let enc = ravif::Encoder::new()
                .with_quality(q as f32)
                .with_speed(speed.clamp(1, 10))
                .with_bit_depth(ravif::BitDepth::Eight);
            if let Ok(encoded) = enc.encode_rgba(img_ref) {
                avif_enc_time = t0.elapsed().as_secs_f64() * 1000.0;
                avif_bytes = encoded.avif_file;
                avif_alpha_size = encoded.alpha_byte_size;
                avif_color_size = encoded.color_byte_size;
            }
        }
        let mut avif_dec_time = 0.0;
        let (avif_psnr, avif_ssim) = if !avif_bytes.is_empty() {
            let t0 = Instant::now();
            if let Ok(dec_rgba) = decode_avif_to_rgba(&avif_bytes, width as usize, height as usize, avif_alpha_size, avif_color_size) {
                avif_dec_time = t0.elapsed().as_secs_f64() * 1000.0;
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
        let avif_bpp = (avif_bytes.len() as f64 * 8.0) / total_pixels as f64;
        let avif_ratio = raw_rgb_bytes / avif_bytes.len().max(1) as f64;

        // --- 3. WebP ---
        let webp_method = match speed.clamp(1, 10) {
            1..=2 => 6,
            3..=4 => 5,
            5..=6 => 4,
            7 => 3,
            8 => 2,
            9 => 1,
            _ => 0,
        };
        let t_w_enc = Instant::now();
        let lossy_webp_bytes = {
            let encoder = webp::Encoder::from_rgba(rgba.as_raw(), width, height);
            if let Ok(mut cfg) = webp::WebPConfig::new() {
                cfg.lossless = 0;
                cfg.alpha_compression = 1;
                cfg.quality = q as f32;
                cfg.method = webp_method;
                match encoder.encode_advanced(&cfg) {
                    Ok(mem) => mem.to_vec(),
                    Err(_) => encoder.encode(q as f32).to_vec(),
                }
            } else {
                encoder.encode(q as f32).to_vec()
            }
        };
        let webp_enc_time = t_w_enc.elapsed().as_secs_f64() * 1000.0;
        let t_w_dec = Instant::now();
        let dec_webp_opt = webp::Decoder::new(&lossy_webp_bytes).decode();
        let webp_dec_time = t_w_dec.elapsed().as_secs_f64() * 1000.0;
        let (webp_psnr, webp_ssim) = if let Some(dec) = dec_webp_opt {
            let dec_rgba = dec.to_image().to_rgba8();
            (
                compute_psnr(rgba.as_raw(), dec_rgba.as_raw()),
                compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize),
            )
        } else {
            (0.0, 0.0)
        };
        let webp_bpp = (lossy_webp_bytes.len() as f64 * 8.0) / total_pixels as f64;
        let webp_ratio = raw_rgb_bytes / lossy_webp_bytes.len().max(1) as f64;

        // --- 4. JPEG ---
        let rgb = img.to_rgb8();
        let mut jpeg_bytes = Vec::new();
        let mut j_enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, q);
        let _ = j_enc.encode(rgb.as_raw(), width, height, image::ColorType::Rgb8.into());
        let (jpeg_psnr, jpeg_ssim) = if let Ok(dec_j) = image::load_from_memory(&jpeg_bytes) {
            let dec_rgba = dec_j.to_rgba8();
            (
                compute_psnr(rgba.as_raw(), dec_rgba.as_raw()),
                compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize),
            )
        } else {
            (0.0, 0.0)
        };
        let jpeg_bpp = (jpeg_bytes.len() as f64 * 8.0) / total_pixels as f64;
        let jpeg_ratio = raw_rgb_bytes / jpeg_bytes.len().max(1) as f64;

        // --- Comparisons ---
        let size_vs_avif_pct = (1.0 - wpg_bytes.len() as f64 / avif_bytes.len().max(1) as f64) * -100.0;
        let psnr_diff_avif = wpg_psnr - avif_psnr;
        let size_vs_webp_pct = (1.0 - wpg_bytes.len() as f64 / lossy_webp_bytes.len().max(1) as f64) * -100.0;
        let psnr_diff_webp = wpg_psnr - webp_psnr;
        let wpg_beats_avif_both = wpg_bytes.len() <= avif_bytes.len() && wpg_psnr >= avif_psnr;

        let win_indicator = if wpg_beats_avif_both {
            "★ WPG WINS BOTH!"
        } else if wpg_bytes.len() < avif_bytes.len() {
            "✔ WPG Smaller"
        } else {
            "✔ WPG Higher PSNR"
        };

        println!(
            "| {:>3} | {:>7} {:>6.3} {:>6.2} {:>6.4} | {:>7} {:>6.3} {:>6.2} {:>6.4} | {:>7} {:>6.3} {:>6.2} {:>6.4} | {:>7} {:>6.3} {:>6.2} | {:>+6.1}% | {:>+5.2}dB {:<14} |",
            q,
            wpg_bytes.len(), wpg_bpp, wpg_psnr, wpg_ssim,
            avif_bytes.len(), avif_bpp, avif_psnr, avif_ssim,
            lossy_webp_bytes.len(), webp_bpp, webp_psnr, webp_ssim,
            jpeg_bytes.len(), jpeg_bpp, jpeg_psnr,
            size_vs_avif_pct,
            psnr_diff_avif,
            win_indicator
        );

        points.push(SweepPoint {
            quality: q,
            wpg: MetricPoint {
                size_bytes: wpg_bytes.len(),
                bpp: wpg_bpp,
                ratio: wpg_ratio,
                psnr_db: wpg_psnr,
                ssim: wpg_ssim,
                enc_ms: wpg_enc_time,
                dec_ms: wpg_dec_time,
            },
            avif: MetricPoint {
                size_bytes: avif_bytes.len(),
                bpp: avif_bpp,
                ratio: avif_ratio,
                psnr_db: avif_psnr,
                ssim: avif_ssim,
                enc_ms: avif_enc_time,
                dec_ms: avif_dec_time,
            },
            webp: MetricPoint {
                size_bytes: lossy_webp_bytes.len(),
                bpp: webp_bpp,
                ratio: webp_ratio,
                psnr_db: webp_psnr,
                ssim: webp_ssim,
                enc_ms: webp_enc_time,
                dec_ms: webp_dec_time,
            },
            jpeg: MetricPoint {
                size_bytes: jpeg_bytes.len(),
                bpp: jpeg_bpp,
                ratio: jpeg_ratio,
                psnr_db: jpeg_psnr,
                ssim: jpeg_ssim,
                enc_ms: 0.0,
                dec_ms: 0.0,
            },
            wpg_vs_avif_size_pct: size_vs_avif_pct,
            wpg_vs_avif_psnr_diff: psnr_diff_avif,
            wpg_vs_webp_size_pct: size_vs_webp_pct,
            wpg_vs_webp_psnr_diff: psnr_diff_webp,
            wpg_beats_avif_both,
        });
    }

    println!("+-----+-------------------------------+-------------------------------+-------------------------------+-----------------------+--------------------+");

    let total_tested = points.len();
    let wpg_smaller_count = points.iter().filter(|p| p.wpg.size_bytes < p.avif.size_bytes).count();
    let wpg_higher_psnr_count = points.iter().filter(|p| p.wpg.psnr_db >= p.avif.psnr_db).count();
    let wpg_beats_avif_both_count = points.iter().filter(|p| p.wpg_beats_avif_both).count();
    let avg_size_savings = points.iter().map(|p| p.wpg_vs_avif_size_pct).sum::<f64>() / total_tested as f64;
    let avg_psnr_gain = points.iter().map(|p| p.wpg_vs_avif_psnr_diff).sum::<f64>() / total_tested as f64;
    let max_size_savings = points.iter().map(|p| p.wpg_vs_avif_size_pct).fold(f64::MAX, f64::min);
    let max_psnr_gain = points.iter().map(|p| p.wpg_vs_avif_psnr_diff).fold(f64::MIN, f64::max);

    let summary = ExperimentSummary {
        total_tested,
        wpg_smaller_count,
        wpg_higher_psnr_count,
        wpg_beats_avif_both_count,
        avg_size_savings_vs_avif_pct: avg_size_savings,
        avg_psnr_gain_vs_avif_db: avg_psnr_gain,
        max_size_savings_pct: max_size_savings,
        max_psnr_gain_db: max_psnr_gain,
    };

    println!("\n╔════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                                  EXPERIMENT VERDICT                                        ║");
    println!("╠════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║  Total Quality Sweep Points Tested: {:>3}                                                    ║", total_tested);
    println!("║  Points where WPG is Smaller than AVIF: {:>2} / {:<2} ({:>5.1}%)                               ║", wpg_smaller_count, total_tested, wpg_smaller_count as f64 / total_tested as f64 * 100.0);
    println!("║  Points where WPG has Higher PSNR:      {:>2} / {:<2} ({:>5.1}%)                               ║", wpg_higher_psnr_count, total_tested, wpg_higher_psnr_count as f64 / total_tested as f64 * 100.0);
    println!("║  Points BEATING AVIF ON BOTH METRICS:   {:>2} / {:<2} ({:>5.1}%)                               ║", wpg_beats_avif_both_count, total_tested, wpg_beats_avif_both_count as f64 / total_tested as f64 * 100.0);
    println!("║  Average Byte Size Savings vs AVIF:     {:>+6.1}%                                           ║", avg_size_savings);
    println!("║  Average PSNR Delta vs AVIF:            {:>+6.2} dB                                         ║", avg_psnr_gain);
    println!("║  Max Compression Advantage:             {:>+6.1}% byte reduction                            ║", max_size_savings);
    println!("║  Max PSNR Quality Advantage:            {:>+6.2} dB fidelity gain                           ║", max_psnr_gain);
    println!("╚════════════════════════════════════════════════════════════════════════════════════════════╝\n");

    let stem = image_path.file_stem().and_then(|s| s.to_str()).unwrap_or("run");
    let report = ExperimentRunReport {
        experiment_name: format!("WPG_vs_AVIF_RD_Sweep_{}", stem),
        timestamp: chrono_now_string(),
        image_name: image_path.file_name().and_then(|s| s.to_str()).unwrap_or("image").to_string(),
        width,
        height,
        total_pixels,
        source_bytes,
        png_lossless_bytes: png_bytes.len(),
        speed,
        points,
        summary,
    };

    fs::create_dir_all(out_dir)?;
    let out_file = out_dir.join(format!("{}.json", stem));
    let json_str = serde_json::to_string_pretty(&report)?;
    fs::write(&out_file, json_str)?;
    println!("✔ Saved full experiment run report to: {}\n", out_file.display());

    Ok(report)
}

fn chrono_now_string() -> String {
    let now = std::time::SystemTime::now();
    let duration = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}Z", duration.as_secs(), duration.subsec_millis())
}

fn serve_playground(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    use std::net::TcpListener;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
    println!("\n╔════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                     WPG LABORATORY PLAYGROUND — INTERACTIVE DASHBOARD                      ║");
    println!("╠════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║  URL: http://127.0.0.1:{:<5}                                                              ║", port);
    println!("║  Rate-Distortion Visualizer, 1:1 Split Diff Heatmap, Live Run Inspector                     ║");
    println!("╚════════════════════════════════════════════════════════════════════════════════════════════╝\n");

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut buffer);
                let req_str = String::from_utf8_lossy(&buffer);
                let first_line = req_str.lines().next().unwrap_or("");
                let path = first_line.split_whitespace().nth(1).unwrap_or("/");

                let (status, content_type, body) = match path {
                    "/" | "/index.html" => {
                        let html = fs::read_to_string("laboratory/playground/index.html")
                            .unwrap_or_else(|_| "<h1>Laboratory Playground file not found.</h1>".to_string());
                        ("200 OK", "text/html; charset=utf-8", html.into_bytes())
                    }
                    p if p.starts_with("/api/runs/") => {
                        let file_name = p.trim_start_matches("/api/runs/");
                        let file_path = PathBuf::from("laboratory/runs").join(file_name);
                        if let Ok(bytes) = fs::read(&file_path) {
                            ("200 OK", "application/json", bytes)
                        } else {
                            ("404 NOT FOUND", "text/plain", b"Run not found".to_vec())
                        }
                    }
                    "/api/runs" => {
                        let mut runs = Vec::new();
                        if let Ok(entries) = fs::read_dir("laboratory/runs") {
                            for e in entries.flatten() {
                                if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
                                    if let Some(name) = e.file_name().to_str() {
                                        runs.push(name.to_string());
                                    }
                                }
                            }
                        }
                        let json = serde_json::to_string(&runs).unwrap_or_else(|_| "[]".to_string());
                        ("200 OK", "application/json", json.into_bytes())
                    }
                    _ => ("404 NOT FOUND", "text/plain", b"Not found".to_vec()),
                };

                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                    status, content_type, body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            image,
            qualities,
            speed,
            output_dir,
        } => {
            let img_path = image.unwrap_or_else(|| PathBuf::from("laboratory/datasets/camera_photo.jpg"));
            let q_list = parse_qualities(&qualities);
            run_sweep(&img_path, &q_list, speed, &output_dir)?;
        }

        Commands::All { qualities, speed } => {
            let q_list = parse_qualities(&qualities);
            let dataset_dir = PathBuf::from("laboratory/datasets");
            let out_dir = PathBuf::from("laboratory/runs");
            if let Ok(entries) = fs::read_dir(&dataset_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                    if ["jpg", "jpeg", "png", "webp"].contains(&ext.as_str()) {
                        let _ = run_sweep(&path, &q_list, speed, &out_dir);
                    }
                }
            }
        }

        Commands::Serve { port } => {
            serve_playground(port)?;
        }
    }

    Ok(())
}
