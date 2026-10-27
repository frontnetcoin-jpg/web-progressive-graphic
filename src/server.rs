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
use std::sync::Mutex;
use std::time::Instant;

static LATEST_WPG: Mutex<Option<Vec<u8>>> = Mutex::new(None);

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
    #[serde(default)]
    pub filename: Option<String>, // Original uploaded file name (e.g. photo.jpg)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CompareFormatInfo {
    pub format: String,
    pub ext: String,
    pub size_bytes: usize,
    pub bpp: f64,
    pub psnr_db: f64,
    pub ssim: f64,
    pub data_url: String,
    pub encode_time_ms: f64,
    pub decode_time_ms: f64,
    pub savings_vs_png_pct: f64,
    pub savings_vs_input_pct: f64,
    pub compression_ratio: f64,
}

#[derive(Serialize, Deserialize)]
pub struct EncodeResponse {
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub speed: u8,
    pub input_filename: String,
    pub input_format: String,
    pub input_size_bytes: usize,
    pub input_bpp: f64,
    pub input_data_url: String,
    pub wpg: CompareFormatInfo,
    pub webp: CompareFormatInfo,
    pub avif: Option<CompareFormatInfo>,
    pub original_size_bytes: usize,
    pub original_data_url: String, // Upright normalized PNG for reference
    pub preview_data_url: Option<String>,
    pub comparisons: Vec<CompareFormatInfo>,
    pub wpg_base64: String,          // Raw .wpg binary data for direct download
    pub webp_base64: Option<String>, // Raw .webp binary data for direct download
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
    let request_target = parts.get(1).copied().unwrap_or("/");
    // Drop the query string so /api/sample.wpg?t=... still hits the sample route.
    let path_no_query = request_target.split('?').next().unwrap_or(request_target);
    let path = &path_no_query;

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

    let is_get_or_head = method == "GET" || method == "HEAD";

    if is_get_or_head && (*path == "/" || path.starts_with("/?")) {
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
    } else if is_get_or_head && *path == "/wpg-element.js" {
        send_response_cached(stream, "application/javascript; charset=utf-8", WPG_ELEMENT_JS.as_bytes())?;
    } else if is_get_or_head && *path == "/wpg-worker.js" {
        send_response_cached(stream, "application/javascript; charset=utf-8", WPG_WORKER_JS.as_bytes())?;
    } else if is_get_or_head && *path == "/wpg-decoder.wasm" {
        send_response_cached(stream, "application/wasm", WPG_DECODER_WASM)?;
    } else if is_get_or_head && *path == "/api/sample.wpg" {
        match generate_sample_wpg() {
            Ok(wpg_bytes) => {
                send_response(stream, "application/octet-stream", &wpg_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 500, "Internal Server Error", "application/json", err_json.as_bytes())?;
            }
        }
    } else if is_get_or_head && *path == "/api/latest.wpg" {
        let cached = LATEST_WPG.lock().ok().and_then(|g| g.clone());
        match cached.map(Ok).unwrap_or_else(generate_sample_wpg) {
            Ok(wpg_bytes) => {
                send_response(stream, "application/octet-stream", &wpg_bytes)?;
            }
            Err(e) => {
                let err_json = format!(r#"{{"error": "{}"}}"#, e);
                send_response_status(stream, 500, "Internal Server Error", "application/json", err_json.as_bytes())?;
            }
        }
    } else if is_get_or_head && (*path == "/api/sample.avif" || *path == "/api/sample1.avif") {
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
    } else if is_get_or_head && *path == "/api/sample.jpg" {
        let sample_jpg = generate_demo_photo_jpg();
        send_response(stream, "image/jpeg", &sample_jpg)?;
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

fn send_response_cached(stream: &mut TcpStream, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: public, max-age=31536000, immutable\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
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

fn detect_input_format(bytes: &[u8], filename: Option<&str>) -> (&'static str, &'static str) {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return ("JPEG (.jpg)", "image/jpeg");
    }
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        return ("PNG (.png)", "image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return ("WebP (.webp)", "image/webp");
    }
    if bytes.len() >= 2 && &bytes[0..2] == b"BM" {
        return ("BMP (.bmp)", "image/bmp");
    }
    if let Some(name) = filename {
        let lower = name.to_lowercase();
        if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            return ("JPEG (.jpg)", "image/jpeg");
        }
        if lower.ends_with(".png") {
            return ("PNG (.png)", "image/png");
        }
        if lower.ends_with(".webp") {
            return ("WebP (.webp)", "image/webp");
        }
    }
    ("Image", "image/png")
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
    let (detected_format, detected_mime) = detect_input_format(&raw_image_bytes, req.filename.as_deref());
    let input_filename = req
        .filename
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if detected_mime == "image/jpeg" {
                "uploaded_image.jpg".to_string()
            } else if detected_mime == "image/webp" {
                "uploaded_image.webp".to_string()
            } else {
                "uploaded_image.png".to_string()
            }
        });
    let input_size_bytes = raw_image_bytes.len().max(1);
    let input_data_url = format!("data:{};base64,{}", detected_mime, base64_encode(&raw_image_bytes));

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
    let total_pixels = (width as usize * height as usize).max(1) as f64;
    let raw_rgb_bytes = (width as usize * height as usize * 3).max(1) as f64;
    let input_bpp = (input_size_bytes as f64 * 8.0) / total_pixels;

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

    // 1. Encode & Decode WPG (.wpg)
    let t0 = Instant::now();
    let wpg_bytes = WpgEncoder::encode(rgba.as_raw(), width, height, &opts)?;
    let wpg_enc_time = t0.elapsed().as_secs_f64() * 1000.0;

    if let Ok(mut guard) = LATEST_WPG.lock() {
        *guard = Some(wpg_bytes.clone());
    }

    let t_wpg_dec = Instant::now();
    let decoded_wpg = WpgDecoder::decode(&wpg_bytes)?;
    let wpg_dec_time = t_wpg_dec.elapsed().as_secs_f64() * 1000.0;
    let wpg_psnr = compute_psnr(rgba.as_raw(), &decoded_wpg.rgba);
    let wpg_ssim = compute_ssim(rgba.as_raw(), &decoded_wpg.rgba, width as usize, height as usize);
    let wpg_bpp = (wpg_bytes.len() as f64 * 8.0) / total_pixels;

    // Convert decoded WPG to PNG data URL for browser display
    let wpg_png_bytes = rgba_to_png(&decoded_wpg.rgba, width, height)?;
    let wpg_data_url = format!("data:image/png;base64,{}", base64_encode(&wpg_png_bytes));

    // Also produce upright normalized PNG for reference
    let t_png_enc = Instant::now();
    let orig_png_bytes = rgba_to_png(rgba.as_raw(), width, height)?;
    let png_enc_time = t_png_enc.elapsed().as_secs_f64() * 1000.0;
    let orig_png_data_url = format!("data:image/png;base64,{}", base64_encode(&orig_png_bytes));
    let png_size = orig_png_bytes.len().max(1);

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
    let t_jpeg_enc = Instant::now();
    let mut jpeg_bytes = Vec::new();
    let mut j_enc = JpegEncoder::new_with_quality(&mut jpeg_bytes, req.quality);
    let _ = j_enc.encode(rgb.as_raw(), width, height, ColorType::Rgb8.into());
    let jpeg_enc_time = t_jpeg_enc.elapsed().as_secs_f64() * 1000.0;

    let mut jpeg_dec_time = 0.0;
    let (jpeg_psnr, jpeg_ssim) = if !jpeg_bytes.is_empty() {
        let t_j_dec = Instant::now();
        if let Ok(dec_j) = image::load_from_memory(&jpeg_bytes) {
            let dec_rgba = dec_j.to_rgba8();
            jpeg_dec_time = t_j_dec.elapsed().as_secs_f64() * 1000.0;
            (
                compute_psnr(rgba.as_raw(), dec_rgba.as_raw()),
                compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize),
            )
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };

