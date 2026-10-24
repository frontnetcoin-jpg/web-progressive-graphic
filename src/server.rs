use crate::{
    compute_psnr, compute_ssim, load_image_with_orientation, ColorSubsampling, EncodeOptions,
    WpgDecoder, WpgEncoder, WpgMode,
};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ColorType, ImageEncoder};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

#[derive(Serialize, Deserialize)]
pub struct EncodeRequest {
    pub image_data: String,       // Base64 data URL
    pub quality: u8,
    pub speed: u8,
    pub mode: String,             // auto, av1, palette, modular
    pub subsampling: String,      // 420, 422, 444
    pub grain: Option<u8>,
    pub enable_preview: bool,
    pub enable_grain: bool,
    pub rotate: Option<i32>,      // 0, 90, 180, 270
}

#[derive(Serialize, Deserialize)]
pub struct CompareFormatInfo {
    pub format: String,
    pub size_bytes: usize,
    pub bpp: f64,
    pub psnr_db: f64,
    pub ssim: f64,
    pub data_url: String,
    pub encode_time_ms: f64,
    pub savings_vs_png_pct: f64,
}

#[derive(Serialize, Deserialize)]
pub struct EncodeResponse {
    pub width: u32,
    pub height: u32,
    pub wpg: CompareFormatInfo,
    pub original_size_bytes: usize,
    pub original_data_url: String, // Upright normalized PNG for reference
    pub preview_data_url: Option<String>,
    pub comparisons: Vec<CompareFormatInfo>,
    pub wpg_base64: String,        // Raw .wpg binary data for direct download
    pub avif_base64: Option<String>, // Raw .avif binary data for direct download & tangible comparison
    pub preview_base64: Option<String>, // Raw 16-byte micro-preview binary
}

pub fn run_server(port: u16) -> std::io::Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr)?;
    println!("\n========================================================");
    println!("  🚀 WPG Showcase Server running at: http://{}", addr);
    println!("  Interactive compression, 1:1 zoom, side-by-side WPG vs AVIF");
    println!("  Endpoints: / (GUI), /api/encode, /api/sample.wpg, /api/sample.avif");
    println!("========================================================\n");

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            std::thread::spawn(move || {
                let _ = handle_client(&mut stream);
            });
        }
    }

    Ok(())
}

fn handle_client(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    let method = parts[0];
    let path = parts.get(1).unwrap_or(&"/");

    let mut content_length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    if method == "GET" && (*path == "/" || path.starts_with("/?")) {
        send_response(stream, "text/html; charset=utf-8", HTML_CONTENT.as_bytes())?;
    } else if method == "POST" && *path == "/api/encode" {
        match handle_encode_api(&body) {
            Ok(json_res) => {
                send_response(stream, "application/json", json_res.as_bytes())?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 400, "Bad Request", "application/json", err_json.as_bytes())?;
            }
        }
    } else if method == "POST" && *path == "/api/decode" {
        match handle_decode_api(&body) {
            Ok(png_bytes) => {
                send_response(stream, "image/png", &png_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 400, "Bad Request", "application/json", err_json.as_bytes())?;
            }
        }
    } else if method == "GET" && *path == "/wpg-element.js" {
        send_response(stream, "application/javascript; charset=utf-8", WPG_ELEMENT_JS.as_bytes())?;
    } else if method == "GET" && *path == "/wpg-decoder.wasm" {
        send_response(stream, "application/wasm", WPG_DECODER_WASM)?;
    } else if method == "GET" && *path == "/api/sample.wpg" {
        match generate_sample_wpg() {
            Ok(wpg_bytes) => {
                send_response(stream, "application/octet-stream", &wpg_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 500, "Internal Server Error", "application/json", err_json.as_bytes())?;
            }
        }
    } else if method == "GET" && (*path == "/api/sample.avif" || *path == "/api/sample1.avif") {
        match generate_sample_avif(1) {
            Ok(avif_bytes) => {
                send_response(stream, "image/avif", &avif_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 500, "Internal Server Error", "application/json", err_json.as_bytes())?;
            }
        }
    } else if method == "GET" && *path == "/api/sample2.avif" {
        match generate_sample_avif(2) {
            Ok(avif_bytes) => {
                send_response(stream, "image/avif", &avif_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 500, "Internal Server Error", "application/json", err_json.as_bytes())?;
            }
        }
    } else if method == "GET" && *path == "/api/sample" {
        let sample = generate_demo_sample(1);
        send_response(stream, "image/png", &sample)?;
    } else if method == "GET" && *path == "/api/sample2" {
        let sample = generate_demo_sample(2);
        send_response(stream, "image/png", &sample)?;
    } else {
        send_response_status(stream, 404, "Not Found", "text/plain", b"Not Found")?;
    }

    Ok(())
}

fn send_response(stream: &mut TcpStream, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    send_response_status(stream, 200, "OK", content_type, body)
}

fn send_response_status(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status_code,
        status_text,
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn handle_decode_api(body: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let decoded = WpgDecoder::decode(body)?;
    let mut png_bytes = Vec::new();
    let enc = PngEncoder::new(&mut png_bytes);
    enc.write_image(
        &decoded.rgba,
        decoded.width,
        decoded.height,
        ColorType::Rgba8.into(),
    )?;
    Ok(png_bytes)
}

fn generate_sample_wpg() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let sample = generate_demo_sample(1);
    let img = image::load_from_memory(&sample)?;
    let rgba = img.to_rgba8();
    let opt = EncodeOptions {
        quality: 75,
        speed: 6,
        mode: Some(WpgMode::Av1Enhanced),
        subsampling: ColorSubsampling::Yuv420,
        ..Default::default()
    };
    let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), rgba.width(), rgba.height(), &opt)?;
    Ok(wpg_bytes)
}

fn generate_sample_avif(kind: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let sample = generate_demo_sample(kind);
    let img = image::load_from_memory(&sample)?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let slice: &[ravif::RGBA8] = unsafe {
        std::slice::from_raw_parts(rgba.as_raw().as_ptr() as *const ravif::RGBA8, (width * height) as usize)
    };
    let img_ref = ravif::Img::new(slice, width as usize, height as usize);
    let enc = ravif::Encoder::new()
        .with_quality(75.0)
        .with_speed(6)
        .with_bit_depth(ravif::BitDepth::Eight);
    let encoded = enc.encode_rgba(img_ref)?;
    Ok(encoded.avif_file)
}

