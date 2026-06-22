use std::sync::Arc;
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::camera::{Camera, CameraController, CameraUniform};
use crate::scene::{Node, NodeData};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.5, 0.0],
        color: [1.0, 0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.0],
        color: [0.0, 1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.0],
        color: [0.0, 0.0, 1.0],
    },
];

#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub w_pressed: bool,
    pub a_pressed: bool,
    pub s_pressed: bool,
    pub d_pressed: bool,
    pub right_mouse_pressed: bool,
    pub mouse_delta: (f32, f32),
    pub scroll_delta: f32,
}

pub struct EngineState {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    model_bind_group_layout: wgpu::BindGroupLayout,
    model_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    model_buffer_capacity: usize,
    vertex_buffer: wgpu::Buffer,
    
    // Terrain resources
    terrain_vertex_buffer: wgpu::Buffer,
    terrain_index_buffer: wgpu::Buffer,
    terrain_index_count: u32,

    pub camera: Camera,
    pub scene_root: Node,
    pub camera_target: glam::Vec3,
    pub camera_controller: CameraController,
}

impl EngineState {
    pub async fn new(window: Arc<Window>, scene_root: Node, camera: Camera) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                    experimental_features: wgpu::ExperimentalFeatures::default(),
                    trace: wgpu::Trace::default(),
                },
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        // Camera uniform configuration
        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("camera_bind_group_layout"),
        });

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });

        // Model matrices configuration (dynamic offset)
        let model_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(std::num::NonZeroU64::new(64).unwrap()),
                },
                count: None,
            }],
            label: Some("model_bind_group_layout"),
        });

        let model_buffer_capacity = 64 * 1024; // 64 KB
        let model_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Model Buffer"),
            size: model_buffer_capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &model_buffer,
                    offset: 0,
                    size: Some(std::num::NonZeroU64::new(64).unwrap()),
                }),
            }],
            label: Some("model_bind_group"),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout), Some(&model_bind_group_layout)],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // ------------------ Terrain Generation ------------------
        let mut terrain_vertices = Vec::with_capacity(256 * 256);
        for y in 0..256 {
            for x in 0..256 {
                let r: f32 = rand::random();
                let g: f32 = rand::random();
                let b: f32 = rand::random();
                terrain_vertices.push(Vertex {
                    position: [x as f32, y as f32, 0.0],
                    color: [r, g, b],
                });
            }
        }

        let terrain_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Terrain Vertex Buffer"),
            contents: bytemuck::cast_slice(&terrain_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let mut terrain_indices = Vec::with_capacity(255 * 255 * 6);
        for y in 0..255 {
            for x in 0..255 {
                let r0 = y * 256 + x;
                let r1 = (y + 1) * 256 + x;
                // Triangle 1
                terrain_indices.push(r0 as u32);
                terrain_indices.push((r0 + 1) as u32);
                terrain_indices.push(r1 as u32);
                // Triangle 2
                terrain_indices.push((r0 + 1) as u32);
                terrain_indices.push((r1 + 1) as u32);
                terrain_indices.push(r1 as u32);
            }
        }

        let terrain_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Terrain Index Buffer"),
            contents: bytemuck::cast_slice(&terrain_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let terrain_index_count = terrain_indices.len() as u32;

        // Initialize Camera target & controller from camera fields
        let camera_target = camera.target;
        let offset = camera.eye - camera.target;
        let distance = offset.length().clamp(10.0, 1500.0);
        let pitch = (offset.z / distance).asin().to_degrees();
        let yaw = offset.y.atan2(offset.x).to_degrees();
        let camera_controller = CameraController::new(yaw, pitch, distance);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            camera_buffer,
            camera_bind_group,
            model_bind_group_layout,
            model_buffer,
            model_bind_group,
            model_buffer_capacity,
            vertex_buffer,
            terrain_vertex_buffer,
            terrain_index_buffer,
            terrain_index_count,
            camera,
            scene_root,
            camera_target,
            camera_controller,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.camera.aspect = new_size.width as f32 / new_size.height as f32;
        }
    }

    pub fn update(&mut self, dt: f32, input: &mut InputState) {
        self.camera_controller.update_camera(&mut self.camera, &mut self.camera_target, input, dt);
        
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&self.camera);
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[camera_uniform]));
    }

    pub fn render(&mut self) -> Result<(), String> {
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.size);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.resize(self.size);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Validation => return Ok(()),
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        // 1. Update transforms in scene graph
        self.scene_root.update_transforms(&glam::Mat4::IDENTITY);

        // 2. Collect render commands from scene graph
        struct RenderCommand {
            world_transform: glam::Mat4,
            node_data: NodeData,
        }

        fn collect_render_commands(node: &Node, commands: &mut Vec<RenderCommand>) {
            if node.data != NodeData::None {
                commands.push(RenderCommand {
                    world_transform: node.world_transform,
                    node_data: node.data,
                });
            }
            for child in &node.children {
                collect_render_commands(child, commands);
            }
        }

        let mut commands = Vec::new();
        collect_render_commands(&self.scene_root, &mut commands);

        // 3. Write model matrices to buffer with correct alignment offsets
        let alignment = self.device.limits().min_uniform_buffer_offset_alignment as usize;
        let matrix_size = 64; // sizeof(Mat4)
        let entry_size = (matrix_size + alignment - 1) & !(alignment - 1);

        let model_data_len = commands.len() * entry_size;
        if model_data_len > self.model_buffer_capacity {
            // Reallocate model buffer
            self.model_buffer_capacity = model_data_len.next_power_of_two();
            self.model_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Model Buffer"),
                size: self.model_buffer_capacity as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.model_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.model_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.model_buffer,
                        offset: 0,
                        size: Some(std::num::NonZeroU64::new(64).unwrap()),
                    }),
                }],
                label: Some("model_bind_group"),
            });
        }

        if !commands.is_empty() {
            let mut model_data = vec![0u8; model_data_len];
            for (i, cmd) in commands.iter().enumerate() {
                let offset = i * entry_size;
                let matrix_array = cmd.world_transform.to_cols_array_2d();
                let matrix_bytes = bytemuck::bytes_of(&matrix_array);
                model_data[offset..offset + matrix_size].copy_from_slice(matrix_bytes);
            }
            self.queue.write_buffer(&self.model_buffer, 0, &model_data);
        }

        // 4. Record render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.2,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

            for (i, cmd) in commands.iter().enumerate() {
                let dynamic_offset = (i * entry_size) as u32;
                render_pass.set_bind_group(1, &self.model_bind_group, &[dynamic_offset]);
                
                match cmd.node_data {
                    NodeData::Triangle => {
                        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                        render_pass.draw(0..3, 0..1);
                    }
                    NodeData::Terrain => {
                        render_pass.set_vertex_buffer(0, self.terrain_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(
                            self.terrain_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        render_pass.draw_indexed(0..self.terrain_index_count, 0, 0..1);
                    }
                    _ => {}
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

pub struct EngineApp {
    title: String,
    scene_root: Node,
    camera: Camera,
    state: Option<EngineState>,
    input_state: InputState,
    last_cursor_pos: Option<(f64, f64)>,
    last_frame_inst: std::time::Instant,
}

impl EngineApp {
    pub fn new(title: &str, scene_root: Node, camera: Camera) -> Self {
        Self {
            title: title.to_string(),
            scene_root,
            camera,
            state: None,
            input_state: InputState::default(),
            last_cursor_pos: None,
            last_frame_inst: std::time::Instant::now(),
        }
    }
}

impl ApplicationHandler for EngineApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title(&self.title)
                .with_inner_size(PhysicalSize::new(1920, 1080));
            let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
            let state = pollster::block_on(EngineState::new(
                window,
                self.scene_root.clone(),
                self.camera.clone(),
            ));
            self.state = Some(state);
            self.last_frame_inst = std::time::Instant::now();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let state = match &mut self.state {
            Some(s) => s,
            None => return,
        };

        if window_id != state.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                state.resize(physical_size);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::PhysicalKey::Code(key_code) = event.physical_key {
                    let is_pressed = event.state.is_pressed();
                    match key_code {
                        winit::keyboard::KeyCode::KeyW => self.input_state.w_pressed = is_pressed,
                        winit::keyboard::KeyCode::KeyA => self.input_state.a_pressed = is_pressed,
                        winit::keyboard::KeyCode::KeyS => self.input_state.s_pressed = is_pressed,
                        winit::keyboard::KeyCode::KeyD => self.input_state.d_pressed = is_pressed,
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseInput { state: mouse_state, button, .. } => {
                if button == winit::event::MouseButton::Right {
                    self.input_state.right_mouse_pressed = mouse_state.is_pressed();
                    if !mouse_state.is_pressed() {
                        self.last_cursor_pos = None;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.input_state.right_mouse_pressed {
                    if let Some((last_x, last_y)) = self.last_cursor_pos {
                        let dx = (position.x - last_x) as f32;
                        let dy = (position.y - last_y) as f32;
                        self.input_state.mouse_delta.0 += dx;
                        self.input_state.mouse_delta.1 += dy;
                    }
                    self.last_cursor_pos = Some((position.x, position.y));
                } else {
                    self.last_cursor_pos = None;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.y / 120.0) as f32,
                };
                self.input_state.scroll_delta += scroll;
            }
            WindowEvent::RedrawRequested => {
                let dt = self.last_frame_inst.elapsed().as_secs_f32();
                self.last_frame_inst = std::time::Instant::now();
                
                // Clamp dt to avoid massive spikes
                let dt = dt.min(0.1);

                state.update(dt, &mut self.input_state);
                if let Err(e) = state.render() {
                    log::error!("Render error: {:?}", e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

pub fn run_engine(title: &str, scene_root: Node, camera: Camera) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = EngineApp::new(title, scene_root, camera);
    event_loop.run_app(&mut app)?;
    Ok(())
}
