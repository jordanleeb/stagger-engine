use std::sync::Arc;
use winit::window::Window;

use crate::render::debug::{DebugTask, EndMarker};
use crate::render::mesh::{MeshId, MeshStore, Vertex};

/// Per-frame draw call data for one renderable entity.
///
/// Passed to `Renderer::render_frame` in a slice, one entry per
/// renderable entity.
pub struct DrawCall {
    /// The model matrix for this entity.
    pub model_matrix: [[f32; 4]; 4],

    /// The material base color for this entity.
    pub material_color: [f32; 3],

    /// The mesh to draw for this entity.
    pub mesh_id: MeshId,
}

/// One vertex in a debug draw call.
///
/// Used by both the debug line pipeline and the debug transparent
/// mesh pipeline. Color carries an alpha channel for transparency.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugVertex {
    /// Position in world space.
    pub position: [f32; 3],

    /// RGBA color. Alpha controls transparency for debug meshes.
    pub color: [f32; 4],
}

impl DebugVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DebugVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    shader_location: 0,
                    offset: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    shader_location: 1,
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// Byte stride between per-draw slots in the dynamic offset buffer.
///
/// Must be a multiple of min_uniform_buffer_offset_alignment (256 on
/// most hardware). Each slot holds 80 bytes of actual data.
const PER_DRAW_STRIDE: usize = 256;

/// Initial maximum number of entities in the per-draw buffer.
const INITIAL_PER_DRAW_CAPACITY: usize = 1024;

/// Depth texture format used by all pipelines.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Maximum number of debug vertices per type per frame.
const DEBUG_BUFFER_CAPACITY: usize = 65536;

/// Sphere tessellation ring count (latitude).
const SPHERE_RINGS: usize = 16;

/// Sphere tessellation segment count (longitude).
const SPHERE_SEGMENTS: usize = 16;

/// Number of latitude rings per hemisphere in capsule tessellation.
const CAPSULE_RINGS: usize = 8;

/// Number of longitude segments in capsule tessellation.
const CAPSULE_SEGMENTS: usize = 16;

/// Number of triangles around the base of a cone end marker.
const CONE_SEGMENTS: usize = 12;

/// Half-size of the 3D cross drawn at contact points.
const CONTACT_CROSS_HALF_SIZE: f32 = 0.05;

/// Length of the normal arrow drawn at contact points.
const CONTACT_NORMAL_LENGTH: f32 = 0.15;

/// Radius of the hit marker sphere drawn at raycast endpoints.
const RAYCAST_HIT_RADIUS: f32 = 0.04;

/// Owns all wgpu state for a single window surface.
///
/// Created once during application startup and used each frame
/// to submit draw calls to the GPU.
pub struct Renderer {
    /// The wgpu instance, used to create surfaces and adapters.
    /// Stored so it outlives the surface.
    #[allow(dead_code)]
    instance: wgpu::Instance,

    /// The surface tied to the OS window.
    surface: wgpu::Surface<'static>,

    /// The physical GPU and its capabilities.
    #[allow(dead_code)]
    adapter: wgpu::Adapter,

    /// The logical device, used to create GPU resources.
    device: wgpu::Device,

    /// The command queue, used to submit draw calls.
    queue: wgpu::Queue,

    /// The surface configuration, including size and format.
    config: wgpu::SurfaceConfiguration,

    /// The depth texture used for depth testing.
    ///
    /// Stored to keep the texture alive for as long as `depth_view`
    /// references it. Recreated when the window is resized.
    #[allow(dead_code)]
    depth_texture: wgpu::Texture,

    /// The view into the depth texture used as a render attachment.
    depth_view: wgpu::TextureView,

    /// The render pipeline for opaque scene meshes.
    pipeline: wgpu::RenderPipeline,

    /// The render pipeline for debug lines.
    ///
    /// Uses LineList topology with depth testing enabled.
    debug_line_pipeline: wgpu::RenderPipeline,

    /// The render pipeline for transparent debug meshes.
    ///
    /// Uses TriangleList topology with alpha blending and depth write
    /// disabled so shapes do not occlude each other.
    debug_mesh_pipeline: wgpu::RenderPipeline,

    /// The uniform buffer holding the VP matrix for the current frame.
    ///
    /// Written once at the start of each frame. Bound at group(0) binding(0).
    vp_uniform_buffer: wgpu::Buffer,

