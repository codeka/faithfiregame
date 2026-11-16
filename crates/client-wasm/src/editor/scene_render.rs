use std::collections::HashMap;

use glam::{Vec3, Vec4};
use shared::world::{AreaKind, PropPlacement, RoadSpline, TILE_SIZE};

use crate::camera::Camera;
use crate::render::{BrushOverlay, Renderer, RoadMeshGpu};

use super::gizmo::gizmo_scale;
use super::state::EditorState;
use super::types::{DecorationSubMode, EditorTool, RoadSubMode, WalkabilityMode};

impl EditorState {
    pub fn render_shadows(&self, renderer: &mut Renderer, camera: &Camera) {
        renderer.begin_shadow_pass(camera);
        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                renderer.draw_terrain_shadow(&tile.gpu);
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_shadows(renderer, &tile.vis.props);
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_shadow(&inside.gpu);
            self.draw_prop_shadows(renderer, &inside.vis.props);
        }
    }

    pub fn render(&self, renderer: &Renderer, camera: &Camera) {
        let show_brush = self.hover_world.is_some()
            && matches!(
                self.tool,
                EditorTool::Heightmap | EditorTool::Texture | EditorTool::Walkability
            );

        let brush = BrushOverlay {
            active: show_brush,
            center: self.hover_world.unwrap_or(Vec3::ZERO),
            radius: self.brush_radius,
            color: match self.tool {
                EditorTool::Heightmap => [0.25, 0.85, 1.0],
                EditorTool::Texture => match self.texture_layer {
                    0 => [0.35, 0.90, 0.35],
                    1 => [0.85, 0.58, 0.30],
                    2 => [0.75, 0.78, 0.85],
                    3 => [0.95, 0.85, 0.45],
                    4 => [0.62, 0.95, 0.42],
                    5 => [0.22, 0.65, 0.28],
                    6 => [0.58, 0.72, 0.32],
                    7 => [0.68, 0.68, 0.72],
                    _ => [0.86, 0.44, 0.32],
                },
                EditorTool::Road => [0.95, 0.76, 0.30],
                EditorTool::Decoration => [1.0, 1.0, 1.0],
                EditorTool::Walkability => match self.walkability_mode {
                    WalkabilityMode::Block => [0.96, 0.30, 0.28],
                    WalkabilityMode::Walkable => [0.28, 0.92, 0.52],
                },
            },
        };

        let show_walkability = self.tool == EditorTool::Walkability;
        let road_opacity = if matches!(self.tool, EditorTool::Heightmap | EditorTool::Walkability) {
            0.42
        } else {
            1.0
        };

        if self.active_area.kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &self.outside_tiles {
                let ox = (tx * TILE_SIZE as i32) as f32;
                let oz = (ty * TILE_SIZE as i32) as f32;
                renderer.draw_terrain_chunk(
                    camera,
                    &tile.gpu,
                    ox,
                    oz,
                    true,
                    show_walkability,
                    &brush,
                );
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_list(renderer, camera, &tile.vis.props);
            }
            for tile in self.outside_tiles.values() {
                self.draw_road_list(
                    renderer,
                    camera,
                    &tile.vis.roads,
                    &tile.road_meshes,
                    road_opacity,
                    &brush,
                );
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_chunk(
                camera,
                &inside.gpu,
                0.0,
                0.0,
                true,
                show_walkability,
                &brush,
            );
            self.draw_prop_list(renderer, camera, &inside.vis.props);
            self.draw_road_list(
                renderer,
                camera,
                &inside.vis.roads,
                &inside.road_meshes,
                road_opacity,
                &brush,
            );
        }

        if self.tool == EditorTool::Road {
            self.draw_road_overlays(renderer, camera, &brush);
        }

        // Ghost preview when placing a decoration
        if self.tool == EditorTool::Decoration && self.decoration_submode == DecorationSubMode::Place
        {
            if let (Some(hit), Some(mesh)) = (self.hover_world, self.prop_meshes.get(&self.active_asset_id))
            {
                let ghost = PropPlacement {
                    id: 0,
                    asset_id: self.active_asset_id.clone(),
                    position: hit.to_array(),
                    rotation: self.preview_rotation,
                    scale: [self.preview_scale; 3],
                    cast_shadow: self.preview_cast_shadow,
                };
                renderer.draw_prop(camera, mesh, &ghost, Vec4::new(0.65, 1.0, 0.78, 0.60));
                let extent = (mesh.aabb_max.x - mesh.aabb_min.x)
                    .max(mesh.aabb_max.z - mesh.aabb_min.z)
                    * 0.55
                    * self.preview_scale;
                renderer.draw_selection_ring(camera, &ghost, extent.max(0.9));
            }
        }

        // 3-axis gizmos in Free Move / Rotate submodes (drawn last after depth clear)
        if self.tool == EditorTool::Decoration {
            if let Some(prop) = self.find_selected_prop() {
                let origin = Vec3::from(prop.position);
                let scale = gizmo_scale(camera, origin);
                match self.decoration_submode {
                    DecorationSubMode::FreeMove => {
                        let active = self
                            .gizmo_drag
                            .map(|d| d.axis)
                            .or(self.hovered_gizmo_axis)
                            .map(|a| a.index());
                        let ground_y = self.terrain_height_at(origin.x, origin.z);
                        renderer.draw_translate_gizmo(camera, origin, scale, active, ground_y);
                    }
                    DecorationSubMode::Rotate => {
                        let active = self
                            .rotate_drag
                            .map(|d| d.axis)
                            .or(self.hovered_rotate_axis)
                            .map(|a| a.index());
                        renderer.draw_rotate_gizmo(camera, origin, scale, active);
                    }
                    DecorationSubMode::Place | DecorationSubMode::Move => {}
                }
            }
        }
    }

    fn draw_prop_shadows(&self, renderer: &Renderer, props: &[PropPlacement]) {
        for prop in props {
            if !prop.cast_shadow {
                continue;
            }
            if let Some(mesh) = self.prop_meshes.get(&prop.asset_id) {
                renderer.draw_prop_shadow(mesh, prop);
            }
        }
    }

    fn draw_prop_list(&self, renderer: &Renderer, camera: &Camera, props: &[PropPlacement]) {
        for prop in props {
            if let Some(mesh) = self.prop_meshes.get(&prop.asset_id) {
                let selected = self.tool == EditorTool::Decoration
                    && self.decoration_submode != DecorationSubMode::Place
                    && self.selected_prop_id == Some(prop.id);
                let tint = if selected {
                    Vec4::new(1.30, 1.18, 0.72, 1.0)
                } else {
                    Vec4::ONE
                };
                renderer.draw_prop(camera, mesh, prop, tint);
                if selected {
                    let extent = (mesh.aabb_max.x - mesh.aabb_min.x)
                        .max(mesh.aabb_max.z - mesh.aabb_min.z)
                        * 0.55
                        * prop.scale[0];
                    renderer.draw_selection_ring(camera, prop, extent.max(0.9));
                }
            }
        }
    }

    fn draw_road_list(
        &self,
        renderer: &Renderer,
        camera: &Camera,
        roads: &[RoadSpline],
        meshes: &HashMap<u64, RoadMeshGpu>,
        opacity: f32,
        brush: &BrushOverlay,
    ) {
        for road in roads {
            let Some(mesh) = meshes.get(&road.id) else {
                continue;
            };
            let selected = self.tool == EditorTool::Road
                && self.road_submode == RoadSubMode::Edit
                && self.selected_road_id == Some(road.id);
            let tint = if selected {
                Vec3::new(1.18, 1.12, 0.88)
            } else {
                Vec3::ONE
            };
            renderer.draw_road(camera, mesh, road.material, opacity, tint, brush);
        }
    }

    fn draw_road_overlays(&self, renderer: &Renderer, camera: &Camera, brush: &BrushOverlay) {
        let draw_road_endpoints = |road: &RoadSpline| {
            if road.points.len() < 2 {
                return;
            }
            let skip_selected =
                self.road_submode == RoadSubMode::Edit && self.selected_road_id == Some(road.id);
            if skip_selected {
                return;
            }
            for idx in [0usize, road.points.len() - 1] {
                let p = road.points[idx];
                renderer.draw_node_ring(
                    camera,
                    Vec3::new(p[0], p[1] + 0.08, p[2]),
                    0.55,
                    [0.35, 0.90, 0.85, 0.82],
                    false,
                );
            }
        };

        if self.active_area.kind == AreaKind::Outside {
            for tile in self.outside_tiles.values() {
                for road in &tile.vis.roads {
                    draw_road_endpoints(road);
                }
            }
        } else if let Some(inside) = &self.inside_area {
            for road in &inside.vis.roads {
                draw_road_endpoints(road);
            }
        }

        if self.road_submode == RoadSubMode::Place {
            if let Some(draft) = &self.road_draft {
                if let Some(preview) = &draft.preview_gpu {
                    renderer.draw_road(
                        camera,
                        preview,
                        self.road_material,
                        0.76,
                        Vec3::new(0.84, 1.04, 0.95),
                        brush,
                    );
                }
                let mut line_pts: Vec<[f32; 3]> = draft
                    .points
                    .iter()
                    .map(|p| [p[0], p[1] + 0.14, p[2]])
                    .collect();
                if let Some((cursor_pt, _)) = self.hover_road_point {
                    line_pts.push([cursor_pt[0], cursor_pt[1] + 0.14, cursor_pt[2]]);
                }
                if line_pts.len() >= 2 {
                    renderer.draw_world_lines(
                        camera,
                        &line_pts,
                        [0.98, 0.85, 0.35, 0.95],
                        true,
                    );
                }
                for p in &draft.points {
                    renderer.draw_node_ring(
                        camera,
                        Vec3::new(p[0], p[1] + 0.14, p[2]),
                        0.55,
                        [0.98, 0.85, 0.35, 0.95],
                        true,
                    );
                }
            }
            if let Some((cursor_pt, snapped)) = self.hover_road_point {
                let (color, radius) = if snapped {
                    ([0.25, 1.0, 0.55, 1.0], 0.85)
                } else {
                    ([0.98, 0.85, 0.35, 0.95], 0.55)
                };
                renderer.draw_node_ring(
                    camera,
                    Vec3::new(cursor_pt[0], cursor_pt[1] + 0.14, cursor_pt[2]),
                    radius,
                    color,
                    true,
                );
            }
        } else if let Some(road) = self.find_selected_road() {
            let line_pts: Vec<[f32; 3]> = road
                .points
                .iter()
                .map(|p| [p[0], p[1] + 0.16, p[2]])
                .collect();
            if line_pts.len() >= 2 {
                renderer.draw_world_lines(camera, &line_pts, [1.0, 0.82, 0.28, 0.95], true);
            }
            for (idx, p) in road.points.iter().enumerate() {
                let active_node = self.selected_road_node == Some(idx);
                let (color, radius) = if active_node {
                    ([0.25, 0.95, 1.0, 1.0], 0.85)
                } else {
                    ([1.0, 0.82, 0.25, 0.95], 0.62)
                };
                renderer.draw_node_ring(
                    camera,
                    Vec3::new(p[0], p[1] + 0.16, p[2]),
                    radius,
                    color,
                    true,
                );
            }
        }
    }
}
