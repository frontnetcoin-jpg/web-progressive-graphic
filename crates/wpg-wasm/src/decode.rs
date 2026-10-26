use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use std::io::{Cursor, Read};

use crate::{
    FLAG_ALPHA, FLAG_FILM_GRAIN, FLAG_MICRO_PREVIEW, FLAG_SECONDARY_ENTROPY, WPG_MAGIC,
};

/// Hard cap so a hostile length cannot force a multi-gigabyte Wasm allocation.
const MAX_PIXELS: u64 = 32_000_000;
const MAX_INFLATE: usize = 256 * 1024 * 1024;

pub struct Located<'a> {
    pub width: u32,
    pub height: u32,
    pub mode: u8,
    pub flags: u16,
    pub payload: &'a [u8],
    pub alpha_compressed: Option<&'a [u8]>,
}

pub fn pixel_count(width: u32, height: u32) -> Result<usize, i32> {
    let pixels = (width as u64).checked_mul(height as u64).ok_or(-3)?;
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(-3);
    }
    usize::try_from(pixels).map_err(|_| -3)
}

/// Walk the optional blocks with checked addition.
/// The alpha block is `u32 BE alpha_len` followed by `alpha_len` compressed bytes.
pub fn locate(data: &[u8]) -> Result<Located<'_>, i32> {
    if data.len() < 16 {
        return Err(-1);
    }
    if &data[0..4] != WPG_MAGIC {
        return Err(-2);
    }

    let width = ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | (data[6] as u32);
    let height = ((data[7] as u32) << 16) | ((data[8] as u32) << 8) | (data[9] as u32);
    if width == 0 || height == 0 {
        return Err(-1);
    }
    let mode = data[10];
    let flags = BigEndian::read_u16(&data[12..14]);

    let mut offset: usize = 16;
    if (flags & FLAG_MICRO_PREVIEW) != 0 {
        offset = offset.checked_add(16).ok_or(-3)?;
    }
    if (flags & FLAG_FILM_GRAIN) != 0 {
        offset = offset.checked_add(8).ok_or(-3)?;
    }
    if offset > data.len() {
        return Err(-3);
    }

    let alpha_compressed = if (flags & FLAG_ALPHA) != 0 {
        let len_end = offset.checked_add(4).ok_or(-3)?;
        if len_end > data.len() {
            return Err(-3);
        }
        let alpha_len = BigEndian::read_u32(&data[offset..len_end]) as usize;
        let alpha_end = len_end.checked_add(alpha_len).ok_or(-3)?;
        if alpha_end > data.len() {
            return Err(-3);
        }
        offset = alpha_end;
        Some(&data[len_end..alpha_end])
    } else {
        None
    };

    if offset > data.len() {
        return Err(-3);
    }

    Ok(Located {
        width,
        height,
        mode,
        flags,
        payload: &data[offset..],
        alpha_compressed,
    })
}

pub fn zstd_decode(src: &[u8], max_out: usize) -> Result<Vec<u8>, i32> {
    if src.is_empty() || max_out == 0 || max_out > MAX_INFLATE {
        return Err(-5);
    }
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(src).map_err(|_| -5)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = decoder.read(&mut buf).map_err(|_| -5)?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > max_out {
            return Err(-5);
        }
        out.extend_from_slice(&buf[..n]);
    }
    if out.is_empty() {
        return Err(-5);
    }
    Ok(out)
}

pub fn merge_alpha(rgba: &mut [u8], compressed: &[u8], pixels: usize) -> Result<(), i32> {
    let max_out = pixels.saturating_add(512);
    let plane = zstd_decode(compressed, max_out)?;
    let n = plane.len().min(pixels);
    for (i, &a) in plane.iter().take(n).enumerate() {
        let idx = i * 4 + 3;
        if idx < rgba.len() {
            rgba[idx] = a;
        }
    }
    Ok(())
}

/// Mode 1: palette + 2D RLE, Zstd compressed.
pub fn decode_palette(compressed: &[u8], width: u32, height: u32) -> Result<Vec<u8>, i32> {
    let pixels = pixel_count(width, height)?;
    let uncompressed = zstd_decode(compressed, pixels.saturating_mul(4).saturating_add(1024))?;
    let mut reader = Cursor::new(uncompressed);

    let palette_len = reader.read_u16::<BigEndian>().map_err(|_| -4)? as usize;
    if palette_len > 256 {
        return Err(-4);
    }
    let mut palette = Vec::with_capacity(palette_len);
    for _ in 0..palette_len {
        let mut color = [0u8; 4];
        reader.read_exact(&mut color).map_err(|_| -4)?;
        palette.push(color);
    }

    let mut rle_stream = Vec::new();
    reader.read_to_end(&mut rle_stream).map_err(|_| -4)?;

    let mut indices = Vec::with_capacity(pixels);
    let mut i = 0;
    while i < rle_stream.len() && indices.len() < pixels {
        if rle_stream[i] == 0xFF && i + 2 < rle_stream.len() {
            let run_len = rle_stream[i + 1] as usize;
            let color_idx = rle_stream[i + 2];
            let new_len = indices.len().saturating_add(run_len).min(pixels);
            indices.resize(new_len, color_idx);
            i += 3;
        } else {
            indices.push(rle_stream[i]);
            i += 1;
        }
    }
    if indices.len() < pixels {
        indices.resize(pixels, 0);
    }

    let mut rgba = vec![0u8; pixels * 4];
    for (px_idx, &p_idx) in indices.iter().enumerate() {
        let color = palette.get(p_idx as usize).copied().unwrap_or([0, 0, 0, 255]);
        let out_idx = px_idx * 4;
        rgba[out_idx] = color[0];
        rgba[out_idx + 1] = color[1];
        rgba[out_idx + 2] = color[2];
        rgba[out_idx + 3] = color[3];
    }
    Ok(rgba)
}