    /// The bind group layout for group(0): per-frame data.
    vp_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the VP uniform buffer to the shader.
    vp_bind_group: wgpu::BindGroup,

    /// The dynamic offset uniform buffer holding per-draw data for all entities.
    ///
    /// Each entity occupies one 256-byte aligned slot containing its
    /// model matrix (64 bytes) and material color (16 bytes, padded from 12).
    per_draw_buffer: wgpu::Buffer,

    /// The bind group layout for group(1): per-draw data.
    per_draw_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the per-draw buffer to the shader.
    per_draw_bind_group: wgpu::BindGroup,

    /// The maximum number of entities the per-draw buffer can hold.
    per_draw_capacity: usize,

    /// GPU vertex buffer for debug line vertices.
    ///
    /// Overwritten each frame with tessellated line geometry.
    debug_line_buffer: wgpu::Buffer,

    /// GPU vertex buffer for debug transparent mesh vertices.
    ///
    /// Overwritten each frame with tessellated shape geometry.
    debug_mesh_buffer: wgpu::Buffer,

    /// All uploaded scene meshes, owned by the renderer.
    mesh_store: MeshStore,
}

impl Renderer {
    /// Creates a new renderer attached to the given window.
    ///
    /// This is async because wgpu adapter and device requests are
    /// async operations. Use `pollster::block_on` to call this from
    /// synchronous code.
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(Arc::clone(&window)).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let (depth_texture, depth_view) = create_depth_texture(&device, &config);

        // Per-frame uniform buffer: holds the VP matrix.
        let vp_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vp uniform buffer"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Per-draw dynamic offset buffer: holds model matrix and material
        // color for every entity, packed into 256-byte aligned slots.
        let per_draw_capacity = INITIAL_PER_DRAW_CAPACITY;
        let per_draw_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("per draw buffer"),
            size: (per_draw_capacity * PER_DRAW_STRIDE) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Debug vertex buffers: overwritten each frame with tessellated geometry.
        let debug_vertex_size = std::mem::size_of::<DebugVertex>();

