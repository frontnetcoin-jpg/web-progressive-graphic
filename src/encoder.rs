use crate::av1_codec::Av1Codec;
use crate::color::rgb_to_yuv;
use crate::grain::FilmGrainParams;
use crate::header::{
    ColorSubsampling, WpgHeader, WpgMode, FLAG_ALPHA, FLAG_FILM_GRAIN, FLAG_LOSSLESS,
    FLAG_MICRO_PREVIEW, FLAG_SECONDARY_ENTROPY,
};
use crate::lossless::{ModularLosslessCodec, PaletteCodec};
use crate::preview::MicroPreview;
use byteorder::{BigEndian, WriteBytesExt};
use std::io;

#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub quality: u8,                   // 1..100 (default 80)
    pub speed: u8,                     // 1..10 (default 6)
    pub mode: Option<WpgMode>,         // None = Auto
    pub subsampling: ColorSubsampling, // Default Yuv420 for lossy, Rgb for lossless
    pub enable_preview: bool,          // Default true (16-byte micro preview)
    pub enable_grain: bool,            // Default true (film grain synthesis)
    pub grain_scale: Option<u8>,       // None = Auto-detect, Some(0..255)
    pub secondary_entropy: bool,       // Default true (zstd post-compression)
    pub tune_ssim: bool,               // Tune PSNR vs Psychovisual
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            quality: 80,
            speed: 6,
            mode: None,
            subsampling: ColorSubsampling::Yuv420,
            enable_preview: true,
            enable_grain: true,
            grain_scale: None,
            secondary_entropy: true,
            tune_ssim: true,
        }
    }
}

pub struct WpgEncoder;

impl WpgEncoder {
    pub fn encode(rgba: &[u8], width: u32, height: u32, options: &EncodeOptions) -> io::Result<Vec<u8>> {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid image dimensions or buffer"));
        }

        let has_alpha = rgba.chunks_exact(4).any(|px| px[3] < 255);

        // Compute palette encoding at most once and reuse the encoded buffer
        let mut cached_palette = if options.mode.is_none() || options.mode == Some(WpgMode::PaletteLossless) {
            PaletteCodec::encode(rgba, width, height)
        } else {
            None
        };

        // Auto mode selection if not specified
        let selected_mode = match options.mode {
            Some(m) => m,
            None => {
                if options.quality == 100 {
                    if cached_palette.is_some() {
                        WpgMode::PaletteLossless
                    } else {
                        WpgMode::ModularLossless
                    }
                } else if cached_palette.is_some() {
                    WpgMode::PaletteLossless
                } else {
                    WpgMode::Av1Enhanced
                }
            }
        };

        let mut header = WpgHeader::new(
            width,
            height,
            selected_mode,
            options.subsampling,
            options.quality,
        );

        if has_alpha {
            header.flags |= FLAG_ALPHA;
            header.alpha_mode = 1; // RLE Lossless Alpha
        }

        if options.enable_preview {
            header.flags |= FLAG_MICRO_PREVIEW;
        }

        if selected_mode != WpgMode::Av1Enhanced || options.quality == 100 {
            header.flags |= FLAG_LOSSLESS;
        }

        // Film grain analysis for Mode 0
        let mut grain_params = FilmGrainParams::default();
        if selected_mode == WpgMode::Av1Enhanced && options.enable_grain {
            grain_params = match options.grain_scale {
                Some(scale) => FilmGrainParams::new(scale, 1337),
                None => FilmGrainParams::estimate_grain(rgba, width, height),
            };
            if grain_params.grain_scale > 0 {
                header.flags |= FLAG_FILM_GRAIN;
            }
        }

        // Generate MicroPreview
        let micro_preview = if header.has_micro_preview() {
            Some(MicroPreview::generate(rgba, width, height))
        } else {
            None
        };

        // Encode Alpha Plane if present
        let mut alpha_bytes = Vec::new();
        if has_alpha {
            let mut alpha_raw = Vec::with_capacity((width * height) as usize);
            for px in rgba.chunks_exact(4) {
                alpha_raw.push(px[3]);
            }
            // Compress alpha with Zstandard level 9
            let compressed_alpha = zstd::bulk::compress(&alpha_raw, 9)?;
            alpha_bytes.write_u32::<BigEndian>(compressed_alpha.len() as u32)?;
            alpha_bytes.extend_from_slice(&compressed_alpha);
        }

        // Encode Main Image Payload based on selected mode
        let payload: Vec<u8> = match selected_mode {
            WpgMode::PaletteLossless => {
                if let Some(data) = cached_palette.take() {
                    data
                } else {
                    // Fallback to Modular Lossless
                    ModularLosslessCodec::encode(rgba, width, height)?
                }
            }
            WpgMode::ModularLossless => ModularLosslessCodec::encode(rgba, width, height)?,
            WpgMode::Av1Enhanced => {
                // If grain synthesis enabled, denoise slightly first
                let processed_rgba = if grain_params.grain_scale > 0 {
                    grain_params.denoise_for_encoding(rgba, width, height)
                } else {
                    rgba.to_vec()
                };

                let planes = rgb_to_yuv(
                    &processed_rgba,
                    width as usize,
                    height as usize,
                    options.subsampling,
                );

                let raw_obu = Av1Codec::encode_intra(
                    &planes,
                    options.quality,
                    options.speed,
                    options.tune_ssim,
                )?;

                // Only apply secondary Zstd entropy on Mode 0 when it achieves meaningful savings (>= 256B and >= 8%),
                // keeping standard Mode 0 streams directly decodable via the 0-KB Wasm pure-JS fast path.
                if options.secondary_entropy && raw_obu.len() >= 1024 {
                    if let Ok(compressed) = zstd::bulk::compress(&raw_obu, 5) {
                        if compressed.len() + 256 < raw_obu.len() && compressed.len() * 100 < raw_obu.len() * 92 {
                            header.flags |= FLAG_SECONDARY_ENTROPY;
                            compressed
                        } else {
                            raw_obu
                        }
                    } else {
                        raw_obu
                    }
                } else {
                    raw_obu
                }
            }
        };

        // Assemble Final WPG Binary Container
        let mut output = Vec::with_capacity(64 + alpha_bytes.len() + payload.len());
        header.serialize(&mut output)?;

        if let Some(preview) = &micro_preview {
            preview.serialize(&mut output)?;
        }

        if header.has_film_grain() {
            grain_params.serialize(&mut output)?;
        }

        if has_alpha {
            output.extend_from_slice(&alpha_bytes);
        }

        output.extend_from_slice(&payload);

        Ok(output)
    }
}
