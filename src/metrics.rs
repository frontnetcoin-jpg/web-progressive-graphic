use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetrics {
    pub psnr_db: f64,
    pub ssim: f64,
    pub file_size_bytes: usize,
    pub bpp: f64,              // Bits per pixel
    pub compression_ratio: f64,
    pub encode_time_ms: f64,
    pub decode_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub format_name: String,
    pub file_size_bytes: usize,
    pub size_vs_wpg_pct: f64,   // e.g. +35.2% larger than WPG
    pub psnr_db: f64,
    pub ssim: f64,
    pub bpp: f64,
    pub encode_time_ms: f64,
}

/// Compute PSNR between two RGBA buffers
pub fn compute_psnr(orig: &[u8], decoded: &[u8]) -> f64 {
    if orig.len() != decoded.len() || orig.is_empty() {
        return 0.0;
    }

    let mut mse_sum = 0f64;
    let mut count = 0u64;

    for i in (0..orig.len()).step_by(4) {
        if i + 2 < orig.len() && i + 2 < decoded.len() {
            let dr = orig[i] as f64 - decoded[i] as f64;
            let dg = orig[i + 1] as f64 - decoded[i + 1] as f64;
            let db = orig[i + 2] as f64 - decoded[i + 2] as f64;
            mse_sum += dr * dr + dg * dg + db * db;
            count += 3;
        }
    }

    if count == 0 {
        return 0.0;
    }

    let mse = mse_sum / count as f64;
    if mse < 1e-10 {
        return 99.99; // Bit-exact lossless
    }

    10.0 * (255.0 * 255.0 / mse).log10()
}

/// Compute Structural Similarity Index (SSIM) between two RGBA buffers
pub fn compute_ssim(orig: &[u8], decoded: &[u8], width: usize, height: usize) -> f64 {
    if width < 8 || height < 8 || orig.len() != decoded.len() {
        return 1.0;
    }

    let c1 = (0.01 * 255.0) * (0.01 * 255.0);
    let c2 = (0.03 * 255.0) * (0.03 * 255.0);

    let mut ssim_sum = 0f64;
    let mut block_count = 0u64;

    let stride = width * 4;

    // 8x8 block sampling
    let block_size = 8;
    for by in (0..(height - block_size)).step_by(block_size) {
        for bx in (0..(width - block_size)).step_by(block_size) {
            let mut mean_x = 0f64;
            let mut mean_y = 0f64;
            let num_pixels = (block_size * block_size) as f64;

            for y in 0..block_size {
                for x in 0..block_size {
                    let idx = (by + y) * stride + (bx + x) * 4;
                    // Luma
                    let lx = 0.299 * orig[idx] as f64 + 0.587 * orig[idx + 1] as f64 + 0.114 * orig[idx + 2] as f64;
                    let ly = 0.299 * decoded[idx] as f64 + 0.587 * decoded[idx + 1] as f64 + 0.114 * decoded[idx + 2] as f64;
                    mean_x += lx;
                    mean_y += ly;
                }
            }

            mean_x /= num_pixels;
            mean_y /= num_pixels;

            let mut var_x = 0f64;
            let mut var_y = 0f64;
            let mut cov_xy = 0f64;

            for y in 0..block_size {
                for x in 0..block_size {
                    let idx = (by + y) * stride + (bx + x) * 4;
                    let lx = 0.299 * orig[idx] as f64 + 0.587 * orig[idx + 1] as f64 + 0.114 * orig[idx + 2] as f64;
                    let ly = 0.299 * decoded[idx] as f64 + 0.587 * decoded[idx + 1] as f64 + 0.114 * decoded[idx + 2] as f64;

                    let dx = lx - mean_x;
                    let dy = ly - mean_y;
                    var_x += dx * dx;
                    var_y += dy * dy;
                    cov_xy += dx * dy;
                }
            }

            var_x /= num_pixels - 1.0;
            var_y /= num_pixels - 1.0;
            cov_xy /= num_pixels - 1.0;

            let numerator = (2.0 * mean_x * mean_y + c1) * (2.0 * cov_xy + c2);
            let denominator = (mean_x * mean_x + mean_y * mean_y + c1) * (var_x + var_y + c2);

            if denominator > 0.0 {
                ssim_sum += numerator / denominator;
                block_count += 1;
            }
        }
    }

    if block_count > 0 {
        (ssim_sum / block_count as f64).clamp(0.0, 1.0)
    } else {
        1.0
    }
}
