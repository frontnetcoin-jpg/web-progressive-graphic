use crate::header::ColorSubsampling;

#[derive(Debug, Clone)]
pub struct YuvPlanes {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub y_stride: usize,
    pub u_stride: usize,
    pub v_stride: usize,
    pub y_width: usize,
    pub y_height: usize,
    pub uv_width: usize,
    pub uv_height: usize,
    pub subsampling: ColorSubsampling,
}

/// Convert sRGB RGBA buffer to full-range BT.709 YUV planes with unbiased rounding
pub fn rgb_to_yuv(rgba: &[u8], width: usize, height: usize, subsampling: ColorSubsampling) -> YuvPlanes {
    let y_width = width;
    let y_height = height;

    let (uv_width, uv_height) = match subsampling {
        ColorSubsampling::Yuv420 => ((width + 1) / 2, (height + 1) / 2),
        ColorSubsampling::Yuv422 => ((width + 1) / 2, height),
        ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (width, height),
    };

    let mut y_plane = vec![0u8; y_width * y_height];
    let mut u_plane = vec![0u8; uv_width * uv_height];
    let mut v_plane = vec![0u8; uv_width * uv_height];

    // Compute full Y plane (BT.709 full range)
    for y in 0..height {
        let row_offset = y * width;
        for x in 0..width {
            let idx = (row_offset + x) * 4;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;

            let y_val = (0.2126 * r + 0.7152 * g + 0.0722 * b).round().clamp(0.0, 255.0) as u8;
            y_plane[row_offset + x] = y_val;
        }
    }

    // Compute U and V planes based on subsampling
    match subsampling {
        ColorSubsampling::Yuv420 => {
            for uy in 0..uv_height {
                for ux in 0..uv_width {
                    let mut u_acc = 0f32;
                    let mut v_acc = 0f32;
                    let mut count = 0f32;

                    for dy in 0..2 {
                        let y = uy * 2 + dy;
                        if y >= height {
                            continue;
                        }
                        for dx in 0..2 {
                            let x = ux * 2 + dx;
                            if x >= width {
                                continue;
                            }
                            let idx = (y * width + x) * 4;
                            let r = rgba[idx] as f32;
                            let g = rgba[idx + 1] as f32;
                            let b = rgba[idx + 2] as f32;

                            let u_val = (-0.114572 * r - 0.385428 * g + 0.500000 * b) + 128.0;
                            let v_val = (0.500000 * r - 0.454153 * g - 0.045847 * b) + 128.0;

                            u_acc += u_val;
                            v_acc += v_val;
                            count += 1.0;
                        }
                    }

                    if count > 0.0 {
                        u_plane[uy * uv_width + ux] = (u_acc / count).round().clamp(0.0, 255.0) as u8;
                        v_plane[uy * uv_width + ux] = (v_acc / count).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        ColorSubsampling::Yuv422 => {
            for y in 0..height {
                for ux in 0..uv_width {
                    let mut u_acc = 0f32;
                    let mut v_acc = 0f32;
                    let mut count = 0f32;

                    for dx in 0..2 {
                        let x = ux * 2 + dx;
                        if x >= width {
                            continue;
                        }
                        let idx = (y * width + x) * 4;
                        let r = rgba[idx] as f32;
                        let g = rgba[idx + 1] as f32;
                        let b = rgba[idx + 2] as f32;

                        let u_val = (-0.114572 * r - 0.385428 * g + 0.500000 * b) + 128.0;
                        let v_val = (0.500000 * r - 0.454153 * g - 0.045847 * b) + 128.0;

                        u_acc += u_val;
                        v_acc += v_val;
                        count += 1.0;
                    }

                    if count > 0.0 {
                        u_plane[y * uv_width + ux] = (u_acc / count).round().clamp(0.0, 255.0) as u8;
                        v_plane[y * uv_width + ux] = (v_acc / count).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => {
            for y in 0..height {
                for x in 0..width {
                    let idx = (y * width + x) * 4;
                    let r = rgba[idx] as f32;
                    let g = rgba[idx + 1] as f32;
                    let b = rgba[idx + 2] as f32;

                    let u_val = (-0.114572 * r - 0.385428 * g + 0.500000 * b) + 128.0;
                    let v_val = (0.500000 * r - 0.454153 * g - 0.045847 * b) + 128.0;

                    u_plane[y * width + x] = u_val.round().clamp(0.0, 255.0) as u8;
                    v_plane[y * width + x] = v_val.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    YuvPlanes {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        y_stride: y_width,
        u_stride: uv_width,
        v_stride: uv_width,
        y_width,
        y_height,
        uv_width,
        uv_height,
        subsampling,
    }
}

#[inline(always)]
fn sample_chroma_bilinear(plane: &[u8], stride: usize, uv_w: usize, uv_h: usize, x: usize, y: usize, subsampling: ColorSubsampling) -> f32 {
    match subsampling {
        ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => {
            plane[y.min(uv_h - 1) * stride + x.min(uv_w - 1)] as f32
        }
        ColorSubsampling::Yuv422 => {
            let max_x = (uv_w.saturating_sub(1)) as f32;
            let fx = ((x as f32 - 0.5) * 0.5).clamp(0.0, max_x);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(uv_w.saturating_sub(1));
            let wx = fx - x0 as f32;
            let row = y.min(uv_h.saturating_sub(1)) * stride;
            let v0 = plane[row + x0] as f32;
            let v1 = plane[row + x1] as f32;
            v0 + (v1 - v0) * wx
        }
        ColorSubsampling::Yuv420 => {
            let max_x = (uv_w.saturating_sub(1)) as f32;
            let max_y = (uv_h.saturating_sub(1)) as f32;
            let fx = ((x as f32 - 0.5) * 0.5).clamp(0.0, max_x);
            let fy = ((y as f32 - 0.5) * 0.5).clamp(0.0, max_y);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(uv_w.saturating_sub(1));
            let y0 = fy.floor() as usize;
            let y1 = (y0 + 1).min(uv_h.saturating_sub(1));
            let wx = fx - x0 as f32;
            let wy = fy - y0 as f32;

            let p00 = plane[y0 * stride + x0] as f32;
            let p10 = plane[y0 * stride + x1] as f32;
            let p01 = plane[y1 * stride + x0] as f32;
            let p11 = plane[y1 * stride + x1] as f32;

            let top = p00 + (p10 - p00) * wx;
            let bot = p01 + (p11 - p01) * wx;
            top + (bot - top) * wy
        }
    }
}

/// Convert YUV planes back to RGBA (BT.709 full range with bilinear chroma upsampling)
pub fn yuv_to_rgb(planes: &YuvPlanes, alpha: Option<&[u8]>) -> Vec<u8> {
    let width = planes.y_width;
    let height = planes.y_height;
    let mut rgba = vec![255u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let y_val = planes.y[y * planes.y_stride + x] as f32;
            let u_val = sample_chroma_bilinear(&planes.u, planes.u_stride, planes.uv_width, planes.uv_height, x, y, planes.subsampling) - 128.0;
            let v_val = sample_chroma_bilinear(&planes.v, planes.v_stride, planes.uv_width, planes.uv_height, x, y, planes.subsampling) - 128.0;

            // Inverse BT.709
            let r = y_val + 1.5748 * v_val;
            let g = y_val - 0.187324 * u_val - 0.468124 * v_val;
            let b = y_val + 1.8556 * u_val;

            let out_idx = (y * width + x) * 4;
            rgba[out_idx] = r.round().clamp(0.0, 255.0) as u8;
            rgba[out_idx + 1] = g.round().clamp(0.0, 255.0) as u8;
            rgba[out_idx + 2] = b.round().clamp(0.0, 255.0) as u8;

            if let Some(a_plane) = alpha {
                let a_idx = y * width + x;
                if a_idx < a_plane.len() {
                    rgba[out_idx + 3] = a_plane[a_idx];
                }
            }
        }
    }

    rgba
}

/// Convert YUV planes back to RGBA using BT.601 matrix (used by standard AVIF and JPEG)
pub fn yuv_to_rgb_bt601(planes: &YuvPlanes, alpha: Option<&[u8]>) -> Vec<u8> {
    let width = planes.y_width;
    let height = planes.y_height;
    let mut rgba = vec![255u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let y_val = planes.y[y * planes.y_stride + x] as f32;
            let u_val = sample_chroma_bilinear(&planes.u, planes.u_stride, planes.uv_width, planes.uv_height, x, y, planes.subsampling) - 128.0;
            let v_val = sample_chroma_bilinear(&planes.v, planes.v_stride, planes.uv_width, planes.uv_height, x, y, planes.subsampling) - 128.0;

            // Inverse BT.601
            let r = y_val + 1.402 * v_val;
            let g = y_val - 0.344136 * u_val - 0.714136 * v_val;
            let b = y_val + 1.772 * u_val;

            let out_idx = (y * width + x) * 4;
            rgba[out_idx] = r.round().clamp(0.0, 255.0) as u8;
            rgba[out_idx + 1] = g.round().clamp(0.0, 255.0) as u8;
            rgba[out_idx + 2] = b.round().clamp(0.0, 255.0) as u8;

            if let Some(a_plane) = alpha {
                let a_idx = y * width + x;
                if a_idx < a_plane.len() {
                    rgba[out_idx + 3] = a_plane[a_idx];
                }
            }
        }
    }

    rgba
}

/// Reversible Color Transform (YCoCg-R) for lossless compression
pub fn rgb_to_ycocg_r(r: u8, g: u8, b: u8) -> (i16, i16, i16) {
    let r = r as i16;
    let g = g as i16;
    let b = b as i16;

    let co = r - b;
    let t = b + (co >> 1);
    let cg = g - t;
    let y = t + (cg >> 1);

    (y, co, cg)
}

/// Inverse Reversible Color Transform (YCoCg-R) with zero mathematical loss
pub fn ycocg_r_to_rgb(y: i16, co: i16, cg: i16) -> (u8, u8, u8) {
    let t = y - (cg >> 1);
    let g = cg + t;
    let b = t - (co >> 1);
    let r = b + co;

    (r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8)
}