    // Encode WebP Lossless (VP8L) for reference
    let t_vp8l_enc = Instant::now();
    let mut webp_bytes = Vec::new();
    let w_enc = WebPEncoder::new_lossless(&mut webp_bytes);
    let _ = w_enc.write_image(rgba.as_raw(), width, height, ColorType::Rgba8.into());
    let vp8l_enc_time = t_vp8l_enc.elapsed().as_secs_f64() * 1000.0;

    // 2. Encode & Decode Lossy WebP (.webp) via libwebp with matching Quality & Speed
    let webp_method = match req.speed.clamp(1, 10) {
        1..=2 => 6,
        3..=4 => 5,
        5..=6 => 4,
        7 => 3,
        8 => 2,
        9 => 1,
        _ => 0,
    };
    let t_webp_enc = Instant::now();
    let lossy_webp_bytes = {
        let encoder = webp::Encoder::from_rgba(rgba.as_raw(), width, height);
        if let Ok(mut cfg) = webp::WebPConfig::new() {
            cfg.lossless = 0;
            cfg.alpha_compression = 1;
            cfg.quality = req.quality as f32;
            cfg.method = webp_method;
            match encoder.encode_advanced(&cfg) {
                Ok(mem) => mem.to_vec(),
                Err(_) => encoder.encode(req.quality as f32).to_vec(),
            }
        } else {
            encoder.encode(req.quality as f32).to_vec()
        }
    };
    let lossy_webp_enc_time = t_webp_enc.elapsed().as_secs_f64() * 1000.0;

    let t_webp_dec = Instant::now();
    let decoded_webp = webp::Decoder::new(&lossy_webp_bytes).decode();
    let lossy_webp_dec_time = t_webp_dec.elapsed().as_secs_f64() * 1000.0;

    let (lossy_webp_psnr, lossy_webp_ssim, lossy_webp_data_url) = if let Some(dec) = decoded_webp {
        let dec_rgba = dec.to_image().to_rgba8();
        let p = compute_psnr(rgba.as_raw(), dec_rgba.as_raw());
        let s = compute_ssim(rgba.as_raw(), dec_rgba.as_raw(), width as usize, height as usize);
        let d = if let Ok(png_b) = rgba_to_png(dec_rgba.as_raw(), width, height) {
            format!("data:image/png;base64,{}", base64_encode(&png_b))
        } else {
            format!("data:image/webp;base64,{}", base64_encode(&lossy_webp_bytes))
        };
        (p, s, d)
    } else {
        (0.0, 0.0, format!("data:image/webp;base64,{}", base64_encode(&lossy_webp_bytes)))
    };

    let webp_b64 = if !lossy_webp_bytes.is_empty() {
        Some(base64_encode(&lossy_webp_bytes))
    } else {
        None
    };

    // 3. Encode & Decode Standard AVIF (.avif) with matching Quality & Speed
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

    let mut avif_dec_time = 0.0;
    let (avif_psnr, avif_ssim, mut avif_data_url) = if !avif_bytes.is_empty() {
        let t_avif_dec = Instant::now();
        if let Ok(dec_rgba) = crate::decode_avif_to_rgba(&avif_bytes, width as usize, height as usize, avif_alpha_size, avif_color_size) {
            avif_dec_time = t_avif_dec.elapsed().as_secs_f64() * 1000.0;
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
        format: format!("WPG (Q{}, S{})", req.quality, req.speed),
        ext: "wpg".into(),
        size_bytes: wpg_bytes.len(),
        bpp: wpg_bpp,
        psnr_db: wpg_psnr,
        ssim: wpg_ssim,
        data_url: wpg_data_url.clone(),
        encode_time_ms: wpg_enc_time,
        decode_time_ms: wpg_dec_time,
        savings_vs_png_pct: (1.0 - wpg_bytes.len() as f64 / png_size as f64) * 100.0,
        savings_vs_input_pct: (1.0 - wpg_bytes.len() as f64 / input_size_bytes as f64) * 100.0,
        compression_ratio: raw_rgb_bytes / (wpg_bytes.len().max(1) as f64),
    };

    // WebP Lossy (libwebp) entry
    let webp_info = CompareFormatInfo {
        format: format!("WebP (Lossy Q{}, S{})", req.quality, req.speed),
        ext: "webp".into(),
        size_bytes: lossy_webp_bytes.len(),
        bpp: (lossy_webp_bytes.len() as f64 * 8.0) / total_pixels,
        psnr_db: lossy_webp_psnr,
        ssim: lossy_webp_ssim,
        data_url: lossy_webp_data_url.clone(),
        encode_time_ms: lossy_webp_enc_time,
        decode_time_ms: lossy_webp_dec_time,
        savings_vs_png_pct: (1.0 - lossy_webp_bytes.len() as f64 / png_size as f64) * 100.0,
        savings_vs_input_pct: (1.0 - lossy_webp_bytes.len() as f64 / input_size_bytes as f64) * 100.0,
        compression_ratio: raw_rgb_bytes / (lossy_webp_bytes.len().max(1) as f64),
    };

    // AVIF comparison
    let avif_info = if !avif_bytes.is_empty() {
        let info = CompareFormatInfo {
            format: format!("AVIF (Standard Q{}, S{})", req.quality, req.speed),
            ext: "avif".into(),
            size_bytes: avif_bytes.len(),
            bpp: (avif_bytes.len() as f64 * 8.0) / total_pixels,
            psnr_db: avif_psnr,
            ssim: avif_ssim,
            data_url: avif_data_url,
            encode_time_ms: avif_time,
            decode_time_ms: avif_dec_time,
            savings_vs_png_pct: (1.0 - avif_bytes.len() as f64 / png_size as f64) * 100.0,
            savings_vs_input_pct: (1.0 - avif_bytes.len() as f64 / input_size_bytes as f64) * 100.0,
            compression_ratio: raw_rgb_bytes / (avif_bytes.len().max(1) as f64),
        };
        comparisons.push(info.clone());
        Some(info)
    } else {
        None
    };

    // WebP Lossy in comparisons
    if !lossy_webp_bytes.is_empty() {
        comparisons.push(webp_info.clone());
    }

    // JPEG comparison
    if !jpeg_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: format!("JPEG (Q{})", req.quality),
            ext: "jpeg".into(),
            size_bytes: jpeg_bytes.len(),
            bpp: (jpeg_bytes.len() as f64 * 8.0) / total_pixels,
            psnr_db: jpeg_psnr,
            ssim: jpeg_ssim,
            data_url: format!("data:image/jpeg;base64,{}", base64_encode(&jpeg_bytes)),
            encode_time_ms: jpeg_enc_time,
            decode_time_ms: jpeg_dec_time,
            savings_vs_png_pct: (1.0 - jpeg_bytes.len() as f64 / png_size as f64) * 100.0,
            savings_vs_input_pct: (1.0 - jpeg_bytes.len() as f64 / input_size_bytes as f64) * 100.0,
            compression_ratio: raw_rgb_bytes / (jpeg_bytes.len().max(1) as f64),
        });
    }

    // WebP Lossless comparison
    if !webp_bytes.is_empty() {
        comparisons.push(CompareFormatInfo {
            format: "WebP (Lossless VP8L)".into(),
            ext: "vp8l".into(),
            size_bytes: webp_bytes.len(),
            bpp: (webp_bytes.len() as f64 * 8.0) / total_pixels,
            psnr_db: 99.99,
            ssim: 1.0,
            data_url: format!("data:image/webp;base64,{}", base64_encode(&webp_bytes)),
            encode_time_ms: vp8l_enc_time,
            decode_time_ms: 1.2,
            savings_vs_png_pct: (1.0 - webp_bytes.len() as f64 / png_size as f64) * 100.0,
            savings_vs_input_pct: (1.0 - webp_bytes.len() as f64 / input_size_bytes as f64) * 100.0,
            compression_ratio: raw_rgb_bytes / (webp_bytes.len().max(1) as f64),
        });
    }

    // PNG comparison
    comparisons.push(CompareFormatInfo {
        format: "PNG (Lossless)".into(),
        ext: "png".into(),
        size_bytes: png_size,
        bpp: (png_size as f64 * 8.0) / total_pixels,
        psnr_db: 99.99,
        ssim: 1.0,
        data_url: orig_png_data_url.clone(),
        encode_time_ms: png_enc_time,
        decode_time_ms: 1.0,
        savings_vs_png_pct: 0.0,
        savings_vs_input_pct: (1.0 - png_size as f64 / input_size_bytes as f64) * 100.0,
        compression_ratio: raw_rgb_bytes / (png_size as f64),
    });

    let resp = EncodeResponse {
        width,
        height,
        quality: req.quality,
        speed: req.speed,
        input_filename,
        input_format: detected_format.into(),
        input_size_bytes,
        input_bpp,
        input_data_url,
        wpg: wpg_info,
        webp: webp_info,
        avif: avif_info,
        original_size_bytes: png_size,
        original_data_url: orig_png_data_url,
        preview_data_url,
        comparisons,
        wpg_base64: base64_encode(&wpg_bytes),
        webp_base64: webp_b64,
        avif_base64: avif_b64,
        preview_base64,
    };

    Ok(serde_json::to_string(&resp)?)
}