        let debug_line_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("debug line buffer"),
            size: (DEBUG_BUFFER_CAPACITY * debug_vertex_size) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let debug_mesh_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("debug mesh buffer"),
            size: (DEBUG_BUFFER_CAPACITY * debug_vertex_size) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // group(0): per-frame data.
        let vp_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vp bind group layout"),
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
            });

        // group(1): per-draw data with a dynamic offset.
        let per_draw_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("per draw bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let vp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp bind group"),
            layout: &vp_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &vp_uniform_buffer,
                    offset: 0,
                    size: None,
                }),
            }],
        });

        // The per-draw bind group points at the start of the buffer.
        // The actual per-entity offset is supplied at draw time.
        let per_draw_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("per draw bind group"),
            layout: &per_draw_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &per_draw_buffer,
                    offset: 0,
                    // 80 bytes: 64 for the model matrix, 16 for the padded color.
                    size: wgpu::BufferSize::new(80),
                }),
            }],
        });

        let shader_source = include_str!("shader.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let debug_shader_source = include_str!("debug_shader.wgsl");
        let debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("debug shader"),
            source: wgpu::ShaderSource::Wgsl(debug_shader_source.into()),
        });

        // Main pipeline layout: VP at group(0), per-draw at group(1).
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[&vp_bind_group_layout, &per_draw_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Debug pipeline layout: VP at group(0) only.
        let debug_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("debug pipeline layout"),
                bind_group_layouts: &[&vp_bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: &"vs_main",
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: &"fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Debug line pipeline: LineList topology, depth test on, opaque.
        let debug_line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug line pipeline"),
            layout: Some(&debug_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &debug_shader,
                entry_point: &"vs_main",
                buffers: &[DebugVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &debug_shader,
                entry_point: &"fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Debug mesh pipeline: TriangleList, alpha blending, depth write off.
        // Rendered last so transparent shapes composite over opaque geometry.
        let debug_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug mesh pipeline"),
            layout: Some(&debug_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &debug_shader,
                entry_point: &"vs_main",
                buffers: &[DebugVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &debug_shader,
                entry_point: &"fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Transparent shapes test depth but do not write it so they
                // do not occlude each other.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self {
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
            depth_texture,
            depth_view,
            pipeline,
            debug_line_pipeline,
            debug_mesh_pipeline,
            vp_uniform_buffer,
            vp_bind_group_layout,
            vp_bind_group,
            per_draw_buffer,
            per_draw_bind_group_layout,
            per_draw_bind_group,
            per_draw_capacity,
            debug_line_buffer,
            debug_mesh_buffer,
            mesh_store: MeshStore::new(),
        }
    }

    /// Resizes the surface to match the new window size.
    ///
    /// Called whenever a WindowEvent::Resized is received.
    /// Does nothing if either dimension is zero, which can happen
    /// when the window is minimized.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);

        // The depth texture must match the surface size.
        let (depth_texture, depth_view) = create_depth_texture(&self.device, &self.config);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
    }

    /// Returns the current surface width divided by height.
    ///
    /// Used by the render system to keep the camera aspect ratio in
    /// sync with the window size.
    pub fn aspect_ratio(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    /// Uploads vertex data to the GPU and returns the assigned `MeshId`.
    ///
    /// Delegates to the internal `MeshStore`. The CPU-side slice is not
    /// retained after this call.
    pub fn upload_mesh(&mut self, vertices: &[Vertex]) -> MeshId {
        self.mesh_store.upload(&self.device, vertices)
    }

    /// Renders one complete frame.
    ///
    /// Writes all per-draw uniforms, tessellates debug tasks, then issues
    /// all draw calls in a single render pass in this order:
    /// opaque scene meshes, debug lines, debug transparent meshes.
    ///
    /// Returns `false` if the surface is lost.
    pub fn render_frame(
        &mut self,
        vp_matrix: [[f32; 4]; 4],
        draw_calls: &[DrawCall],
        debug_tasks: &[DebugTask],
    ) -> bool {
        let draw_count = draw_calls.len().min(self.per_draw_capacity);

        // Write the VP matrix into the per-frame buffer.
        self.queue
            .write_buffer(&self.vp_uniform_buffer, 0, bytemuck::cast_slice(&vp_matrix));

        // Write all per-draw data into the dynamic offset buffer.
        for (i, draw) in draw_calls[..draw_count].iter().enumerate() {
            let offset = (i * PER_DRAW_STRIDE) as u64;

            self.queue.write_buffer(
                &self.per_draw_buffer,
                offset,
                bytemuck::cast_slice(&draw.model_matrix),
            );

            let padded = [
                draw.material_color[0],
                draw.material_color[1],
                draw.material_color[2],
                0.0f32,
            ];

            self.queue.write_buffer(
                &self.per_draw_buffer,
                offset + 64,
                bytemuck::cast_slice(&padded),
            );
        }

        // Tessellate all debug tasks into line and mesh vertex lists.
        let (mut line_verts, mut mesh_verts) = tessellate_debug_tasks(debug_tasks);
        line_verts.truncate(DEBUG_BUFFER_CAPACITY);
        mesh_verts.truncate(DEBUG_BUFFER_CAPACITY);

        let line_vertex_count = line_verts.len() as u32;
        let mesh_vertex_count = mesh_verts.len() as u32;

        if !line_verts.is_empty() {
            self.queue.write_buffer(
                &self.debug_line_buffer,
                0,
                bytemuck::cast_slice(&line_verts),
            );
        }

        if !mesh_verts.is_empty() {
            self.queue.write_buffer(
                &self.debug_mesh_buffer,
                0,
                bytemuck::cast_slice(&mesh_verts),
            );
        }

        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            Err(_) => return false,
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // group(0) is set once and shared by all pipelines in this pass.
            pass.set_bind_group(0, &self.vp_bind_group, &[]);

            // Draw opaque scene meshes.
            pass.set_pipeline(&self.pipeline);
            for (i, draw) in draw_calls[..draw_count].iter().enumerate() {
                let offset = (i * PER_DRAW_STRIDE) as u32;

                if let Some(mesh) = self.mesh_store.get(draw.mesh_id) {
                    pass.set_bind_group(1, &self.per_draw_bind_group, &[offset]);
                    pass.set_vertex_buffer(0, mesh.buffer().slice(..));
                    pass.draw(0..mesh.vertex_count(), 0..1);
                }
            }

            // Draw debug lines.
            if line_vertex_count > 0 {
                pass.set_pipeline(&self.debug_line_pipeline);
                pass.set_vertex_buffer(0, self.debug_line_buffer.slice(..));
                pass.draw(0..line_vertex_count, 0..1);
            }

            // Draw debug transparent meshes last so alpha blending composites
            // correctly over the opaque geometry.
            if mesh_vertex_count > 0 {
                pass.set_pipeline(&self.debug_mesh_pipeline);
                pass.set_vertex_buffer(0, self.debug_mesh_buffer.slice(..));
                pass.draw(0..mesh_vertex_count, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        true
    }
}

/// Creates a depth texture and its default view for the given surface size.
fn create_depth_texture(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Tessellates all debug tasks into line vertices and mesh vertices.
///
/// Returns two vertex lists:
/// - The first is for the line pipeline (LineList topology).
/// - The second is for the transparent mesh pipeline (TriangleList topology).
fn tessellate_debug_tasks(tasks: &[DebugTask]) -> (Vec<DebugVertex>, Vec<DebugVertex>) {
    let mut line_verts = Vec::new();
    let mut mesh_verts = Vec::new();

    for task in tasks {
        match task {
            DebugTask::Line {
                start,
                end,
                color,
                end_marker,
            } => {
                tessellate_line(
                    *start,
                    *end,
                    *color,
                    end_marker.as_ref(),
                    &mut line_verts,
                    &mut mesh_verts,
                );
            }
            DebugTask::Box {
                center,
                half_extents,
                rotation,
                color,
            } => {
                tessellate_box(*center, *half_extents, *rotation, *color, &mut mesh_verts);
            }
            DebugTask::Sphere {
                center,
                radius,
                color,
            } => {
                tessellate_sphere(*center, *radius, *color, &mut mesh_verts);
            }
            DebugTask::Capsule {
                start,
                end,
                radius,
                color,
            } => {
                tessellate_capsule(*start, *end, *radius, *color, &mut mesh_verts);
            }
            DebugTask::Contact {
                position,
                normal,
                color,
            } => {
                tessellate_contact(*position, *normal, *color, &mut line_verts, &mut mesh_verts);
            }
            DebugTask::Raycast {
                origin,
                end,
                color,
                hit,
            } => {
                tessellate_raycast(
                    *origin,
                    *end,
                    *color,
                    *hit,
                    &mut line_verts,
                    &mut mesh_verts,
                );
            }
        }
    }

    (line_verts, mesh_verts)
}

fn tessellate_line(
    start: [f32; 3],
    end: [f32; 3],
    color: [f32; 3],
    end_marker: Option<&EndMarker>,
    line_verts: &mut Vec<DebugVertex>,
    mesh_verts: &mut Vec<DebugVertex>,
) {
    let c = [color[0], color[1], color[2], 1.0];
    line_verts.push(DebugVertex {
        position: start,
        color: c,
    });
    line_verts.push(DebugVertex {
        position: end,
        color: c,
    });

    if let Some(marker) = end_marker {
        let dir = normalize(sub(end, start));
        tessellate_end_marker(end, dir, marker, c, mesh_verts);
    }
}

fn tessellate_end_marker(
    position: [f32; 3],
    direction: [f32; 3],
    marker: &EndMarker,
    color: [f32; 4],
    mesh_verts: &mut Vec<DebugVertex>,
) {
    match marker {
        EndMarker::Cone { length, radius } => {
            let tip = add(position, scale(direction, *length));
            let (right, up) = make_basis(direction);

            for j in 0..CONE_SEGMENTS {
                let phi0 = 2.0 * std::f32::consts::PI * j as f32 / CONE_SEGMENTS as f32;
                let phi1 = 2.0 * std::f32::consts::PI * (j + 1) as f32 / CONE_SEGMENTS as f32;

                let base0 = ring_point(position, right, up, *radius, phi0);
                let base1 = ring_point(position, right, up, *radius, phi1);

                // Side triangle.
                mesh_verts.push(DebugVertex {
                    position: tip,
                    color,
                });
                mesh_verts.push(DebugVertex {
                    position: base0,
                    color,
                });
                mesh_verts.push(DebugVertex {
                    position: base1,
                    color,
                });

                // Base cap triangle.
                mesh_verts.push(DebugVertex { position, color });
                mesh_verts.push(DebugVertex {
                    position: base1,
                    color,
                });
                mesh_verts.push(DebugVertex {
                    position: base0,
                    color,
                });
            }
        }
        EndMarker::Box { half_extents } => {
            tessellate_box(position, *half_extents, [0.0, 0.0, 0.0], color, mesh_verts);
        }
        EndMarker::Sphere { radius } => {
            tessellate_sphere(position, *radius, color, mesh_verts);
        }
    }
}

fn tessellate_box(
    center: [f32; 3],
    half_extents: [f32; 3],
    rotation: [f32; 3],
    color: [f32; 4],
    mesh_verts: &mut Vec<DebugVertex>,
) {
    let [hx, hy, hz] = half_extents;

    // 8 corners in local space, rotated and translated into world space.
    let corners: [[f32; 3]; 8] = [
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ]
    .map(|p| add(rotate_point(p, rotation), center));

    // 6 faces as index quads (a, b, c, d) => triangles (a,b,c) and (a,c,d).
    let faces: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 4, 7, 3],
        [1, 2, 6, 5],
    ];

    for face in &faces {
        let [a, b, c, d] = [
            corners[face[0]],
            corners[face[1]],
            corners[face[2]],
            corners[face[3]],
        ];

        mesh_verts.push(DebugVertex { position: a, color });
        mesh_verts.push(DebugVertex { position: b, color });
        mesh_verts.push(DebugVertex { position: c, color });

        mesh_verts.push(DebugVertex { position: a, color });
        mesh_verts.push(DebugVertex { position: c, color });
        mesh_verts.push(DebugVertex { position: d, color });
    }
}

fn tessellate_sphere(
    center: [f32; 3],
    radius: f32,
    color: [f32; 4],
    mesh_verts: &mut Vec<DebugVertex>,
) {
    for i in 0..SPHERE_RINGS {
        let theta0 = std::f32::consts::PI * i as f32 / SPHERE_RINGS as f32;
        let theta1 = std::f32::consts::PI * (i + 1) as f32 / SPHERE_RINGS as f32;

        for j in 0..SPHERE_SEGMENTS {
            let phi0 = 2.0 * std::f32::consts::PI * j as f32 / SPHERE_SEGMENTS as f32;
            let phi1 = 2.0 * std::f32::consts::PI * (j + 1) as f32 / SPHERE_SEGMENTS as f32;

            let p00 = sphere_point(center, radius, theta0, phi0);
            let p10 = sphere_point(center, radius, theta1, phi0);
            let p11 = sphere_point(center, radius, theta1, phi1);
            let p01 = sphere_point(center, radius, theta0, phi1);

            mesh_verts.push(DebugVertex {
                position: p00,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p10,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p11,
                color,
            });

            mesh_verts.push(DebugVertex {
                position: p00,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p11,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p01,
                color,
            });
        }
    }
}

fn tessellate_capsule(
    start: [f32; 3],
    end: [f32; 3],
    radius: f32,
    color: [f32; 4],
    mesh_verts: &mut Vec<DebugVertex>,
) {
    let axis = normalize(sub(end, start));
    let (right, up) = make_basis(axis);

    let n = CAPSULE_RINGS;

    struct Ring {
        center: [f32; 3],
        radius: f32,
    }

    let mut rings: Vec<Ring> = Vec::with_capacity(2 * n + 2);

    // Start hemisphere: pole at start - axis*radius, equator at start.
    for i in 0..=n {
        let angle = std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        let ring_radius = radius * angle.sin();
        let offset = -(radius * angle.cos());
        rings.push(Ring {
            center: add(start, scale(axis, offset)),
            radius: ring_radius,
        });
    }

    // End hemisphere: equator at end, pole at end + axis*radius.
    // rings[n] (equator at start) and rings[n+1] (equator at end) are
    // adjacent in the ring array and form the cylinder body between them.
    for i in 0..=n {
        let angle = std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        let ring_radius = radius * angle.cos();
        let offset = radius * angle.sin();
        rings.push(Ring {
            center: add(end, scale(axis, offset)),
            radius: ring_radius,
        });
    }

    // Connect adjacent rings with quads.
    for i in 0..rings.len() - 1 {
        for j in 0..CAPSULE_SEGMENTS {
            let phi0 = 2.0 * std::f32::consts::PI * j as f32 / CAPSULE_SEGMENTS as f32;
            let phi1 = 2.0 * std::f32::consts::PI * (j + 1) as f32 / CAPSULE_SEGMENTS as f32;

            let p00 = ring_point(rings[i].center, right, up, rings[i].radius, phi0);
            let p01 = ring_point(rings[i].center, right, up, rings[i].radius, phi1);
            let p10 = ring_point(rings[i + 1].center, right, up, rings[i + 1].radius, phi0);
            let p11 = ring_point(rings[i + 1].center, right, up, rings[i + 1].radius, phi1);

            mesh_verts.push(DebugVertex {
                position: p00,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p10,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p11,
                color,
            });

            mesh_verts.push(DebugVertex {
                position: p00,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p11,
                color,
            });
            mesh_verts.push(DebugVertex {
                position: p01,
                color,
            });
        }
    }
}

fn tessellate_contact(
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
    line_verts: &mut Vec<DebugVertex>,
    mesh_verts: &mut Vec<DebugVertex>,
) {
    let c = [color[0], color[1], color[2], 1.0];
    let s = CONTACT_CROSS_HALF_SIZE;

    // 3D cross: one line pair per axis.
    line_verts.push(DebugVertex {
        position: [position[0] - s, position[1], position[2]],
        color: c,
    });
    line_verts.push(DebugVertex {
        position: [position[0] + s, position[1], position[2]],
        color: c,
    });

    line_verts.push(DebugVertex {
        position: [position[0], position[1] - s, position[2]],
        color: c,
    });
    line_verts.push(DebugVertex {
        position: [position[0], position[1] + s, position[2]],
        color: c,
    });

    line_verts.push(DebugVertex {
        position: [position[0], position[1], position[2] - s],
        color: c,
    });
    line_verts.push(DebugVertex {
        position: [position[0], position[1], position[2] + s],
        color: c,
    });

    // Normal arrow: line to tip then a cone.
    let dir = normalize(normal);
    let tip = add(position, scale(dir, CONTACT_NORMAL_LENGTH));

    line_verts.push(DebugVertex {
        position: position,
        color: c,
    });
    line_verts.push(DebugVertex {
        position: tip,
        color: c,
    });

    tessellate_end_marker(
        tip,
        dir,
        &EndMarker::Cone {
            length: s * 0.8,
            radius: s * 0.4,
        },
        c,
        mesh_verts,
    );
}

fn tessellate_raycast(
    origin: [f32; 3],
    end: [f32; 3],
    color: [f32; 3],
    hit: bool,
    line_verts: &mut Vec<DebugVertex>,
    mesh_verts: &mut Vec<DebugVertex>,
) {
    let c = [color[0], color[1], color[2], 1.0];

    line_verts.push(DebugVertex {
        position: origin,
        color: c,
    });
    line_verts.push(DebugVertex {
        position: end,
        color: c,
    });

    if hit {
        tessellate_sphere(end, RAYCAST_HIT_RADIUS, c, mesh_verts);
    }
}

// Math helpers.

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len < 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    scale(v, 1.0 / len)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Builds a right-handed orthonormal basis from a unit axis vector.
///
/// Returns (right, up) where both are perpendicular to `axis` and to each other.
fn make_basis(axis: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let arbitrary = if axis[0].abs() < 0.9 {
        [1.0_f32, 0.0, 0.0]
    } else {
        [0.0_f32, 1.0, 0.0]
    };

    let right = normalize(cross(axis, arbitrary));
    let up = cross(right, axis);
    (right, up)
}

/// Returns a point on a circle in the plane spanned by `right` and `up`.
fn ring_point(center: [f32; 3], right: [f32; 3], up: [f32; 3], radius: f32, phi: f32) -> [f32; 3] {
    add(
        add(center, scale(right, radius * phi.cos())),
        scale(up, radius * phi.sin()),
    )
}

/// Returns a point on a sphere at the given latitude and longitude.
fn sphere_point(center: [f32; 3], radius: f32, theta: f32, phi: f32) -> [f32; 3] {
    [
        center[0] + radius * theta.sin() * phi.cos(),
        center[1] + radius * theta.cos(),
        center[2] + radius * theta.sin() * phi.sin(),
    ]
}

/// Rotates a point by XYZ Euler angles, matching the convention used by Transform.
fn rotate_point(p: [f32; 3], rotation: [f32; 3]) -> [f32; 3] {
    let (sx, cx) = (rotation[0].sin(), rotation[0].cos());
    let (sy, cy) = (rotation[1].sin(), rotation[1].cos());
    let (sz, cz) = (rotation[2].sin(), rotation[2].cos());

    [
        cy * cz * p[0] - cy * sz * p[1] + sy * p[2],
        (sx * sy * cz + cx * sz) * p[0] + (-sx * sy * sz + cx * cz) * p[1] + (-sx * cy) * p[2],
        (-cx * sy * cz + sx * sz) * p[0] + (cx * sy * sz + sx * cz) * p[1] + cx * cy * p[2],
    ]
}
