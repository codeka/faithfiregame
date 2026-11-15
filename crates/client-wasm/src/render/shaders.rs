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
precision highp sampler2DArray;
precision highp sampler2DShadow;

in vec3 v_world_pos;
in vec3 v_normal;
in vec2 v_uv;

uniform sampler2D u_splat_tex;
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
void main() {
    vec4 weights = texture(u_splat_tex, v_uv);
    float sum_w = max(weights.r + weights.g + weights.b + weights.a, 0.0001);
    weights /= sum_w;

    vec2 world_uv = v_world_pos.xz * 0.25;
    vec3 c0 = texture(u_tex_array, vec3(world_uv, 0.0)).rgb;
    vec3 c1 = texture(u_tex_array, vec3(world_uv, 1.0)).rgb;
    vec3 c2 = texture(u_tex_array, vec3(world_uv, 2.0)).rgb;
    vec3 c3 = texture(u_tex_array, vec3(world_uv, 3.0)).rgb;

    vec3 albedo = c0 * weights.r + c1 * weights.g + c2 * weights.b + c3 * weights.a;

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
