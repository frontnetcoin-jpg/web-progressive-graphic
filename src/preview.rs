use std::io::{self, Read, Write};

pub const MICRO_PREVIEW_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicroPreview {
    pub palette: [[u8; 3]; 4], // 4 RGB colors (12 bytes)
    pub indices: [u8; 16],      // 16 2-bit pixel indices (4 bytes packed)
}

impl MicroPreview {
    /// Generate a 16-byte micro-preview from RGBA/RGB pixel buffer
    pub fn generate(rgba: &[u8], width: u32, height: u32) -> Self {
        if width == 0 || height == 0 || rgba.is_empty() {
            return Self::empty();
        }

        // Downsample to a 4x4 grid of RGB colors
        let mut grid = [[0u8; 3]; 16];
        for gy in 0..4usize {
            for gx in 0..4usize {
                let start_x = (gx as u32 * width) / 4;
                let end_x = (((gx as u32 + 1) * width) / 4).max(start_x + 1).min(width);
                let start_y = (gy as u32 * height) / 4;
                let end_y = (((gy as u32 + 1) * height) / 4).max(start_y + 1).min(height);

                let mut r_sum = 0u64;
                let mut g_sum = 0u64;
                let mut b_sum = 0u64;
                let mut count = 0u64;

                for y in start_y..end_y {
                    for x in start_x..end_x {
                        let idx = ((y * width + x) * 4) as usize;
                        if idx + 3 < rgba.len() {
                            let a = rgba[idx + 3] as u64;
                            if a > 16 {
                                r_sum += rgba[idx] as u64;
                                g_sum += rgba[idx + 1] as u64;
                                b_sum += rgba[idx + 2] as u64;
                                count += 1;
                            }
                        }
                    }
                }

                if count > 0 {
                    grid[gy * 4 + gx] = [
                        (r_sum / count) as u8,
                        (g_sum / count) as u8,
                        (b_sum / count) as u8,
                    ];
                } else {
                    grid[gy * 4 + gx] = [0, 0, 0];
                }
            }
        }

        // K-means (k=4) to find 4 representative palette colors
        let mut palette = [
            grid[0],
            grid[5],
            grid[10],
            grid[15],
        ];

        // 4 iterations of k-means
        for _ in 0..4 {
            let mut clusters: [Vec<[u8; 3]>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            for &pixel in &grid {
                let mut best_idx = 0;
                let mut best_dist = u32::MAX;
                for (ci, &center) in palette.iter().enumerate() {
                    let dr = pixel[0] as i32 - center[0] as i32;
                    let dg = pixel[1] as i32 - center[1] as i32;
                    let db = pixel[2] as i32 - center[2] as i32;
                    let dist = (dr * dr + dg * dg + db * db) as u32;
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = ci;
                    }
                }
                clusters[best_idx].push(pixel);
            }

            for ci in 0..4 {
                if !clusters[ci].is_empty() {
                    let mut r_acc = 0u32;
                    let mut g_acc = 0u32;
                    let mut b_acc = 0u32;
                    for p in &clusters[ci] {
                        r_acc += p[0] as u32;
                        g_acc += p[1] as u32;
                        b_acc += p[2] as u32;
                    }
                    let len = clusters[ci].len() as u32;
                    palette[ci] = [(r_acc / len) as u8, (g_acc / len) as u8, (b_acc / len) as u8];
                }
            }
        }

        // Assign each grid pixel to closest palette entry
        let mut indices = [0u8; 16];
        for i in 0..16 {
            let pixel = grid[i];
            let mut best_idx = 0;
            let mut best_dist = u32::MAX;
            for (ci, &center) in palette.iter().enumerate() {
                let dr = pixel[0] as i32 - center[0] as i32;
                let dg = pixel[1] as i32 - center[1] as i32;
                let db = pixel[2] as i32 - center[2] as i32;
                let dist = (dr * dr + dg * dg + db * db) as u32;
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = ci as u8;
                }
            }
            indices[i] = best_idx;
        }

