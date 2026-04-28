use std::sync::Arc;
use winit::window::Window;

/// One vertex in a mesh.
///
/// The layout of this struct must match the vertex buffer layout
/// passed to the pipeline and the @location attributes in the shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Position in clip space.
    pub position: [f32; 3],

    /// RGB color for this vertex.
    pub color: [f32; 3],
}

impl Vertex {
    /// Returns the wgpu vertex buffer layout for this type.
    ///
    /// This tells the pipeline how to interpret the raw bytes in the
    /// vertex buffer: where each attribute starts and what type it is.
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
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
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
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

    /// The GPU buffer holding the triangle's vertex data.
    vertex_buffer: wgpu::Buffer,

    /// The number of vertices in the vertex buffer.
    vertex_count: u32,

    /// The uniform buffer holding the VP matrix for the current frame.
    ///
    /// Written once per frame before any draw calls.
    /// Bound at group(0) binding(0).
    vp_uniform_buffer: wgpu::Buffer,

    /// The bind group layout for group(0): per-frame data.
    vp_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the VP uniform buffer to the shader.
    vp_bind_group: wgpu::BindGroup,

    /// The uniform buffer holding the model matrix for the current draw call.
    ///
    /// Written once per entity. Bound at group(1) binding(0).
    model_uniform_buffer: wgpu::Buffer,

    /// The bind group layout for group(1): per-draw data.
    model_bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the model uniform buffer to the shader.
    model_bind_group: wgpu::BindGroup,
}

const VERTICES: &[Vertex] = &[
    Vertex { position: [0.0,  0.5, 0.0], color: [1.0, 0.0, 0.0] },
    Vertex { position: [-0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0] },
    Vertex { position: [0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0] },
];

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
        // Written once at the start of each frame.
        let vp_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vp uniform buffer"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Per-draw uniform buffer: holds the model matrix.
        // Written once per entity draw call.
        let model_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model uniform buffer"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // group(0): per-frame data. One uniform buffer at binding 0.
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

        // group(1): per-draw data. One uniform buffer at binding 0.
        let model_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("model bind group layout"),
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

        let vp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp bind group"),
            layout: &vp_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: vp_uniform_buffer.as_entire_binding(),
            }],
        });

        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model bind group"),
            layout: &model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: model_uniform_buffer.as_entire_binding(),
            }],
        });

        let shader_source = include_str!("shader.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // The pipeline layout lists both bind group layouts in group order.
        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("pipeline layout"),
                bind_group_layouts: &[&vp_bind_group_layout, &model_bind_group_layout],
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

        use wgpu::util::DeviceExt;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertex buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let vertex_count = VERTICES.len() as u32;

        Self {
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
            pipeline,
            vertex_buffer,
            vertex_count,
            vp_uniform_buffer,
            vp_bind_group_layout,
            vp_bind_group,
            model_uniform_buffer,
            model_bind_group_layout,
            model_bind_group,
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

    /// Writes the VP matrix into the per-frame uniform buffer.
    ///
    /// Call this once per frame before any draw calls.
    pub fn set_vp_matrix(&mut self, vp_matrix: [[f32; 4]; 4]) {
        self.queue.write_buffer(
            &self.vp_uniform_buffer,
            0,
            bytemuck::cast_slice(&vp_matrix),
        );
    }

    /// Renders one frame.
    ///
    /// Clears the screen, then draws one mesh per model matrix supplied.
    /// Returns early if the surface is lost, which can happen when the
    /// window is minimized or resized mid-frame.
    pub fn render(&mut self, model_matrix: [[f32; 4]; 4]) -> bool {
        self.queue.write_buffer(
            &self.model_uniform_buffer,
            0,
            bytemuck::cast_slice(&model_matrix),
        );

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
            pass.set_bind_group(1, &self.model_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        true
    }
}