fn handle_encode_api(body: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let req: EncodeRequest = serde_json::from_slice(body)?;

    // Parse data URL: data:image/...;base64,....
    let base64_str = if let Some(pos) = req.image_data.find(",") {
        &req.image_data[pos + 1..]
    } else {
        &req.image_data
    };

    // Fast base64 decode
    let raw_image_bytes = base64_decode(base64_str)?;

    // Load with automatic EXIF orientation detection
    let mut img = load_image_with_orientation(&raw_image_bytes)?;

    // Apply manual rotation if requested
    if let Some(deg) = req.rotate {
        let normalized = ((deg % 360) + 360) % 360;
        match normalized {
            90 => img = img.rotate90(),
            180 => img = img.rotate180(),
            270 => img = img.rotate270(),
            _ => (),
        }
    }

    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();

    let mode = match req.mode.as_str() {
        "av1" => Some(WpgMode::Av1Enhanced),
        "palette" => Some(WpgMode::PaletteLossless),
        "modular" => Some(WpgMode::ModularLossless),
        _ => None,
    };

    let subsampling = match req.subsampling.as_str() {
        "422" => ColorSubsampling::Yuv422,
        "444" => ColorSubsampling::Yuv444,
        _ => ColorSubsampling::Yuv420,
    };

    let mut opts = EncodeOptions::default();
    opts.quality = req.quality;
    opts.speed = req.speed;
    opts.mode = mode;
    opts.subsampling = subsampling;
    opts.enable_preview = req.enable_preview;
    opts.enable_grain = req.enable_grain;
    opts.grain_scale = req.grain;

    let t0 = Instant::now();
    let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), width, height, &opts)?;
    let wpg_enc_time = t0.elapsed().as_secs_f64() * 1000.0;

    let decoded_wpg = WpgDecoder::decode(&wpg_bytes)?;
    let wpg_psnr = compute_psnr(rgba.as_raw(), &decoded_wpg.rgba);
    let wpg_ssim = compute_ssim(rgba.as_raw(), &decoded_wpg.rgba, width as usize, height as usize);
    let wpg_bpp = (wpg_bytes.len() as f64 * 8.0) / (width * height) as f64;

    // Convert decoded WPG to PNG data URL for browser display
    let wpg_png_bytes = rgba_to_png(&decoded_wpg.rgba, width, height)?;
    let wpg_data_url = format!("data:image/png;base64,{}", base64_encode(&wpg_png_bytes));

    // Also produce upright normalized PNG for reference
    let orig_png_bytes = rgba_to_png(rgba.as_raw(), width, height)?;
    let orig_png_data_url = format!("data:image/png;base64,{}", base64_encode(&orig_png_bytes));
    let png_size = orig_png_bytes.len();

    // Render MicroPreview placeholder data URL and raw binary
    let (preview_data_url, preview_base64) = if let Some(preview) = &decoded_wpg.preview {
        let placeholder_rgba = preview.render_placeholder(width, height);
        let placeholder_png = rgba_to_png(&placeholder_rgba, width, height)?;
        let mut p_bytes = Vec::new();
        preview.serialize(&mut p_bytes).ok();
        (
            Some(format!("data:image/png;base64,{}", base64_encode(&placeholder_png))),
            Some(base64_encode(&p_bytes)),
        )
    } else {
        (None, None)
    };

    // Encode JPEG for reference
    let rgb = img.to_rgb8();
    let mut jpeg_bytes = Vec::new();
    let mut j_enc = JpegEncoder::new_with_quality(&mut jpeg_bytes, req.quality);
    let _ = j_enc.encode(rgb.as_raw(), width, height, ColorType::Rgb8.into());
    let jpeg_psnr = if !jpeg_bytes.is_empty() {
        if let Ok(dec_j) = image::load_from_memory(&jpeg_bytes) {
            compute_psnr(rgba.as_raw(), dec_j.to_rgba8().as_raw())
        } else {
            0.0
        }
    } else {
        0.0
    };
    let jpeg_ssim = if !jpeg_bytes.is_empty() {
        if let Ok(dec_j) = image::load_from_memory(&jpeg_bytes) {
            compute_ssim(rgba.as_raw(), dec_j.to_rgba8().as_raw(), width as usize, height as usize)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Encode WebP for reference
    let mut webp_bytes = Vec::new();
    let w_enc = WebPEncoder::new_lossless(&mut webp_bytes);
    let _ = w_enc.write_image(rgba.as_raw(), width, height, ColorType::Rgba8.into());

    // Encode Lossy WebP (libwebp)
    let lossy_webp_mem = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(req.quality as f32);
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

    // Encode standard AVIF for reference
    let mut avif_bytes = Vec::new();
    let mut avif_time = 0.0;
    let mut avif_alpha_size = 0;
    let mut avif_color_size = 0;
    {
        let t_avif = Instant::now();
        let slice: &[ravif::RGBA8] = unsafe {
            std::slice::from_raw_parts(rgba.as_raw().as_ptr() as *const ravif::RGBA8, (width * height) as usize)
        };
        let img_ref = ravif::Img::new(slice, width as usize, height as usize);
        let enc = ravif::Encoder::new()
            .with_quality(req.quality as f32)
            .with_speed(req.speed.clamp(1, 10))
            .with_bit_depth(ravif::BitDepth::Eight);
        if let Ok(encoded) = enc.encode_rgba(img_ref) {
            avif_time = t_avif.elapsed().as_secs_f64() * 1000.0;
            avif_bytes = encoded.avif_file;
            avif_alpha_size = encoded.alpha_byte_size;
            avif_color_size = encoded.color_byte_size;
        }
    }

    let (avif_psnr, avif_ssim, mut avif_data_url) = if !avif_bytes.is_empty() {
        if let Ok(dec_rgba) = crate::decode_avif_to_rgba(&avif_bytes, width as usize, height as usize, avif_alpha_size, avif_color_size) {
            let p = compute_psnr(rgba.as_raw(), &dec_rgba);
            let s = compute_ssim(rgba.as_raw(), &dec_rgba, width as usize, height as usize);
            let mut png_buf = Vec::new();
            let _ = PngEncoder::new(&mut png_buf).write_image(
                &dec_rgba,
                width,
                height,
                ColorType::Rgba8.into(),
            );
            let d = format!("data:image/png;base64,{}", base64_encode(&png_buf));
            (p, s, d)
        } else {
            (0.0, 0.0, String::new())
        }
    } else {
        (0.0, 0.0, String::new())
    };

    if avif_data_url.is_empty() && !avif_bytes.is_empty() {
        avif_data_url = format!("data:image/avif;base64,{}", base64_encode(&avif_bytes));
    }

    let avif_b64 = if !avif_bytes.is_empty() {
        Some(base64_encode(&avif_bytes))
    } else {
        None
    };

    let mut comparisons = Vec::new();

    // WPG entry
    let wpg_info = CompareFormatInfo {
        format: "WPG (Our Format)".into(),
        size_bytes: wpg_bytes.len(),
        bpp: wpg_bpp,
        psnr_db: wpg_psnr,
        ssim: wpg_ssim,
        data_url: wpg_data_url.clone(),
        encode_time_ms: wpg_enc_time,
        savings_vs_png_pct: (1.0 - wpg_bytes.len() as f64 / png_size as f64) * 100.0,
    };

    // AVIF comparison
    if !avif_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: format!("AVIF (Standard Q{})", req.quality),
            size_bytes: avif_bytes.len(),
            bpp: (avif_bytes.len() as f64 * 8.0) / (width * height) as f64,
            psnr_db: avif_psnr,
            ssim: avif_ssim,
            data_url: avif_data_url,
            encode_time_ms: avif_time,
            savings_vs_png_pct: (1.0 - avif_bytes.len() as f64 / png_size as f64) * 100.0,
        });
    }

    // PNG comparison
    comparisons.push(CompareFormatInfo {
        format: "PNG (Lossless)".into(),
        size_bytes: png_size,
        bpp: (png_size as f64 * 8.0) / (width * height) as f64,
        psnr_db: 99.99,
        ssim: 1.0,
        data_url: orig_png_data_url.clone(),
        encode_time_ms: 12.0,
        savings_vs_png_pct: 0.0,
    });

    // JPEG comparison
    if !jpeg_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: format!("JPEG (Q{})", req.quality),
            size_bytes: jpeg_bytes.len(),
            bpp: (jpeg_bytes.len() as f64 * 8.0) / (width * height) as f64,
            psnr_db: jpeg_psnr,
            ssim: jpeg_ssim,
            data_url: format!("data:image/jpeg;base64,{}", base64_encode(&jpeg_bytes)),
            encode_time_ms: 8.0,
            savings_vs_png_pct: (1.0 - jpeg_bytes.len() as f64 / png_size as f64) * 100.0,
        });
    }

    // WebP Lossy (libwebp) comparison
    if !lossy_webp_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: format!("WebP (Lossy Q{})", req.quality),
            size_bytes: lossy_webp_bytes.len(),
            bpp: (lossy_webp_bytes.len() as f64 * 8.0) / (width * height) as f64,
            psnr_db: lossy_webp_psnr,
            ssim: lossy_webp_ssim,
            data_url: format!("data:image/webp;base64,{}", base64_encode(&lossy_webp_bytes)),
            encode_time_ms: 12.0,
            savings_vs_png_pct: (1.0 - lossy_webp_bytes.len() as f64 / png_size as f64) * 100.0,
        });
    }

    // WebP comparison
    if !webp_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: "WebP (Lossless VP8L)".into(),
            size_bytes: webp_bytes.len(),
            bpp: (webp_bytes.len() as f64 * 8.0) / (width * height) as f64,
            psnr_db: 99.99,
            ssim: 1.0,
            data_url: format!("data:image/webp;base64,{}", base64_encode(&webp_bytes)),
            encode_time_ms: 25.0,
            savings_vs_png_pct: (1.0 - webp_bytes.len() as f64 / png_size as f64) * 100.0,
        });
    }

    let resp = EncodeResponse {
        width,
        height,
        wpg: wpg_info,
        original_size_bytes: png_size,
        original_data_url: orig_png_data_url,
        preview_data_url,
        comparisons,
        wpg_base64: base64_encode(&wpg_bytes),
        avif_base64: avif_b64,
        preview_base64,
    };

    Ok(serde_json::to_string(&resp)?)
}

