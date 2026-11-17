use std::f32::consts::TAU;

use glam::Vec3;

use crate::glb_loader::{ParsedPropMesh, ParsedPropSubmesh, PropVertex};

use super::prop_mesh::PropMeshGpu;

#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub struct StudioStageGpu {
    pub(super) pedestal: PropMeshGpu,
    pub(super) grid_segments: Vec<[f32; 3]>,
    pub(super) axis_x: Vec<[f32; 3]>,
    pub(super) axis_z: Vec<[f32; 3]>,
}

impl StudioStageGpu {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let radius = 3.2f32;
        let segs = 48usize;
        let mut vertices = Vec::with_capacity(segs + 1);
        let mut indices = Vec::with_capacity(segs * 3);

        vertices.push(PropVertex {
            pos: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            color: [0.25, 0.28, 0.35, 1.0],
            uv: [0.5, 0.5],
        });
        for i in 0..segs {
            let a = (i as f32 / segs as f32) * TAU;
            let x = a.cos() * radius;
            let z = a.sin() * radius;
            vertices.push(PropVertex {
                pos: [x, 0.0, z],
                normal: [0.0, 1.0, 0.0],
                color: [0.19, 0.22, 0.28, 1.0],
                uv: [0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin()],
            });
            let curr = 1 + i as u32;
            let next = 1 + ((i + 1) % segs) as u32;
            // Winding for +Y normal (CCW when viewed from +Y): 0 -> next -> curr
            indices.extend_from_slice(&[0, next, curr]);
        }

        let parsed = ParsedPropMesh {
            vertices,
            indices: indices.clone(),
            textures: Vec::new(),
            submeshes: vec![ParsedPropSubmesh {
                index_offset: 0,
                index_count: indices.len() as u32,
                texture_index: None,
                emissive: [0.0; 3],
                double_sided: false,
                alpha_cutoff: 0.0,
            }],
            aabb_min: Vec3::new(-radius, 0.0, -radius),
            aabb_max: Vec3::new(radius, 0.0, radius),
        };
        let pedestal = PropMeshGpu::from_parsed(gl, &parsed)?;

        let mut grid_segments = Vec::new();
        let half = 3.0f32;
        for step in -6..=6 {
            let v = step as f32 * 0.5;
            if step == 0 {
                continue;
            }
            grid_segments.push([-half, 0.003, v]);
            grid_segments.push([half, 0.003, v]);
            grid_segments.push([v, 0.003, -half]);
            grid_segments.push([v, 0.003, half]);
        }
        // Outer pedestal ring line segments
        for i in 0..segs {
            let a0 = (i as f32 / segs as f32) * TAU;
            let a1 = ((i + 1) as f32 / segs as f32) * TAU;
            grid_segments.push([a0.cos() * radius, 0.004, a0.sin() * radius]);
            grid_segments.push([a1.cos() * radius, 0.004, a1.sin() * radius]);
        }

        let axis_x = vec![[-half, 0.005, 0.0], [half, 0.005, 0.0]];
        // +Z forward axis + small chevron arrow at +1.4m
        let axis_z = vec![
            [0.0, 0.005, -half],
            [0.0, 0.005, half],
            [-0.14, 0.006, 1.25],
            [0.0, 0.006, 1.45],
            [0.14, 0.006, 1.25],
            [0.0, 0.006, 1.45],
        ];

        Ok(Self {
            pedestal,
            grid_segments,
            axis_x,
            axis_z,
        })
    }
}
