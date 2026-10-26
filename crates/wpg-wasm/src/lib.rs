mod decode;

use std::alloc::{alloc, dealloc, Layout};

use byteorder::{BigEndian, ByteOrder};
use decode::{decode_modular, decode_palette, locate, merge_alpha, payload_obu, pixel_count};

pub const WPG_MAGIC: &[u8; 4] = b"WPG\x01";
pub const FLAG_ALPHA: u16 = 0x0001;
pub const FLAG_MICRO_PREVIEW: u16 = 0x0002;
pub const FLAG_FILM_GRAIN: u16 = 0x0004;
pub const FLAG_SECONDARY_ENTROPY: u16 = 0x0008;
pub const FLAG_LOSSLESS: u16 = 0x0010;

#[repr(C)]
pub struct WpgWasmInfo {
    pub width: u32,
    pub height: u32,
    pub mode: u8,
    pub subsampling: u8,
    pub flags: u16,
    pub quality: u8,
    pub alpha_mode: u8,
    pub has_preview: u8,
    pub has_grain: u8,
    pub grain_scale: u8,
    pub grain_seed: u16,
    pub grain_cutoff: u8,
    pub chroma_scale: u8,
}

/// Allocate `size` bytes. The pointer is freed with `wpg_free(ptr, size)` using that same size.
/// `Vec::with_capacity` is not used: its capacity may be larger than `size`, and freeing with
/// `size` as the capacity is undefined behavior.
#[no_mangle]
pub extern "C" fn wpg_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let Ok(layout) = Layout::from_size_align(size, 1) else {
        return std::ptr::null_mut();
    };
    unsafe { alloc(layout) }
}

/// Free a pointer from `wpg_alloc` or from an export that copied into an exact-size allocation.
/// `size` is the allocation size, which equals the logical length of every buffer this crate returns.
#[no_mangle]
pub unsafe extern "C" fn wpg_free(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(size, 1) {
        dealloc(ptr, layout);
    }
}

fn export_bytes(bytes: &[u8]) -> Result<(*mut u8, usize), i32> {
    let len = bytes.len();
    if len == 0 {
        return Err(-4);
    }
    let layout = Layout::from_size_align(len, 1).map_err(|_| -4)?;
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        return Err(-4);
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len) };
    Ok((ptr, len))
}

/// Parse WPG header and metadata
#[no_mangle]
pub unsafe extern "C" fn wpg_parse_info(ptr: *const u8, len: usize, out_info: *mut WpgWasmInfo) -> i32 {
    if ptr.is_null() || len < 16 || out_info.is_null() {
        return -1;
    }
    let data = std::slice::from_raw_parts(ptr, len);
    if &data[0..4] != WPG_MAGIC {
        return -2;
    }

    let width = ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | (data[6] as u32);
    let height = ((data[7] as u32) << 16) | ((data[8] as u32) << 8) | (data[9] as u32);
    let mode = data[10];
    let subsampling = data[11];
    let flags = BigEndian::read_u16(&data[12..14]);
    let quality = data[14];
    let alpha_mode = data[15];

    let has_preview = if (flags & FLAG_MICRO_PREVIEW) != 0 { 1 } else { 0 };
    let has_grain = if (flags & FLAG_FILM_GRAIN) != 0 { 1 } else { 0 };

    let mut grain_scale = 0;
    let mut grain_seed = 0;
    let mut grain_cutoff = 0;
    let mut chroma_scale = 0;

    let mut offset = 16usize;
    if has_preview == 1 {
        offset = match offset.checked_add(16) {
            Some(v) => v,
            None => return -3,
        };
    }
    if has_grain == 1 && len >= offset.saturating_add(8) && offset.saturating_add(8) >= offset {
        grain_scale = data[offset];
        grain_seed = BigEndian::read_u16(&data[offset + 1..offset + 3]);
        grain_cutoff = data[offset + 3];
        chroma_scale = data[offset + 4];
    }

    *out_info = WpgWasmInfo {
        width,
        height,
        mode,
        subsampling,
        flags,
        quality,
        alpha_mode,
        has_preview,
        has_grain,
        grain_scale,
        grain_seed,
        grain_cutoff,
        chroma_scale,
    };

    0
}

