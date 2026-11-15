use glam::{Vec3, Vec4};
use shared::world::{AreaKind, PropPlacement, TILE_SIZE};

use crate::camera::Camera;
use crate::render::{BrushOverlay, Renderer};

use super::gizmo::gizmo_scale;
use super::state::EditorState;
use super::types::{DecorationSubMode, EditorTool};

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
            && matches!(self.tool, EditorTool::Heightmap | EditorTool::Texture);

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
                    _ => [0.95, 0.85, 0.45],
                },
                EditorTool::Decoration => [1.0, 1.0, 1.0],
            },
        };

        if self.active_area.kind == AreaKind::Outside {
            for (&(tx, ty), tile) in &self.outside_tiles {
                let ox = (tx * TILE_SIZE as i32) as f32;
                let oz = (ty * TILE_SIZE as i32) as f32;
                renderer.draw_terrain_chunk(camera, &tile.gpu, ox, oz, true, &brush);
            }
            for tile in self.outside_tiles.values() {
                self.draw_prop_list(renderer, camera, &tile.vis.props);
            }
        } else if let Some(inside) = &self.inside_area {
            renderer.draw_terrain_chunk(camera, &inside.gpu, 0.0, 0.0, true, &brush);
            self.draw_prop_list(renderer, camera, &inside.vis.props);
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
}
