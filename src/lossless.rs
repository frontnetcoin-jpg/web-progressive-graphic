use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};

/// Lossless Mode 1: Palette + 2D RLE + Zstd
pub struct PaletteCodec;

impl PaletteCodec {
    /// Attempt to encode as a palette image. Returns None if unique colors > 256.
    pub fn encode(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
        let total_pixels = (width * height) as usize;
        if total_pixels == 0 || rgba.len() < total_pixels * 4 {
            return None;
        }

        let mut color_map = HashMap::<[u8; 4], u8>::new();
        let mut palette = Vec::<[u8; 4]>::new();
        let mut indices = Vec::<u8>::with_capacity(total_pixels);

        for i in 0..total_pixels {
            let idx = i * 4;
            let color = [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]];

            if let Some(&p_idx) = color_map.get(&color) {
                indices.push(p_idx);
            } else {
                if palette.len() >= 256 {
                    return None; // Exceeds 256 colors, cannot use palette mode
                }
                let new_idx = palette.len() as u8;
                palette.push(color);
                color_map.insert(color, new_idx);
                indices.push(new_idx);
            }
        }

        // Encode palette table
        let mut uncompressed = Vec::new();
        uncompressed.write_u16::<BigEndian>(palette.len() as u16).ok()?;
        for color in &palette {
            uncompressed.write_all(color).ok()?;
        }

        // Apply 2D RLE on index stream
        let mut rle_stream = Vec::new();
        let mut i = 0;
        while i < indices.len() {
            let current_idx = indices[i];
            let mut run_len = 1u8;
            while (i + (run_len as usize)) < indices.len() && indices[i + (run_len as usize)] == current_idx && run_len < 255 {
                run_len += 1;
            }

            if run_len >= 3 || current_idx == 0xFF {
                rle_stream.push(0xFF); // Escape byte
                rle_stream.push(run_len);
                rle_stream.push(current_idx);
                i += run_len as usize;
            } else {
                rle_stream.push(current_idx);
                i += 1;
            }
        }

        uncompressed.write_all(&rle_stream).ok()?;

        // Secondary entropy super-compression with Zstandard level 19
        zstd::bulk::compress(&uncompressed, 19).ok()
    }

    pub fn decode(compressed: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
        let total_pixels = (width * height) as usize;
        let uncompressed = zstd::bulk::decompress(compressed, total_pixels * 4 + 1024)?;
        let mut reader = Cursor::new(uncompressed);

        let palette_len = reader.read_u16::<BigEndian>()? as usize;
        if palette_len > 256 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid palette length"));
        }

        let mut palette = Vec::with_capacity(palette_len);
        for _ in 0..palette_len {
            let mut color = [0u8; 4];
            reader.read_exact(&mut color)?;
            palette.push(color);
        }

        let mut rle_stream = Vec::new();
        reader.read_to_end(&mut rle_stream)?;

        let mut indices = Vec::with_capacity(total_pixels);
        let mut i = 0;
        while i < rle_stream.len() && indices.len() < total_pixels {
            if rle_stream[i] == 0xFF && i + 2 < rle_stream.len() {
                let run_len = rle_stream[i + 1] as usize;
                let color_idx = rle_stream[i + 2];
                indices.resize(indices.len() + run_len, color_idx);
                i += 3;
            } else {
                indices.push(rle_stream[i]);
                i += 1;
            }
        }

        if indices.len() < total_pixels {
            indices.resize(total_pixels, 0);
        }

        let mut rgba = vec![0u8; total_pixels * 4];
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
}

/// Lossless Mode 2: Reversible YCoCg-R + Adaptive Spatial Predictors + Zstandard
pub struct ModularLosslessCodec;