/// Decode the 16-byte micro-preview into 64 bytes of RGBA (4x4 pixels)
#[no_mangle]
pub unsafe extern "C" fn wpg_decode_preview(ptr: *const u8, len: usize, out_rgba_64: *mut u8) -> i32 {
    if ptr.is_null() || len < 32 || out_rgba_64.is_null() {
        return -1;
    }
    let data = std::slice::from_raw_parts(ptr, len);
    if &data[0..4] != WPG_MAGIC {
        return -2;
    }
    let flags = BigEndian::read_u16(&data[12..14]);
    if (flags & FLAG_MICRO_PREVIEW) == 0 {
        return -3;
    }

    let preview_bytes = &data[16..32];
    let mut palette = [[0u8; 3]; 4];
    for i in 0..4 {
        palette[i] = [
            preview_bytes[i * 3],
            preview_bytes[i * 3 + 1],
            preview_bytes[i * 3 + 2],
        ];
    }

    let out = std::slice::from_raw_parts_mut(out_rgba_64, 64);
    let mut pixel_idx = 0;
    for byte_idx in 0..4 {
        let b = preview_bytes[12 + byte_idx];
        for shift in [6, 4, 2, 0] {
            let color_idx = ((b >> shift) & 0x03) as usize;
            let c = palette[color_idx];
            let o = pixel_idx * 4;
            out[o] = c[0];
            out[o + 1] = c[1];
            out[o + 2] = c[2];
            out[o + 3] = 255;
            pixel_idx += 1;
        }
    }

    0
}

/// Wrap a Mode 0 AV1 OBU into an AVIF container.
///
/// Returns `-5` when `mode != 0` (palette / modular). Those files are decoded by
/// `wpg_decode_rgba`. A Mode 0 payload with `FLAG_SECONDARY_ENTROPY` is Zstd-inflated
/// before wrapping. If that inflate fails, this also returns `-5`.
/// The alpha plane is skipped here and applied later by `wpg_apply_alpha`.
#[no_mangle]
pub unsafe extern "C" fn wpg_wrap_to_avif(
    ptr: *const u8,
    len: usize,
    out_avif_ptr: *mut *mut u8,
    out_avif_len: *mut usize,
) -> i32 {
    if ptr.is_null() || len < 16 || out_avif_ptr.is_null() || out_avif_len.is_null() {
        return -1;
    }
    let data = std::slice::from_raw_parts(ptr, len);
    let located = match locate(data) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if located.mode != 0 {
        return -5;
    }

    let obu = match payload_obu(&located) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let aviffy = avif_serialize::Aviffy::new();
    let mut avif_file = aviffy.to_vec(&obu, None, located.width, located.height, 8);
    // Drop spare capacity before the Vec is freed normally. The pointer handed to JS
    // is a fresh exact-size allocation, so `wpg_free(ptr, len)` matches the allocator.
    avif_file.shrink_to_fit();
    let (exported, out_len) = match export_bytes(&avif_file) {
        Ok(v) => v,
        Err(code) => return code,
    };

    *out_avif_ptr = exported;
    *out_avif_len = out_len;
    0
}

/// Decode Mode 1 (palette) and Mode 2 (modular) to tightly packed RGBA.
/// Mode 0 returns `-5`; the caller wraps that payload as AVIF instead.
/// A separate alpha plane, when present, is merged into the alpha channel.
#[no_mangle]
pub unsafe extern "C" fn wpg_decode_rgba(
    ptr: *const u8,
    len: usize,
    out_rgba_ptr: *mut *mut u8,
    out_rgba_len: *mut usize,
) -> i32 {
    if ptr.is_null() || len < 16 || out_rgba_ptr.is_null() || out_rgba_len.is_null() {
        return -1;
    }
    let data = std::slice::from_raw_parts(ptr, len);
    let located = match locate(data) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if located.mode == 0 || (located.mode != 1 && located.mode != 2) {
        return -5;
    }
    if located.payload.is_empty() {
        return -4;
    }

    let mut rgba = match located.mode {
        1 => match decode_palette(located.payload, located.width, located.height) {
            Ok(v) => v,
            Err(code) => return code,
        },
        2 => match decode_modular(located.payload, located.width, located.height) {
            Ok(v) => v,
            Err(code) => return code,
        },
        _ => return -5,
    };

    if let Some(alpha) = located.alpha_compressed {
        let pixels = match pixel_count(located.width, located.height) {
            Ok(v) => v,
            Err(code) => return code,
        };
        if let Err(code) = merge_alpha(&mut rgba, alpha, pixels) {
            return code;
        }
    }

    let (exported, out_len) = match export_bytes(&rgba) {
        Ok(v) => v,
        Err(code) => return code,
    };
    *out_rgba_ptr = exported;
    *out_rgba_len = out_len;
    0
}

