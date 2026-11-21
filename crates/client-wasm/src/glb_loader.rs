use std::collections::HashMap;
use std::io::Cursor;

use base64::Engine as _;
use glam::{Mat3, Mat4, Vec3};
use gltf::Gltf;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropTexWrap {
    Repeat,
    ClampToEdge,
    MirroredRepeat,
}

#[derive(Debug, Clone)]
pub struct ParsedPropTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub wrap_s: PropTexWrap,
    pub wrap_t: PropTexWrap,
    pub nearest_filter: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPropSubmesh {
    pub index_offset: u32,
    pub index_count: u32,
    pub texture_index: Option<usize>,
    pub emissive: [f32; 3],
    pub double_sided: bool,
    pub alpha_cutoff: f32,
}

pub struct ParsedPropMesh {
    pub vertices: Vec<PropVertex>,
    pub indices: Vec<u32>,
    pub textures: Vec<ParsedPropTexture>,
    pub submeshes: Vec<ParsedPropSubmesh>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

struct BuildState<'a> {
    blob: Option<&'a [u8]>,
    vertices: Vec<PropVertex>,
    indices: Vec<u32>,
    textures: Vec<ParsedPropTexture>,
    tex_map: HashMap<usize, usize>,
    submeshes: Vec<ParsedPropSubmesh>,
    aabb_min: Vec3,
    aabb_max: Vec3,
}

/// Parses a binary `.glb` asset into a 32-bit indexed mesh with embedded textures, UVs, and local AABB bounds.
pub fn parse_glb(bytes: &[u8]) -> Result<ParsedPropMesh, String> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| format!("gltf parse failed: {e}"))?;
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or_else(|| "GLB has no scenes".to_owned())?;

    let mut state = BuildState {
        blob: gltf.blob.as_deref(),
        vertices: Vec::new(),
        indices: Vec::new(),
        textures: Vec::new(),
        tex_map: HashMap::new(),
        submeshes: Vec::new(),
        aabb_min: Vec3::splat(f32::MAX),
        aabb_max: Vec3::splat(f32::MIN),
    };

    for node in scene.nodes() {
        visit_node(&node, Mat4::IDENTITY, &mut state);
    }

    if state.vertices.is_empty() {
        state.aabb_min = Vec3::splat(-0.5);
        state.aabb_max = Vec3::splat(0.5);
    }

    Ok(ParsedPropMesh {
        vertices: state.vertices,
        indices: state.indices,
        textures: state.textures,
        submeshes: state.submeshes,
        aabb_min: state.aabb_min,
        aabb_max: state.aabb_max,
    })
}

fn map_wrap(mode: gltf::texture::WrappingMode) -> PropTexWrap {
    match mode {
        gltf::texture::WrappingMode::ClampToEdge => PropTexWrap::ClampToEdge,
        gltf::texture::WrappingMode::MirroredRepeat => PropTexWrap::MirroredRepeat,
        gltf::texture::WrappingMode::Repeat => PropTexWrap::Repeat,
    }
}

fn resolve_texture(tex: &gltf::Texture<'_>, state: &mut BuildState<'_>) -> Option<usize> {
    load_gltf_texture(tex, state.blob, &mut state.tex_map, &mut state.textures)
}

/// Decodes and caches an embedded glTF texture (PNG/JPEG) into `textures`.
pub fn load_gltf_texture(
    tex: &gltf::Texture<'_>,
    blob: Option<&[u8]>,
    tex_map: &mut HashMap<usize, usize>,
    textures: &mut Vec<ParsedPropTexture>,
) -> Option<usize> {
    if let Some(&existing) = tex_map.get(&tex.index()) {
        return Some(existing);
    }

    let decoded_uri_bytes: Vec<u8>;
    let (raw_bytes, mime_hint): (&[u8], Option<&str>) = match tex.source().source() {
        gltf::image::Source::View { view, mime_type } => {
            let blob = blob?;
            let start = view.offset();
            let end = start.checked_add(view.length())?;
            if end > blob.len() {
                return None;
            }
            (&blob[start..end], Some(mime_type))
        }
        gltf::image::Source::Uri { uri, mime_type } => {
            let rest = uri.strip_prefix("data:")?;
            let (meta, payload) = rest.split_once(',')?;
            if !meta.ends_with(";base64") {
                return None;
            }
            decoded_uri_bytes = base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .ok()?;
            let inferred_mime = mime_type.or_else(|| meta.strip_suffix(";base64"));
            (decoded_uri_bytes.as_slice(), inferred_mime)
        }
    };

    let (width, height, rgba8) = match decode_embedded_rgba8(raw_bytes, mime_hint) {
        Ok(img) => img,
        Err(_err) => {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::warn_1(
                &format!("Skipping embedded GLB texture #{}: {_err}", tex.index()).into(),
            );
            return None;
        }
    };

    let sampler = tex.sampler();
    let nearest_filter = matches!(
        sampler.mag_filter(),
        Some(gltf::texture::MagFilter::Nearest)
    ) || matches!(
        sampler.min_filter(),
        Some(
            gltf::texture::MinFilter::Nearest
                | gltf::texture::MinFilter::NearestMipmapNearest
                | gltf::texture::MinFilter::NearestMipmapLinear
        )
    );

    let new_idx = textures.len();
    textures.push(ParsedPropTexture {
        width,
        height,
        rgba8,
        wrap_s: map_wrap(sampler.wrap_s()),
        wrap_t: map_wrap(sampler.wrap_t()),
        nearest_filter,
    });
    tex_map.insert(tex.index(), new_idx);
    Some(new_idx)
}

