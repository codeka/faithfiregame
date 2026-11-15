use std::f32::consts::TAU;

use glow::HasContext;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(super) struct GizmoMeshGpu {
    pub(super) arrow_vao: glow::VertexArray,
    #[allow(dead_code)]
    arrow_vbo: glow::Buffer,
    #[allow(dead_code)]
    arrow_ebo: glow::Buffer,
    pub(super) arrow_index_count: i32,
    pub(super) hub_vao: glow::VertexArray,
    #[allow(dead_code)]
    hub_vbo: glow::Buffer,
    #[allow(dead_code)]
    hub_ebo: glow::Buffer,
    pub(super) hub_index_count: i32,
    pub(super) ref_vao: glow::VertexArray,
    #[allow(dead_code)]
    ref_vbo: glow::Buffer,
    pub(super) ring_vao: glow::VertexArray,
    #[allow(dead_code)]
    ring_vbo: glow::Buffer,
    #[allow(dead_code)]
    ring_ebo: glow::Buffer,
    pub(super) ring_index_count: i32,
}

impl GizmoMeshGpu {
    pub(super) fn new(gl: &glow::Context) -> Result<Self, String> {
        let (arrow_verts, arrow_indices) = build_unit_y_arrow();
        let (arrow_vao, arrow_vbo, arrow_ebo, arrow_index_count) =
            upload_pos_norm_mesh(gl, &arrow_verts, &arrow_indices)?;

        let (hub_verts, hub_indices) = build_hub_cube(0.034);
        let (hub_vao, hub_vbo, hub_ebo, hub_index_count) =
            upload_pos_norm_mesh(gl, &hub_verts, &hub_indices)?;

        let (ref_vao, ref_vbo) = upload_reference_lines(gl)?;

        let (ring_verts, ring_indices) = build_xz_torus_ring(0.88, 0.014, 48, 8);
        let (ring_vao, ring_vbo, ring_ebo, ring_index_count) =
            upload_pos_norm_mesh(gl, &ring_verts, &ring_indices)?;

        Ok(Self {
            arrow_vao,
            arrow_vbo,
            arrow_ebo,
            arrow_index_count,
            hub_vao,
            hub_vbo,
            hub_ebo,
            hub_index_count,
            ref_vao,
            ref_vbo,
            ring_vao,
            ring_vbo,
            ring_ebo,
            ring_index_count,
        })
    }
}

fn push_vertex(verts: &mut Vec<f32>, pos: [f32; 3], norm: [f32; 3]) -> u16 {
    let idx = (verts.len() / 6) as u16;
    verts.extend_from_slice(&pos);
    verts.extend_from_slice(&norm);
    idx
}

fn build_unit_y_arrow() -> (Vec<f32>, Vec<u16>) {
    let mut verts = Vec::new();
    let mut indices = Vec::new();

    // 12-sided cylinder shaft from y = 0.04 to y = 0.81
    let shaft_sides = 12usize;
    let shaft_r = 0.013f32;
    let shaft_y0 = 0.04f32;
    let shaft_y1 = 0.81f32;
    let shaft_base = (verts.len() / 6) as u16;
    for i in 0..shaft_sides {
        let a = (i as f32 / shaft_sides as f32) * TAU;
        let (sa, ca) = a.sin_cos();
        push_vertex(&mut verts, [ca * shaft_r, shaft_y0, sa * shaft_r], [ca, 0.0, sa]);
        push_vertex(&mut verts, [ca * shaft_r, shaft_y1, sa * shaft_r], [ca, 0.0, sa]);
    }
    for i in 0..shaft_sides {
        let next = (i + 1) % shaft_sides;
        let b0 = shaft_base + (i * 2) as u16;
        let t0 = b0 + 1;
        let b1 = shaft_base + (next * 2) as u16;
        let t1 = b1 + 1;
        indices.extend_from_slice(&[b0, t0, t1, b0, t1, b1]);
    }

    // 16-sided cone arrowhead from y = 0.80 to y = 1.00
    let cone_sides = 16usize;
    let cone_r = 0.044f32;
    let cone_y0 = 0.80f32;
    let cone_y1 = 1.0f32;
    let slope_h = cone_y1 - cone_y0;
    let n_len = (slope_h * slope_h + cone_r * cone_r).sqrt();
    let nr = slope_h / n_len;
    let ny = cone_r / n_len;

    let tip_idx = push_vertex(&mut verts, [0.0, cone_y1, 0.0], [0.0, 1.0, 0.0]);
    let rim_base = (verts.len() / 6) as u16;
    for i in 0..cone_sides {
        let a = (i as f32 / cone_sides as f32) * TAU;
        let (sa, ca) = a.sin_cos();
        push_vertex(
            &mut verts,
            [ca * cone_r, cone_y0, sa * cone_r],
            [ca * nr, ny, sa * nr],
        );
    }
    for i in 0..cone_sides {
        let next = (i + 1) % cone_sides;
        indices.extend_from_slice(&[rim_base + i as u16, tip_idx, rim_base + next as u16]);
    }

    // Cone base cap at y = 0.80
    let cap_center = push_vertex(&mut verts, [0.0, cone_y0, 0.0], [0.0, -1.0, 0.0]);
    let cap_base = (verts.len() / 6) as u16;
    for i in 0..cone_sides {
        let a = (i as f32 / cone_sides as f32) * TAU;
        let (sa, ca) = a.sin_cos();
        push_vertex(
            &mut verts,
            [ca * cone_r, cone_y0, sa * cone_r],
            [0.0, -1.0, 0.0],
        );
    }
    for i in 0..cone_sides {
        let next = (i + 1) % cone_sides;
        indices.extend_from_slice(&[cap_center, cap_base + i as u16, cap_base + next as u16]);
    }

    (verts, indices)
}

