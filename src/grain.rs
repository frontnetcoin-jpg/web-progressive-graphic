use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Read, Write};

pub const FILM_GRAIN_HEADER_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FilmGrainParams {
    pub grain_scale: u8,   // 0 = disabled, 1..255 intensity
    pub grain_seed: u16,   // PRNG seed for deterministic synthesis
    pub grain_cutoff: u8,  // High frequency cutoff (filter responsiveness)
    pub chroma_scale: u8,  // Grain intensity on chroma channels
    pub blend_mode: u8,    // 0 = Additive with luma weighting, 1 = Uniform
}

impl Default for FilmGrainParams {
    fn default() -> Self {
        Self {
            grain_scale: 0,
            grain_seed: 42,
            grain_cutoff: 128,
            chroma_scale: 32,
            blend_mode: 0,
        }
    }
}

impl FilmGrainParams {
    pub fn new(scale: u8, seed: u16) -> Self {
        Self {
            grain_scale: scale,
            grain_seed: seed,
            grain_cutoff: 128,
            chroma_scale: scale / 4,
            blend_mode: 0,
        }
    }

    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u8(self.grain_scale)?;
        writer.write_u16::<BigEndian>(self.grain_seed)?;
        writer.write_u8(self.grain_cutoff)?;
        writer.write_u8(self.chroma_scale)?;
        writer.write_u8(self.blend_mode)?;
        writer.write_u16::<BigEndian>(0)?; // 2 bytes padding for 8 bytes total
        Ok(())
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let grain_scale = reader.read_u8()?;
        let grain_seed = reader.read_u16::<BigEndian>()?;
        let grain_cutoff = reader.read_u8()?;
        let chroma_scale = reader.read_u8()?;
        let blend_mode = reader.read_u8()?;
        let _padding = reader.read_u16::<BigEndian>()?;