/// Mode 2: reversible YCoCg-R + Paeth residuals, Zstd compressed.
pub fn decode_modular(compressed: &[u8], width: u32, height: u32) -> Result<Vec<u8>, i32> {
    let w = width as usize;
    let h = height as usize;
    let pixels = pixel_count(width, height)?;
    let buffer = zstd_decode(compressed, pixels.saturating_mul(8).saturating_add(4096))?;
    let mut reader = Cursor::new(buffer);

    let y_res = read_zigzag_plane(&mut reader, pixels)?;
    let co_res = read_zigzag_plane(&mut reader, pixels)?;
    let cg_res = read_zigzag_plane(&mut reader, pixels)?;
    let mut a_res = vec![0u8; pixels];
    reader.read_exact(&mut a_res).map_err(|_| -4)?;

    let y_plane = unpredict_plane(&y_res, w, h);
    let co_plane = unpredict_plane(&co_res, w, h);
    let cg_plane = unpredict_plane(&cg_res, w, h);
    let a_plane = unpredict_alpha_plane(&a_res, w, h);

    let mut rgba = vec![255u8; pixels * 4];
    for i in 0..pixels {
        let (r, g, b) = ycocg_r_to_rgb(y_plane[i], co_plane[i], cg_plane[i]);
        let idx = i * 4;
        rgba[idx] = r;
        rgba[idx + 1] = g;
        rgba[idx + 2] = b;
        rgba[idx + 3] = a_plane[i];
    }
    Ok(rgba)
}

fn read_zigzag_plane(reader: &mut Cursor<Vec<u8>>, pixels: usize) -> Result<Vec<i16>, i32> {
    let mut plane = Vec::with_capacity(pixels);
    for _ in 0..pixels {
        let z = reader.read_u16::<BigEndian>().map_err(|_| -4)?;
        let val = if (z & 1) == 0 {
            (z >> 1) as i16
        } else {
            -(((z + 1) >> 1) as i16)
        };
        plane.push(val);
    }
    Ok(plane)
}

fn unpredict_plane(res: &[i16], w: usize, h: usize) -> Vec<i16> {
    let mut plane = vec![0i16; res.len()];
    for y in 0..h {
        for x in 0..w {
            let diff = res[y * w + x];
            let pred = if x == 0 && y == 0 {
                0
            } else if y == 0 {
                plane[y * w + x - 1]
            } else if x == 0 {
                plane[(y - 1) * w + x]
            } else {
                let a = plane[y * w + x - 1];
                let b = plane[(y - 1) * w + x];
                let c = plane[(y - 1) * w + x - 1];
                let p = a.wrapping_add(b).wrapping_sub(c);
                let pa = (p as i32 - a as i32).abs();
                let pb = (p as i32 - b as i32).abs();
                let pc = (p as i32 - c as i32).abs();
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            };
            plane[y * w + x] = diff.wrapping_add(pred);
        }
    }
    plane
}

fn unpredict_alpha_plane(res: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut plane = vec![0u8; res.len()];
    for y in 0..h {
        for x in 0..w {
            let diff = res[y * w + x];
            let pred = if x == 0 {
                if y == 0 {
                    255
                } else {
                    plane[(y - 1) * w + x]
                }
            } else {
                plane[y * w + x - 1]
            };
            plane[y * w + x] = pred.wrapping_add(diff);
        }
    }
    plane
}

fn ycocg_r_to_rgb(y: i16, co: i16, cg: i16) -> (u8, u8, u8) {
    let t = y.wrapping_sub(cg >> 1);
    let g = cg.wrapping_add(t);
    let b = t.wrapping_sub(co >> 1);
    let r = b.wrapping_add(co);
    (
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
    )
}

pub fn payload_obu(located: &Located<'_>) -> Result<Vec<u8>, i32> {
    if located.payload.is_empty() {
        return Err(-4);
    }
    if (located.flags & FLAG_SECONDARY_ENTROPY) != 0 {
        let pixels = pixel_count(located.width, located.height)?;
        let max_out = pixels.saturating_mul(4).saturating_add(65536);
        return zstd_decode(located.payload, max_out);
    }
    Ok(located.payload.to_vec())
}