fn build_hub_cube(half: f32) -> (Vec<f32>, Vec<u16>) {
    let mut verts = Vec::with_capacity(24 * 6);
    let mut indices = Vec::with_capacity(36);
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [
                [half, -half, half],
                [half, -half, -half],
                [half, half, -half],
                [half, half, half],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-half, -half, -half],
                [-half, -half, half],
                [-half, half, half],
                [-half, half, -half],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-half, half, half],
                [half, half, half],
                [half, half, -half],
                [-half, half, -half],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-half, -half, -half],
                [half, -half, -half],
                [half, -half, half],
                [-half, -half, half],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-half, -half, half],
                [half, -half, half],
                [half, half, half],
                [-half, half, half],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [half, -half, -half],
                [-half, -half, -half],
                [-half, half, -half],
                [half, half, -half],
            ],
        ),
    ];

    for (norm, corners) in faces {
        let base = (verts.len() / 6) as u16;
        for pos in corners {
            push_vertex(&mut verts, pos, norm);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (verts, indices)
}

fn upload_pos_norm_mesh(
    gl: &glow::Context,
    verts: &[f32],
    indices: &[u16],
) -> Result<(glow::VertexArray, glow::Buffer, glow::Buffer, i32), String> {
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        let ebo = gl.create_buffer()?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let vbytes: &[u8] =
            std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vbytes, glow::STATIC_DRAW);

        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
        let ibytes: &[u8] =
            std::slice::from_raw_parts(indices.as_ptr() as *const u8, indices.len() * 2);
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);

        let stride = 6 * 4;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
        gl.bind_vertex_array(None);

        Ok((vao, vbo, ebo, indices.len() as i32))
    }
}

fn upload_reference_lines(gl: &glow::Context) -> Result<(glow::VertexArray, glow::Buffer), String> {
    // Vertices 0..2: vertical segment (0..1 on +Y)
    // Vertices 2..6: ground footprint cross in XZ plane
    #[rustfmt::skip]
    let pts: [f32; 18] = [
         0.0, 0.0,  0.0,   0.0, 1.0, 0.0,
        -1.0, 0.0,  0.0,   1.0, 0.0, 0.0,
         0.0, 0.0, -1.0,   0.0, 0.0, 1.0,
    ];
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let bytes: &[u8] = std::slice::from_raw_parts(pts.as_ptr() as *const u8, pts.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
        gl.bind_vertex_array(None);
        Ok((vao, vbo))
    }
}

fn build_xz_torus_ring(
    major_r: f32,
    minor_r: f32,
    major_seg: usize,
    minor_seg: usize,
) -> (Vec<f32>, Vec<u16>) {
    let mut verts = Vec::with_capacity(major_seg * minor_seg * 6);
    let mut indices = Vec::with_capacity(major_seg * minor_seg * 6);
    for i in 0..major_seg {
        let theta = (i as f32 / major_seg as f32) * TAU;
        let (st, ct) = theta.sin_cos();
        let cx = major_r * ct;
        let cz = major_r * st;
        for j in 0..minor_seg {
            let phi = (j as f32 / minor_seg as f32) * TAU;
            let (sp, cp) = phi.sin_cos();
            let norm = [cp * ct, sp, cp * st];
            let pos = [cx + minor_r * norm[0], minor_r * norm[1], cz + minor_r * norm[2]];
            push_vertex(&mut verts, pos, norm);
        }
    }
    for i in 0..major_seg {
        let ni = (i + 1) % major_seg;
        for j in 0..minor_seg {
            let nj = (j + 1) % minor_seg;
            let v00 = (i * minor_seg + j) as u16;
            let v10 = (ni * minor_seg + j) as u16;
            let v01 = (i * minor_seg + nj) as u16;
            let v11 = (ni * minor_seg + nj) as u16;
            indices.extend_from_slice(&[v00, v10, v11, v00, v11, v01]);
        }
    }
    (verts, indices)
}
