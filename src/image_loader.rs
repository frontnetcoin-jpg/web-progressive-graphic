use std::path::Path;

/// Load image from memory and automatically apply EXIF orientation if present
pub fn load_image_with_orientation(raw_bytes: &[u8]) -> Result<image::DynamicImage, Box<dyn std::error::Error>> {
    let mut img = image::load_from_memory(raw_bytes)?;

    // Check for Exif in JPEG or TIFF
    if let Some(pos) = raw_bytes.windows(6).position(|w| w == b"Exif\0\0") {
        let chunk = &raw_bytes[pos + 6..];
        if let Some(orient) = image::metadata::Orientation::from_exif_chunk(chunk) {
            img.apply_orientation(orient);
        }
    } else {
        // Direct TIFF headers
        for &magic in &[b"\x49\x49\x2A\x00", b"\x4D\x4D\x00\x2A"] {
            if let Some(pos) = raw_bytes.windows(4).position(|w| w == magic) {
                if let Some(orient) = image::metadata::Orientation::from_exif_chunk(&raw_bytes[pos..]) {
                    img.apply_orientation(orient);
                    break;
                }
            }
        }
    }

    Ok(img)
}

/// Load image from file path with automatic EXIF orientation
pub fn open_image_with_orientation<P: AsRef<Path>>(path: P) -> Result<image::DynamicImage, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path.as_ref())?;
    load_image_with_orientation(&bytes)
}
