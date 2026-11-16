use std::io::Cursor;

use glow::HasContext;
use shared::world::{MAX_ROAD_MATERIALS, ROAD_MAT_DIRT};

pub struct RoadLayerImage {
    pub layer: u8,
    pub is_soft_edge: bool,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<[u8; 4]>,
}

fn fallback_road_rgba(layer: usize) -> [u8; 4] {
    match layer {
        0 => [139, 102, 66, 255],  // Dirt Path
        1 => [118, 121, 128, 255], // Cobblestone
        2 => [161, 77, 58, 255],   // Brick
        3 => [55, 58, 64, 255],    // Asphalt
        4 => [127, 132, 124, 255], // Flagstone
        _ => [120, 120, 124, 255],
    }
}

pub fn decode_road_rgba_png(bytes: &[u8]) -> Result<(u32, u32, Vec<[u8; 4]>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("Road PNG header decode failed: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "Road PNG output buffer size overflow".to_owned())?;
    let mut buf = vec![0u8; buf_size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("Road PNG frame decode failed: {e}"))?;
    if frame.width == 0 || frame.height == 0 {
        return Err("Road PNG has zero dimensions".to_owned());
    }
    let data = &buf[..frame.buffer_size()];
    let pixel_count = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .ok_or_else(|| "Road PNG dimensions overflow".to_owned())?;
    let mut out = Vec::with_capacity(pixel_count);

    match frame.color_type {
        png::ColorType::Rgba => {
            for px in data.chunks_exact(4) {
                out.push([px[0], px[1], px[2], px[3]]);
            }
        }
        png::ColorType::Rgb => {
            for px in data.chunks_exact(3) {
                out.push([px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &v in data {
                out.push([v, v, v, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in data.chunks_exact(2) {
                out.push([px[0], px[0], px[0], px[1]]);
            }
        }
        other => {
            return Err(format!("Unsupported road PNG color type: {other:?}"));
        }
    }

    if out.len() != pixel_count {
        return Err("Decoded road PNG byte count mismatch".to_owned());
    }
    Ok((frame.width, frame.height, out))
}

/// Resamples a road texture with `CLAMP_TO_EDGE` in U (across road width) and `REPEAT` in V (along road length).
fn sample_rgba_clamp_u_wrap_v(
    pixels: &[[u8; 4]],
    w: usize,
    h: usize,
    u: f32,
    v: f32,
) -> [u8; 4] {
    let sx = (u.clamp(0.0, 1.0) * (w.saturating_sub(1)) as f32).clamp(0.0, w.saturating_sub(1) as f32);
    let sy = v * h as f32;

    let x0 = (sx.floor() as usize).min(w - 1);
    let x1 = (x0 + 1).min(w - 1);
    let fx = sx - sx.floor();

    let y0 = (sy.floor() as isize).rem_euclid(h as isize) as usize;
    let y1 = (y0 + 1) % h;
    let fy = sy - sy.floor();

    let p00 = pixels[y0 * w + x0];
    let p10 = pixels[y0 * w + x1];
    let p01 = pixels[y1 * w + x0];
    let p11 = pixels[y1 * w + x1];

    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f32 + (p10[c] as f32 - p00[c] as f32) * fx;
        let bot = p01[c] as f32 + (p11[c] as f32 - p01[c] as f32) * fx;
        let val = top + (bot - top) * fy;
        out[c] = val.round().clamp(0.0, 255.0) as u8;
    }
    out
}

pub fn pack_road_layer_rgba(img: &RoadLayerImage, target_size: usize) -> Vec<u8> {
    let pixel_count = target_size * target_size;
    let mut out = vec![0u8; pixel_count * 4];
    let w = img.width as usize;
    let h = img.height as usize;

    if w == target_size && h == target_size && img.rgba.len() == pixel_count {
        for (i, px) in img.rgba.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(px);
        }
        return out;
    }

    let u_denom = target_size.saturating_sub(1).max(1) as f32;
    for y in 0..target_size {
        let v = y as f32 / target_size as f32;
        for x in 0..target_size {
            let u = if target_size > 1 {
                x as f32 / u_denom
            } else {
                0.5
            };
            let rgba = if w > 0 && h > 0 && !img.rgba.is_empty() {
                sample_rgba_clamp_u_wrap_v(&img.rgba, w, h, u, v)
            } else {
                let mut fb = fallback_road_rgba(img.layer as usize);
                if img.layer == ROAD_MAT_DIRT && (x == 0 || x + 1 == target_size) && target_size > 2 {
                    fb[3] = 0;
                }
                fb
            };
            let idx = (y * target_size + x) * 4;
            out[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
    out
}

unsafe fn upload_road_texture_array(
    gl: &glow::Context,
    size: usize,
    layers: usize,
    pixels: &[u8],
) -> Result<glow::Texture, String> {
    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
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
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        Ok(tex)
    }
}

pub(super) fn create_placeholder_road_texture_array(
    gl: &glow::Context,
) -> Result<glow::Texture, String> {
    let mut pixels = Vec::with_capacity(MAX_ROAD_MATERIALS * 4);
    for layer in 0..MAX_ROAD_MATERIALS {
        pixels.extend_from_slice(&fallback_road_rgba(layer));
    }
    unsafe { upload_road_texture_array(gl, 1, MAX_ROAD_MATERIALS, &pixels) }
}

pub(super) fn create_road_texture_array_from_images(
    gl: &glow::Context,
    images: &[RoadLayerImage],
) -> Result<glow::Texture, String> {
    let max_dim = images
        .iter()
        .flat_map(|img| [img.width, img.height])
        .max()
        .unwrap_or(256) as usize;
    let size = max_dim.max(256).next_power_of_two().clamp(256, 1024);

    let layer_bytes = size * size * 4;
    let mut pixels = vec![0u8; MAX_ROAD_MATERIALS * layer_bytes];

    for layer in 0..MAX_ROAD_MATERIALS {
        let rgba = fallback_road_rgba(layer);
        let base = layer * layer_bytes;
        for px in pixels[base..base + layer_bytes].chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
    }

    for img in images {
        let layer = img.layer as usize;
        if layer >= MAX_ROAD_MATERIALS {
            continue;
        }
        let packed = pack_road_layer_rgba(img, size);
        let base = layer * layer_bytes;
        pixels[base..base + layer_bytes].copy_from_slice(&packed);
    }

    unsafe { upload_road_texture_array(gl, size, MAX_ROAD_MATERIALS, &pixels) }
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
    fn decodes_rgba_and_rgb_road_pngs_and_clamps_u_while_wrapping_v() {
        let rgba_bytes = encode_test_png(
            2,
            2,
            png::ColorType::Rgba,
            &[
                100, 80, 60, 0,   // left edge transparent
                120, 90, 70, 255, // right edge opaque
                100, 80, 60, 0,
                120, 90, 70, 255,
            ],
        );
        let (w, h, rgba) = decode_road_rgba_png(&rgba_bytes).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba[0][3], 0);
        assert_eq!(rgba[1][3], 255);

        let packed = pack_road_layer_rgba(
            &RoadLayerImage {
                layer: 0,
                is_soft_edge: true,
                width: w,
                height: h,
                rgba,
            },
            4,
        );
        // x=0 clamps to u=0 (alpha 0), x=3 clamps to u=1 (alpha 255) without wrapping across width!
        assert_eq!(packed[3], 0);
        assert_eq!(packed[3 * 4 + 3], 255);

        let rgb_bytes = encode_test_png(1, 1, png::ColorType::Rgb, &[50, 60, 70]);
        let (_, _, rgb_as_rgba) = decode_road_rgba_png(&rgb_bytes).unwrap();
        assert_eq!(rgb_as_rgba, vec![[50, 60, 70, 255]]);
    }

    #[test]
    fn seeded_disk_road_pngs_decode_and_preserve_dirt_alpha_when_present() {
        let road_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
            .join("assets")
            .join("roads");
        if !road_dir.exists() {
            return;
        }
        for (id, is_dirt) in [
            ("dirt_path", true),
            ("cobblestone", false),
            ("brick", false),
            ("asphalt", false),
            ("flagstone", false),
        ] {
            let bytes = std::fs::read(road_dir.join(format!("{id}.png"))).unwrap();
            let (w, h, rgba) = decode_road_rgba_png(&bytes).unwrap();
            assert_eq!((w, h), (256, 256));
            if is_dirt {
                assert_eq!(rgba[128 * 256][3], 0, "dirt left border must be transparent");
                assert_eq!(rgba[128 * 256 + 255][3], 0, "dirt right border must be transparent");
                assert!(rgba[128 * 256 + 128][3] > 240, "dirt center must be opaque");
            } else {
                assert!(rgba.iter().all(|px| px[3] == 255), "{id} must be opaque");
            }
        }
    }
}