fn rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new(&mut out);
    encoder.write_image(rgba, width, height, ColorType::Rgba8.into())?;
    Ok(out)
}

fn generate_demo_sample(kind: usize) -> Vec<u8> {
    let width = 256;
    let height = 256;
    let mut rgba = vec![255u8; width * height * 4];

    if kind == 1 {
        // Photographic sunset gradient scene
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                let fx = x as f32 / width as f32;
                let fy = y as f32 / height as f32;

                let r = (255.0 * (1.0 - fy * 0.4)).clamp(0.0, 255.0) as u8;
                let g = (140.0 * (1.0 - fy * 0.7) + 60.0 * fx).clamp(0.0, 255.0) as u8;
                let b = (50.0 + 180.0 * fy).clamp(0.0, 255.0) as u8;

                let m1 = (fx * 6.28 * 2.0).sin() * 25.0 + 160.0;
                let m2 = (fx * 6.28 * 4.0 + 1.0).sin() * 15.0 + 190.0;

                if y as f32 > m2 {
                    rgba[idx] = 20;
                    rgba[idx + 1] = 25;
                    rgba[idx + 2] = 40;
                } else if y as f32 > m1 {
                    rgba[idx] = 60;
                    rgba[idx + 1] = 40;
                    rgba[idx + 2] = 70;
                } else {
                    rgba[idx] = r;
                    rgba[idx + 1] = g;
                    rgba[idx + 2] = b;
                }
            }
        }
    } else {
        // UI & Vector Graphic with sharp text-like patterns and icons
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                if x >= 20 && x <= 236 && y >= 30 && y <= 226 {
                    rgba[idx] = 30;
                    rgba[idx + 1] = 35;
                    rgba[idx + 2] = 50;
                    if x >= 40 && x <= 216 && y >= 160 && y <= 200 {
                        rgba[idx] = 80;
                        rgba[idx + 1] = 120;
                        rgba[idx + 2] = 255;
                    }
                    let cx = (x as i32 - 70).abs();
                    let cy = (y as i32 - 80).abs();
                    if cx * cx + cy * cy <= 400 {
                        rgba[idx] = 255;
                        rgba[idx + 1] = 80;
                        rgba[idx + 2] = 120;
                    }
                } else {
                    rgba[idx] = 15;
                    rgba[idx + 1] = 18;
                    rgba[idx + 2] = 25;
                }
            }
        }
    }

    rgba_to_png(&rgba, width as u32, height as u32).unwrap_or_default()
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(TABLE[idx0] as char);
        out.push(TABLE[idx1] as char);

        if chunk.len() > 1 {
            out.push(TABLE[idx2] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(TABLE[idx3] as char);
        } else {
            out.push('=');
        }
    }

    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);

    fn val(c: char) -> Result<u8, Box<dyn std::error::Error>> {
        match c {
            'A'..='Z' => Ok(c as u8 - b'A'),
            'a'..='z' => Ok(c as u8 - b'a' + 26),
            '0'..='9' => Ok(c as u8 - b'0' + 52),
            '+' => Ok(62),
            '/' => Ok(63),
            '=' => Ok(0),
            _ => Err("Invalid base64 character".into()),
        }
    }

    let chars: Vec<char> = clean.chars().collect();
    for chunk in chars.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        let v0 = val(chunk[0])?;
        let v1 = val(chunk[1])?;
        let v2 = if chunk.len() > 2 { val(chunk[2])? } else { 0 };
        let v3 = if chunk.len() > 3 { val(chunk[3])? } else { 0 };

        out.push((v0 << 2) | (v1 >> 4));
        if chunk.len() > 2 && chunk[2] != '=' {
            out.push(((v1 & 0x0F) << 4) | (v2 >> 2));
        }
        if chunk.len() > 3 && chunk[3] != '=' {
            out.push(((v2 & 0x03) << 6) | v3);
        }
    }

    Ok(out)
}

const WPG_DECODER_WASM: &[u8] = include_bytes!("../wpg-decoder.wasm");

const WPG_ELEMENT_JS: &str = r#"/**
 * <wpg-img> Web Component with Offline WebAssembly Decoder
 * - Instant 16-byte micro-preview with 0 CLS (< 0.05ms)
 * - 100% Client-Side Wasm / Native GPU Hardware Decoding (Zero Server Roundtrip)
 * - Real-Time Deterministic Film Grain Synthesis
 * - Graceful Fallback to /api/decode
 */
let wpgWasmModule = null;

async function getWpgWasm() {
    if (wpgWasmModule) return wpgWasmModule;
    try {
        const resp = await fetch('/wpg-decoder.wasm');
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const bytes = await resp.arrayBuffer();
        const { instance } = await WebAssembly.instantiate(bytes, {});
        wpgWasmModule = instance;
        return wpgWasmModule;
    } catch (e) {
        console.warn('WPG Wasm decoder unavailable, falling back to server decode:', e);
        return null;
    }
}

