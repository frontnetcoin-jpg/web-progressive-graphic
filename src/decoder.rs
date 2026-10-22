use crate::av1_codec::Av1Codec;
use crate::color::yuv_to_rgb;
use crate::grain::FilmGrainParams;
use crate::header::{WpgHeader, WpgMode};
use crate::lossless::{ModularLosslessCodec, PaletteCodec};
use crate::preview::MicroPreview;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{self, Cursor, Read};

#[derive(Debug, Clone)]
pub struct DecodedWpg {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub header: WpgHeader,
    pub preview: Option<MicroPreview>,
    pub grain: Option<FilmGrainParams>,
}

pub struct WpgDecoder;

impl WpgDecoder {
    /// Fast micro-preview reader: extracts dimensions and 4x4 RGB placeholder in microseconds
    pub fn decode_preview(bytes: &[u8]) -> io::Result<(u32, u32, [u8; 48])> {
        let mut reader = Cursor::new(bytes);
        let header = WpgHeader::deserialize(&mut reader)?;
        if !header.has_micro_preview() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Image has no micro-preview embedded"));
        }
        let preview = MicroPreview::deserialize(&mut reader)?;
        Ok((header.width, header.height, preview.to_rgb4x4()))
    }

    /// Full image decoding
    pub fn decode(bytes: &[u8]) -> io::Result<DecodedWpg> {
        let mut reader = Cursor::new(bytes);
        let header = WpgHeader::deserialize(&mut reader)?;

        let preview = if header.has_micro_preview() {
            Some(MicroPreview::deserialize(&mut reader)?)
        } else {
            None
        };

        let grain = if header.has_film_grain() {
            Some(FilmGrainParams::deserialize(&mut reader)?)
        } else {
            None
        };

        let alpha_plane = if header.has_alpha() {
            let alpha_len = reader.read_u32::<BigEndian>()? as usize;
            let mut alpha_compressed = vec![0u8; alpha_len];
            reader.read_exact(&mut alpha_compressed)?;
            let total_pixels = (header.width * header.height) as usize;
            let decompressed = zstd::bulk::decompress(&alpha_compressed, total_pixels + 512)?;
            Some(decompressed)
        } else {
            None
        };

        // Remaining bytes are image payload
        let current_pos = reader.position() as usize;
        let payload = &bytes[current_pos..];
        if payload.is_empty() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Missing image payload"));
        }

        let mut rgba = match header.mode {
            WpgMode::PaletteLossless => {
                PaletteCodec::decode(payload, header.width, header.height)?
            }
            WpgMode::ModularLossless => {
                ModularLosslessCodec::decode(payload, header.width, header.height)?
            }
            WpgMode::Av1Enhanced => {
                // Secondary entropy decompression if applied
                let raw_obu = if header.is_secondary_entropy() {
                    // Maximum plausible OBU size for single image
                    let max_size = (header.width * header.height * 4) as usize + 65536;
                    zstd::bulk::decompress(payload, max_size)?
                } else {
                    payload.to_vec()
                };

                let planes = Av1Codec::decode_intra(
                    &raw_obu,
                    header.width as usize,
                    header.height as usize,
                    header.subsampling,
                )?;

                let mut rgb_buffer = yuv_to_rgb(&planes, alpha_plane.as_deref());

                // Apply Film Grain Synthesis if enabled
                if let Some(grain_params) = &grain {
                    grain_params.apply_synthesis(&mut rgb_buffer, header.width, header.height);
                }

                rgb_buffer
            }
        };

        // Attach alpha plane if not already applied in Mode 1 / 2
        if let Some(alpha) = &alpha_plane {
            if header.mode != WpgMode::Av1Enhanced {
                for (i, &a) in alpha.iter().enumerate() {
                    let idx = i * 4 + 3;
                    if idx < rgba.len() {
                        rgba[idx] = a;
                    }
                }
            }
        }

        Ok(DecodedWpg {
            width: header.width,
            height: header.height,
            rgba,
            header,
            preview,
            grain,
        })
    }
}
