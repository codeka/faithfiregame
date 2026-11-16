use std::io::Cursor;

use glow::HasContext;
use shared::world::MAX_TERRAIN_MATERIALS;

pub struct TerrainLayerImage {
    pub layer: u8,
    pub albedo_width: u32,
    pub albedo_height: u32,
    pub albedo_rgb: Vec<[u8; 3]>,
    pub height_map: Option<(u32, u32, Vec<u8>)>,
}

fn fallback_layer_rgb(layer: usize) -> [u8; 3] {
    match layer {
        0 => [61, 133, 46],   // Grass
        1 => [140, 104, 71],  // Dirt
        2 => [122, 125, 133], // Rock
        3 => [196, 179, 136], // Sand
        4 => [99, 173, 61],   // Light Grass
        5 => [36, 92, 41],    // Dark Grass
        6 => [92, 118, 62],   // Patchy Grass
        7 => [115, 115, 118], // Cobblestone
        8 => [138, 72, 56],   // Brick
        _ => [128, 128, 128],
    }
}

fn decode_png_frame(bytes: &[u8]) -> Result<(u32, u32, png::ColorType, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG header decode failed: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG output buffer size overflow".to_owned())?;
    let mut buf = vec![0u8; buf_size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame decode failed: {e}"))?;
    if frame.width == 0 || frame.height == 0 {
        return Err("PNG has zero dimensions".to_owned());
    }
    buf.truncate(frame.buffer_size());
    Ok((frame.width, frame.height, frame.color_type, buf))
}

pub fn decode_albedo_png(bytes: &[u8]) -> Result<(u32, u32, Vec<[u8; 3]>), String> {
    let (width, height, color_type, data) = decode_png_frame(bytes)?;
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "PNG dimensions overflow".to_owned())?;
    let mut out = Vec::with_capacity(pixel_count);

    match color_type {
        png::ColorType::Rgb => {
            for px in data.chunks_exact(3) {
                out.push([px[0], px[1], px[2]]);
            }
        }
        png::ColorType::Rgba => {
            for px in data.chunks_exact(4) {
                out.push([px[0], px[1], px[2]]);
            }
        }
        png::ColorType::Grayscale => {
            for &v in &data {
                out.push([v, v, v]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in data.chunks_exact(2) {
                out.push([px[0], px[0], px[0]]);
            }
        }
        other => {
            return Err(format!("Unsupported albedo PNG color type: {other:?}"));
        }
    }

    if out.len() != pixel_count {
        return Err("Decoded albedo PNG byte count mismatch".to_owned());
    }
    Ok((width, height, out))
}

pub fn decode_height_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let (width, height, color_type, data) = decode_png_frame(bytes)?;
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "PNG dimensions overflow".to_owned())?;
    let mut out = Vec::with_capacity(pixel_count);

    match color_type {
        png::ColorType::Grayscale => out.extend_from_slice(&data),
        png::ColorType::GrayscaleAlpha => {
            for px in data.chunks_exact(2) {
                out.push(px[0]);
            }
        }
        png::ColorType::Rgb => {
            for px in data.chunks_exact(3) {
                let lum = ((77 * px[0] as u32 + 150 * px[1] as u32 + 29 * px[2] as u32) >> 8) as u8;
                out.push(lum);
            }
        }
        png::ColorType::Rgba => {
            for px in data.chunks_exact(4) {
                let lum = ((77 * px[0] as u32 + 150 * px[1] as u32 + 29 * px[2] as u32) >> 8) as u8;
                out.push(lum);
            }
        }
        other => {
            return Err(format!("Unsupported height PNG color type: {other:?}"));
        }
    }

    if out.len() != pixel_count {
        return Err("Decoded height PNG byte count mismatch".to_owned());
    }
    Ok((width, height, out))
}

