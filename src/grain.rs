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

    /// Analyze an RGBA image and estimate optimal film grain synthesis parameters.
    /// Uses a 2D sign-reversal (local extremum) detector restricted to flat patches
    /// so smooth gradients and anti-aliased edges are never misclassified as camera noise.
    pub fn estimate_grain(rgba: &[u8], width: u32, height: u32) -> Self {
        if width < 8 || height < 8 || rgba.is_empty() || rgba.chunks_exact(4).any(|px| px[3] < 255) {
            return Self::default();
        }

        let w = width as usize;
        let h = height as usize;
        let stride = w * 4;

        let mut flat_count = 0u64;
        let mut extremum_count = 0u64;
        let mut residual_sum = 0f64;

        let step = (w / 160).max(1);
        for y in (1..(h - 1)).step_by(step) {
            for x in (1..(w - 1)).step_by(step) {
                let idx = y * stride + x * 4;
                let left_idx = idx - 4;
                let right_idx = idx + 4;
                let up_idx = idx - stride;
                let down_idx = idx + stride;

                let c_y = 0.2126 * rgba[idx] as f32 + 0.7152 * rgba[idx + 1] as f32 + 0.0722 * rgba[idx + 2] as f32;
                let l_y = 0.2126 * rgba[left_idx] as f32 + 0.7152 * rgba[left_idx + 1] as f32 + 0.0722 * rgba[left_idx + 2] as f32;
                let r_y = 0.2126 * rgba[right_idx] as f32 + 0.7152 * rgba[right_idx + 1] as f32 + 0.0722 * rgba[right_idx + 2] as f32;
                let u_y = 0.2126 * rgba[up_idx] as f32 + 0.7152 * rgba[up_idx + 1] as f32 + 0.0722 * rgba[up_idx + 2] as f32;
                let d_y = 0.2126 * rgba[down_idx] as f32 + 0.7152 * rgba[down_idx + 1] as f32 + 0.0722 * rgba[down_idx + 2] as f32;

                // Reject edges and gradients where the outer span changes significantly
                let span_x = (r_y - l_y).abs();
                let span_y = (d_y - u_y).abs();
                if span_x > 8.0 || span_y > 8.0 {
                    continue;
                }

                flat_count += 1;

                // Check if center pixel is a strict 2D local peak or valley (true high-frequency noise)
                let dl = c_y - l_y;
                let dr = c_y - r_y;
                let du = c_y - u_y;
                let dd = c_y - d_y;
                if dl * dr > 0.25 && du * dd > 0.25 {
                    let neighbor_avg = (l_y + r_y + u_y + d_y) * 0.25;
                    let diff = (c_y - neighbor_avg).abs();
                    if diff > 0.8 && diff < 14.0 {
                        extremum_count += 1;
                        residual_sum += diff as f64;
                    }
                }
            }
        }

        // True camera sensor noise causes >= 18% of flat interior pixels to be 2D local extrema
        if flat_count >= 64 && extremum_count >= 32 {
            let osc_ratio = extremum_count as f64 / flat_count as f64;
            if osc_ratio >= 0.18 {
                let mean_noise = residual_sum / extremum_count as f64;
                let scale = (mean_noise * 10.0).round().clamp(0.0, 56.0) as u8;
                if scale >= 10 {
                    return Self::new(scale, 1337);
                }
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
        let threshold = (self.grain_scale as f32 * 0.24).clamp(1.5, 9.0);

        // Edge-preserving bilateral smoothing on interior pixels
        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                let center_idx = y * stride + x * 4;
                for ch in 0..3 {
                    let c = rgba[center_idx + ch] as f32;
                    let l = rgba[center_idx - 4 + ch] as f32;
                    let r = rgba[center_idx + 4 + ch] as f32;
                    let u = rgba[center_idx - stride + ch] as f32;
                    let d = rgba[center_idx + stride + ch] as f32;

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
                    out[center_idx + ch] = (sum / weight).round().clamp(0.0, 255.0) as u8;
                }
            }
        }

        out
    }

    /// Synthesize deterministic Xorshift32 film grain onto decoded image.
    /// Matches `wpg_apply_film_grain` in `wpg-wasm` and `applyFilmGrainJs` in `wpg-worker.js`.
    pub fn apply_synthesis(&self, rgba: &mut [u8], width: u32, height: u32) {
        if self.grain_scale == 0 || width == 0 || height == 0 {
            return;
        }

        let total_pixels = (width as usize) * (height as usize);
        let mut prng_state = if self.grain_seed == 0 {
            0x6d2b_79f5u32
        } else {
            (self.grain_seed as u32) | 0x10001
        };
        let grain_amplitude = (self.grain_scale as f32) * 0.04;

        for i in 0..total_pixels {
            let offset = i * 4;
            if offset + 3 >= rgba.len() {
                break;
            }
            let r = rgba[offset] as f32;
            let g = rgba[offset + 1] as f32;
            let b = rgba[offset + 2] as f32;

            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let norm_y = y / 255.0;
            let lum_weight = 1.0 - 4.0 * (norm_y - 0.5) * (norm_y - 0.5);

            if lum_weight > 0.05 {
                prng_state ^= prng_state << 13;
                prng_state ^= prng_state >> 17;
                prng_state ^= prng_state << 5;

                let rand_val = ((prng_state & 0xFFFF) as f32 / 32768.0) - 1.0;
                let noise = rand_val * grain_amplitude * lum_weight;

                rgba[offset] = (r + noise).round().clamp(0.0, 255.0) as u8;
                rgba[offset + 1] = (g + noise).round().clamp(0.0, 255.0) as u8;
                rgba[offset + 2] = (b + noise).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}
