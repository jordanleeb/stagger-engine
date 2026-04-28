use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::render::renderer::Renderer;
use crate::ecs::world::World;

/// Owns the window and renderer, and responds to OS events.
pub struct App {
    /// The OS window.
    ///
    /// Stored in an Option because the window does not exist until
    /// resumed() is called. Arc is required because wgpu's surface
    /// needs to share ownership of the window.
    window: Option<Arc<Window>>,

    /// The ECS world holding all entities, components, and resources.
    ///
    /// The renderer is stored inside the world as a resource so that
    /// systems can access it without borrowing the app directly.
    world: World,
}

impl App {
    /// Creates a new app with no window or renderer yet.
    pub fn new() -> Self {
        Self {
            window: None,
            world: World::new(),
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

        // Register components.
        self.world.register_component::<crate::render::transform::Transform>();
        self.world.register_component::<crate::render::camera::Camera>();

        // Insert the renderer as a resource so systems can access it.
        self.world.insert_resource(renderer);

        // Spawn the camera entity with a default perspective projection
        // and a transform positioned 3 units back from the origin.
        let camera = self.world.spawn();
        self.world.add_component(camera, crate::render::transform::Transform {
            position: [0.0, 0.0, 3.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        });
        self.world.add_component(camera, crate::render::camera::Camera::default_perspective());
        self.world.insert_resource(crate::render::camera::ActiveCamera::new(camera));

        // Spawn a test entity at the origin.
        let e = self.world.spawn();
        self.world.add_component(e, crate::render::transform::Transform::identity());
    }

    /// Called for every event that belongs to a window.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = self.world.get_resource_mut::<Renderer>() {
                    renderer.resize(new_size);
                }
            }

            WindowEvent::RedrawRequested => {
                crate::render::render_system::render_system(&mut self.world);

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}