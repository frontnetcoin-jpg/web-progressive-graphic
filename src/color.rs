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

/// Convert sRGB RGBA buffer to YUV planes
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

    // Compute full Y plane
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;

            // BT.709
            let y_val = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 255.0) as u8;
            y_plane[y * y_width + x] = y_val;
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

                            let u_val = (-0.1146 * r - 0.3854 * g + 0.5000 * b) + 128.0;
                            let v_val = (0.5000 * r - 0.4542 * g - 0.0458 * b) + 128.0;

                            u_acc += u_val;
                            v_acc += v_val;
                            count += 1.0;
                        }
                    }

                    if count > 0.0 {
                        u_plane[uy * uv_width + ux] = (u_acc / count).clamp(0.0, 255.0) as u8;
                        v_plane[uy * uv_width + ux] = (v_acc / count).clamp(0.0, 255.0) as u8;
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

                        let u_val = (-0.1146 * r - 0.3854 * g + 0.5000 * b) + 128.0;
                        let v_val = (0.5000 * r - 0.4542 * g - 0.0458 * b) + 128.0;

                        u_acc += u_val;
                        v_acc += v_val;
                        count += 1.0;
                    }

                    if count > 0.0 {
                        u_plane[y * uv_width + ux] = (u_acc / count).clamp(0.0, 255.0) as u8;
                        v_plane[y * uv_width + ux] = (v_acc / count).clamp(0.0, 255.0) as u8;
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

                    let u_val = (-0.1146 * r - 0.3854 * g + 0.5000 * b) + 128.0;
                    let v_val = (0.5000 * r - 0.4542 * g - 0.0458 * b) + 128.0;

                    u_plane[y * width + x] = u_val.clamp(0.0, 255.0) as u8;
                    v_plane[y * width + x] = v_val.clamp(0.0, 255.0) as u8;
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

/// Convert YUV planes back to RGBA
pub fn yuv_to_rgb(planes: &YuvPlanes, alpha: Option<&[u8]>) -> Vec<u8> {
    let width = planes.y_width;
    let height = planes.y_height;
    let mut rgba = vec![255u8; width * height * 4];

    for y in 0..height {
        let (uy, vy) = match planes.subsampling {
            ColorSubsampling::Yuv420 => (y / 2, y / 2),
            ColorSubsampling::Yuv422 | ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (y, y),
        };

        for x in 0..width {
            let (ux, vx) = match planes.subsampling {
                ColorSubsampling::Yuv420 | ColorSubsampling::Yuv422 => (x / 2, x / 2),
                ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (x, x),
            };

            let y_val = planes.y[y * planes.y_stride + x] as f32;
            let u_val = planes.u[uy * planes.u_stride + ux] as f32 - 128.0;
            let v_val = planes.v[vy * planes.v_stride + vx] as f32 - 128.0;

            // Inverse BT.709
            let r = y_val + 1.5748 * v_val;
            let g = y_val - 0.1873 * u_val - 0.4681 * v_val;
            let b = y_val + 1.8556 * u_val;

            let out_idx = (y * width + x) * 4;
            rgba[out_idx] = r.clamp(0.0, 255.0) as u8;
            rgba[out_idx + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[out_idx + 2] = b.clamp(0.0, 255.0) as u8;

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
        let (uy, vy) = match planes.subsampling {
            ColorSubsampling::Yuv420 => (y / 2, y / 2),
            ColorSubsampling::Yuv422 | ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (y, y),
        };

        for x in 0..width {
            let (ux, vx) = match planes.subsampling {
                ColorSubsampling::Yuv420 | ColorSubsampling::Yuv422 => (x / 2, x / 2),
                ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (x, x),
            };

            let y_val = planes.y[y * planes.y_stride + x] as f32;
            let u_val = planes.u[uy * planes.u_stride + ux] as f32 - 128.0;
            let v_val = planes.v[vy * planes.v_stride + vx] as f32 - 128.0;

            // Inverse BT.601
            let r = y_val + 1.402 * v_val;
            let g = y_val - 0.344136 * u_val - 0.714136 * v_val;
            let b = y_val + 1.772 * u_val;

            let out_idx = (y * width + x) * 4;
            rgba[out_idx] = r.clamp(0.0, 255.0) as u8;
            rgba[out_idx + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[out_idx + 2] = b.clamp(0.0, 255.0) as u8;

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