        Self { palette, indices }
    }

    pub fn empty() -> Self {
        Self {
            palette: [[0; 3]; 4],
            indices: [0; 16],
        }
    }

    /// Pack into exactly 16 bytes: 12 bytes palette + 4 bytes indices (2 bits each)
    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        for c in &self.palette {
            writer.write_all(c)?;
        }

        let mut packed = [0u8; 4];
        for i in 0..4 {
            let b0 = self.indices[i * 4] & 0x03;
            let b1 = self.indices[i * 4 + 1] & 0x03;
            let b2 = self.indices[i * 4 + 2] & 0x03;
            let b3 = self.indices[i * 4 + 3] & 0x03;
            packed[i] = (b0 << 6) | (b1 << 4) | (b2 << 2) | b3;
        }
        writer.write_all(&packed)?;
        Ok(())
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut palette = [[0u8; 3]; 4];
        for c in &mut palette {
            reader.read_exact(c)?;
        }

        let mut packed = [0u8; 4];
        reader.read_exact(&mut packed)?;

        let mut indices = [0u8; 16];
        for i in 0..4 {
            let byte = packed[i];
            indices[i * 4] = (byte >> 6) & 0x03;
            indices[i * 4 + 1] = (byte >> 4) & 0x03;
            indices[i * 4 + 2] = (byte >> 2) & 0x03;
            indices[i * 4 + 3] = byte & 0x03;
        }

        Ok(Self { palette, indices })
    }

    /// Reconstruct 4x4 RGB pixels (48 bytes: 16 pixels * 3 channels)
    pub fn to_rgb4x4(&self) -> [u8; 48] {
        let mut rgb = [0u8; 48];
        for i in 0..16 {
            let color = self.palette[self.indices[i] as usize];
            rgb[i * 3] = color[0];
            rgb[i * 3 + 1] = color[1];
            rgb[i * 3 + 2] = color[2];
        }
        rgb
    }

    /// Convert to RGBA pixels upscaled to (out_w, out_h) using bilinear filtering
    pub fn render_placeholder(&self, out_w: u32, out_h: u32) -> Vec<u8> {
        let grid = self.to_rgb4x4();
        let mut out = vec![255u8; (out_w * out_h * 4) as usize];

        for y in 0..out_h {
            let fy = (y as f32 / out_h as f32) * 3.0;
            let y0 = (fy.floor() as usize).min(3);
            let y1 = (y0 + 1).min(3);
            let ty = fy - y0 as f32;

            for x in 0..out_w {
                let fx = (x as f32 / out_w as f32) * 3.0;
                let x0 = (fx.floor() as usize).min(3);
                let x1 = (x0 + 1).min(3);
                let tx = fx - x0 as f32;

                let c00 = [grid[(y0 * 4 + x0) * 3], grid[(y0 * 4 + x0) * 3 + 1], grid[(y0 * 4 + x0) * 3 + 2]];
                let c10 = [grid[(y0 * 4 + x1) * 3], grid[(y0 * 4 + x1) * 3 + 1], grid[(y0 * 4 + x1) * 3 + 2]];
                let c01 = [grid[(y1 * 4 + x0) * 3], grid[(y1 * 4 + x0) * 3 + 1], grid[(y1 * 4 + x0) * 3 + 2]];
                let c11 = [grid[(y1 * 4 + x1) * 3], grid[(y1 * 4 + x1) * 3 + 1], grid[(y1 * 4 + x1) * 3 + 2]];

                let out_idx = ((y * out_w + x) * 4) as usize;
                for ch in 0..3 {
                    let top = c00[ch] as f32 * (1.0 - tx) + c10[ch] as f32 * tx;
                    let bottom = c01[ch] as f32 * (1.0 - tx) + c11[ch] as f32 * tx;
                    let val = (top * (1.0 - ty) + bottom * ty).clamp(0.0, 255.0) as u8;
                    out[out_idx + ch] = val;
                }
            }
        }

        out
    }
}