/// Computes smooth area-weighted vertex normals from triangle positions and indices.
pub fn compute_vertex_normals(positions: &[Vec3], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut accum = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if let (Some(&p0), Some(&p1), Some(&p2)) =
            (positions.get(i0), positions.get(i1), positions.get(i2))
        {
            let fnrm = (p1 - p0).cross(p2 - p0);
            accum[i0] += fnrm;
            accum[i1] += fnrm;
            accum[i2] += fnrm;
        }
    }
    accum
        .into_iter()
        .map(|n| {
            let nn = n.normalize_or_zero();
            if nn.length_squared() > 1e-6 {
                nn.to_array()
            } else {
                [0.0, 1.0, 0.0]
            }
        })
        .collect()
}

pub fn decode_embedded_rgba8(
    bytes: &[u8],
    mime_hint: Option<&str>,
) -> Result<(u32, u32, Vec<u8>), String> {
    let is_png = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || mime_hint.is_some_and(|m| m.eq_ignore_ascii_case("image/png"));
    let is_jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || mime_hint.is_some_and(|m| {
            m.eq_ignore_ascii_case("image/jpeg") || m.eq_ignore_ascii_case("image/jpg")
        });

    if is_png {
        decode_png_rgba8(bytes)
    } else if is_jpeg {
        decode_jpeg_rgba8(bytes)
    } else {
        Err(format!(
            "unsupported GLB image format (mime={:?}, len={})",
            mime_hint,
            bytes.len()
        ))
    }
}

fn decode_png_rgba8(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG header decode failed: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG buffer overflow".to_owned())?;
    let mut buf = vec![0u8; buf_size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame decode failed: {e}"))?;
    if frame.width == 0 || frame.height == 0 {
        return Err("PNG has zero dimensions".to_owned());
    }
    buf.truncate(frame.buffer_size());

    let px_count = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .ok_or_else(|| "PNG pixel overflow".to_owned())?;
    let mut rgba = Vec::with_capacity(px_count * 4);

    match frame.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(&buf),
        png::ColorType::Rgb => {
            for px in buf.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &v in &buf {
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in buf.chunks_exact(2) {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
        other => return Err(format!("unsupported PNG color type: {other:?}")),
    }

    if rgba.len() != px_count * 4 {
        return Err("decoded PNG RGBA byte length mismatch".to_owned());
    }
    Ok((frame.width, frame.height, rgba))
}

fn decode_jpeg_rgba8(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), opts);
    let rgba = decoder
        .decode()
        .map_err(|e| format!("JPEG decode failed: {e:?}"))?;
    let (w, h) = decoder
        .dimensions()
        .ok_or_else(|| "JPEG missing dimensions".to_owned())?;
    if w == 0 || h == 0 {
        return Err("JPEG has zero dimensions".to_owned());
    }
    let expected = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "JPEG pixel overflow".to_owned())?;
    if rgba.len() != expected {
        return Err("decoded JPEG RGBA byte length mismatch".to_owned());
    }
    Ok((w as u32, h as u32, rgba))
}

