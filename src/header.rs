use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Read, Write};

pub const WPG_MAGIC: &[u8; 4] = b"WPG\x01"; // Web Progressive Graphic v1.0
pub const WPG_HEADER_SIZE: usize = 16;

pub const FLAG_ALPHA: u16 = 1 << 0;
pub const FLAG_MICRO_PREVIEW: u16 = 1 << 1;
pub const FLAG_FILM_GRAIN: u16 = 1 << 2;
pub const FLAG_SECONDARY_ENTROPY: u16 = 1 << 3;
pub const FLAG_LOSSLESS: u16 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum WpgMode {
    Av1Enhanced = 0,     // Lossy or high-fidelity AV1 intra frame
    PaletteLossless = 1, // Palette + 2D RLE + Entropy (flat UI, anime, logos)
    ModularLossless = 2, // Reversible YCoCg-R + Spatial Predictor (lossless photo)
}

impl WpgMode {
    pub fn from_u8(v: u8) -> io::Result<Self> {
        match v {
            0 => Ok(WpgMode::Av1Enhanced),
            1 => Ok(WpgMode::PaletteLossless),
            2 => Ok(WpgMode::ModularLossless),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unknown WPG mode: {}", v))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ColorSubsampling {
    Yuv420 = 0,
    Yuv422 = 1,
    Yuv444 = 2,
    Rgb = 3,
}

impl ColorSubsampling {
    pub fn from_u8(v: u8) -> io::Result<Self> {
        match v {
            0 => Ok(ColorSubsampling::Yuv420),
            1 => Ok(ColorSubsampling::Yuv422),
            2 => Ok(ColorSubsampling::Yuv444),
            3 => Ok(ColorSubsampling::Rgb),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unknown subsampling: {}", v))),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WpgHeader {
    pub width: u32,
    pub height: u32,
    pub mode: WpgMode,
    pub subsampling: ColorSubsampling,
    pub flags: u16,
    pub quality: u8,
    pub alpha_mode: u8, // 0 = None, 1 = RLE Lossless, 2 = Compressed Plane
}

impl WpgHeader {
    pub fn new(width: u32, height: u32, mode: WpgMode, subsampling: ColorSubsampling, quality: u8) -> Self {
        Self {
            width,
            height,
            mode,
            subsampling,
            flags: 0,
            quality,
            alpha_mode: 0,
        }
    }

    pub fn has_alpha(&self) -> bool {
        (self.flags & FLAG_ALPHA) != 0
    }

    pub fn has_micro_preview(&self) -> bool {
        (self.flags & FLAG_MICRO_PREVIEW) != 0
    }

    pub fn has_film_grain(&self) -> bool {
        (self.flags & FLAG_FILM_GRAIN) != 0
    }

    pub fn is_secondary_entropy(&self) -> bool {
        (self.flags & FLAG_SECONDARY_ENTROPY) != 0
    }

    pub fn is_lossless(&self) -> bool {
        (self.flags & FLAG_LOSSLESS) != 0
    }

    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(WPG_MAGIC)?;
        writer.write_u24::<BigEndian>(self.width)?;
        writer.write_u24::<BigEndian>(self.height)?;
        writer.write_u8(self.mode as u8)?;
        writer.write_u8(self.subsampling as u8)?;
        writer.write_u16::<BigEndian>(self.flags)?;
        writer.write_u8(self.quality)?;
        writer.write_u8(self.alpha_mode)?;
        Ok(())
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != WPG_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid WPG magic: {:?}, expected {:?}", magic, WPG_MAGIC),
            ));
        }

        let width = reader.read_u24::<BigEndian>()?;
        let height = reader.read_u24::<BigEndian>()?;
        let mode_raw = reader.read_u8()?;
        let subsampling_raw = reader.read_u8()?;
        let flags = reader.read_u16::<BigEndian>()?;
        let quality = reader.read_u8()?;
        let alpha_mode = reader.read_u8()?;

        let mode = WpgMode::from_u8(mode_raw)?;
        let subsampling = ColorSubsampling::from_u8(subsampling_raw)?;

        Ok(Self {
            width,
            height,
            mode,
            subsampling,
            flags,
            quality,
            alpha_mode,
        })
    }
}
