use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::renderer::Renderer;

/// Owns the window and renderer, and responds to OS events.
pub struct App {
    /// The OS window.
    /// 
    /// Stored in an Option because the window does not exist until
    /// resumed() is called. Arc is required because wgpu's surface
    /// needed to share ownership of the window.
    window: Option<Arc<Window>>,

    /// The wgpu renderer attached to the window.
    /// 
    /// Stored in an Option for the same reason as window: it cannot
    /// be created until the window exists.
    renderer: Option<Renderer>,
}

impl App {
    /// Creates a new app with no window or renderer yet.
    pub fn new() -> Self {
        Self {
            window: None,
            renderer: None,
        }
    }
}

impl ApplicationHandler for App {
    /// Called when the OS signals the app is ready to create a window.
    /// 
    /// On Linux desktop this is called once at startup.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes().with_title("Stagger");

        let window = Arc::new(event_loop.create_window(attributes).unwrap());

        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    /// Called for every event that belongs to a window.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(new_size);
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.render();
                }

                // Request another frame immediately to keep the loop running.
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}