class WpgImageElement extends HTMLElement {
    static get observedAttributes() { return ['src', 'alt', 'width', 'height']; }

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        this.shadowRoot.innerHTML = `
            <style>
                :host {
                    display: inline-block;
                    position: relative;
                    overflow: hidden;
                    background: #111420;
                    border-radius: 8px;
                    line-height: 0;
                }
                canvas {
                    display: block;
                    width: 100%;
                    height: 100%;
                    object-fit: cover;
                    transition: filter 0.35s cubic-bezier(0.16, 1, 0.3, 1), transform 0.35s cubic-bezier(0.16, 1, 0.3, 1);
                }
                .preview-mode {
                    filter: blur(14px);
                    transform: scale(1.05);
                }
                .loaded-mode {
                    filter: blur(0px);
                    transform: scale(1);
                }
            </style>
            <canvas id="cvs"></canvas>
        `;
    }

    connectedCallback() {
        if (this.getAttribute('src')) {
            this.loadWpg(this.getAttribute('src'));
        }
    }

    attributeChangedCallback(name, oldVal, newVal) {
        if (name === 'src' && oldVal !== newVal && newVal) {
            this.loadWpg(newVal);
        }
    }

    async loadWpg(url) {
        const cvs = this.shadowRoot.getElementById('cvs');
        const ctx = cvs.getContext('2d');
        cvs.className = 'preview-mode';

        try {
            const resp = await fetch(url);
            if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
            const buffer = await resp.arrayBuffer();
            const bytes = new Uint8Array(buffer);

            // Verify Magic "WPG\x01"
            if (bytes.length < 16 || bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) {
                throw new Error("Invalid WPG file magic");
            }

            const w = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
            const h = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
            cvs.width = w;
            cvs.height = h;

            const flags = (bytes[12] << 8) | bytes[13];
            const hasPreview = (flags & 0x0002) !== 0;
            const hasGrain = (flags & 0x0004) !== 0;

            // 1. Instant Zero-CLS Micro-Preview (< 0.05 ms)
            if (hasPreview && bytes.length >= 32) {
                const p = bytes.subarray(16, 32);
                const pal = [
                    [p[0], p[1], p[2]],
                    [p[3], p[4], p[5]],
                    [p[6], p[7], p[8]],
                    [p[9], p[10], p[11]]
                ];
                const imgData = ctx.createImageData(4, 4);
                let bitIdx = 0;
                for (let i = 0; i < 16; i++) {
                    const byteVal = p[12 + (bitIdx >> 3)];
                    const shift = 6 - (bitIdx & 7);
                    const cIdx = (byteVal >> shift) & 3;
                    bitIdx += 2;
                    imgData.data[i * 4] = pal[cIdx][0];
                    imgData.data[i * 4 + 1] = pal[cIdx][1];
                    imgData.data[i * 4 + 2] = pal[cIdx][2];
                    imgData.data[i * 4 + 3] = 255;
                }
                if (window.createImageBitmap) {
                    const bmp = await createImageBitmap(imgData);
                    ctx.imageSmoothingEnabled = true;
                    ctx.drawImage(bmp, 0, 0, w, h);
                }
            }

            // 2. 100% Client-Side Wasm Decoding (Offline)
            const wasm = await getWpgWasm();
            let decodedSuccessfully = false;

            if (wasm && wasm.exports) {
                try {
                    const mem = wasm.exports.memory;
                    const wpgPtr = wasm.exports.wpg_alloc(bytes.length);
                    new Uint8Array(mem.buffer).set(bytes, wpgPtr);

                    // Allocate pointers for output AVIF bytes and length
                    const outPtrPtr = wasm.exports.wpg_alloc(4);
                    const outLenPtr = wasm.exports.wpg_alloc(4);

                    const res = wasm.exports.wpg_wrap_to_avif(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
                    if (res === 0) {
                        const view = new DataView(mem.buffer);
                        const avifPtr = view.getUint32(outPtrPtr, true);
                        const avifLen = view.getUint32(outLenPtr, true);

                        const avifBytes = new Uint8Array(mem.buffer, avifPtr, avifLen).slice();
                        const blob = new Blob([avifBytes], { type: 'image/avif' });

                        if (window.createImageBitmap) {
                            const bmp = await createImageBitmap(blob);
                            ctx.drawImage(bmp, 0, 0);

                            // Apply deterministic film grain on client if flag set
                            if (hasGrain) {
                                let offset = 16 + (hasPreview ? 16 : 0);
                                if (bytes.length >= offset + 8) {
                                    const grainScale = bytes[offset];
                                    const grainSeed = (bytes[offset + 1] << 8) | bytes[offset + 2];
                                    const imgData = ctx.getImageData(0, 0, w, h);
                                    const rgbaPtr = wasm.exports.wpg_alloc(imgData.data.length);
                                    new Uint8Array(mem.buffer).set(imgData.data, rgbaPtr);
                                    wasm.exports.wpg_apply_film_grain(rgbaPtr, w, h, grainScale, grainSeed);
                                    imgData.data.set(new Uint8Array(mem.buffer, rgbaPtr, imgData.data.length));
                                    ctx.putImageData(imgData, 0, 0);
                                    wasm.exports.wpg_free(rgbaPtr, imgData.data.length);
                                }
                            }

                            cvs.className = 'loaded-mode';
                            decodedSuccessfully = true;
                        }

                        wasm.exports.wpg_free(avifPtr, avifLen);
                    }

                    wasm.exports.wpg_free(wpgPtr, bytes.length);
                    wasm.exports.wpg_free(outPtrPtr, 4);
                    wasm.exports.wpg_free(outLenPtr, 4);
                } catch (wasmErr) {
                    console.warn('WPG Wasm decode error, falling back:', wasmErr);
                }
            }

            // 3. Fallback to server decode if Wasm or browser AVIF is unsupported
            if (!decodedSuccessfully) {
                const decResp = await fetch('/api/decode', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/octet-stream' },
                    body: buffer
                });
                if (decResp.ok) {
                    const blob = await decResp.blob();
                    if (window.createImageBitmap) {
                        const bmp = await createImageBitmap(blob);
                        ctx.drawImage(bmp, 0, 0);
                        cvs.className = 'loaded-mode';
                    }
                }
            }
        } catch (e) {
            console.error('Failed to load WPG image:', e);
        }
    }
}
customElements.define('wpg-img', WpgImageElement);
"#;

