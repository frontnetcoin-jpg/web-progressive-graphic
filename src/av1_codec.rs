use crate::color::YuvPlanes;
use crate::header::ColorSubsampling;
use std::io;
use std::ptr::NonNull;

/// Map perceptual quality 1..100 to AV1 quantizer 0..255 using the calibrated AVIF curve
pub fn quality_to_quantizer(quality: u8) -> usize {
    if quality >= 100 {
        return 1;
    }
    let q = (quality as f32 / 100.0).clamp(0.01, 0.999);
    let x = if q >= 0.82 {
        (1.0 - q) * 2.6
    } else if q > 0.25 {
        (1.0 - 0.125) - 0.5 * q
    } else {
        1.0 - q
    };
    (x * 255.0).round().clamp(1.0, 255.0) as usize
}

pub struct Av1Codec;

impl Av1Codec {
    /// Encode YUV planes to raw AV1 OBU bitstream using rav1e with still-image & multi-tile optimizations
    pub fn encode_intra(
        planes: &YuvPlanes,
        quality: u8,
        speed: u8,
        tune_ssim: bool,
    ) -> io::Result<Vec<u8>> {
        use rav1e::config::SpeedSettings;
        use rav1e::prelude::*;

        let chroma_sampling = match planes.subsampling {
            ColorSubsampling::Yuv420 => ChromaSampling::Cs420,
            ColorSubsampling::Yuv422 => ChromaSampling::Cs422,
            ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => ChromaSampling::Cs444,
        };

        let clamped_speed = speed.clamp(1, 10);
        let q = quality_to_quantizer(quality);
        let low_quality = q < quality_to_quantizer(55);
        let high_quality = q > quality_to_quantizer(80);

        // Configure SpeedSettings specifically for single-frame still images (disables video lookahead & motion search overhead)
        let mut speed_settings = SpeedSettings::from_preset(clamped_speed);
        speed_settings.multiref = false;
        speed_settings.rdo_lookahead_frames = 1;
        speed_settings.scene_detection_mode = SceneDetectionSpeed::None;
        speed_settings.motion.include_near_mvs = false;
        speed_settings.motion.use_satd_subpel = false;
        speed_settings.partition.encode_bottomup = clamped_speed <= 2;
        speed_settings.sgr_complexity = if clamped_speed <= 2 {
            SGRComplexityLevel::Full
        } else {
            SGRComplexityLevel::Reduced
        };
        speed_settings.transform.rdo_tx_decision = clamped_speed <= 4 && !high_quality;
        speed_settings.transform.reduced_tx_set = clamped_speed == 4 || clamped_speed >= 9;
        speed_settings.prediction.fine_directional_intra = clamped_speed <= 6;
        speed_settings.fast_deblock = clamped_speed >= 7 && !high_quality;
        speed_settings.lrf = low_quality && clamped_speed <= 8;
        speed_settings.cdef = low_quality && clamped_speed <= 9;
        speed_settings.prediction.prediction_modes = if clamped_speed <= 1 {
            PredictionModesSetting::ComplexAll
        } else {
            PredictionModesSetting::Simple
        };

        let (min_b, max_b) = match clamped_speed {
            1..=4 => (BlockSize::BLOCK_4X4, BlockSize::BLOCK_16X16),
            5..=8 => (BlockSize::BLOCK_8X8, BlockSize::BLOCK_16X16),
            _ => (BlockSize::BLOCK_16X16, BlockSize::BLOCK_16X16),
        };
        speed_settings.partition.partition_range = PartitionRange::new(min_b, max_b);

        let hw_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let min_tile_dim: usize = match clamped_speed {
            1 => 2048,
            2 => 1024,
            3 => 512,
            4 => 256,
            _ => 128,
        } * if high_quality { 2 } else { 1 };
        let tiles = hw_threads
            .min((planes.y_width * planes.y_height) / (min_tile_dim * min_tile_dim).max(1))
            .max(1);

        let mut enc_cfg = EncoderConfig::default();
        enc_cfg.width = planes.y_width;
        enc_cfg.height = planes.y_height;
        enc_cfg.speed_settings = speed_settings;
        enc_cfg.tune = if tune_ssim { Tune::Psnr } else { Tune::Psychovisual };
        enc_cfg.chroma_sampling = chroma_sampling;
        enc_cfg.pixel_range = PixelRange::Full;
        enc_cfg.color_description = Some(ColorDescription {
            transfer_characteristics: TransferCharacteristics::SRGB,
            color_primaries: ColorPrimaries::BT709,
            matrix_coefficients: MatrixCoefficients::BT709,
        });
        enc_cfg.enable_timing_info = false;
        enc_cfg.still_picture = true;
        enc_cfg.tiles = tiles;
        enc_cfg.quantizer = q;
        enc_cfg.min_quantizer = q as u8;

        let cfg = Config::new()
            .with_encoder_config(enc_cfg)
            .with_threads(hw_threads);

        let mut ctx: Context<u8> = cfg.new_context().map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Failed to create rav1e context: {:?}", e))
        })?;

        let mut frame = ctx.new_frame();

        // Copy Y plane
        frame.planes[0].copy_from_raw_u8(&planes.y, planes.y_stride, 1);
        // Copy U plane
        frame.planes[1].copy_from_raw_u8(&planes.u, planes.u_stride, 1);
        // Copy V plane
        frame.planes[2].copy_from_raw_u8(&planes.v, planes.v_stride, 1);

        ctx.send_frame(frame).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Failed to send frame to rav1e: {:?}", e))
        })?;
        ctx.flush();

        let mut obu_data = Vec::new();
        loop {
            match ctx.receive_packet() {
                Ok(packet) => {
                    obu_data.extend_from_slice(&packet.data);
                }
                Err(EncoderStatus::Encoded) => break,
                Err(EncoderStatus::NeedMoreData) => break,
                Err(EncoderStatus::LimitReached) => break,
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("rav1e receive_packet error: {:?}", e),
                    ));
                }
            }
        }

        if obu_data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "rav1e failed to produce any OBU data",
            ));
        }

        Ok(obu_data)
    }

    /// Decode raw AV1 OBU bitstream back to YuvPlanes using rav1d
    pub fn decode_intra(
        obu_data: &[u8],
        width: usize,
        height: usize,
        subsampling: ColorSubsampling,
    ) -> io::Result<YuvPlanes> {
        use rav1d::include::dav1d::data::Dav1dData;
        use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
        use rav1d::include::dav1d::picture::Dav1dPicture;
        use rav1d::src::lib::*;

        let mut settings = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
        unsafe {
            dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap());
        }
        let mut settings = unsafe { settings.assume_init() };
        settings.n_threads = 2;
        settings.max_frame_delay = 1;

        let mut ctx_opt: Option<Dav1dContext> = None;
        let res = unsafe {
            dav1d_open(
                NonNull::new(&mut ctx_opt),
                NonNull::new(&mut settings),
            )
        };
        if res.0 != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to open dav1d decoder"));
        }

        let ctx = ctx_opt.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Decoder context is null"))?;

        // Allocate and fill Dav1dData
        let mut data = std::mem::MaybeUninit::<Dav1dData>::zeroed();
        let ptr = unsafe {
            dav1d_data_create(
                NonNull::new(data.as_mut_ptr()),
                obu_data.len(),
            )
        };
        if ptr.is_null() {
            unsafe { dav1d_close(NonNull::new(&mut ctx_opt)) };
            return Err(io::Error::new(io::ErrorKind::Other, "dav1d_data_create failed"));
        }
        unsafe {
            std::slice::from_raw_parts_mut(ptr, obu_data.len()).copy_from_slice(obu_data);
        }
        let mut data = unsafe { data.assume_init() };

        let send_res = unsafe {
            dav1d_send_data(Some(ctx), NonNull::new(&mut data))
        };
        if send_res.0 != 0 {
            unsafe {
                dav1d_data_unref(NonNull::new(&mut data));
                dav1d_close(NonNull::new(&mut ctx_opt));
            }
            return Err(io::Error::new(io::ErrorKind::Other, "dav1d_send_data failed"));
        }

        let mut pic = std::mem::MaybeUninit::<Dav1dPicture>::zeroed();
        let mut got_picture = false;
        for _ in 0..10 {
            let get_res = unsafe {
                dav1d_get_picture(Some(ctx), NonNull::new(pic.as_mut_ptr()))
            };
            if get_res.0 == 0 {
                got_picture = true;
                break;
            }
        }

        if !got_picture {
            unsafe {
                dav1d_close(NonNull::new(&mut ctx_opt));
            }
            return Err(io::Error::new(io::ErrorKind::Other, "dav1d_get_picture failed"));
        }
        let mut pic = unsafe { pic.assume_init() };

        let (uv_width, uv_height) = match subsampling {
            ColorSubsampling::Yuv420 => ((width + 1) / 2, (height + 1) / 2),
            ColorSubsampling::Yuv422 => ((width + 1) / 2, height),
            ColorSubsampling::Yuv444 | ColorSubsampling::Rgb => (width, height),
        };

        let mut y_plane = vec![0u8; width * height];
        let mut u_plane = vec![0u8; uv_width * uv_height];
        let mut v_plane = vec![0u8; uv_width * uv_height];

        unsafe {
            if let Some(y_data) = pic.data[0] {
                let y_ptr = y_data.as_ptr() as *const u8;
                let stride = pic.stride[0].unsigned_abs();
                for y in 0..height {
                    let src = std::slice::from_raw_parts(y_ptr.add(y * stride), width);
                    y_plane[y * width..(y + 1) * width].copy_from_slice(src);
                }
            }

            if let Some(u_data) = pic.data[1] {
                let u_ptr = u_data.as_ptr() as *const u8;
                let stride = pic.stride[1].unsigned_abs();
                for y in 0..uv_height {
                    let src = std::slice::from_raw_parts(u_ptr.add(y * stride), uv_width);
                    u_plane[y * uv_width..(y + 1) * uv_width].copy_from_slice(src);
                }
            }

            if let Some(v_data) = pic.data[2] {
                let v_ptr = v_data.as_ptr() as *const u8;
                let stride = pic.stride[1].unsigned_abs();
                for y in 0..uv_height {
                    let src = std::slice::from_raw_parts(v_ptr.add(y * stride), uv_width);
                    v_plane[y * uv_width..(y + 1) * uv_width].copy_from_slice(src);
                }
            }

            dav1d_picture_unref(NonNull::new(&mut pic));
            dav1d_close(NonNull::new(&mut ctx_opt));
        }

        Ok(YuvPlanes {
            y: y_plane,
            u: u_plane,
            v: v_plane,
            y_stride: width,
            u_stride: uv_width,
            v_stride: uv_width,
            y_width: width,
            y_height: height,
            uv_width,
            uv_height,
            subsampling,
        })
    }
}

/// Decode raw AVIF image bytes to RGBA pixels using pure Rust rav1d
pub fn decode_avif_to_rgba(
    avif_bytes: &[u8],
    width: usize,
    height: usize,
    alpha_byte_size: usize,
    color_byte_size: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mdat_pos = avif_bytes.windows(4).position(|w| w == b"mdat")
        .ok_or("mdat box not found in AVIF")?;
    let color_start = mdat_pos + 4 + alpha_byte_size;
    let color_end = if color_byte_size > 0 {
        color_start + color_byte_size
    } else {
        avif_bytes.len()
    };
    let obu_data = &avif_bytes[color_start..color_end.min(avif_bytes.len())];
    let planes = Av1Codec::decode_intra(obu_data, width, height, ColorSubsampling::Yuv444)?;
    let rgba = crate::color::yuv_to_rgb_bt601(&planes, None);
    Ok(rgba)
}
