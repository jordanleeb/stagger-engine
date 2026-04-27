use std::sync::Arc;
use winit::window::Window;

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
}

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

        Self {
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
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
    pub fn render(&mut self) -> bool {
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
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        }

        // Submit the recorded commands to the GPU and present the frame.
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        true
    }
}