fn sample_rgb_bilinear_wrap(
    pixels: &[[u8; 3]],
    w: usize,
    h: usize,
    u: f32,
    v: f32,
) -> [u8; 3] {
    let sx = u * w as f32;
    let sy = v * h as f32;
    let x0 = (sx.floor() as isize).rem_euclid(w as isize) as usize;
    let y0 = (sy.floor() as isize).rem_euclid(h as isize) as usize;
    let x1 = (x0 + 1) % w;
    let y1 = (y0 + 1) % h;
    let fx = sx - sx.floor();
    let fy = sy - sy.floor();

    let p00 = pixels[y0 * w + x0];
    let p10 = pixels[y0 * w + x1];
    let p01 = pixels[y1 * w + x0];
    let p11 = pixels[y1 * w + x1];

    let mut out = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] as f32 + (p10[c] as f32 - p00[c] as f32) * fx;
        let bot = p01[c] as f32 + (p11[c] as f32 - p01[c] as f32) * fx;
        let val = top + (bot - top) * fy;
        out[c] = val.round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn sample_gray_bilinear_wrap(pixels: &[u8], w: usize, h: usize, u: f32, v: f32) -> u8 {
    let sx = u * w as f32;
    let sy = v * h as f32;
    let x0 = (sx.floor() as isize).rem_euclid(w as isize) as usize;
    let y0 = (sy.floor() as isize).rem_euclid(h as isize) as usize;
    let x1 = (x0 + 1) % w;
    let y1 = (y0 + 1) % h;
    let fx = sx - sx.floor();
    let fy = sy - sy.floor();

    let p00 = pixels[y0 * w + x0] as f32;
    let p10 = pixels[y0 * w + x1] as f32;
    let p01 = pixels[y1 * w + x0] as f32;
    let p11 = pixels[y1 * w + x1] as f32;
    let top = p00 + (p10 - p00) * fx;
    let bot = p01 + (p11 - p01) * fx;
    (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8
}

pub fn pack_terrain_layer_rgba(img: &TerrainLayerImage, target_size: usize) -> Vec<u8> {
    let pixel_count = target_size * target_size;
    let mut out = vec![0u8; pixel_count * 4];

    let aw = img.albedo_width as usize;
    let ah = img.albedo_height as usize;
    let albedo_exact = aw == target_size && ah == target_size && img.albedo_rgb.len() == pixel_count;

    let height_exact = img
        .height_map
        .as_ref()
        .is_some_and(|(hw, hh, data)| {
            *hw as usize == target_size && *hh as usize == target_size && data.len() == pixel_count
        });

    for y in 0..target_size {
        let v = y as f32 / target_size as f32;
        for x in 0..target_size {
            let p = y * target_size + x;
            let u = x as f32 / target_size as f32;

            let rgb = if albedo_exact {
                img.albedo_rgb[p]
            } else if aw > 0 && ah > 0 && !img.albedo_rgb.is_empty() {
                sample_rgb_bilinear_wrap(&img.albedo_rgb, aw, ah, u, v)
            } else {
                fallback_layer_rgb(img.layer as usize)
            };

            let h = if height_exact {
                img.height_map.as_ref().unwrap().2[p]
            } else if let Some((hw, hh, data)) = &img.height_map {
                let hw = *hw as usize;
                let hh = *hh as usize;
                if hw > 0 && hh > 0 && !data.is_empty() {
                    sample_gray_bilinear_wrap(data, hw, hh, u, v)
                } else {
                    128
                }
            } else {
                128
            };

            let idx = p * 4;
            out[idx] = rgb[0];
            out[idx + 1] = rgb[1];
            out[idx + 2] = rgb[2];
            out[idx + 3] = h;
        }
    }

    out
}

unsafe fn upload_terrain_texture_array(
    gl: &glow::Context,
    size: usize,
    layers: usize,
    pixels: &[u8],
) -> Result<glow::Texture, String> {
    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        // NOTE: glow 0.16.0's web_sys `tex_image_3d` swaps `border` and `depth`.
        // Use immutable `tex_storage_3d` + `tex_sub_image_3d`, whose parameters are ordered properly.
        let mip_levels = (size.ilog2() + 1) as i32;
        gl.tex_storage_3d(
            glow::TEXTURE_2D_ARRAY,
            mip_levels,
            glow::RGBA8,
            size as i32,
            size as i32,
            layers as i32,
        );
        gl.tex_sub_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            0,
            0,
            0,
            size as i32,
            size as i32,
            layers as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(pixels)),
        );
        gl.generate_mipmap(glow::TEXTURE_2D_ARRAY);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        Ok(tex)
    }
}

