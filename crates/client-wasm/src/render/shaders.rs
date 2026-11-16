use glow::HasContext;

pub(super) const SHADOW_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;

uniform mat4 u_light_view_proj;
uniform mat4 u_model;

void main() {
    gl_Position = u_light_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

pub(super) const SHADOW_FRAG: &str = r#"#version 300 es
precision highp float;

void main() {}
"#;

macro_rules! shadow_sampler_glsl {
    () => {
        r#"
uniform mat4 u_light_view_proj;
uniform sampler2DShadow u_shadow_map;
uniform vec3 u_sun_dir;
uniform float u_shadow_texel;

float sample_shadow(vec3 world_pos, vec3 normal) {
    vec4 light_clip = u_light_view_proj * vec4(world_pos + normal * 0.06, 1.0);
    vec3 proj = (light_clip.xyz / light_clip.w) * 0.5 + 0.5;
    if (proj.x <= 0.0 || proj.x >= 1.0 || proj.y <= 0.0 || proj.y >= 1.0 || proj.z <= 0.0 || proj.z >= 1.0) {
        return 1.0;
    }

    float ndotl = clamp(dot(normal, u_sun_dir), 0.0, 1.0);
    float bias = mix(0.0014, 0.0003, ndotl);
    float ref_depth = proj.z - bias;

    float sum = 0.0;
    for (int iy = -1; iy <= 1; iy++) {
        float wy = (iy == 0) ? 2.0 : 1.0;
        for (int ix = -1; ix <= 1; ix++) {
            float wx = (ix == 0) ? 2.0 : 1.0;
            vec2 offset = vec2(float(ix), float(iy)) * u_shadow_texel;
            sum += texture(u_shadow_map, vec3(proj.xy + offset, ref_depth)) * (wx * wy);
        }
    }
    float pcf = sum / 16.0;

    vec2 edge = min(proj.xy, 1.0 - proj.xy);
    float fade = smoothstep(0.0, 0.06, min(edge.x, edge.y)) * (1.0 - smoothstep(0.92, 1.0, proj.z));
    return mix(1.0, pcf, fade);
}
"#
    };
}

pub(super) const TERRAIN_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;

uniform mat4 u_view_proj;

out vec3 v_world_pos;
out vec3 v_normal;
out vec2 v_uv;

void main() {
    v_world_pos = a_pos;
    v_normal = a_normal;
    v_uv = a_uv;
    gl_Position = u_view_proj * vec4(a_pos, 1.0);
}
"#;

pub(super) const TERRAIN_FRAG: &str = concat!(
    r#"#version 300 es
precision highp float;
precision highp usampler2D;
precision highp sampler2DArray;
precision highp sampler2DShadow;

in vec3 v_world_pos;
in vec3 v_normal;
in vec2 v_uv;

uniform usampler2D u_splat_ids;
uniform sampler2D u_splat_weights;
uniform sampler2DArray u_tex_array;
uniform vec3 u_camera_pos;
uniform vec3 u_tile_origin;
uniform vec2 u_tile_size;
uniform int u_show_grid;
uniform vec3 u_brush_center;
uniform float u_brush_radius;
uniform vec3 u_brush_color;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_fog_color;

out vec4 frag_color;
"#,
    shadow_sampler_glsl!(),
    r#"
const int MAT_COUNT = 9;
const float HEIGHT_STRENGTH = 0.85;
const float BLEND_DEPTH = 0.15;

float corner_mat_weight(uvec4 ids, vec4 w, uint m) {
    return (ids.x == m ? w.x : 0.0)
         + (ids.y == m ? w.y : 0.0)
         + (ids.z == m ? w.z : 0.0)
         + (ids.w == m ? w.w : 0.0);
}

vec3 sample_height_blended_terrain(ivec2 c00, vec2 f, vec2 world_uv, vec2 uv_dx, vec2 uv_dy) {
    float b00 = (1.0 - f.x) * (1.0 - f.y);
    float b10 = f.x * (1.0 - f.y);
    float b01 = (1.0 - f.x) * f.y;
    float b11 = f.x * f.y;

    uvec4 ids00 = texelFetch(u_splat_ids, c00, 0);
    vec4 w00 = texelFetch(u_splat_weights, c00, 0);
    w00 *= b00 / max(w00.x + w00.y + w00.z + w00.w, 0.0001);

    uvec4 ids10 = texelFetch(u_splat_ids, c00 + ivec2(1, 0), 0);
    vec4 w10 = texelFetch(u_splat_weights, c00 + ivec2(1, 0), 0);
    w10 *= b10 / max(w10.x + w10.y + w10.z + w10.w, 0.0001);

    uvec4 ids01 = texelFetch(u_splat_ids, c00 + ivec2(0, 1), 0);
    vec4 w01 = texelFetch(u_splat_weights, c00 + ivec2(0, 1), 0);
    w01 *= b01 / max(w01.x + w01.y + w01.z + w01.w, 0.0001);

    uvec4 ids11 = texelFetch(u_splat_ids, c00 + ivec2(1, 1), 0);
    vec4 w11 = texelFetch(u_splat_weights, c00 + ivec2(1, 1), 0);
    w11 *= b11 / max(w11.x + w11.y + w11.z + w11.w, 0.0001);

    float mat_w[MAT_COUNT];
    float mat_s[MAT_COUNT];
    vec3 mat_rgb[MAT_COUNT];
    float s_max = -1.0;

    for (int m = 0; m < MAT_COUNT; m++) {
        uint mu = uint(m);
        float wm = corner_mat_weight(ids00, w00, mu)
                 + corner_mat_weight(ids10, w10, mu)
                 + corner_mat_weight(ids01, w01, mu)
                 + corner_mat_weight(ids11, w11, mu);
        mat_w[m] = wm;
        if (wm > 0.001) {
            vec4 tex = textureGrad(u_tex_array, vec3(world_uv, float(m)), uv_dx, uv_dy);
            mat_rgb[m] = tex.rgb;
            float s = wm + HEIGHT_STRENGTH * tex.a * smoothstep(0.0, 0.15, wm);
            mat_s[m] = s;
            s_max = max(s_max, s);
        } else {
            mat_rgb[m] = vec3(0.0);
            mat_s[m] = -1.0;
        }
    }

    vec3 albedo_sum = vec3(0.0);
    float blend_sum = 0.0;
    float threshold = s_max - BLEND_DEPTH;
    for (int m = 0; m < MAT_COUNT; m++) {
        float wm = mat_w[m];
        if (wm > 0.001) {
            float bw = smoothstep(0.0, 0.05, wm) * max(mat_s[m] - threshold, 0.0);
            albedo_sum += mat_rgb[m] * bw;
            blend_sum += bw;
        }
    }
    return albedo_sum / max(blend_sum, 0.0001);
}

void main() {
    vec2 world_uv = v_world_pos.xz * 0.25;
    vec2 uv_dx = dFdx(world_uv);
    vec2 uv_dy = dFdy(world_uv);

    vec2 cell_pos = clamp(v_uv, 0.0, 1.0) * u_tile_size;
    ivec2 max_cell = max(ivec2(round(u_tile_size)) - ivec2(1), ivec2(0));
    ivec2 c00 = clamp(ivec2(floor(cell_pos)), ivec2(0), max_cell);
    vec2 f = clamp(cell_pos - vec2(c00), 0.0, 1.0);

    vec3 albedo = sample_height_blended_terrain(c00, f, world_uv, uv_dx, uv_dy);

    vec3 smooth_n = normalize(v_normal);
    vec3 geom_n = normalize(cross(dFdx(v_world_pos), dFdy(v_world_pos)));
    if (geom_n.y < 0.0) {
        geom_n = -geom_n;
    }
    vec3 n = normalize(mix(smooth_n, geom_n, 0.22));

    vec3 sun_dir = normalize(u_sun_dir);
    float diff = max(dot(n, sun_dir), 0.0);
    float shadow = (diff > 0.0) ? sample_shadow(v_world_pos, n) : 1.0;
    float hemi = 0.5 + 0.5 * n.y;
    vec3 ambient = mix(vec3(0.22, 0.25, 0.30), vec3(0.34, 0.38, 0.45), hemi);
    vec3 sun_light = diff * vec3(1.06, 1.00, 0.88) * 1.05;
    vec3 lit = albedo * (ambient + sun_light * shadow);

    if (u_show_grid != 0) {
        vec2 local_xz = v_world_pos.xz - u_tile_origin.xz;
        vec2 cell_f = abs(fract(local_xz - 0.5) - 0.5);
        float cell_dist = min(cell_f.x, cell_f.y);
        float cell_line = 1.0 - smoothstep(0.0, 0.035, cell_dist);
        lit = mix(lit, lit * 0.82 + vec3(0.06), cell_line * 0.35);

        float edge_x = min(local_xz.x, u_tile_size.x - local_xz.x);
        float edge_z = min(local_xz.y, u_tile_size.y - local_xz.y);
        float edge_dist = min(edge_x, edge_z);
        float border = 1.0 - smoothstep(0.0, 0.18, edge_dist);
        lit = mix(lit, vec3(0.95, 0.78, 0.28), border * 0.65);
    }

    if (u_brush_radius > 0.05) {
        float d = length(v_world_pos.xz - u_brush_center.xz);
        float ring = 1.0 - smoothstep(0.0, 0.16, abs(d - u_brush_radius));
        float fill = smoothstep(u_brush_radius, 0.0, d) * 0.12;
        lit = mix(lit, u_brush_color, clamp(ring * 0.85 + fill, 0.0, 1.0));
    }

    float cam_dist = length(v_world_pos - u_camera_pos);
    float fog_t = smoothstep(u_fog_start, u_fog_end, cam_dist);
    vec3 final_rgb = mix(lit, u_fog_color, fog_t);

    frag_color = vec4(final_rgb, 1.0);
}
"#
);

pub(super) const PROP_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec4 a_color;

uniform mat4 u_view_proj;
uniform mat4 u_model;

out vec3 v_world_pos;
out vec3 v_normal;
out vec4 v_color;

void main() {
    vec4 wp = u_model * vec4(a_pos, 1.0);
    v_world_pos = wp.xyz;
    mat3 normal_mat = transpose(inverse(mat3(u_model)));
    v_normal = normalize(normal_mat * a_normal);
    v_color = a_color;
    gl_Position = u_view_proj * wp;
}
"#;

pub(super) const PROP_FRAG: &str = concat!(
    r#"#version 300 es
precision highp float;
precision highp sampler2DShadow;

in vec3 v_world_pos;
in vec3 v_normal;
in vec4 v_color;

uniform vec3 u_camera_pos;
uniform vec4 u_tint;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_fog_color;

out vec4 frag_color;
"#,
    shadow_sampler_glsl!(),
    r#"
void main() {
    vec3 n = normalize(v_normal);
    vec3 sun_dir = normalize(u_sun_dir);
    float diff = max(dot(n, sun_dir), 0.0);
    float shadow = (diff > 0.0) ? sample_shadow(v_world_pos, n) : 1.0;
    float hemi = 0.5 + 0.5 * n.y;
    vec3 ambient = mix(vec3(0.22, 0.25, 0.30), vec3(0.34, 0.38, 0.45), hemi);
    vec3 base = v_color.rgb * u_tint.rgb;
    vec3 sun_light = diff * vec3(1.06, 1.00, 0.88) * 1.00;
    vec3 lit = base * (ambient + sun_light * shadow);

    float cam_dist = length(v_world_pos - u_camera_pos);
    float fog_t = smoothstep(u_fog_start, u_fog_end, cam_dist);
    vec3 final_rgb = mix(lit, u_fog_color, fog_t);

    frag_color = vec4(final_rgb, v_color.a * u_tint.a);
}
"#
);

pub(super) const LINE_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;

uniform mat4 u_view_proj;
uniform mat4 u_model;

void main() {
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

pub(super) const LINE_FRAG: &str = r#"#version 300 es
precision highp float;

uniform vec4 u_color;
out vec4 frag_color;

void main() {
    frag_color = u_color;
}
"#;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(super) const ROAD_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;

uniform mat4 u_view_proj;

out vec3 v_world_pos;
out vec3 v_normal;
out vec2 v_uv;

void main() {
    v_world_pos = a_pos;
    v_normal = a_normal;
    v_uv = a_uv;
    gl_Position = u_view_proj * vec4(a_pos, 1.0);
}
"#;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(super) const ROAD_FRAG: &str = concat!(
    r#"#version 300 es
precision highp float;
precision highp sampler2DArray;
precision highp sampler2DShadow;

in vec3 v_world_pos;
in vec3 v_normal;
in vec2 v_uv;

uniform sampler2DArray u_road_tex_array;
uniform int u_material;
uniform float u_opacity;
uniform vec3 u_tint;
uniform vec3 u_camera_pos;
uniform vec3 u_brush_center;
uniform float u_brush_radius;
uniform vec3 u_brush_color;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_fog_color;

out vec4 frag_color;
"#,
    shadow_sampler_glsl!(),
    r#"
void main() {
    vec4 tex = texture(u_road_tex_array, vec3(v_uv, float(u_material)));
    float alpha = tex.a * u_opacity;
    if (alpha < 0.01) {
        discard;
    }

    vec3 n = normalize(v_normal);
    vec3 sun_dir = normalize(u_sun_dir);
    float diff = max(dot(n, sun_dir), 0.0);
    float shadow = (diff > 0.0) ? sample_shadow(v_world_pos, n) : 1.0;
    float hemi = 0.5 + 0.5 * n.y;
    vec3 ambient = mix(vec3(0.22, 0.25, 0.30), vec3(0.34, 0.38, 0.45), hemi);
    vec3 sun_light = diff * vec3(1.06, 1.00, 0.88) * 1.05;
    vec3 lit = (tex.rgb * u_tint) * (ambient + sun_light * shadow);

    if (u_brush_radius > 0.05) {
        float d = length(v_world_pos.xz - u_brush_center.xz);
        float ring = 1.0 - smoothstep(0.0, 0.16, abs(d - u_brush_radius));
        float fill = smoothstep(u_brush_radius, 0.0, d) * 0.12;
        lit = mix(lit, u_brush_color, clamp(ring * 0.85 + fill, 0.0, 1.0));
    }

    float cam_dist = length(v_world_pos - u_camera_pos);
    float fog_t = smoothstep(u_fog_start, u_fog_end, cam_dist);
    vec3 final_rgb = mix(lit, u_fog_color, fog_t);

    frag_color = vec4(final_rgb, alpha);
}
"#
);

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(super) const GIZMO_VERT: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;

uniform mat4 u_view_proj;
uniform mat4 u_model;

out vec3 v_normal;

void main() {
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
    v_normal = mat3(u_model) * a_normal;
}
"#;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(super) const GIZMO_FRAG: &str = r#"#version 300 es
precision highp float;

uniform vec4 u_color;
in vec3 v_normal;
out vec4 frag_color;

void main() {
    vec3 n = normalize(v_normal);
    float shade = 0.68 + 0.32 * max(dot(n, normalize(vec3(0.35, 0.85, 0.45))), 0.0);
    frag_color = vec4(u_color.rgb * shade, u_color.a);
}
"#;

pub(super) fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::Program, String> {
    unsafe {
        let prog = gl.create_program()?;
        let vs = compile_shader(gl, glow::VERTEX_SHADER, vert_src)?;
        let fs = compile_shader(gl, glow::FRAGMENT_SHADER, frag_src)?;
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(format!("shader link error: {}", gl.get_program_info_log(prog)));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        Ok(prog)
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    shader_type: u32,
    src: &str,
) -> Result<glow::Shader, String> {
    unsafe {
        let shader = gl.create_shader(shader_type)?;
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(format!("shader compile error: {}", gl.get_shader_info_log(shader)));
        }
        Ok(shader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_shader_material_count_matches_shared_constant() {
        let expected = format!(
            "const int MAT_COUNT = {};",
            shared::world::TERRAIN_MATERIAL_COUNT
        );
        assert!(
            TERRAIN_FRAG.contains(&expected),
            "TERRAIN_FRAG must define `{expected}`"
        );
    }
}
