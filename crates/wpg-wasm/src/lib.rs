use byteorder::{BigEndian, ByteOrder};

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

#[no_mangle]
pub extern "C" fn wpg_alloc(size: usize) -> *mut u8 {
    let mut vec = Vec::with_capacity(size);
    let ptr = vec.as_mut_ptr();
    std::mem::forget(vec);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn wpg_free(ptr: *mut u8, size: usize) {
    if !ptr.is_null() && size > 0 {
        let _ = Vec::from_raw_parts(ptr, 0, size);
    }
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

    let mut offset = 16;
    if has_preview == 1 {
        offset += 16;
    }
    if has_grain == 1 && len >= offset + 8 {
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

/// Wrap WPG Mode 0 raw AV1 OBU payload into a standard AVIF container for native GPU browser decoding
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
    if &data[0..4] != WPG_MAGIC {
        return -2;
    }

    let width = ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | (data[6] as u32);
    let height = ((data[7] as u32) << 16) | ((data[8] as u32) << 8) | (data[9] as u32);
    let flags = BigEndian::read_u16(&data[12..14]);

    let mut offset = 16;
    if (flags & FLAG_MICRO_PREVIEW) != 0 {
        offset += 16;
    }
    if (flags & FLAG_FILM_GRAIN) != 0 {
        offset += 8;
    }
    if (flags & FLAG_ALPHA) != 0 {
        if len < offset + 4 {
            return -3;
        }
        let alpha_len = BigEndian::read_u32(&data[offset..offset + 4]) as usize;
        offset += 4 + alpha_len;
    }

    if offset >= len {
        return -4;
    }
    let obu_payload = &data[offset..];

    let aviffy = avif_serialize::Aviffy::new();
    let avif_file = aviffy.to_vec(obu_payload, None, width, height, 8);
    let out_len = avif_file.len();
    let mut out_vec = avif_file;
    let ptr = out_vec.as_mut_ptr();
    std::mem::forget(out_vec);

    *out_avif_ptr = ptr;
    *out_avif_len = out_len;
    0
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
    let total_pixels = (width as usize) * (height as usize);
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