pub(super) fn create_placeholder_terrain_texture_array(
    gl: &glow::Context,
) -> Result<glow::Texture, String> {
    let mut pixels = Vec::with_capacity(MAX_TERRAIN_MATERIALS * 4);
    for layer in 0..MAX_TERRAIN_MATERIALS {
        let rgb = fallback_layer_rgb(layer);
        pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 128]);
    }
    unsafe { upload_terrain_texture_array(gl, 1, MAX_TERRAIN_MATERIALS, &pixels) }
}

pub(super) fn create_terrain_texture_array_from_images(
    gl: &glow::Context,
    images: &[TerrainLayerImage],
) -> Result<glow::Texture, String> {
    let max_dim = images
        .iter()
        .flat_map(|img| {
            [
                img.albedo_width,
                img.albedo_height,
                img.height_map.as_ref().map_or(0, |h| h.0),
                img.height_map.as_ref().map_or(0, |h| h.1),
            ]
        })
        .max()
        .unwrap_or(256) as usize;
    let size = max_dim.max(256).next_power_of_two().clamp(256, 1024);

    let layer_bytes = size * size * 4;
    let mut pixels = vec![0u8; MAX_TERRAIN_MATERIALS * layer_bytes];

    for layer in 0..MAX_TERRAIN_MATERIALS {
        let rgb = fallback_layer_rgb(layer);
        let base = layer * layer_bytes;
        for px in pixels[base..base + layer_bytes].chunks_exact_mut(4) {
            px[0] = rgb[0];
            px[1] = rgb[1];
            px[2] = rgb[2];
            px[3] = 128;
        }
    }

    for img in images {
        let layer = img.layer as usize;
        if layer >= MAX_TERRAIN_MATERIALS {
            continue;
        }
        let packed = pack_terrain_layer_rgba(img, size);
        let base = layer * layer_bytes;
        pixels[base..base + layer_bytes].copy_from_slice(&packed);
    }

    unsafe { upload_terrain_texture_array(gl, size, MAX_TERRAIN_MATERIALS, &pixels) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_test_png(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn decodes_rgb_rgba_and_grayscale_pngs() {
        let rgb_bytes = encode_test_png(
            2,
            2,
            png::ColorType::Rgb,
            &[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        );
        let (w, h, rgb) = decode_albedo_png(&rgb_bytes).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgb[0], [10, 20, 30]);
        assert_eq!(rgb[3], [100, 110, 120]);

        let rgba_bytes = encode_test_png(
            2,
            1,
            png::ColorType::Rgba,
            &[15, 25, 35, 0, 45, 55, 65, 255],
        );
        let (w, h, rgba_rgb) = decode_albedo_png(&rgba_bytes).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba_rgb, vec![[15, 25, 35], [45, 55, 65]]);

        let gray_bytes = encode_test_png(2, 2, png::ColorType::Grayscale, &[12, 64, 192, 250]);
        let (hw, hh, hmap) = decode_height_png(&gray_bytes).unwrap();
        assert_eq!((hw, hh), (2, 2));
        assert_eq!(hmap, vec![12, 64, 192, 250]);

        // RGB heightmaps also decode cleanly via luminance.
        let (_, _, lum) = decode_height_png(&rgb_bytes).unwrap();
        assert_eq!(lum.len(), 4);
        assert!(lum[3] > lum[0]);
    }

    #[test]
    fn packs_missing_heightmap_as_128_and_resamples_mismatched_resolutions_seamlessly() {
        let no_height = TerrainLayerImage {
            layer: 0,
            albedo_width: 2,
            albedo_height: 2,
            albedo_rgb: vec![[10, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]],
            height_map: None,
        };
        let packed = pack_terrain_layer_rgba(&no_height, 4);
        assert_eq!(packed.len(), 4 * 4 * 4);
        for px in packed.chunks_exact(4) {
            assert_eq!(px[3], 128);
        }
        assert_eq!(&packed[0..4], &[10, 20, 30, 128]);
        // Center sample at (2, 2) corresponds to u=0.5, v=0.5 -> pixel (1, 1) of 2x2
        let mid_idx = (2 * 4 + 2) * 4;
        assert_eq!(&packed[mid_idx..mid_idx + 4], &[100, 110, 120, 128]);

        // Resampling a mismatched heightmap preserves seamless edge continuity.
        let with_height = TerrainLayerImage {
            layer: 1,
            albedo_width: 4,
            albedo_height: 4,
            albedo_rgb: vec![[200, 150, 100]; 16],
            height_map: Some((2, 2, vec![40, 200, 200, 40])),
        };
        let packed_h = pack_terrain_layer_rgba(&with_height, 8);
        assert_eq!(packed_h.len(), 8 * 8 * 4);
        assert_eq!(packed_h[3], 40);
        // Sample at x=4, y=0 (u=0.5, v=0) hits (1, 0) -> 200
        assert_eq!(packed_h[4 * 4 + 3], 200);
        // Sample at x=6, y=0 (u=0.75) hits midpoint between 200 and wrapped 40 -> 120;
        // x=7, y=0 (u=0.875) sits 75% toward wrapped 40 -> 80.
        assert_eq!(packed_h[6 * 4 + 3], 120);
        assert_eq!(packed_h[7 * 4 + 3], 80);
    }

    #[test]
    fn seeded_disk_terrain_pngs_decode_with_rich_variation_when_present() {
        let terrain_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
            .join("assets")
            .join("terrain");
        if !terrain_dir.exists() {
            return;
        }
        for id in [
            "grass",
            "light_grass",
            "dark_grass",
            "patchy_grass",
            "dirt",
            "sand",
            "rock",
            "cobblestone",
            "brick",
        ] {
            let albedo_bytes = std::fs::read(terrain_dir.join(format!("{id}.png"))).unwrap();
            let height_bytes =
                std::fs::read(terrain_dir.join(format!("{id}_height.png"))).unwrap();
            let (aw, ah, albedo_rgb) = decode_albedo_png(&albedo_bytes).unwrap();
            let (hw, hh, height_vec) = decode_height_png(&height_bytes).unwrap();
            assert_eq!((aw, ah), (256, 256));
            assert_eq!((hw, hh), (256, 256));

            let packed = pack_terrain_layer_rgba(
                &TerrainLayerImage {
                    layer: 0,
                    albedo_width: aw,
                    albedo_height: ah,
                    albedo_rgb,
                    height_map: Some((hw, hh, height_vec)),
                },
                256,
            );
            let mut min_r = u8::MAX;
            let mut max_r = u8::MIN;
            let mut min_h = u8::MAX;
            let mut max_h = u8::MIN;
            for px in packed.chunks_exact(4) {
                min_r = min_r.min(px[0]);
                max_r = max_r.max(px[0]);
                min_h = min_h.min(px[3]);
                max_h = max_h.max(px[3]);
            }
            assert!(
                max_r - min_r >= 12,
                "{id}.png decoded to near-solid albedo: min_r={min_r}, max_r={max_r}"
            );
            assert!(
                max_h - min_h >= 40,
                "{id}_height.png decoded to near-flat height: min_h={min_h}, max_h={max_h}"
            );
        }
    }
}