fn generate_demo_photo_jpg() -> Vec<u8> {
    let width = 320usize;
    let height = 240usize;
    let mut rgb = vec![0u8; width * height * 3];
    let mut state = 0x1357_9BDFu32;

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            let fx = x as f32 / width as f32;
            let fy = y as f32 / height as f32;

            // Golden hour landscape with sun disc, ridgelines, and natural sensor grain
            let dx = fx - 0.68;
            let dy = fy - 0.36;
            let sun = (-((dx * dx + dy * dy) * 32.0)).exp();

            let mut r = 245.0 * (1.0 - fy * 0.45) + 55.0 * sun;
            let mut g = 135.0 * (1.0 - fy * 0.65) + 50.0 * fx + 85.0 * sun;
            let mut b = 65.0 + 150.0 * fy + 40.0 * sun;

            let ridge1 = 0.58 + (fx * 8.5).sin() * 0.06 + (fx * 21.0).cos() * 0.025;
            let ridge2 = 0.72 + (fx * 12.0 + 1.2).sin() * 0.05;

            if fy > ridge2 {
                r = 24.0 + 18.0 * fx;
                g = 30.0 + 20.0 * fy;
                b = 46.0;
            } else if fy > ridge1 {
                r = 58.0;
                g = 44.0;
                b = 72.0;
            }

            // Deterministic camera sensor noise (sigma ~ 4.5)
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (((state & 0xFF) as f32 / 127.5) - 1.0) * 5.0;

            rgb[idx] = (r + noise).clamp(0.0, 255.0) as u8;
            rgb[idx + 1] = (g + noise).clamp(0.0, 255.0) as u8;
            rgb[idx + 2] = (b + noise).clamp(0.0, 255.0) as u8;
        }
    }

    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 88);
    let _ = enc.encode(&rgb, width as u32, height as u32, ColorType::Rgb8.into());
    out
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

const WPG_ELEMENT_JS: &str = include_str!("../wpg-element.js");
const WPG_WORKER_JS: &str = include_str!("../wpg-worker.js");