/// Write the Zstd alpha plane of a `.wpg` file into an existing RGBA buffer.
/// No-op (returns 0) when `FLAG_ALPHA` is clear. Offset advances use checked arithmetic.
#[no_mangle]
pub unsafe extern "C" fn wpg_apply_alpha(
    rgba_ptr: *mut u8,
    width: u32,
    height: u32,
    wpg_ptr: *const u8,
    wpg_len: usize,
) -> i32 {
    if rgba_ptr.is_null() || wpg_ptr.is_null() || wpg_len < 16 || width == 0 || height == 0 {
        return -1;
    }
    let pixels = match pixel_count(width, height) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rgba_len = match pixels.checked_mul(4) {
        Some(v) => v,
        None => return -3,
    };
    let wpg = std::slice::from_raw_parts(wpg_ptr, wpg_len);
    let located = match locate(wpg) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if located.width != width || located.height != height {
        return -3;
    }
    let Some(alpha) = located.alpha_compressed else {
        return 0;
    };
    let rgba = std::slice::from_raw_parts_mut(rgba_ptr, rgba_len);
    match merge_alpha(rgba, alpha, pixels) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// Apply deterministic Xorshift32 parametric film grain to an RGBA pixel buffer in-place
#[no_mangle]
pub unsafe extern "C" fn wpg_apply_film_grain(
    rgba_ptr: *mut u8,
    width: u32,
    height: u32,
    scale: u8,
    seed: u16,
) -> i32 {
    if rgba_ptr.is_null() || width == 0 || height == 0 || scale == 0 {
        return 0;
    }
    let total_pixels = match pixel_count(width, height) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let data = std::slice::from_raw_parts_mut(rgba_ptr, total_pixels * 4);

    let mut prng_state = if seed == 0 { 0x6d2b79f5 } else { (seed as u32) | 0x10001 };
    let grain_amplitude = (scale as f32) * 0.18;

    for i in 0..total_pixels {
        let offset = i * 4;
        let r = data[offset] as f32;
        let g = data[offset + 1] as f32;
        let b = data[offset + 2] as f32;

        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let norm_y = y / 255.0;
        let lum_weight = 1.0 - 4.0 * (norm_y - 0.5) * (norm_y - 0.5);

        if lum_weight > 0.05 {
            prng_state ^= prng_state << 13;
            prng_state ^= prng_state >> 17;
            prng_state ^= prng_state << 5;

            let rand_val = ((prng_state & 0xFFFF) as f32 / 32768.0) - 1.0;
            let noise = rand_val * grain_amplitude * lum_weight;

            data[offset] = (r + noise).clamp(0.0, 255.0) as u8;
            data[offset + 1] = (g + noise).clamp(0.0, 255.0) as u8;
            data[offset + 2] = (b + noise).clamp(0.0, 255.0) as u8;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;
    use std::io::Write;

    fn header(mode: u8, flags: u16, w: u32, h: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 16];
        bytes[0..4].copy_from_slice(b"WPG\x01");
        bytes[4] = (w >> 16) as u8;
        bytes[5] = (w >> 8) as u8;
        bytes[6] = w as u8;
        bytes[7] = (h >> 16) as u8;
        bytes[8] = (h >> 8) as u8;
        bytes[9] = h as u8;
        bytes[10] = mode;
        bytes[12] = (flags >> 8) as u8;
        bytes[13] = flags as u8;
        bytes
    }

    #[test]
    fn alloc_free_roundtrip_exact_size() {
        let ptr = wpg_alloc(32);
        assert!(!ptr.is_null());
        unsafe {
            std::ptr::write_bytes(ptr, 0xAB, 32);
            wpg_free(ptr, 32);
        }
    }

    #[test]
    fn wrap_rejects_palette_and_huge_alpha_length() {
        let mut file = header(1, 0, 2, 2);
        file.extend_from_slice(&[1, 2, 3, 4]);
        let mut out_ptr = std::ptr::null_mut();
        let mut out_len = 0usize;
        let code = unsafe {
            wpg_wrap_to_avif(file.as_ptr(), file.len(), &mut out_ptr, &mut out_len)
        };
        assert_eq!(code, -5);
        assert!(out_ptr.is_null());

        let mut alpha_file = header(0, FLAG_ALPHA, 1, 1);
        alpha_file.extend_from_slice(&u32::MAX.to_be_bytes());
        let code = unsafe {
            wpg_wrap_to_avif(alpha_file.as_ptr(), alpha_file.len(), &mut out_ptr, &mut out_len)
        };
        assert_eq!(code, -3);
    }

    #[test]
    fn wrap_mode0_exports_buffer_that_free_accepts() {
        let mut file = header(0, 0, 1, 1);
        file.extend_from_slice(&[0x0A, 0x0B]);
        let mut out_ptr = std::ptr::null_mut();
        let mut out_len = 0usize;
        let code = unsafe {
            wpg_wrap_to_avif(file.as_ptr(), file.len(), &mut out_ptr, &mut out_len)
        };
        assert_eq!(code, 0);
        assert!(out_len > 16);
        unsafe { wpg_free(out_ptr, out_len) };
    }

    #[test]
    fn wrap_inflates_secondary_entropy_before_avif() {
        let obu = b"raw-obu-payload";
        let compressed = zstd::bulk::compress(obu, 3).unwrap();
        let mut file = header(0, FLAG_SECONDARY_ENTROPY, 1, 1);
        file.extend_from_slice(&compressed);
        let mut out_ptr = std::ptr::null_mut();
        let mut out_len = 0usize;
        let code = unsafe {
            wpg_wrap_to_avif(file.as_ptr(), file.len(), &mut out_ptr, &mut out_len)
        };
        assert_eq!(code, 0, "secondary entropy on mode 0 should inflate then wrap");
        let avif = unsafe { std::slice::from_raw_parts(out_ptr, out_len) };
        assert!(avif.windows(obu.len()).any(|w| w == obu));
        unsafe { wpg_free(out_ptr, out_len) };
    }

    #[test]
    fn decode_rgba_palette_and_alpha_plane() {
        let pixels = [[255u8, 0, 0, 10], [0, 255, 0, 20]];
        let mut raw = Vec::new();
        raw.write_u16::<BigEndian>(1).unwrap();
        raw.write_all(&[255, 0, 0, 10]).unwrap();
        raw.push(0xFF);
        raw.push(2);
        raw.push(0);
        let compressed = zstd::bulk::compress(&raw, 3).unwrap();
        let alpha = zstd::bulk::compress(&[7u8, 9], 3).unwrap();

        let mut file = header(1, FLAG_ALPHA, 2, 1);
        file.write_u32::<BigEndian>(alpha.len() as u32).unwrap();
        file.extend_from_slice(&alpha);
        file.extend_from_slice(&compressed);

        let mut out_ptr = std::ptr::null_mut();
        let mut out_len = 0usize;
        let code = unsafe {
            wpg_decode_rgba(file.as_ptr(), file.len(), &mut out_ptr, &mut out_len)
        };
        assert_eq!(code, 0);
        assert_eq!(out_len, 8);
        let rgba = unsafe { std::slice::from_raw_parts(out_ptr, out_len) };
        assert_eq!(&rgba[0..3], &pixels[0][0..3]);
        assert_eq!(rgba[3], 7);
        assert_eq!(rgba[7], 9);
        unsafe { wpg_free(out_ptr, out_len) };
    }

    #[test]
    fn apply_alpha_writes_plane_into_existing_rgba() {
        let alpha = zstd::bulk::compress(&[0u8, 128, 255], 3).unwrap();
        let mut file = header(0, FLAG_ALPHA, 3, 1);
        file.write_u32::<BigEndian>(alpha.len() as u32).unwrap();
        file.extend_from_slice(&alpha);
        file.extend_from_slice(&[9, 9, 9]);

        let mut rgba = vec![255u8; 12];
        let code = unsafe {
            wpg_apply_alpha(rgba.as_mut_ptr(), 3, 1, file.as_ptr(), file.len())
        };
        assert_eq!(code, 0);
        assert_eq!(rgba[3], 0);
        assert_eq!(rgba[7], 128);
        assert_eq!(rgba[11], 255);
        assert_eq!(rgba[0], 255);
    }
}
