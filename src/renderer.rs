use std::sync::Arc;
use winit::window::Window;

/// One vertex in a mesh.
/// 
/// The layout of this struct must match the vertex buffer layout
/// passedd to the pipeline and the @location attributes in the shader.
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
            // The number of bytes between the start of one vertex and
            // the start of the next.
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,

            // step_mode::Vertex means one set of attributes per vertex.
            // The alternative is VertexBufferLayout::Instance for
            // per-instance data.
            step_mode: wgpu::VertexStepMode::Vertex,

            // Each attribute maps to one @location in the shader.
            attributes: &[
                wgpu::VertexAttribute {
                    // @location(0) in the shader: position.
                    shader_location: 0,
                    // Starts at byte 0 of the vertex.
                    offset: 0,
                    // Three f32 values.
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    // @location(1) in the shader: color.
                    shader_location: 1,
                    // Starts after the position field (3 * 4 = 12 bytes in).
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    // Three f32 values.
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

    /// The renderer pipeline describing the shaders and draw settings.
    pipeline: wgpu::RenderPipeline,

    /// The GPU buffer holding the triangle's vertex data.
    vertex_buffer: wgpu::Buffer,

    /// The number of vertices in the vertex buffer.
    vertex_count: u32,

    /// The uniform buffer holding the model matrix for the current draw call.
    uniform_buffer: wgpu::Buffer,

    /// The bind group layout describing what the shader expects at group 0.
    bind_group_layout: wgpu::BindGroupLayout,

    /// The bind group connecting the uniform buffer to the shader.
    bind_group: wgpu::BindGroup,
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

        // The instance is the entry point to wgpu.
        // Backends::PRIMARY selects Vulkan on Linux.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // The surface is the wgpu abstraction over the OS window.
        // The Arc clone keeps the window alive for the surface's lifetime.
        let surface = instance.create_surface(Arc::clone(&window)).unwrap();

        // The adapter represents the physical GPU.
        // RequestAdapterOptions picks the best available GPU that can
        // render to our surface
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        // The device and quque are created from the adapter.
        // The device creates GPU resources; the queue submits commands.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .unwrap();

        // The surface configuration describes the format and size of
        // the images the surface will produce each frame.
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

        // The uniform buffer holds one model matrix (16 floats = 64 bytes).
        // UNIFORM usage tells wgpu this buffer will be used as a uniform.
        // COPY_DST allows the CPU to write into it each frame.
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform buffer"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The bind group layout describes the shape of group(0) in the shader:
        // one uniform buffer at binding 0, visible to the vertex shader.
        let bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("bind group layout"),
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

        // The bind group connects the actual buffer to the layout slot.
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Load the shader source at compile time.
        // include_str! embeds the file contents as a &str in the binary.
        let shader_source = include_str!("shader.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // An empty pipeline layout means the shaders use no external
        // resources such as textures or uniform buffers.
        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("pipeline layout"),
                bind_group_layouts: &[&bind_group_layout],
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

        // Upload the vertex data into a GPU buffer.
        // VERTEX usage tells wgpu this buffer will be used as a vertex buffer.
        // COPY_DST allows data to be written into it from the CPU.
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
            uniform_buffer,
            bind_group_layout,
            bind_group,
        }
    }

    /// Resizes the surface to match the new window size.
    /// 
    /// Called whenever a WindowEvent::Resized is recieved.
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

    /// Renders one frame.
    /// 
    /// Clears the screen to a solid color to confirm the pipeline
    /// is working. Returns early if the surface is lost, which can
    /// happen when the window is minimized or resized mid-frame.
    pub fn render(&mut self, model_matrix: [[f32; 4]; 4]) -> bool {
        // Write the model matrix into the uniform buffer.
        // cast_slice reinterprets the matrix as raw bytes for the GPU.
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&model_matrix),
        );

        // Get the next texture to render into.
        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            Err(_) => return false,
        };

        // A view describes how to access the texture.
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // A command encoder records GPU commands before they are submitted.
        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("render encoder") }
        );

        // A render pass describes what to draw and where to draw it.
        // This one just clears the screen to a dark grey.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
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
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }

        // Submit the recorded commands to the GPU and present the frame.
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        true
    }
}