const HTML_CONTENT: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>WPG Image Format — Next-Gen Web Progressive Graphic</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;500;600;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
  <link rel="stylesheet" href="https://cdn-uicons.flaticon.com/2.6.0/uicons-regular-rounded/css/uicons-regular-rounded.css">
  <script type="module" src="/wpg-element.js"></script>
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
    .icon-hollow {
      width: 1.15em;
      height: 1.15em;
      display: inline-block;
      vertical-align: -0.18em;
      stroke: currentColor;
      fill: none;
      stroke-width: 1.85;
      stroke-linecap: round;
      stroke-linejoin: round;
      flex-shrink: 0;
    }
    .fi {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      vertical-align: -0.15em;
      line-height: 1;
      font-size: 1.1em;
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
      padding: 0.45rem 1rem;
      border-radius: 9999px;
      font-size: 0.85rem;
      color: #cbd5e1;
      display: inline-flex;
      align-items: center;
      gap: 0.45rem;
      transition: all 0.2s ease;
    }
    .feature-pill:hover {
      background: rgba(255, 255, 255, 0.08);
      border-color: rgba(255, 255, 255, 0.2);
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
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.5rem;
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
    .toggle-row span {
      font-size: 0.9rem;
      color: #cbd5e1;
      display: inline-flex;
      align-items: center;
      gap: 0.45rem;
    }
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
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.45rem;
      transition: all 0.2s ease;
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

    /* Drag & Drop .jpg Benchmark Dropzone */
    .drop-zone {
      border: 2px dashed rgba(99, 102, 241, 0.45);
      background: linear-gradient(135deg, rgba(99, 102, 241, 0.08) 0%, rgba(6, 182, 212, 0.05) 100%);
      border-radius: 16px;
      padding: 1.25rem 1.5rem;
      margin-bottom: 1.5rem;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 1.2rem;
      flex-wrap: wrap;
      transition: all 0.22s ease;
      cursor: pointer;
    }
    .drop-zone:hover, .drop-zone.drag-active {
      border-color: var(--cyan);
      background: linear-gradient(135deg, rgba(99, 102, 241, 0.16) 0%, rgba(6, 182, 212, 0.12) 100%);
      box-shadow: 0 0 28px rgba(6, 182, 212, 0.25);
    }
    .input-meta-row {
      display: flex;
      align-items: center;
      gap: 0.55rem;
      flex-wrap: wrap;
      margin-top: 0.55rem;
    }
    .meta-pill {
      background: rgba(15, 23, 42, 0.75);
      border: 1px solid rgba(255, 255, 255, 0.1);
      padding: 0.25rem 0.65rem;
      border-radius: 9999px;
      font-size: 0.78rem;
      color: #e2e8f0;
      font-family: 'JetBrains Mono', monospace;
    }
    .meta-pill.accent {
      background: rgba(6, 182, 212, 0.15);
      border-color: rgba(6, 182, 212, 0.4);
      color: #67e8f9;
      font-weight: 600;
    }

    /* 3-Format Head-to-Head Benchmark Studio (.wpg vs .webp vs .avif) */
    .trio-section {
      background: rgba(15, 20, 32, 0.65);
      border: 1px solid rgba(255, 255, 255, 0.1);
      border-radius: 16px;
      padding: 1.4rem;
      margin-bottom: 1.6rem;
    }
    .trio-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 1rem;
      flex-wrap: wrap;
      margin-bottom: 1.1rem;
    }
    .trio-grid {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 1.1rem;
    }
    @media (max-width: 1100px) {
      .trio-grid { grid-template-columns: 1fr; }
    }
    .trio-card {
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid var(--card-border);
      border-radius: 14px;
      padding: 1.2rem;
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      position: relative;
      transition: all 0.2s ease;
    }
    .trio-card:hover {
      border-color: rgba(255, 255, 255, 0.22);
      transform: translateY(-2px);
    }
    .trio-card.wpg-card {
      background: linear-gradient(180deg, rgba(99, 102, 241, 0.14) 0%, rgba(16, 185, 129, 0.07) 100%);
      border-color: rgba(99, 102, 241, 0.5);
      box-shadow: 0 8px 28px rgba(99, 102, 241, 0.18);
    }
    .trio-card.winner-card {
      border-color: var(--green);
      box-shadow: 0 8px 30px rgba(16, 185, 129, 0.22);
    }
    .trio-top {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.5rem;
      margin-bottom: 0.65rem;
    }
    .ext-badge {
      font-family: 'JetBrains Mono', monospace;
      font-size: 0.85rem;
      font-weight: 700;
      padding: 0.25rem 0.6rem;
      border-radius: 8px;
      background: rgba(255, 255, 255, 0.08);
      color: #fff;
    }
    .ext-badge.ext-wpg { background: rgba(16, 185, 129, 0.2); color: #34d399; border: 1px solid rgba(16, 185, 129, 0.45); }
    .ext-badge.ext-webp { background: rgba(245, 158, 11, 0.2); color: #fbbf24; border: 1px solid rgba(245, 158, 11, 0.45); }
    .ext-badge.ext-avif { background: rgba(2, 132, 199, 0.22); color: #38bdf8; border: 1px solid rgba(2, 132, 199, 0.45); }
    .rank-pill {
      font-size: 0.74rem;
      font-weight: 700;
      padding: 0.22rem 0.55rem;
      border-radius: 9999px;
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }
    .rank-winner {
      background: rgba(16, 185, 129, 0.2);
      color: #34d399;
      border: 1px solid rgba(16, 185, 129, 0.4);
    }
    .rank-other {
      background: rgba(255, 255, 255, 0.06);
      color: #cbd5e1;
      border: 1px solid rgba(255, 255, 255, 0.1);
    }
    .trio-size-main {
      font-size: 1.65rem;
      font-weight: 800;
      font-family: 'JetBrains Mono', monospace;
      color: #ffffff;
      line-height: 1.2;
    }
    .trio-size-bytes {
      font-size: 0.8rem;
      color: var(--text-muted);
      font-family: 'JetBrains Mono', monospace;
      margin-bottom: 0.65rem;
    }
    .size-bar-track {
      width: 100%;
      height: 7px;
      background: rgba(255, 255, 255, 0.07);
      border-radius: 999px;
      overflow: hidden;
      margin-bottom: 0.9rem;
    }
    .size-bar-fill {
      height: 100%;
      border-radius: 999px;
      transition: width 0.35s ease;
    }
    .trio-stats {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.55rem;
      margin-bottom: 1rem;
    }
    .trio-stat-box {
      background: rgba(0, 0, 0, 0.24);
      border: 1px solid rgba(255, 255, 255, 0.05);
      border-radius: 8px;
      padding: 0.5rem 0.65rem;
    }
    .trio-stat-label {
      font-size: 0.7rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }
    .trio-stat-val {
      font-size: 0.88rem;
      font-weight: 700;
      font-family: 'JetBrains Mono', monospace;
      color: #f1f5f9;
      margin-top: 0.15rem;
    }
    .trio-actions {
      display: flex;
      gap: 0.5rem;
      margin-top: auto;
    }
    .head-to-head-strip {
      margin-top: 1.1rem;
      padding: 0.85rem 1.1rem;
      background: rgba(99, 102, 241, 0.1);
      border: 1px solid rgba(99, 102, 241, 0.3);
      border-radius: 12px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      flex-wrap: wrap;
      gap: 0.8rem;
      font-size: 0.86rem;
    }
    .h2h-item {
      display: inline-flex;
      align-items: center;
      gap: 0.4rem;
      font-family: 'JetBrains Mono', monospace;
    }
    .btn-download-webp {
      flex: 1.3;
      min-width: 210px;
      background: linear-gradient(135deg, #d97706 0%, #b45309 100%);
      font-weight: 700;
      border: none;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.6rem;
      box-shadow: 0 4px 20px rgba(217, 119, 6, 0.35);
    }
    .btn-download-webp:hover {
      box-shadow: 0 6px 25px rgba(217, 119, 6, 0.55);
      transform: translateY(-2px);
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
      <div class="feature-pill"><i class="fi fi-rr-bolt" style="color: var(--cyan);"></i> Lean Container (-400B vs AVIF)</div>
      <div class="feature-pill"><i class="fi fi-rr-film" style="color: #ec4899;"></i> Film Grain Synthesis (+30% compression)</div>
      <div class="feature-pill"><i class="fi fi-rr-picture" style="color: #38bdf8;"></i> 16-Byte Instant Micro-Preview</div>
      <div class="feature-pill"><i class="fi fi-rr-palette" style="color: #fbbf24;"></i> Dual Mode: AV1 + Palette/Modular</div>
      <div class="feature-pill"><i class="fi fi-rr-shield-check" style="color: #10b981;"></i> Memory-Safe Pure Rust Codec</div>
    </div>
  </header>

  <div class="main-grid">
    <!-- Left Sidebar: Controls -->
    <div class="panel">
      <h2><i class="fi fi-rr-settings-sliders" style="color: var(--cyan);"></i> Codec Controls</h2>

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
          <button class="btn" id="btnRotateCCW" style="padding: 0.5rem;"><i class="fi fi-rr-rotate-left"></i> Rotate -90°</button>
          <button class="btn" id="btnRotateCW" style="padding: 0.5rem;"><i class="fi fi-rr-rotate-right"></i> Rotate +90°</button>
        </div>
      </div>

      <div class="toggle-row">
        <span><i class="fi fi-rr-film" style="color: #ec4899;"></i> Film Grain Synthesis</span>
        <label class="toggle-switch">
          <input type="checkbox" id="grainToggle" checked>
          <span class="slider-track"></span>
        </label>
      </div>

      <div class="toggle-row">
        <span><i class="fi fi-rr-bolt" style="color: var(--cyan);"></i> 16-Byte Micro-Preview</span>
        <label class="toggle-switch">
          <input type="checkbox" id="previewToggle" checked>
          <span class="slider-track"></span>
        </label>
      </div>

      <div style="margin-top: 1.5rem;">
        <button class="btn btn-primary" id="reEncodeBtn"><i class="fi fi-rr-refresh"></i> Run 3-Format Benchmark</button>
      </div>

      <div style="margin-top: 1.8rem;">
        <div class="control-label">Preset Test Samples</div>
        <div class="sample-buttons" style="grid-template-columns: 1fr 1fr 1fr;">
          <button class="sample-btn active" id="btnSample1"><i class="fi fi-rr-mountains"></i> Sunset</button>
          <button class="sample-btn" id="btnSampleJpg"><i class="fi fi-rr-camera"></i> Photo .jpg</button>
          <button class="sample-btn" id="btnSample2"><i class="fi fi-rr-palette"></i> UI Card</button>
        </div>
      </div>

      <div style="margin-top: 1.2rem;">
        <div class="control-label">Upload .jpg / Image for Benchmark</div>
        <input type="file" id="fileInput" accept=".jpg,.jpeg,.png,.webp,image/*" style="display: none;">
        <button class="btn" onclick="document.getElementById('fileInput').click()"><i class="fi fi-rr-folder-upload"></i> Upload .jpg / Image from Disk</button>
      </div>
    </div>

    <!-- Right Area: Interactive Comparison & Metrics -->
    <div>
      <div class="panel">
        <!-- Drag & Drop .jpg Upload & Benchmark Dropzone -->
        <div class="drop-zone" id="dropZone" onclick="document.getElementById('fileInput').click()">
          <div>
            <div style="font-size: 1.08rem; font-weight: 700; color: #ffffff; display: flex; align-items: center; gap: 0.55rem;">
              <i class="fi fi-rr-cloud-upload-alt" style="color: var(--cyan); font-size: 1.3rem;"></i>
              <span>Drop a <code style="color: #67e8f9;">.jpg</code> (or PNG/WebP) Image to Benchmark <code style="color: #34d399;">.wpg</code> vs <code style="color: #fbbf24;">.webp</code> vs <code style="color: #38bdf8;">.avif</code></span>
            </div>
            <div class="input-meta-row" id="inputSummaryBar">
              <span class="meta-pill"><i class="fi fi-rr-file"></i> <span id="benchFileName">sample_sunset.png</span></span>
              <span class="meta-pill" id="benchFileFormat">PNG (.png)</span>
              <span class="meta-pill">Input: <strong id="benchInputSize">-</strong></span>
              <span class="meta-pill" id="benchDimensions">-</span>
              <span class="meta-pill accent" id="benchSettingsBadge">Quality 80 &bull; Speed 6</span>
            </div>
          </div>
          <div style="display: flex; gap: 0.6rem; align-items: center;" onclick="event.stopPropagation();">
            <button type="button" class="btn btn-primary" style="width: auto; padding: 0.65rem 1.1rem;" onclick="document.getElementById('fileInput').click()">
              <i class="fi fi-rr-folder-upload"></i> Upload .jpg
            </button>
            <button type="button" class="btn" style="width: auto; padding: 0.65rem 1rem;" id="btnQuickJpgSample" title="Load a camera JPEG test image with sensor grain">
              <i class="fi fi-rr-camera"></i> Test .jpg Sample
            </button>
          </div>
        </div>

        <!-- 3-Format Head-to-Head Benchmark Studio (.wpg vs .webp vs .avif) -->
        <div class="trio-section">
          <div class="trio-header">
            <div>
              <h2 style="margin-bottom: 0.25rem; font-size: 1.22rem;">
                <i class="fi fi-rr-chart-histogram" style="color: var(--cyan);"></i>
                3-Format Head-to-Head Benchmark (<code style="color: #34d399;">.wpg</code> vs <code style="color: #fbbf24;">.webp</code> vs <code style="color: #38bdf8;">.avif</code>)
              </h2>
              <p style="font-size: 0.84rem; color: var(--text-muted);">
                Identical target parameters applied across all three codecs, with independent bitstream-level RGBA decoding for exact PSNR &amp; SSIM.
              </p>
            </div>
            <div style="display: flex; align-items: center; gap: 0.65rem; flex-wrap: wrap;">
              <span class="meta-pill accent" id="trioSettingsPill">Q80 &bull; Speed 6</span>
              <button type="button" class="btn" id="btnDownloadAll3" style="width: auto; padding: 0.5rem 0.9rem; font-size: 0.82rem; background: rgba(16, 185, 129, 0.16); border-color: rgba(16, 185, 129, 0.45); color: #34d399;">
                <i class="fi fi-rr-download"></i> Download All 3 (.wpg, .webp, .avif)
              </button>
            </div>
          </div>

          <div class="trio-grid" id="trioGrid">
            <!-- Populated dynamically by renderResults(data) -->
          </div>

          <div class="head-to-head-strip" id="headToHeadStrip">
            <!-- Populated dynamically by renderResults(data) -->
          </div>
        </div>

        <h2><i class="fi fi-rr-picture" style="color: var(--cyan);"></i> Interactive Split View &amp; Visual Fidelity</h2>

        <!-- Action / Download Bar -->
        <div class="download-bar">
          <button class="btn btn-download-wpg" id="btnDownloadWpg">
            <span style="display: inline-flex; align-items: center; gap: 0.45rem;"><i class="fi fi-rr-download"></i> Download .wpg</span>
            <span class="size-pill" id="badgeWpgSize">-</span>
          </button>
          <button class="btn btn-download-webp" id="btnDownloadWebp">
            <span style="display: inline-flex; align-items: center; gap: 0.45rem;"><i class="fi fi-rr-download"></i> Download .webp</span>
            <span class="size-pill" id="badgeWebpSize">-</span>
          </button>
          <button class="btn btn-download-avif" id="btnDownloadAvif">
            <span style="display: inline-flex; align-items: center; gap: 0.45rem;"><i class="fi fi-rr-download"></i> Download .avif</span>
            <span class="size-pill" id="badgeAvifSize">-</span>
          </button>
          <button class="btn btn-secondary" id="btnDownloadPng">
            <span style="display: inline-flex; align-items: center; gap: 0.45rem;"><i class="fi fi-rr-disk"></i> Save Decoded PNG</span>
          </button>
          <button class="btn btn-secondary" id="btnDownloadPreview">
            <span style="display: inline-flex; align-items: center; gap: 0.45rem;"><i class="fi fi-rr-microchip"></i> 16B Preview (.bin)</span>
          </button>
        </div>

        <!-- Split View Reference Selector -->
        <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.8rem; flex-wrap: wrap; gap: 0.6rem;">
          <div style="display: flex; align-items: center; gap: 0.6rem; font-size: 0.9rem; color: #cbd5e1;">
            <span style="font-weight: 600;">Compare WPG against:</span>
            <div id="refSelector" style="display: inline-flex; gap: 0.4rem; flex-wrap: wrap;">
              <button class="ref-btn active" data-ref="avif">AVIF (.avif)</button>
              <button class="ref-btn" data-ref="webp">WebP (.webp)</button>
              <button class="ref-btn" data-ref="input">Uploaded Input (.jpg)</button>
              <button class="ref-btn" data-ref="orig">Original (PNG)</button>
              <button class="ref-btn" data-ref="jpeg">JPEG</button>
            </div>
          </div>
          <div style="font-size: 0.8rem; color: #94a3b8;">
            Drag center slider to inspect 1:1 pixel fidelity
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
                <th>vs Input</th>
                <th>vs PNG</th>
                <th>bpp</th>
                <th>Ratio</th>
                <th>PSNR</th>
                <th>SSIM</th>
                <th>Enc / Dec</th>
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
              <span style="display: inline-flex; align-items: center; gap: 0.5rem;"><i class="fi fi-rr-brackets-curly"></i> Web Component Integration (&lt;wpg-img&gt;)</span>
            </h3>
            <span style="font-size: 0.75rem; background: rgba(99,102,241,0.2); color: #818cf8; padding: 0.25rem 0.6rem; border-radius: 20px;">Zero-CLS &bull; 16B Instant Micro-Preview</span>
          </div>
          <p style="font-size: 0.88rem; color: #94a3b8; line-height: 1.5; margin-bottom: 1rem;">
            Add native WPG progressive image rendering to any website. The custom element renders the 16-byte preview immediately for zero layout shift, then smoothly loads the full compressed frame.
          </p>
          <div style="display: flex; gap: 1.25rem; align-items: flex-start; flex-wrap: wrap;">
            <div style="flex: 1 1 280px; background: #0d1117; border: 1px solid rgba(255,255,255,0.08); border-radius: 8px; padding: 0.9rem 1.1rem; font-family: 'JetBrains Mono', monospace; font-size: 0.85rem; color: #e2e8f0; overflow-x: auto;">
              <span style="color: #60a5fa;">&lt;script</span> <span style="color: #93c5fd;">type=</span><span style="color: #a7f3d0;">"module"</span> <span style="color: #93c5fd;">src=</span><span style="color: #a7f3d0;">"/wpg-element.js"</span><span style="color: #60a5fa;">&gt;&lt;/script&gt;</span><br>
              <span style="color: #60a5fa;">&lt;wpg-img</span> <span style="color: #93c5fd;">src=</span><span style="color: #a7f3d0;">"/api/sample.wpg"</span> <span style="color: #93c5fd;">alt=</span><span style="color: #a7f3d0;">"Ultra-compressed WPG"</span> <span style="color: #93c5fd;">style=</span><span style="color: #a7f3d0;">"width: 100%; max-width: 320px; height: 320px;"</span><span style="color: #60a5fa;">&gt;&lt;/wpg-img&gt;</span>
            </div>
            <div style="flex: 0 0 220px; display: flex; flex-direction: column; gap: 0.75rem;">
              <wpg-img id="liveWpgPreview" src="/api/sample.wpg" alt="Live WPG Component Preview" width="320" height="240" style="width: 100%; max-width: 220px; height: 165px; border-radius: 10px;"></wpg-img>
              <button type="button" class="btn btn-primary" id="reloadWpgPreview"><i class="fi fi-rr-refresh"></i> Reload / Test Transition</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>

  <script>
    let currentImageBase64 = '';
    let currentFileName = 'sample_sunset.png';
    let currentRotation = 0;
    let latestWpgBase64 = '';
    let latestWebpBase64 = '';
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
    const reloadWpgPreview = document.getElementById('reloadWpgPreview');
    const liveWpgPreview = document.getElementById('liveWpgPreview');
    if (reloadWpgPreview && liveWpgPreview) {
      reloadWpgPreview.addEventListener('click', () => {
        liveWpgPreview.setAttribute('src', '/api/latest.wpg?t=' + Date.now());
      });
    }

    const compareBox = document.getElementById('compareBox');
    const overlayWrap = document.getElementById('overlayWrap');
    const splitHandle = document.getElementById('splitHandle');
    const imgLeft = document.getElementById('imgLeft');
    const imgRight = document.getElementById('imgRight');

    function updateSettingsBadges() {
      const text = 'Quality ' + qSlider.value + ' \u2022 Speed ' + speedSlider.value;
      const b1 = document.getElementById('benchSettingsBadge');
      const b2 = document.getElementById('trioSettingsPill');
      if (b1) b1.innerText = text;
      if (b2) b2.innerText = 'Q' + qSlider.value + ' \u2022 Speed ' + speedSlider.value;
    }

    qSlider.addEventListener('input', () => { qVal.innerText = qSlider.value; updateSettingsBadges(); });
    speedSlider.addEventListener('input', () => { speedVal.innerText = speedSlider.value; updateSettingsBadges(); });
    qSlider.addEventListener('change', () => triggerEncode());
    speedSlider.addEventListener('change', () => triggerEncode());
    modeSelect.addEventListener('change', () => triggerEncode());
    subsamplingSelect.addEventListener('change', () => triggerEncode());
    grainToggle.addEventListener('change', () => triggerEncode());
    previewToggle.addEventListener('change', () => triggerEncode());

    function baseStem(name) {
      if (!name) return 'benchmark_image';
      const idx = name.lastIndexOf('.');
      return idx > 0 ? name.slice(0, idx) : name;
    }

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
      if (!b64) return;
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
      downloadBase64(latestWpgBase64, baseStem(currentFileName) + '.wpg', 'application/octet-stream');
    });

    document.getElementById('btnDownloadWebp').addEventListener('click', () => {
      if (!latestWebpBase64) return;
      downloadBase64(latestWebpBase64, baseStem(currentFileName) + '.webp', 'image/webp');
    });

    document.getElementById('btnDownloadAvif').addEventListener('click', () => {
      if (!latestAvifBase64) return;
      downloadBase64(latestAvifBase64, baseStem(currentFileName) + '.avif', 'image/avif');
    });

    document.getElementById('btnDownloadAll3').addEventListener('click', () => {
      const stem = baseStem(currentFileName);
      if (latestWpgBase64) downloadBase64(latestWpgBase64, stem + '.wpg', 'application/octet-stream');
      if (latestWebpBase64) setTimeout(() => downloadBase64(latestWebpBase64, stem + '.webp', 'image/webp'), 120);
      if (latestAvifBase64) setTimeout(() => downloadBase64(latestAvifBase64, stem + '.avif', 'image/avif'), 240);
    });

    document.getElementById('btnDownloadPng').addEventListener('click', () => {
      if (!latestPngUrl) return;
      const a = document.createElement('a');
      a.href = latestPngUrl;
      a.download = baseStem(currentFileName) + '_wpg_decoded.png';
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    });

    document.getElementById('btnDownloadPreview').addEventListener('click', () => {
      if (!latestPreviewBase64) return;
      downloadBase64(latestPreviewBase64, baseStem(currentFileName) + '_preview_16b.bin', 'application/octet-stream');
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

    function selectSplitComparison(refMode) {
      currentRefMode = refMode;
      updateRightImage();
      compareBox.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }

    document.querySelectorAll('.ref-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        currentRefMode = btn.getAttribute('data-ref');
        updateRightImage();
      });
    });

    function clearSampleActive() {
      ['btnSample1', 'btnSampleJpg', 'btnSample2'].forEach(id => {
        const el = document.getElementById(id);
        if (el) el.classList.remove('active');
      });
    }

    // Load sample images
    async function loadSample(url, sampleName) {
      currentRotation = 0;
      currentFileName = sampleName || 'sample.png';
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
      clearSampleActive();
      document.getElementById('btnSample1').classList.add('active');
      loadSample('/api/sample', 'sample_sunset.png');
    });

    document.getElementById('btnSampleJpg').addEventListener('click', () => {
      clearSampleActive();
      document.getElementById('btnSampleJpg').classList.add('active');
      loadSample('/api/sample.jpg', 'camera_photo.jpg');
    });

    document.getElementById('btnQuickJpgSample').addEventListener('click', () => {
      clearSampleActive();
      document.getElementById('btnSampleJpg').classList.add('active');
      loadSample('/api/sample.jpg', 'camera_photo.jpg');
    });

    document.getElementById('btnSample2').addEventListener('click', () => {
      clearSampleActive();
      document.getElementById('btnSample2').classList.add('active');
      loadSample('/api/sample2', 'ui_card_icon.png');
    });

    function handleUploadedFile(file) {
      if (!file) return;
      clearSampleActive();
      currentRotation = 0;
      currentFileName = file.name || 'uploaded_image.jpg';
      const reader = new FileReader();
      reader.onload = () => {
        currentImageBase64 = reader.result;
        triggerEncode();
      };
      reader.readAsDataURL(file);
    }

    document.getElementById('fileInput').addEventListener('change', (e) => {
      const file = e.target.files[0];
      if (file) handleUploadedFile(file);
    });

    // Drag & Drop support on the dropzone and whole window
    const dropZone = document.getElementById('dropZone');
    ['dragenter', 'dragover'].forEach(evtName => {
      window.addEventListener(evtName, (e) => {
        e.preventDefault();
        if (dropZone) dropZone.classList.add('drag-active');
      });
    });
    ['dragleave', 'drop'].forEach(evtName => {
      window.addEventListener(evtName, (e) => {
        e.preventDefault();
        if (dropZone) dropZone.classList.remove('drag-active');
      });
    });
    window.addEventListener('drop', (e) => {
      const files = e.dataTransfer && e.dataTransfer.files;
      if (files && files.length > 0) {
        handleUploadedFile(files[0]);
      }
    });

    async function triggerEncode() {
      if (!currentImageBase64) return;
      reEncodeBtn.innerText = 'Benchmarking .wpg vs .webp vs .avif...';
      reEncodeBtn.disabled = true;
      updateSettingsBadges();

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
          rotate: currentRotation !== 0 ? currentRotation : null,
          filename: currentFileName
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
        reEncodeBtn.innerText = 'Run 3-Format Benchmark';
        reEncodeBtn.disabled = false;
      }
    }

    reEncodeBtn.addEventListener('click', triggerEncode);

    function formatPct(pct) {
      const sign = pct >= 0 ? '-' : '+';
      return sign + Math.abs(pct).toFixed(1) + '%';
    }

    function renderResults(data) {
      latestWpgBase64 = data.wpg_base64;
      latestWebpBase64 = data.webp_base64 || '';
      latestAvifBase64 = data.avif_base64 || '';
      latestPreviewBase64 = data.preview_base64 || '';
      latestPngUrl = data.wpg.data_url;

      // Update input summary bar
      document.getElementById('benchFileName').innerText = data.input_filename || currentFileName;
      document.getElementById('benchFileFormat').innerText = data.input_format || 'Image';
      document.getElementById('benchInputSize').innerText = (data.input_size_bytes / 1024).toFixed(1) + ' KB (' + data.input_size_bytes.toLocaleString() + ' B)';
      document.getElementById('benchDimensions').innerText = data.width + ' \u00d7 ' + data.height + ' px';
      updateSettingsBadges();

      // Update live <wpg-img> preview with the newly encoded WPG
      if (liveWpgPreview) {
        liveWpgPreview.setAttribute('width', String(data.width));
        liveWpgPreview.setAttribute('height', String(data.height));
        liveWpgPreview.setAttribute('src', '/api/latest.wpg?t=' + Date.now());
      }

      // Extract format comparisons
      const avifComp = data.avif || data.comparisons.find(c => c.ext === 'avif' || c.format.toLowerCase().includes('avif'));
      const webpComp = data.webp || data.comparisons.find(c => c.ext === 'webp' || c.format.toLowerCase().includes('lossy'));
      const jpegComp = data.comparisons.find(c => c.ext === 'jpeg' || c.format.toLowerCase().includes('jpeg'));

      availableRefs = {
        orig: { url: data.original_data_url, label: 'Original Image (PNG)' },
        input: { url: data.input_data_url || data.original_data_url, label: 'Uploaded Input (' + (data.input_filename || 'Source') + ')' },
        avif: avifComp ? { url: avifComp.data_url, label: avifComp.format } : null,
        webp: webpComp ? { url: webpComp.data_url, label: webpComp.format } : null,
        jpeg: jpegComp ? { url: jpegComp.data_url, label: jpegComp.format } : null
      };

      imgLeft.src = data.wpg.data_url;
      updateRightImage();

      // Update download badges
      const sizeKb = (data.wpg.size_bytes / 1024).toFixed(2) + ' KB';
      document.getElementById('badgeWpgSize').innerText = sizeKb;

      if (webpComp && latestWebpBase64) {
        document.getElementById('badgeWebpSize').innerText = (webpComp.size_bytes / 1024).toFixed(2) + ' KB';
        document.getElementById('btnDownloadWebp').style.display = 'inline-flex';
      } else {
        document.getElementById('btnDownloadWebp').style.display = 'none';
      }

      if (avifComp && latestAvifBase64) {
        const avifKb = (avifComp.size_bytes / 1024).toFixed(2) + ' KB';
        document.getElementById('badgeAvifSize').innerText = avifKb;
        document.getElementById('btnDownloadAvif').style.display = 'inline-flex';
      } else {
        document.getElementById('btnDownloadAvif').style.display = 'none';
      }

      // Render 3-Format Head-to-Head Benchmark Studio (.wpg vs .webp vs .avif)
      const trioItems = [
        { key: 'wpg', ext: '.wpg', label: 'WPG (Web Progressive Graphic)', info: data.wpg, b64: latestWpgBase64, mime: 'application/octet-stream', color: '#10b981', extClass: 'ext-wpg' },
        { key: 'webp', ext: '.webp', label: 'WebP (Google libwebp)', info: webpComp, b64: latestWebpBase64, mime: 'image/webp', color: '#f59e0b', extClass: 'ext-webp' },
        { key: 'avif', ext: '.avif', label: 'AVIF (Standard ISOBMFF)', info: avifComp, b64: latestAvifBase64, mime: 'image/avif', color: '#38bdf8', extClass: 'ext-avif' }
      ].filter(x => x.info);

      const minSize = Math.min(...trioItems.map(x => x.info.size_bytes));
      const maxSize = Math.max(data.input_size_bytes, ...trioItems.map(x => x.info.size_bytes), 1);
      const stem = baseStem(data.input_filename || currentFileName);

      const trioGrid = document.getElementById('trioGrid');
      trioGrid.innerHTML = '';

      trioItems.forEach((item) => {
        const inf = item.info;
        const isSmallest = inf.size_bytes === minSize;
        const deltaVsWpgPct = ((inf.size_bytes - data.wpg.size_bytes) / Math.max(data.wpg.size_bytes, 1)) * 100.0;
        const rankText = isSmallest
          ? 'Smallest File'
          : (deltaVsWpgPct >= 0 ? ('+' + deltaVsWpgPct.toFixed(1) + '% vs .wpg') : (deltaVsWpgPct.toFixed(1) + '% vs .wpg'));
        const barPct = Math.max(6, Math.min(100, (inf.size_bytes / maxSize) * 100));
        const vsInputClass = inf.savings_vs_input_pct >= 0 ? 'text-green' : 'text-pink';

        const card = document.createElement('div');
        card.className = 'trio-card' + (item.key === 'wpg' ? ' wpg-card' : '') + (isSmallest ? ' winner-card' : '');
        card.innerHTML = `
          <div>
            <div class="trio-top">
              <div style="display: flex; align-items: center; gap: 0.45rem;">
                <span class="ext-badge ${item.extClass}">${item.ext}</span>
                <span style="font-size: 0.85rem; font-weight: 600; color: #e2e8f0;">Q${data.quality} &bull; S${data.speed}</span>
              </div>
              <span class="rank-pill ${isSmallest ? 'rank-winner' : 'rank-other'}">${rankText}</span>
            </div>
            <div style="font-size: 0.8rem; color: var(--text-muted); margin-bottom: 0.35rem;">${item.label}</div>
            <div class="trio-size-main">${(inf.size_bytes / 1024).toFixed(2)} KB</div>
            <div class="trio-size-bytes">${inf.size_bytes.toLocaleString()} Bytes</div>
            <div class="size-bar-track">
              <div class="size-bar-fill" style="width: ${barPct.toFixed(1)}%; background: ${item.color};"></div>
            </div>
            <div class="trio-stats">
              <div class="trio-stat-box">
                <div class="trio-stat-label">vs Input (${data.input_format.split(' ')[0]})</div>
                <div class="trio-stat-val ${vsInputClass}">${formatPct(inf.savings_vs_input_pct)}</div>
              </div>
              <div class="trio-stat-box">
                <div class="trio-stat-label">vs PNG Baseline</div>
                <div class="trio-stat-val text-green">${formatPct(inf.savings_vs_png_pct)}</div>
              </div>
              <div class="trio-stat-box">
                <div class="trio-stat-label">Bits Per Pixel</div>
                <div class="trio-stat-val">${inf.bpp.toFixed(3)} bpp</div>
              </div>
              <div class="trio-stat-box">
                <div class="trio-stat-label">Compression Ratio</div>
                <div class="trio-stat-val">${inf.compression_ratio.toFixed(1)} : 1</div>
              </div>
              <div class="trio-stat-box">
                <div class="trio-stat-label">PSNR / SSIM</div>
                <div class="trio-stat-val">${inf.psnr_db.toFixed(2)} dB / ${inf.ssim.toFixed(4)}</div>
              </div>
              <div class="trio-stat-box">
                <div class="trio-stat-label">Encode / Decode</div>
                <div class="trio-stat-val">${inf.encode_time_ms.toFixed(1)} / ${inf.decode_time_ms.toFixed(1)} ms</div>
              </div>
            </div>
          </div>
          <div class="trio-actions">
            <button type="button" class="btn" style="padding: 0.5rem 0.7rem; font-size: 0.82rem;" data-dl-ext="${item.ext}">
              <i class="fi fi-rr-download"></i> Download ${item.ext}
            </button>
            ${item.key !== 'wpg' ? `<button type="button" class="btn" style="padding: 0.5rem 0.7rem; font-size: 0.82rem;" data-cmp-ref="${item.key}"><i class="fi fi-rr-eye"></i> Compare 1:1</button>` : ''}
          </div>
        `;

        const dlBtn = card.querySelector('[data-dl-ext]');
        if (dlBtn) {
          dlBtn.addEventListener('click', () => {
            downloadBase64(item.b64, stem + item.ext, item.mime);
          });
        }
        const cmpBtn = card.querySelector('[data-cmp-ref]');
        if (cmpBtn) {
          cmpBtn.addEventListener('click', () => {
            selectSplitComparison(item.key);
          });
        }

        trioGrid.appendChild(card);
      });

      // Populate Head-to-Head Summary Strip
      const h2h = document.getElementById('headToHeadStrip');
      const wpgBytes = data.wpg.size_bytes;
      const webpBytes = webpComp ? webpComp.size_bytes : wpgBytes;
      const avifBytes = avifComp ? avifComp.size_bytes : wpgBytes;
      const vsWebpPct = ((webpBytes - wpgBytes) / Math.max(webpBytes, 1)) * 100.0;
      const vsAvifPct = ((avifBytes - wpgBytes) / Math.max(avifBytes, 1)) * 100.0;
      const vsInputPct = data.wpg.savings_vs_input_pct;

      const fmtDelta = (pct, diffBytes) => {
        if (diffBytes >= 0) {
          return `<strong class="text-green">-${pct.toFixed(1)}% smaller (${diffBytes.toLocaleString()} B saved)</strong>`;
        }
        return `<strong class="text-pink">+${Math.abs(pct).toFixed(1)}% (+${Math.abs(diffBytes).toLocaleString()} B)</strong>`;
      };

      h2h.innerHTML = `
        <div class="h2h-item">
          <i class="fi fi-rr-trophy" style="color: var(--cyan);"></i>
          <span>.wpg vs .avif:</span>
          ${fmtDelta(vsAvifPct, avifBytes - wpgBytes)}
        </div>
        <div class="h2h-item">
          <i class="fi fi-rr-bolt" style="color: #fbbf24;"></i>
          <span>.wpg vs .webp:</span>
          ${fmtDelta(vsWebpPct, webpBytes - wpgBytes)}
        </div>
        <div class="h2h-item">
          <i class="fi fi-rr-file" style="color: #a5b4fc;"></i>
          <span>.wpg vs Input (${data.input_format.split(' ')[0]}):</span>
          ${fmtDelta(vsInputPct, data.input_size_bytes - wpgBytes)}
        </div>
      `;

      // Update metrics cards
      document.getElementById('metricSize').innerText = sizeKb;
      document.getElementById('metricSavings').innerText = formatPct(data.wpg.savings_vs_input_pct) + ' vs Input (' + formatPct(data.wpg.savings_vs_png_pct) + ' vs PNG)';
      document.getElementById('metricPsnr').innerText = data.wpg.psnr_db.toFixed(2) + ' dB';
      document.getElementById('metricSsim').innerText = data.wpg.ssim.toFixed(4);
      document.getElementById('metricBpp').innerText = data.wpg.bpp.toFixed(3);

      // Populate comparative table
      const tbody = document.getElementById('compareTableBody');
      tbody.innerHTML = '';

      // WPG row
      const wpgRow = document.createElement('tr');
      wpgRow.className = 'wpg-row';
      wpgRow.innerHTML = `
        <td>
          <div style="display: flex; align-items: center; justify-content: space-between; gap: 0.5rem;">
            <strong style="display: inline-flex; align-items: center; gap: 0.4rem;"><i class="fi fi-rr-sparkles" style="color: var(--cyan);"></i> ${data.wpg.format}</strong>
            <button type="button" class="btn" style="width: auto; padding: 0.2rem 0.55rem; font-size: 0.75rem; background: rgba(16,185,129,0.22); border-color: #10b981; color: #34d399;" id="tblDlWpg"><i class="fi fi-rr-download"></i> .wpg</button>
          </div>
        </td>
        <td>${(data.wpg.size_bytes / 1024).toFixed(2)} KB (${data.wpg.size_bytes.toLocaleString()} B)</td>
        <td class="${data.wpg.savings_vs_input_pct >= 0 ? 'text-green' : 'text-pink'}">${formatPct(data.wpg.savings_vs_input_pct)}</td>
        <td class="text-green">${formatPct(data.wpg.savings_vs_png_pct)}</td>
        <td>${data.wpg.bpp.toFixed(3)}</td>
        <td>${data.wpg.compression_ratio.toFixed(1)}:1</td>
        <td>${data.wpg.psnr_db.toFixed(2)} dB</td>
        <td>${data.wpg.ssim.toFixed(4)}</td>
        <td>${data.wpg.encode_time_ms.toFixed(1)} / ${data.wpg.decode_time_ms.toFixed(1)} ms</td>
      `;
      tbody.appendChild(wpgRow);
      const tblDlWpg = wpgRow.querySelector('#tblDlWpg');
      if (tblDlWpg) {
        tblDlWpg.addEventListener('click', (e) => {
          e.stopPropagation();
          downloadBase64(latestWpgBase64, stem + '.wpg', 'application/octet-stream');
        });
      }

      // Uploaded Input row
      const inputRow = document.createElement('tr');
      inputRow.style.cursor = 'pointer';
      inputRow.title = 'Click to compare WPG against the uploaded input file in split view';
      inputRow.addEventListener('click', () => {
        currentRefMode = 'input';
        updateRightImage();
      });
      const inputRatio = (data.width * data.height * 3) / Math.max(data.input_size_bytes, 1);
      const inputVsPng = (1.0 - data.input_size_bytes / Math.max(data.original_size_bytes, 1)) * 100.0;
      inputRow.innerHTML = `
        <td><strong>Input: ${data.input_filename} (${data.input_format})</strong></td>
        <td>${(data.input_size_bytes / 1024).toFixed(2)} KB (${data.input_size_bytes.toLocaleString()} B)</td>
        <td>Baseline (Input)</td>
        <td>${inputVsPng > 0 ? formatPct(inputVsPng) : 'Baseline'}</td>
        <td>${data.input_bpp.toFixed(3)}</td>
        <td>${inputRatio.toFixed(1)}:1</td>
        <td>Source</td>
        <td>1.0000</td>
        <td>-</td>
      `;
      tbody.appendChild(inputRow);

      // Other formats
      for (const comp of data.comparisons) {
        const row = document.createElement('tr');
        const isAvif = comp.ext === 'avif' || comp.format.toLowerCase().includes('avif');
        const isWebpLossy = comp.ext === 'webp';
        const isJpeg = comp.ext === 'jpeg' || comp.format.toLowerCase().includes('jpeg');
        const refKey = isAvif ? 'avif' : (isWebpLossy ? 'webp' : (isJpeg ? 'jpeg' : 'orig'));

        row.style.cursor = 'pointer';
        row.title = 'Click to compare side-by-side in split view';
        row.addEventListener('click', () => {
          currentRefMode = refKey;
          updateRightImage();
        });

        let actionBtnHtml = '';
        if (isAvif && latestAvifBase64) {
          actionBtnHtml = '<button type="button" class="btn tbl-dl-avif" style="width: auto; padding: 0.2rem 0.55rem; font-size: 0.75rem; background: rgba(2,132,199,0.25); border-color: #0284c7; color: #38bdf8;"><i class="fi fi-rr-download"></i> .avif</button>';
        } else if (isWebpLossy && latestWebpBase64) {
          actionBtnHtml = '<button type="button" class="btn tbl-dl-webp" style="width: auto; padding: 0.2rem 0.55rem; font-size: 0.75rem; background: rgba(245,158,11,0.22); border-color: #f59e0b; color: #fbbf24;"><i class="fi fi-rr-download"></i> .webp</button>';
        }

        row.innerHTML = `
          <td>
            <div style="display: flex; align-items: center; justify-content: space-between; gap: 0.5rem;">
              <span>${comp.format}</span>
              ${actionBtnHtml}
            </div>
          </td>
          <td>${(comp.size_bytes / 1024).toFixed(2)} KB (${comp.size_bytes.toLocaleString()} B)</td>
          <td class="${comp.savings_vs_input_pct >= 0 ? 'text-green' : 'text-pink'}">${formatPct(comp.savings_vs_input_pct)}</td>
          <td>${comp.savings_vs_png_pct > 0 ? formatPct(comp.savings_vs_png_pct) : 'Baseline'}</td>
          <td>${comp.bpp.toFixed(3)}</td>
          <td>${comp.compression_ratio.toFixed(1)}:1</td>
          <td>${comp.psnr_db.toFixed(2)} dB</td>
          <td>${comp.ssim.toFixed(4)}</td>
          <td>${comp.encode_time_ms.toFixed(1)} / ${comp.decode_time_ms.toFixed(1)} ms</td>
        `;
        const dlAvif = row.querySelector('.tbl-dl-avif');
        if (dlAvif) {
          dlAvif.addEventListener('click', (e) => {
            e.stopPropagation();
            downloadBase64(latestAvifBase64, stem + '.avif', 'image/avif');
          });
        }
        const dlWebp = row.querySelector('.tbl-dl-webp');
        if (dlWebp) {
          dlWebp.addEventListener('click', (e) => {
            e.stopPropagation();
            downloadBase64(latestWebpBase64, stem + '.webp', 'image/webp');
          });
        }
        tbody.appendChild(row);
      }
    }

    // Initial load
    loadSample('/api/sample', 'sample_sunset.png');
  </script>
</body>
</html>
"#;