impl ModularLosslessCodec {
    pub fn encode(rgba: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
        let w = width as usize;
        let h = height as usize;
        let total = w * h;

        let mut y_plane = Vec::<i16>::with_capacity(total);
        let mut co_plane = Vec::<i16>::with_capacity(total);
        let mut cg_plane = Vec::<i16>::with_capacity(total);
        let mut a_plane = Vec::<u8>::with_capacity(total);

        // Transform to YCoCg-R
        for i in 0..total {
            let idx = i * 4;
            let (y, co, cg) = crate::color::rgb_to_ycocg_r(rgba[idx], rgba[idx + 1], rgba[idx + 2]);
            y_plane.push(y);
            co_plane.push(co);
            cg_plane.push(cg);
            a_plane.push(rgba[idx + 3]);
        }

        // Apply Adaptive Median/Paeth Predictive filtering per plane
        let y_res = Self::predict_plane(&y_plane, w, h);
        let co_res = Self::predict_plane(&co_plane, w, h);
        let cg_res = Self::predict_plane(&cg_plane, w, h);
        let a_res = Self::predict_alpha_plane(&a_plane, w, h);

        // ZigZag encode residuals into u16
        let mut buffer = Vec::with_capacity(total * 8);
        for &val in &y_res {
            let z = if val >= 0 { (val as u16) << 1 } else { (((-val) as u16) << 1) - 1 };
            buffer.write_u16::<BigEndian>(z)?;
        }
        for &val in &co_res {
            let z = if val >= 0 { (val as u16) << 1 } else { (((-val) as u16) << 1) - 1 };
            buffer.write_u16::<BigEndian>(z)?;
        }
        for &val in &cg_res {
            let z = if val >= 0 { (val as u16) << 1 } else { (((-val) as u16) << 1) - 1 };
            buffer.write_u16::<BigEndian>(z)?;
        }
        buffer.write_all(&a_res)?;

        // Entropy super-compression
        zstd::bulk::compress(&buffer, 12)
    }

    pub fn decode(compressed: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
        let w = width as usize;
        let h = height as usize;
        let total = w * h;

        let buffer = zstd::bulk::decompress(compressed, total * 8 + 4096)?;
        let mut reader = Cursor::new(buffer);

        let mut y_res = Vec::with_capacity(total);
        for _ in 0..total {
            let z = reader.read_u16::<BigEndian>()?;
            let val = if (z & 1) == 0 { (z >> 1) as i16 } else { -(((z + 1) >> 1) as i16) };
            y_res.push(val);
        }

        let mut co_res = Vec::with_capacity(total);
        for _ in 0..total {
            let z = reader.read_u16::<BigEndian>()?;
            let val = if (z & 1) == 0 { (z >> 1) as i16 } else { -(((z + 1) >> 1) as i16) };
            co_res.push(val);
        }

        let mut cg_res = Vec::with_capacity(total);
        for _ in 0..total {
            let z = reader.read_u16::<BigEndian>()?;
            let val = if (z & 1) == 0 { (z >> 1) as i16 } else { -(((z + 1) >> 1) as i16) };
            cg_res.push(val);
        }

        let mut a_res = vec![0u8; total];
        reader.read_exact(&mut a_res)?;

        // Inverse prediction
        let y_plane = Self::unpredict_plane(&y_res, w, h);
        let co_plane = Self::unpredict_plane(&co_res, w, h);
        let cg_plane = Self::unpredict_plane(&cg_res, w, h);
        let a_plane = Self::unpredict_alpha_plane(&a_res, w, h);

        // Inverse YCoCg-R to sRGB
        let mut rgba = vec![255u8; total * 4];
        for i in 0..total {
            let (r, g, b) = crate::color::ycocg_r_to_rgb(y_plane[i], co_plane[i], cg_plane[i]);
            let idx = i * 4;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = a_plane[i];
        }

        Ok(rgba)
    }

    fn predict_plane(plane: &[i16], w: usize, h: usize) -> Vec<i16> {
        let mut res = vec![0i16; plane.len()];
        for y in 0..h {
            for x in 0..w {
                let current = plane[y * w + x];
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
                    // Median / Paeth predictor
                    let p = a + b - c;
                    let pa = (p - a).abs();
                    let pb = (p - b).abs();
                    let pc = (p - c).abs();
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                };
                res[y * w + x] = current - pred;
            }
        }
        res
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
                    let p = a + b - c;
                    let pa = (p - a).abs();
                    let pb = (p - b).abs();
                    let pc = (p - c).abs();
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                };
                plane[y * w + x] = diff + pred;
            }
        }
        plane
    }

    fn predict_alpha_plane(plane: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut res = vec![0u8; plane.len()];
        for y in 0..h {
            for x in 0..w {
                let current = plane[y * w + x];
                let pred = if x == 0 { if y == 0 { 255 } else { plane[(y - 1) * w + x] } } else { plane[y * w + x - 1] };
                res[y * w + x] = current.wrapping_sub(pred);
            }
        }
        res
    }

    fn unpredict_alpha_plane(res: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut plane = vec![0u8; res.len()];
        for y in 0..h {
            for x in 0..w {
                let diff = res[y * w + x];
                let pred = if x == 0 { if y == 0 { 255 } else { plane[(y - 1) * w + x] } } else { plane[y * w + x - 1] };
                plane[y * w + x] = pred.wrapping_add(diff);
            }
        }
        plane
    }
}