        Ok(Self {
            grain_scale,
            grain_seed,
            grain_cutoff,
            chroma_scale,
            blend_mode,
        })
    }

    /// Analyze an RGBA image and estimate optimal film grain synthesis parameters
    pub fn estimate_grain(rgba: &[u8], width: u32, height: u32) -> Self {
        if width < 8 || height < 8 || rgba.is_empty() {
            return Self::default();
        }

        // Measure local variance of high-pass residual on luminance
        let mut residual_sum = 0f64;
        let mut sample_count = 0u64;

        let w = width as usize;
        let h = height as usize;
        let stride = w * 4;

        // Sample interior blocks
        let step = (w / 128).max(1);
        for y in (1..(h - 1)).step_by(step) {
            for x in (1..(w - 1)).step_by(step) {
                let idx = y * stride + x * 4;
                let c_r = rgba[idx] as f32;
                let c_g = rgba[idx + 1] as f32;
                let c_b = rgba[idx + 2] as f32;
                let center_y = 0.299 * c_r + 0.587 * c_g + 0.114 * c_b;

                // 4-neighbor average
                let left_idx = y * stride + (x - 1) * 4;
                let right_idx = y * stride + (x + 1) * 4;
                let up_idx = (y - 1) * stride + x * 4;
                let down_idx = (y + 1) * stride + x * 4;

                let l_y = 0.299 * rgba[left_idx] as f32 + 0.587 * rgba[left_idx + 1] as f32 + 0.114 * rgba[left_idx + 2] as f32;
                let r_y = 0.299 * rgba[right_idx] as f32 + 0.587 * rgba[right_idx + 1] as f32 + 0.114 * rgba[right_idx + 2] as f32;
                let u_y = 0.299 * rgba[up_idx] as f32 + 0.587 * rgba[up_idx + 1] as f32 + 0.114 * rgba[up_idx + 2] as f32;
                let d_y = 0.299 * rgba[down_idx] as f32 + 0.587 * rgba[down_idx + 1] as f32 + 0.114 * rgba[down_idx + 2] as f32;

                let neighbor_avg = (l_y + r_y + u_y + d_y) * 0.25;
                let diff = (center_y - neighbor_avg).abs();

                // Only consider small high-frequency differences (ignore strong edges > 20)
                if diff > 0.5 && diff < 16.0 {
                    residual_sum += diff as f64;
                    sample_count += 1;
                }
            }
        }

        if sample_count > 64 {
            let mean_noise = residual_sum / sample_count as f64;
            // Map mean noise to scale (e.g. 1.0 noise -> scale ~ 12, 3.0 noise -> scale ~ 36)
            let scale = (mean_noise * 12.0).clamp(0.0, 64.0) as u8;
            if scale >= 8 {
                return Self::new(scale, 1337);
            }
        }

        Self::default()
    }

    /// Pre-process / gentle denoise to allow AV1 intra to encode with much higher efficiency
    pub fn denoise_for_encoding(&self, rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
        if self.grain_scale < 8 || width < 4 || height < 4 {
            return rgba.to_vec();
        }

        let mut out = rgba.to_vec();
        let w = width as usize;
        let h = height as usize;
        let stride = w * 4;
        let threshold = (self.grain_scale as f32 * 0.25).max(1.0);

        // Gentle edge-preserving smoothing
        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                let center_idx = y * stride + x * 4;
                for ch in 0..3 {
                    let c = rgba[center_idx + ch] as f32;
                    let l = rgba[center_idx - 4 + ch] as f32;
                    let r = rgba[center_idx + 4 + ch] as f32;
                    let u = rgba[center_idx - stride + ch] as f32;
                    let d = rgba[center_idx + stride + ch] as f32;

                    // If neighbors are within noise threshold, average them
                    let mut sum = c;
                    let mut weight = 1.0f32;
                    for &n in &[l, r, u, d] {
                        let diff = (n - c).abs();
                        if diff < threshold {
                            let w_val = 1.0 - (diff / threshold);
                            sum += n * w_val;
                            weight += w_val;
                        }
                    }
                    out[center_idx + ch] = (sum / weight).clamp(0.0, 255.0) as u8;
                }
            }
        }

        out
    }

    /// Synthesize film grain onto decoded image
    pub fn apply_synthesis(&self, rgba: &mut [u8], width: u32, height: u32) {
        if self.grain_scale == 0 {
            return;
        }

        let scale = self.grain_scale as f32 / 255.0 * 28.0;
        let chroma_scale = self.chroma_scale as f32 / 255.0 * 14.0;
        let mut prng_state = (self.grain_seed as u32).wrapping_mul(1664525).wrapping_add(1013904223);

        let total_pixels = (width * height) as usize;
        for i in 0..total_pixels {
            let idx = i * 4;
            if idx + 3 >= rgba.len() {
                break;
            }

            // Fast Xorshift PRNG
            prng_state ^= prng_state << 13;
            prng_state ^= prng_state >> 17;
            prng_state ^= prng_state << 5;
            let rand_luma = ((prng_state & 0x1FF) as i32 - 256) as f32 / 256.0;

            prng_state ^= prng_state << 13;
            prng_state ^= prng_state >> 17;
            prng_state ^= prng_state << 5;
            let rand_chroma = ((prng_state & 0x1FF) as i32 - 256) as f32 / 256.0;

            let cur_r = rgba[idx] as f32;
            let cur_g = rgba[idx + 1] as f32;
            let cur_b = rgba[idx + 2] as f32;
            let luma = 0.299 * cur_r + 0.587 * cur_g + 0.114 * cur_b;

            // Perceptual grain modulation: grain is visible in midtones, less in extreme darks/whites
            let luma_norm = luma / 255.0;
            let weight = (luma_norm * (1.0 - luma_norm) * 4.0).clamp(0.2, 1.0);

            let luma_noise = rand_luma * scale * weight;
            let chroma_noise = rand_chroma * chroma_scale * weight;

            rgba[idx] = (cur_r + luma_noise + chroma_noise).clamp(0.0, 255.0) as u8;
            rgba[idx + 1] = (cur_g + luma_noise).clamp(0.0, 255.0) as u8;
            rgba[idx + 2] = (cur_b + luma_noise - chroma_noise).clamp(0.0, 255.0) as u8;
        }
    }
}