fn visit_node(node: &gltf::Node<'_>, parent_xform: Mat4, state: &mut BuildState<'_>) {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_xform * local;
    let normal_mat = Mat3::from_mat4(world).inverse().transpose();

    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let blob = state.blob;
            let reader = prim.reader(|buffer| {
                if buffer.index() == 0 {
                    blob
                } else {
                    None
                }
            });

            let Some(positions_iter) = reader.read_positions() else {
                continue;
            };
            let local_positions: Vec<[f32; 3]> = positions_iter.collect();
            if local_positions.is_empty() {
                continue;
            }

            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let base_color = pbr.base_color_factor();
            let emissive = mat.emissive_factor();
            let double_sided = mat.double_sided();
            let alpha_cutoff = match mat.alpha_mode() {
                gltf::material::AlphaMode::Opaque => 0.0,
                gltf::material::AlphaMode::Mask => mat.alpha_cutoff().unwrap_or(0.5),
                gltf::material::AlphaMode::Blend => 0.01,
            };

            let (texture_index, uv_set) = if let Some(info) = pbr.base_color_texture() {
                let tex_idx = resolve_texture(&info.texture(), state);
                (tex_idx, info.tex_coord())
            } else {
                (None, 0)
            };

            let normals_opt: Option<Vec<[f32; 3]>> =
                reader.read_normals().map(|iter| iter.collect());
            let colors: Vec<[f32; 4]> = reader
                .read_colors(0)
                .map(|c| c.into_rgba_f32().collect())
                .unwrap_or_default();
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(uv_set)
                .map(|tc| tc.into_f32().collect())
                .unwrap_or_default();

            let prim_indices: Vec<u32> = if let Some(idx_reader) = reader.read_indices() {
                idx_reader.into_u32().collect()
            } else {
                (0..local_positions.len() as u32).collect()
            };
            if prim_indices.len() < 3 {
                continue;
            }

            let base_vert = state.vertices.len() as u32;
            let mut world_positions = Vec::with_capacity(local_positions.len());
            for &pos in &local_positions {
                let wp = world.transform_point3(Vec3::from(pos));
                state.aabb_min = state.aabb_min.min(wp);
                state.aabb_max = state.aabb_max.max(wp);
                world_positions.push(wp);
            }

            let world_normals: Vec<[f32; 3]> = if let Some(raw_normals) = normals_opt {
                (0..local_positions.len())
                    .map(|i| {
                        let n_raw = raw_normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        let wn = (normal_mat * Vec3::from(n_raw)).normalize_or_zero();
                        if wn.length_squared() > 1e-6 {
                            wn.to_array()
                        } else {
                            [0.0, 1.0, 0.0]
                        }
                    })
                    .collect()
            } else {
                compute_vertex_normals(&world_positions, &prim_indices)
            };

            for (i, wp) in world_positions.into_iter().enumerate() {
                let vc = colors.get(i).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let col = [
                    vc[0] * base_color[0],
                    vc[1] * base_color[1],
                    vc[2] * base_color[2],
                    vc[3] * base_color[3],
                ];
                let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                state.vertices.push(PropVertex {
                    pos: wp.to_array(),
                    normal: world_normals[i],
                    color: col,
                    uv,
                });
            }

            let index_offset = state.indices.len() as u32;
            let tri_index_count = (prim_indices.len() / 3) * 3;
            for &idx in &prim_indices[..tri_index_count] {
                if (idx as usize) < local_positions.len() {
                    state.indices.push(base_vert + idx);
                }
            }
            let added_count = (state.indices.len() as u32).saturating_sub(index_offset);
            if added_count == 0 {
                continue;
            }

            if let Some(last) = state.submeshes.last_mut().filter(|s| {
                s.index_offset + s.index_count == index_offset
                    && s.texture_index == texture_index
                    && s.emissive == emissive
                    && s.double_sided == double_sided
                    && (s.alpha_cutoff - alpha_cutoff).abs() < 1e-5
            }) {
                last.index_count += added_count;
            } else {
                state.submeshes.push(ParsedPropSubmesh {
                    index_offset,
                    index_count: added_count,
                    texture_index,
                    emissive,
                    double_sided,
                    alpha_cutoff,
                });
            }
        }
    }

    for child in node.children() {
        visit_node(&child, world, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack_glb(gltf_json: serde_json::Value, mut bin: Vec<u8>) -> Vec<u8> {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let mut json_bytes = serde_json::to_vec(&gltf_json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
        out
    }

    fn make_2x2_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer
                .write_image_data(&[
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 128,
                ])
                .unwrap();
        }
        out
    }

    #[test]
    fn parses_textured_and_emissive_primitives_and_computes_missing_normals() {
        let png_bytes = make_2x2_png();
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let uvs: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let indices: [u16; 3] = [0, 1, 2];

        let mut bin = Vec::new();
        let pos_off = bin.len();
        for p in positions {
            for f in p {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let pos_len = bin.len() - pos_off;

        let uv_off = bin.len();
        for uv in uvs {
            for f in uv {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let uv_len = bin.len() - uv_off;

        let idx_off = bin.len();
        for idx in indices {
            bin.extend_from_slice(&idx.to_le_bytes());
        }
        let idx_len = bin.len() - idx_off;
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let img_off = bin.len();
        bin.extend_from_slice(&png_bytes);
        let img_len = png_bytes.len();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let doc = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "images": [{ "bufferView": 3, "mimeType": "image/png" }],
            "samplers": [{ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 10497 }],
            "textures": [{ "source": 0, "sampler": 0 }],
            "materials": [
                {
                    "pbrMetallicRoughness": {
                        "baseColorTexture": { "index": 0 }
                    },
                    "alphaMode": "MASK",
                    "alphaCutoff": 0.4,
                    "doubleSided": true
                },
                {
                    "pbrMetallicRoughness": {
                        "baseColorFactor": [0.2, 1.0, 0.4, 1.0]
                    },
                    "emissiveFactor": [0.1, 0.9, 0.3]
                }
            ],
            "meshes": [{
                "primitives": [
                    { "attributes": { "POSITION": 0, "TEXCOORD_0": 1 }, "indices": 2, "material": 0 },
                    { "attributes": { "POSITION": 0 }, "indices": 2, "material": 1 }
                ]
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_off, "byteLength": pos_len },
                { "buffer": 0, "byteOffset": uv_off, "byteLength": uv_len },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": idx_len },
                { "buffer": 0, "byteOffset": img_off, "byteLength": img_len }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2" },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ]
        });

        let glb = pack_glb(doc, bin);
        let parsed = parse_glb(&glb).unwrap();
        assert_eq!(parsed.textures.len(), 1);
        assert_eq!(parsed.textures[0].width, 2);
        assert_eq!(parsed.textures[0].height, 2);
        assert!(parsed.textures[0].nearest_filter);
        assert_eq!(parsed.textures[0].wrap_s, PropTexWrap::ClampToEdge);
        assert_eq!(parsed.textures[0].wrap_t, PropTexWrap::Repeat);
        assert_eq!(&parsed.textures[0].rgba8[0..4], &[255, 0, 0, 255]);

        assert_eq!(parsed.submeshes.len(), 2);
        assert_eq!(parsed.submeshes[0].texture_index, Some(0));
        assert!(parsed.submeshes[0].double_sided);
        assert!((parsed.submeshes[0].alpha_cutoff - 0.4).abs() < 1e-5);
        assert_eq!(parsed.submeshes[1].texture_index, None);
        assert_eq!(parsed.submeshes[1].emissive, [0.1, 0.9, 0.3]);

        // Auto-computed face normal for XY triangle [0,0,0],[1,0,0],[0,1,0] points along +Z
        assert!((parsed.vertices[0].normal[2] - 1.0).abs() < 1e-5);
        assert_eq!(parsed.vertices[1].uv, [1.0, 0.0]);
    }

    #[test]
    fn supports_u32_indices_past_65535_vertices() {
        let vert_count = 66_000usize;
        let mut bin = Vec::with_capacity(vert_count * 12 + 12);
        for i in 0..vert_count {
            let x = (i % 256) as f32 * 0.05;
            let y = (i / 256) as f32 * 0.05;
            bin.extend_from_slice(&x.to_le_bytes());
            bin.extend_from_slice(&y.to_le_bytes());
            bin.extend_from_slice(&0.0f32.to_le_bytes());
        }
        let pos_len = bin.len();
        let idx_off = bin.len();
        let hi_tri = [65_997u32, 65_998u32, 65_999u32];
        for idx in hi_tri {
            bin.extend_from_slice(&idx.to_le_bytes());
        }
        let idx_len = bin.len() - idx_off;

        let doc = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0 },
                    "indices": 1
                }]
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": pos_len },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": idx_len }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": vert_count, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [12.75, 12.9, 0.0] },
                { "bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR" }
            ]
        });

        let glb = pack_glb(doc, bin);
        let parsed = parse_glb(&glb).unwrap();
        assert_eq!(parsed.vertices.len(), 66_000);
        assert_eq!(parsed.indices, vec![65_997, 65_998, 65_999]);
    }

    #[test]
    fn seeded_cryo_prison_glbs_parse_with_textures_and_emissives_when_present() {
        let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("world")
            .join("areas")
            .join("cryo_prison")
            .join("assets");
        if !assets_dir.exists() {
            return;
        }
        for slug in [
            "cryo_rack_bay",
            "hero_cryo_pod",
            "cage_elevator",
            "emergency_locker",
            "cave_wall_seg",
            "cave_pipes",
            "wall_sconce",
        ] {
            let bytes = std::fs::read(assets_dir.join(format!("{slug}.glb"))).unwrap();
            let parsed = parse_glb(&bytes).unwrap();
            assert!(!parsed.vertices.is_empty(), "{slug} had no vertices");
            assert!(!parsed.indices.is_empty(), "{slug} had no indices");
            assert_eq!(parsed.textures.len(), 1, "{slug} should decode embedded PNG atlas");
        }
    }
}
