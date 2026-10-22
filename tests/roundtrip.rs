use wpg::{
    compute_psnr, compute_ssim, ColorSubsampling, EncodeOptions, WpgDecoder, WpgEncoder, WpgMode,
};

#[test]
fn test_palette_lossless_roundtrip() {
    let width = 64;
    let height = 64;
    let mut rgba = vec![0u8; width * height * 4];

    // Synthetic image with 4 distinct colors (flat UI / icon)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            if x < 32 && y < 32 {
                rgba[idx..idx + 4].copy_from_slice(&[255, 60, 60, 255]); // Red
            } else if x >= 32 && y < 32 {
                rgba[idx..idx + 4].copy_from_slice(&[60, 255, 60, 255]); // Green
            } else if x < 32 && y >= 32 {
                rgba[idx..idx + 4].copy_from_slice(&[60, 60, 255, 255]); // Blue
            } else {
                rgba[idx..idx + 4].copy_from_slice(&[240, 240, 60, 255]); // Yellow
            }
        }
    }

    let mut opts = EncodeOptions::default();
    opts.mode = Some(WpgMode::PaletteLossless);

    let encoded = WpgEncoder::encode(&rgba, width as u32, height as u32, &opts)
        .expect("Palette encode failed");

    println!("Palette mode compressed size: {} bytes", encoded.len());
    assert!(encoded.len() < 300, "Palette mode should be tiny for 4 colors");

    let decoded = WpgDecoder::decode(&encoded).expect("Palette decode failed");
    assert_eq!(decoded.width, width as u32);
    assert_eq!(decoded.height, height as u32);
    assert_eq!(decoded.rgba, rgba, "Palette mode MUST be 100% bit-exact lossless!");

    let psnr = compute_psnr(&rgba, &decoded.rgba);
    assert!(psnr > 90.0, "PSNR should indicate bit-exact match");
}

#[test]
fn test_modular_lossless_roundtrip() {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; width * height * 4];

    // Smooth gradient image (many unique colors)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            rgba[idx] = (x * 8) as u8;
            rgba[idx + 1] = (y * 8) as u8;
            rgba[idx + 2] = ((x + y) * 4) as u8;
            rgba[idx + 3] = 255;
        }
    }

    let mut opts = EncodeOptions::default();
    opts.mode = Some(WpgMode::ModularLossless);

    let encoded = WpgEncoder::encode(&rgba, width as u32, height as u32, &opts)
        .expect("Modular encode failed");

    let decoded = WpgDecoder::decode(&encoded).expect("Modular decode failed");
    assert_eq!(decoded.rgba, rgba, "Modular YCoCg-R mode MUST be 100% bit-exact lossless!");
}

#[test]
fn test_av1_enhanced_lossy_roundtrip() {
    let width = 64;
    let height = 64;
    let mut rgba = vec![0u8; width * height * 4];

    // Natural gradient scene
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            rgba[idx] = (128.0 + 100.0 * (x as f32 / 10.0).sin()) as u8;
            rgba[idx + 1] = (128.0 + 100.0 * (y as f32 / 10.0).cos()) as u8;
            rgba[idx + 2] = (128.0 + 80.0 * ((x + y) as f32 / 12.0).sin()) as u8;
            rgba[idx + 3] = 255;
        }
    }

    let mut opts = EncodeOptions::default();
    opts.mode = Some(WpgMode::Av1Enhanced);
    opts.quality = 85;
    opts.speed = 8;
    opts.subsampling = ColorSubsampling::Yuv420;
    opts.enable_preview = true;
    opts.enable_grain = true;

    let encoded = WpgEncoder::encode(&rgba, width as u32, height as u32, &opts)
        .expect("AV1 encode failed");

    println!("AV1 Enhanced compressed size: {} bytes", encoded.len());

    let decoded = WpgDecoder::decode(&encoded).expect("AV1 decode failed");
    assert_eq!(decoded.width, width as u32);
    assert_eq!(decoded.height, height as u32);

    let psnr = compute_psnr(&rgba, &decoded.rgba);
    let ssim = compute_ssim(&rgba, &decoded.rgba, width, height);

    println!("AV1 Enhanced PSNR: {:.2} dB, SSIM: {:.4}", psnr, ssim);
    assert!(psnr > 30.0, "PSNR should be good for quality 85");
    assert!(ssim > 0.85, "SSIM should be high for quality 85");

    // Test micro-preview extraction
    let (pw, ph, preview_rgb) = WpgDecoder::decode_preview(&encoded)
        .expect("Micro-preview extraction failed");
    assert_eq!(pw, width as u32);
    assert_eq!(ph, height as u32);
    assert_eq!(preview_rgb.len(), 48);
}