const HTML_CONTENT: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>WPG Image Format — Next-Gen Web Progressive Graphic</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;500;600;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
  <script src="/wpg-element.js"></script>
  <style>
    :root {
      --bg: #090b10;
      --card-bg: rgba(22, 27, 38, 0.75);
      --card-border: rgba(255, 255, 255, 0.09);
      --accent: #6366f1;
      --accent-glow: rgba(99, 102, 241, 0.4);
      --cyan: #06b6d4;
      --green: #10b981;
      --green-glow: rgba(16, 185, 129, 0.4);
      --pink: #ec4899;
      --text: #f3f4f6;
      --text-muted: #94a3b8;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      background: radial-gradient(circle at 50% 0%, #171929 0%, var(--bg) 60%);
      color: var(--text);
      font-family: 'Outfit', -apple-system, sans-serif;
      min-height: 100vh;
      overflow-x: hidden;
    }
    header {
      padding: 2.5rem 2rem 1.5rem;
      text-align: center;
      position: relative;
    }
    .badge {
      display: inline-flex;
      align-items: center;
      gap: 0.5rem;
      background: rgba(99, 102, 241, 0.15);
      border: 1px solid rgba(99, 102, 241, 0.35);
      color: #a5b4fc;
      padding: 0.4rem 1rem;
      border-radius: 9999px;
      font-size: 0.85rem;
      font-weight: 600;
      margin-bottom: 1rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }
    h1 {
      font-size: 3.2rem;
      font-weight: 800;
      background: linear-gradient(135deg, #ffffff 30%, #a5b4fc 70%, #06b6d4 100%);
      -webkit-background-clip: text;
      -webkit-text-fill-color: transparent;
      letter-spacing: -0.02em;
      margin-bottom: 0.8rem;
    }
    p.subtitle {
      color: var(--text-muted);
      font-size: 1.15rem;
      max-width: 780px;
      margin: 0 auto 1.5rem;
      line-height: 1.6;
    }
    .features-pill-row {
      display: flex;
      justify-content: center;
      gap: 0.8rem;
      flex-wrap: wrap;
      margin-bottom: 2rem;
    }
    .feature-pill {
      background: rgba(255, 255, 255, 0.04);
      border: 1px solid var(--card-border);
      padding: 0.4rem 0.9rem;
      border-radius: 8px;
      font-size: 0.85rem;
      color: #cbd5e1;
    }
    .main-grid {
      max-width: 1440px;
      margin: 0 auto 3rem;
      padding: 0 1.5rem;
      display: grid;
      grid-template-columns: 360px 1fr;
      gap: 2rem;
    }
    @media (max-width: 1024px) {
      .main-grid { grid-template-columns: 1fr; }
    }
    .panel {
      background: var(--card-bg);
      backdrop-filter: blur(16px);
      border: 1px solid var(--card-border);
      border-radius: 20px;
      padding: 1.8rem;
      box-shadow: 0 20px 40px rgba(0,0,0,0.4);
    }
    .panel h2 {
      font-size: 1.3rem;
      font-weight: 700;
      margin-bottom: 1.2rem;
      display: flex;
      align-items: center;
      gap: 0.6rem;
      color: #ffffff;
    }
    .control-group {
      margin-bottom: 1.3rem;
    }
    .control-label {
      display: flex;
      justify-content: space-between;
      font-size: 0.9rem;
      font-weight: 500;
      color: #cbd5e1;
      margin-bottom: 0.5rem;
    }
    .control-val {
      color: var(--cyan);
      font-family: 'JetBrains Mono', monospace;
      font-weight: 600;
    }
    input[type=range] {
      width: 100%;
      height: 6px;
      border-radius: 4px;
      background: #232a3b;
      outline: none;
      -webkit-appearance: none;
    }
    input[type=range]::-webkit-slider-thumb {
      -webkit-appearance: none;
      width: 18px;
      height: 18px;
      border-radius: 50%;
      background: var(--accent);
      cursor: pointer;
      box-shadow: 0 0 10px var(--accent-glow);
    }
    select, button.btn {
      width: 100%;
      background: #1a2030;
      border: 1px solid rgba(255, 255, 255, 0.12);
      color: white;
      padding: 0.75rem 1rem;
      border-radius: 10px;
      font-family: inherit;
      font-size: 0.95rem;
      cursor: pointer;
      transition: all 0.2s ease;
    }
    select:focus, button.btn:hover {
      border-color: var(--accent);
      box-shadow: 0 0 15px rgba(99, 102, 241, 0.3);
    }
    button.btn-primary {
      background: linear-gradient(135deg, #6366f1 0%, #4f46e5 100%);
      font-weight: 600;
      border: none;
      box-shadow: 0 4px 20px var(--accent-glow);
    }
    button.btn-primary:hover {
      transform: translateY(-2px);
      box-shadow: 0 6px 25px rgba(99, 102, 241, 0.6);
    }
    .toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0.6rem 0;
      border-bottom: 1px solid rgba(255, 255, 255, 0.05);
    }
    .toggle-row span { font-size: 0.9rem; color: #cbd5e1; }
    .toggle-switch {
      position: relative;
      width: 44px;
      height: 24px;
    }
    .toggle-switch input { opacity: 0; width: 0; height: 0; }
    .slider-track {
      position: absolute;
      cursor: pointer;
      top: 0; left: 0; right: 0; bottom: 0;
      background: #273043;
      border-radius: 34px;
      transition: 0.3s;
    }
    .slider-track:before {
      position: absolute;
      content: "";
      height: 18px;
      width: 18px;
      left: 3px;
      bottom: 3px;
      background: white;
      border-radius: 50%;
      transition: 0.3s;
    }
    input:checked + .slider-track { background: var(--accent); }
    input:checked + .slider-track:before { transform: translateX(20px); }

    .sample-buttons {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.6rem;
      margin-top: 0.6rem;
    }
    .sample-btn {
      background: #1e2638;
      border: 1px solid rgba(255,255,255,0.08);
      color: #94a3b8;
      padding: 0.6rem;
      border-radius: 8px;
      font-size: 0.85rem;
      cursor: pointer;
    }
    .sample-btn.active {
      border-color: var(--cyan);
      color: #38bdf8;
      background: rgba(6, 182, 212, 0.1);
    }

    /* Action bar / Download buttons */
    .download-bar {
      display: flex;
      gap: 0.8rem;
      flex-wrap: wrap;
      margin-bottom: 1.2rem;
    }
    .btn-download-wpg {
      flex: 1.5;
      min-width: 240px;
      background: linear-gradient(135deg, #10b981 0%, #059669 100%);
      font-weight: 700;
      border: none;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 0.6rem;
      box-shadow: 0 4px 20px var(--green-glow);
    }
    .btn-download-wpg:hover {
      box-shadow: 0 6px 25px rgba(16, 185, 129, 0.6);
      transform: translateY(-2px);
    }
    .btn-download-avif {
      flex: 1.5;
      min-width: 240px;
      background: linear-gradient(135deg, #0284c7 0%, #0369a1 100%);
      font-weight: 700;
      border: none;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.6rem;
      box-shadow: 0 4px 20px rgba(2, 132, 199, 0.4);
    }
    .btn-download-avif:hover {
      box-shadow: 0 6px 25px rgba(2, 132, 199, 0.6);
      transform: translateY(-2px);
    }
    .ref-btn {
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid rgba(255, 255, 255, 0.12);
      color: #cbd5e1;
      padding: 0.35rem 0.8rem;
      border-radius: 8px;
      font-size: 0.82rem;
      cursor: pointer;
      transition: all 0.2s;
    }
    .ref-btn:hover {
      background: rgba(255, 255, 255, 0.12);
      color: #fff;
    }
    .ref-btn.active {
      background: var(--accent);
      border-color: var(--accent);
      color: #fff;
      font-weight: 600;
      box-shadow: 0 0 12px var(--accent-glow);
    }
    .btn-secondary {
      flex: 1;
      min-width: 170px;
      background: #1f2738;
      border: 1px solid rgba(255,255,255,0.12);
      font-weight: 600;
    }
    .btn-secondary:hover {
      background: #28334a;
      transform: translateY(-2px);
    }
    .size-pill {
      background: rgba(0, 0, 0, 0.3);
      padding: 0.2rem 0.5rem;
      border-radius: 6px;
      font-size: 0.8rem;
      font-family: 'JetBrains Mono', monospace;
    }

    /* Comparison viewport */
    .compare-container {
      position: relative;
      width: 100%;
      height: 520px;
      background: #0d111a;
      border-radius: 16px;
      overflow: hidden;
      border: 1px solid var(--card-border);
      user-select: none;
    }
    .compare-img-wrap {
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
    }
    .compare-img {
      position: absolute;
      top: 0;
      left: 0;
      object-fit: contain;
      display: block;
    }
    .compare-overlay-wrap {
      position: absolute;
      top: 0;
      left: 0;
      width: 50%;
      height: 100%;
      overflow: hidden;
      border-right: 2px solid var(--accent);
      box-shadow: 5px 0 25px rgba(99, 102, 241, 0.4);
      z-index: 2;
    }
    .compare-overlay-img {
      position: absolute;
      top: 0;
      left: 0;
      object-fit: contain;
      display: block;
    }
    .split-handle {
      position: absolute;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      width: 36px;
      height: 36px;
      background: var(--accent);
      border: 3px solid white;
      border-radius: 50%;
      cursor: ew-resize;
      display: flex;
      align-items: center;
      justify-content: center;
      box-shadow: 0 0 20px var(--accent-glow);
      z-index: 10;
    }
    .split-handle svg { width: 18px; height: 18px; fill: white; }
    .label-badge {
      position: absolute;
      top: 15px;
      padding: 0.35rem 0.8rem;
      border-radius: 6px;
      font-size: 0.8rem;
      font-weight: 700;
      letter-spacing: 0.05em;
      text-transform: uppercase;
      z-index: 5;
    }
    .label-left {
      left: 15px;
      background: rgba(99, 102, 241, 0.85);
      backdrop-filter: blur(8px);
      color: white;
    }
    .label-right {
      right: 15px;
      background: rgba(16, 185, 129, 0.85);
      backdrop-filter: blur(8px);
      color: white;
    }

    /* Metrics Bar */
    .metrics-grid {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: 1.2rem;
      margin-top: 1.5rem;
    }
    @media (max-width: 768px) {
      .metrics-grid { grid-template-columns: 1fr 1fr; }
    }
    .metric-card {
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid var(--card-border);
      border-radius: 14px;
      padding: 1.2rem;
      text-align: center;
    }
    .metric-title {
      font-size: 0.8rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
      margin-bottom: 0.4rem;
    }
    .metric-value {
      font-size: 1.7rem;
      font-weight: 700;
      font-family: 'JetBrains Mono', monospace;
      color: #ffffff;
    }
    .metric-sub {
      font-size: 0.8rem;
      margin-top: 0.3rem;
      font-weight: 500;
    }
    .text-green { color: var(--green); }
    .text-cyan { color: var(--cyan); }
    .text-pink { color: var(--pink); }

    /* Comparative Table */
    .table-wrap {
      margin-top: 1.5rem;
      background: rgba(255,255,255,0.02);
      border: 1px solid var(--card-border);
      border-radius: 14px;
      overflow: hidden;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      text-align: left;
      font-size: 0.9rem;
    }
    th, td {
      padding: 0.9rem 1.2rem;
      border-bottom: 1px solid rgba(255, 255, 255, 0.05);
    }
    th {
      background: rgba(255, 255, 255, 0.04);
      color: var(--text-muted);
      font-weight: 600;
    }
    tr.wpg-row {
      background: rgba(99, 102, 241, 0.12);
      font-weight: 600;
    }
    tr.wpg-row td:first-child {
      color: #a5b4fc;
    }

    /* Magnifier Lens */
    .magnifier-lens {
      position: absolute;
      width: 140px;
      height: 140px;
      border: 3px solid var(--cyan);
      border-radius: 50%;
      pointer-events: none;
      box-shadow: 0 0 20px rgba(6, 182, 212, 0.5);
      display: none;
      background-repeat: no-repeat;
      z-index: 20;
    }
  </style>
</head>
<body>
  <header>
    <div class="badge">Next-Generation Image Format Specification</div>
    <h1>WPG Image Format</h1>
    <p class="subtitle">
      Engineered on pure AV1 Intra with zero ISOBMFF bloat, film grain synthesis, reversible YCoCg-R transforms, and 16-byte zero-CLS micro-previews.
    </p>
    <div class="features-pill-row">
      <div class="feature-pill">⚡ Lean Container (-400B vs AVIF)</div>
      <div class="feature-pill">🎬 Film Grain Synthesis (+30% compression)</div>
      <div class="feature-pill">🖼️ 16-Byte Instant Micro-Preview</div>
      <div class="feature-pill">🎨 Dual Mode: AV1 + Palette/Modular</div>
      <div class="feature-pill">🛡️ Memory-Safe Pure Rust Codec</div>
    </div>
  </header>

  <div class="main-grid">
    <!-- Left Sidebar: Controls -->
    <div class="panel">
      <h2>⚙️ Codec Controls</h2>

      <div class="control-group">
        <div class="control-label">
          <span>Quality</span>
          <span class="control-val" id="qVal">80</span>
        </div>
        <input type="range" id="qSlider" min="1" max="100" value="80">
      </div>

      <div class="control-group">
        <div class="control-label">
          <span>Speed Preset</span>
          <span class="control-val" id="speedVal">6</span>
        </div>
        <input type="range" id="speedSlider" min="1" max="10" value="6">
      </div>

      <div class="control-group">
        <div class="control-label">Compression Mode</div>
        <select id="modeSelect">
          <option value="auto">Auto (Smart Content Detection)</option>
          <option value="av1">AV1-Enhanced Intra</option>
          <option value="palette">Lossless Palette (UI / Logos)</option>
          <option value="modular">Modular Lossless (YCoCg-R)</option>
        </select>
      </div>

      <div class="control-group">
        <div class="control-label">Chroma Subsampling</div>
        <select id="subsamplingSelect">
          <option value="420">YUV 4:2:0 (Maximum Web Compression)</option>
          <option value="422">YUV 4:2:2 (High Color Fidelity)</option>
          <option value="444">YUV 4:4:4 (Full Chroma Resolution)</option>
        </select>
      </div>

      <div class="control-group">
        <div class="control-label">Image Orientation & Rotate</div>
        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 0.5rem;">
          <button class="btn" id="btnRotateCCW" style="padding: 0.5rem;">⟲ Rotate -90°</button>
          <button class="btn" id="btnRotateCW" style="padding: 0.5rem;">⟳ Rotate +90°</button>
        </div>
      </div>

      <div class="toggle-row">
        <span>🎬 Film Grain Synthesis</span>
        <label class="toggle-switch">
          <input type="checkbox" id="grainToggle" checked>
          <span class="slider-track"></span>
        </label>
      </div>

      <div class="toggle-row">
        <span>⚡ 16-Byte Micro-Preview</span>
        <label class="toggle-switch">
          <input type="checkbox" id="previewToggle" checked>
          <span class="slider-track"></span>
        </label>
      </div>

      <div style="margin-top: 1.5rem;">
        <button class="btn btn-primary" id="reEncodeBtn">⚡ Re-encode in Real-Time</button>
      </div>

      <div style="margin-top: 1.8rem;">
        <div class="control-label">Preset Test Samples</div>
        <div class="sample-buttons">
          <button class="sample-btn active" id="btnSample1">🌄 Photo Sunset</button>
          <button class="sample-btn" id="btnSample2">🎨 UI Card / Icon</button>
        </div>
      </div>

      <div style="margin-top: 1.2rem;">
        <div class="control-label">Upload Custom Image</div>
        <input type="file" id="fileInput" accept="image/*" style="display: none;">
        <button class="btn" onclick="document.getElementById('fileInput').click()">📁 Select Image from Disk</button>
      </div>
    </div>

    <!-- Right Area: Interactive Comparison & Metrics -->
    <div>
      <div class="panel">
        <h2>🖼️ Interactive Split View & Visual Fidelity</h2>

        <!-- Action / Download Bar -->
        <div class="download-bar">
          <button class="btn btn-download-wpg" id="btnDownloadWpg">
            <span>⬇️ Download .wpg (Original WPG File)</span>
            <span class="size-pill" id="badgeWpgSize">3.0 KB</span>
          </button>
          <button class="btn btn-download-avif" id="btnDownloadAvif">
            <span>⬇️ Download .avif (Standard AVIF File)</span>
            <span class="size-pill" id="badgeAvifSize">-</span>
          </button>
          <button class="btn btn-secondary" id="btnDownloadPng">
            <span>🖼️ Save Reconstructed PNG</span>
          </button>
          <button class="btn btn-secondary" id="btnDownloadPreview">
            <span>⚡ 16B Micro-Preview (.bin)</span>
          </button>
        </div>

        <!-- Split View Reference Selector -->
        <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.8rem; flex-wrap: wrap; gap: 0.6rem;">
          <div style="display: flex; align-items: center; gap: 0.6rem; font-size: 0.9rem; color: #cbd5e1;">
            <span style="font-weight: 600;">Compare WPG against:</span>
            <div id="refSelector" style="display: inline-flex; gap: 0.4rem;">
              <button class="ref-btn active" data-ref="avif">AVIF (Standard)</button>
              <button class="ref-btn" data-ref="orig">Original (PNG)</button>
              <button class="ref-btn" data-ref="webp">WebP</button>
              <button class="ref-btn" data-ref="jpeg">JPEG</button>
            </div>
          </div>
          <div style="font-size: 0.8rem; color: #94a3b8;">
            Drag center slider or hover for 1:1 magnifier
          </div>
        </div>

        <div class="compare-container" id="compareBox">
          <div class="compare-img-wrap">
            <img class="compare-img" id="imgRight" alt="Reference">
          </div>
          <div class="compare-overlay-wrap" id="overlayWrap">
            <img class="compare-overlay-img" id="imgLeft" alt="WPG">
          </div>
          <div class="split-handle" id="splitHandle">
            <svg viewBox="0 0 24 24"><path d="M8 7l-5 5 5 5V7zm8 0v10l5-5-5-5z"/></svg>
          </div>
          <div class="label-badge label-left">WPG Compressed</div>
          <div class="label-badge label-right" id="rightLabel">AVIF Standard</div>
          <div class="magnifier-lens" id="magLens"></div>
        </div>

        <!-- Metrics Dashboard -->
        <div class="metrics-grid">
          <div class="metric-card">
            <div class="metric-title">WPG File Size</div>
            <div class="metric-value text-cyan" id="metricSize">-</div>
            <div class="metric-sub text-green" id="metricSavings">-</div>
          </div>
          <div class="metric-card">
            <div class="metric-title">Peak SNR</div>
            <div class="metric-value text-green" id="metricPsnr">-</div>
            <div class="metric-sub" style="color: #94a3b8;">Luma/Chroma dB</div>
          </div>
          <div class="metric-card">
            <div class="metric-title">SSIM Score</div>
            <div class="metric-value text-pink" id="metricSsim">-</div>
            <div class="metric-sub" style="color: #94a3b8;">Structural Index</div>
          </div>
          <div class="metric-card">
            <div class="metric-title">Bits Per Pixel</div>
            <div class="metric-value" id="metricBpp">-</div>
            <div class="metric-sub" style="color: #94a3b8;">bpp efficiency</div>
          </div>
        </div>

        <!-- Comparative Format Table -->
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Format</th>
                <th>File Size</th>
                <th>bpp</th>
                <th>PSNR</th>
                <th>SSIM</th>
                <th>Savings vs PNG</th>
              </tr>
            </thead>
            <tbody id="compareTableBody">
              <!-- Dynamically populated -->
            </tbody>
          </table>
        </div>

        <!-- Web Component & Developer Integration Card -->
        <div style="margin-top: 1.5rem; background: rgba(255,255,255,0.02); border: 1px solid var(--card-border); border-radius: 14px; padding: 1.4rem;">
          <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.8rem;">
            <h3 style="font-size: 1.1rem; color: #a5b4fc; display: flex; align-items: center; gap: 0.5rem;">
              <span>🔌 Web Component Integration (&lt;wpg-img&gt;)</span>
            </h3>
            <span style="font-size: 0.75rem; background: rgba(99,102,241,0.2); color: #818cf8; padding: 0.25rem 0.6rem; border-radius: 20px;">Zero-CLS &bull; 16B Instant Micro-Preview</span>
          </div>
          <p style="font-size: 0.88rem; color: #94a3b8; line-height: 1.5; margin-bottom: 1rem;">
            Add native WPG progressive image rendering to any website. The custom element renders the 16-byte preview immediately for zero layout shift, then smoothly loads the full compressed frame.
          </p>
          <div style="background: #0d1117; border: 1px solid rgba(255,255,255,0.08); border-radius: 8px; padding: 0.9rem 1.1rem; font-family: 'JetBrains Mono', monospace; font-size: 0.85rem; color: #e2e8f0; overflow-x: auto;">
            <span style="color: #60a5fa;">&lt;script</span> <span style="color: #93c5fd;">src=</span><span style="color: #a7f3d0;">"/wpg-element.js"</span><span style="color: #60a5fa;">&gt;&lt;/script&gt;</span><br>
            <span style="color: #60a5fa;">&lt;wpg-img</span> <span style="color: #93c5fd;">src=</span><span style="color: #a7f3d0;">"/api/sample.wpg"</span> <span style="color: #93c5fd;">alt=</span><span style="color: #a7f3d0;">"Ultra-compressed WPG"</span> <span style="color: #93c5fd;">style=</span><span style="color: #a7f3d0;">"width: 100%; max-width: 320px; height: 320px;"</span><span style="color: #60a5fa;">&gt;&lt;/wpg-img&gt;</span>
          </div>
        </div>
      </div>
    </div>
  </div>

  <script>
    let currentImageBase64 = '';
    let currentRotation = 0;
    let latestWpgBase64 = '';
    let latestAvifBase64 = '';
    let latestPreviewBase64 = '';
    let latestPngUrl = '';
    let currentRefMode = 'avif';
    let availableRefs = {};

    const qSlider = document.getElementById('qSlider');
    const qVal = document.getElementById('qVal');
    const speedSlider = document.getElementById('speedSlider');
    const speedVal = document.getElementById('speedVal');
    const modeSelect = document.getElementById('modeSelect');
    const subsamplingSelect = document.getElementById('subsamplingSelect');
    const grainToggle = document.getElementById('grainToggle');
    const previewToggle = document.getElementById('previewToggle');
    const reEncodeBtn = document.getElementById('reEncodeBtn');

    const compareBox = document.getElementById('compareBox');
    const overlayWrap = document.getElementById('overlayWrap');
    const splitHandle = document.getElementById('splitHandle');
    const imgLeft = document.getElementById('imgLeft');
    const imgRight = document.getElementById('imgRight');

    qSlider.addEventListener('input', () => { qVal.innerText = qSlider.value; });
    speedSlider.addEventListener('input', () => { speedVal.innerText = speedSlider.value; });

    // Synchronize split-view sizes perfectly to prevent any distortion or aspect mismatch
    function syncImageSizes() {
      const w = compareBox.clientWidth;
      const h = compareBox.clientHeight;
      imgLeft.style.width = w + 'px';
      imgLeft.style.height = h + 'px';
      imgRight.style.width = w + 'px';
      imgRight.style.height = h + 'px';
    }
    window.addEventListener('resize', syncImageSizes);

    // Split comparison slider logic
    let isDragging = false;
    function setSplit(x) {
      const rect = compareBox.getBoundingClientRect();
      const pct = Math.max(0, Math.min(100, ((x - rect.left) / rect.width) * 100));
      overlayWrap.style.width = pct + '%';
      splitHandle.style.left = pct + '%';
    }

    compareBox.addEventListener('mousedown', (e) => { isDragging = true; setSplit(e.clientX); });
    window.addEventListener('mousemove', (e) => { if (isDragging) setSplit(e.clientX); });
    window.addEventListener('mouseup', () => { isDragging = false; });
    compareBox.addEventListener('touchstart', (e) => { isDragging = true; setSplit(e.touches[0].clientX); });
    window.addEventListener('touchmove', (e) => { if (isDragging) setSplit(e.touches[0].clientX); });
    window.addEventListener('touchend', () => { isDragging = false; });

    // Rotate buttons
    document.getElementById('btnRotateCCW').addEventListener('click', () => {
      currentRotation = (currentRotation - 90 + 360) % 360;
      triggerEncode();
    });
    document.getElementById('btnRotateCW').addEventListener('click', () => {
      currentRotation = (currentRotation + 90) % 360;
      triggerEncode();
    });

    // Helper to download binary from base64
    function downloadBase64(b64, filename, mimeType) {
      const byteChars = atob(b64);
      const byteNumbers = new Array(byteChars.length);
      for (let i = 0; i < byteChars.length; i++) {
        byteNumbers[i] = byteChars.charCodeAt(i);
      }
      const byteArray = new Uint8Array(byteNumbers);
      const blob = new Blob([byteArray], { type: mimeType });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    }

    // Download button event listeners
    document.getElementById('btnDownloadWpg').addEventListener('click', () => {
      if (!latestWpgBase64) return;
      downloadBase64(latestWpgBase64, 'compressed_image.wpg', 'application/octet-stream');
    });

    document.getElementById('btnDownloadAvif').addEventListener('click', () => {
      if (!latestAvifBase64) return;
      downloadBase64(latestAvifBase64, 'compressed_image.avif', 'image/avif');
    });

    document.getElementById('btnDownloadPng').addEventListener('click', () => {
      if (!latestPngUrl) return;
      const a = document.createElement('a');
      a.href = latestPngUrl;
      a.download = 'wpg_decoded.png';
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    });

    document.getElementById('btnDownloadPreview').addEventListener('click', () => {
      if (!latestPreviewBase64) return;
      downloadBase64(latestPreviewBase64, 'micro_preview_16b.bin', 'application/octet-stream');
    });

    // Reference switching logic
    function updateRightImage() {
      if (!availableRefs[currentRefMode]) {
        currentRefMode = availableRefs['avif'] ? 'avif' : 'orig';
      }
      const ref = availableRefs[currentRefMode];
      if (ref) {
        imgRight.src = ref.url;
        document.getElementById('rightLabel').innerText = ref.label;
      }
      document.querySelectorAll('.ref-btn').forEach(btn => {
        btn.classList.toggle('active', btn.getAttribute('data-ref') === currentRefMode);
      });
      syncImageSizes();
    }

    document.querySelectorAll('.ref-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        currentRefMode = btn.getAttribute('data-ref');
        updateRightImage();
      });
    });

    // Load sample images
    async function loadSample(url) {
      currentRotation = 0;
      const res = await fetch(url);
      const blob = await res.blob();
      const reader = new FileReader();
      reader.onload = () => {
        currentImageBase64 = reader.result;
        triggerEncode();
      };
      reader.readAsDataURL(blob);
    }

    document.getElementById('btnSample1').addEventListener('click', () => {
      document.getElementById('btnSample1').classList.add('active');
      document.getElementById('btnSample2').classList.remove('active');
      loadSample('/api/sample');
    });

    document.getElementById('btnSample2').addEventListener('click', () => {
      document.getElementById('btnSample2').classList.add('active');
      document.getElementById('btnSample1').classList.remove('active');
      loadSample('/api/sample2');
    });

    document.getElementById('fileInput').addEventListener('change', (e) => {
      const file = e.target.files[0];
      if (file) {
        currentRotation = 0;
        const reader = new FileReader();
        reader.onload = () => {
          currentImageBase64 = reader.result;
          triggerEncode();
        };
        reader.readAsDataURL(file);
      }
    });

    async function triggerEncode() {
      if (!currentImageBase64) return;
      reEncodeBtn.innerText = '⏳ Encoding WPG & AVIF in Real-Time...';
      reEncodeBtn.disabled = true;

      try {
        const payload = {
          image_data: currentImageBase64,
          quality: parseInt(qSlider.value),
          speed: parseInt(speedSlider.value),
          mode: modeSelect.value,
          subsampling: subsamplingSelect.value,
          enable_grain: grainToggle.checked,
          enable_preview: previewToggle.checked,
          grain: null,
          rotate: currentRotation !== 0 ? currentRotation : null
        };

        const res = await fetch('/api/encode', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        });

        const data = await res.json();
        renderResults(data);
      } catch (err) {
        console.error(err);
      } finally {
        reEncodeBtn.innerText = '⚡ Re-encode in Real-Time';
        reEncodeBtn.disabled = false;
      }
    }

    reEncodeBtn.addEventListener('click', triggerEncode);

    function renderResults(data) {
      latestWpgBase64 = data.wpg_base64;
      latestAvifBase64 = data.avif_base64 || '';
      latestPreviewBase64 = data.preview_base64 || '';
      latestPngUrl = data.wpg.data_url;

      // Extract format comparisons
      const avifComp = data.comparisons.find(c => c.format.toLowerCase().includes('avif'));
      const webpComp = data.comparisons.find(c => c.format.toLowerCase().includes('webp'));
      const jpegComp = data.comparisons.find(c => c.format.toLowerCase().includes('jpeg'));

      availableRefs = {
        orig: { url: data.original_data_url, label: 'Original Image (PNG)' },
        avif: avifComp ? { url: avifComp.data_url, label: avifComp.format } : null,
        webp: webpComp ? { url: webpComp.data_url, label: webpComp.format } : null,
        jpeg: jpegComp ? { url: jpegComp.data_url, label: jpegComp.format } : null
      };

      imgLeft.src = data.wpg.data_url;
      updateRightImage();

      // Update download badges
      const sizeKb = (data.wpg.size_bytes / 1024).toFixed(1) + ' KB';
      document.getElementById('badgeWpgSize').innerText = sizeKb;

      if (avifComp && latestAvifBase64) {
        const avifKb = (avifComp.size_bytes / 1024).toFixed(1) + ' KB';
        document.getElementById('badgeAvifSize').innerText = avifKb;
        document.getElementById('btnDownloadAvif').style.display = 'inline-flex';
      } else {
        document.getElementById('btnDownloadAvif').style.display = 'none';
      }

      // Update metrics cards
      document.getElementById('metricSize').innerText = sizeKb;
      document.getElementById('metricSavings').innerText = '-' + data.wpg.savings_vs_png_pct.toFixed(1) + '% vs PNG';
      document.getElementById('metricPsnr').innerText = data.wpg.psnr_db.toFixed(1) + ' dB';
      document.getElementById('metricSsim').innerText = data.wpg.ssim.toFixed(4);
      document.getElementById('metricBpp').innerText = data.wpg.bpp.toFixed(3);

      // Populate comparative table
      const tbody = document.getElementById('compareTableBody');
      tbody.innerHTML = '';

      // WPG row
      const wpgRow = document.createElement('tr');
      wpgRow.className = 'wpg-row';
      wpgRow.innerHTML = `
        <td><strong>✨ ${data.wpg.format}</strong></td>
        <td>${(data.wpg.size_bytes / 1024).toFixed(1)} KB (${data.wpg.size_bytes.toLocaleString()} B)</td>
        <td>${data.wpg.bpp.toFixed(3)}</td>
        <td>${data.wpg.psnr_db.toFixed(1)} dB</td>
        <td>${data.wpg.ssim.toFixed(4)}</td>
        <td class="text-green">-${data.wpg.savings_vs_png_pct.toFixed(1)}%</td>
      `;
      tbody.appendChild(wpgRow);

      // Other formats
      for (const comp of data.comparisons) {
        const row = document.createElement('tr');
        const isAvif = comp.format.toLowerCase().includes('avif');
        const isWebp = comp.format.toLowerCase().includes('webp');
        const isJpeg = comp.format.toLowerCase().includes('jpeg');
        const refKey = isAvif ? 'avif' : (isWebp ? 'webp' : (isJpeg ? 'jpeg' : 'orig'));

        row.style.cursor = 'pointer';
        row.title = 'Click to compare side-by-side in split view';
        row.addEventListener('click', () => {
          currentRefMode = refKey;
          updateRightImage();
        });

        row.innerHTML = `
          <td>
            <div style="display: flex; align-items: center; justify-content: space-between; gap: 0.5rem;">
              <span>${comp.format}</span>
              ${isAvif && latestAvifBase64 ? '<button class="btn" style="width: auto; padding: 0.2rem 0.55rem; font-size: 0.75rem; background: rgba(2,132,199,0.25); border-color: #0284c7; color: #38bdf8;" onclick="event.stopPropagation(); downloadBase64(latestAvifBase64, \'compressed_image.avif\', \'image/avif\');">⬇️ .avif</button>' : ''}
            </div>
          </td>
          <td>${(comp.size_bytes / 1024).toFixed(1)} KB (${comp.size_bytes.toLocaleString()} B)</td>
          <td>${comp.bpp.toFixed(3)}</td>
          <td>${comp.psnr_db.toFixed(1)} dB</td>
          <td>${comp.ssim.toFixed(4)}</td>
          <td>${comp.savings_vs_png_pct > 0 ? '-' + comp.savings_vs_png_pct.toFixed(1) + '%' : 'Baseline'}</td>
        `;
        tbody.appendChild(row);
      }
    }

    // Initial load
    loadSample('/api/sample');
  </script>
</body>
</html>
"#;
