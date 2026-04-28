use std::collections::HashMap;
use std::sync::Arc;
use winit::window::Window;

use crate::render::mesh::{GpuMesh, Mesh, MeshId, MeshStore, Vertex};

/// One draw call's worth of per-entity data.
///
/// Passed to `Renderer::render_frame` in a slice, one entry per
/// renderable entity. The renderer writes all entries into the
/// dynamic offset uniform buffer before issuing any draw calls.
pub struct DrawCall {
    /// The model matrix for this entity.
    pub model_matrix: [[f32; 4]; 4],

    /// The material base color for this entity.
    pub material_color: [f32; 3],

    /// The mesh to draw for this entity.
    pub mesh_id: MeshId,
}

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

    /// The render pipeline describing the shaders and draw settings.
    pipeline: wgpu::RenderPipeline,

    /// The uniform buffer holding the VP matrix for the current frame.
    ///
    /// Written once at the start of each frame before any draw calls.
    /// Bound at group(0) binding(0).
    vp_uniform_buffer: wgpu::Buffer,

    /// The bind group layout for group(0): per-frame data.
    vp_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the VP uniform buffer to the shader.
    vp_bind_group: wgpu::BindGroup,

    /// The dynamic offset uniform buffer holding per-draw data for all entities.
    ///
    /// Each entity occupies one 256-byte aligned slot containing its
    /// model matrix (64 bytes) and material color (16 bytes, padded from 12).
    /// The GPU reads from the correct slot via a dynamic offset passed to
    /// set_bind_group each draw call.
    per_draw_buffer: wgpu::Buffer,

    /// The bind group layout for group(1): per-draw data.
    per_draw_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the per-draw buffer to the shader.
    per_draw_bind_group: wgpu::BindGroup,

    /// The maximum number of entities the per-draw buffer can hold.
    ///
    /// Determines the size of `per_draw_buffer` at creation time.
    per_draw_capacity: usize,

    /// All uploaded meshes owned by the renderer.
    mesh_store: MeshStore,
}

/// Byte stride between per-draw slots in the dynamic offset buffer.
///
/// Must be a multiple of `min_uniform_buffer_offset_alignment` (256 on
/// most hardware). Each slot holds 80 bytes of actual data with 176 bytes
/// of padding.
const PER_DRAW_STRIDE: usize = 256;

/// Initial capacity of the per-draw buffer in number of entities.
const INITIAL_PER_DRAW_CAPACITY: usize = 1024;

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

        // group(0): per-frame data.
        let vp_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
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
            }
        );

        // group(1): per-draw data with a dynamic offset.
        let per_draw_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
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
            }
        );

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
        // The actual per-entity offset is supplied at draw time via set_bind_group.
        let per_draw_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("per draw bind group"),
            layout: &per_draw_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &per_draw_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(PER_DRAW_STRIDE as u64),
                }),
            }],
        });

        let shader_source = include_str!("shader.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("pipeline layout"),
                bind_group_layouts: &[&vp_bind_group_layout, &per_draw_bind_group_layout],
                push_constant_ranges: &[],
            }
        );

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
            depth_stencil: None,
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
            pipeline,
            vp_uniform_buffer,
            vp_bind_group_layout,
            vp_bind_group,
            per_draw_buffer,
            per_draw_bind_group_layout,
            per_draw_bind_group,
            per_draw_capacity,
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

    /// Renders one complete frame from a list of draw calls.
    ///
    /// Writes the VP matrix and all per-draw data to the GPU once,
    /// then issues one draw call per entry using dynamic offsets to
    /// isolate each entity's uniform data.
    ///
    /// Returns `false` if the surface is lost.
    pub fn render_frame(
        &mut self,
        vp_matrix: [[f32; 4]; 4],
        draw_calls: &[DrawCall],
    ) -> bool {
        // Write the VP matrix into the per-frame buffer.
        self.queue.write_buffer(
            &self.vp_uniform_buffer,
            0,
            bytemuck::cast_slice(&vp_matrix),
        );

        // Pack all per-draw data into the dynamic offset buffer.
        //
        // Each slot is PER_DRAW_STRIDE bytes. The model matrix occupies
        // the first 64 bytes, the padded material color the next 16.
        for (i, draw) in draw_calls.iter().enumerate() {
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

        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            Err(_) => return false,
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("render encoder") }
        );

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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.vp_bind_group, &[]);

            for (i, draw) in draw_calls.iter().enumerate() {
                let offset = (i * PER_DRAW_STRIDE) as u32;

                if let Some(mesh) = self.mesh_store.get(draw.mesh_id) {
                    pass.set_bind_group(1, &self.per_draw_bind_group, &[offset]);
                    pass.set_vertex_buffer(0, mesh.buffer().slice(..));
                    pass.draw(0..mesh.vertex_count(), 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        true
    }
}