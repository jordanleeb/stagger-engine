use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::ecs::world::World;
use crate::render::camera::{ActiveCamera, Camera};
use crate::render::debug::DebugDraw;
use crate::render::material::Material;
use crate::render::mesh::{Mesh, Vertex};
use crate::render::renderer::Renderer;
use crate::render::transform::Transform;

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
        let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));

        self.window = Some(window);

        // Register components.
        self.world.register_component::<Transform>();
        self.world.register_component::<Camera>();
        self.world.register_component::<Mesh>();
        self.world.register_component::<Material>();

        // Upload the test triangle mesh.
        let triangle_id = renderer.upload_mesh(&[
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
        ]);

        self.world.insert_resource(renderer);
        self.world.insert_resource(DebugDraw::new());

        // Spawn the camera entity.
        let camera = self.world.spawn();
        self.world.add_component(
            camera,
            Transform {
                position: [0.0, 0.0, 3.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
            },
        );
        self.world
            .add_component(camera, Camera::default_perspective());
        self.world.insert_resource(ActiveCamera::new(camera));

        // Spawn a test entity at the origin with the triangle mesh
        // and a white material so per-vertex colors show through.
        let e = self.world.spawn();
        self.world.add_component(e, Transform::identity());
        self.world.add_component(e, Mesh::new(triangle_id));
        self.world.add_component(e, Material::white());
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
                // Test debug drawing.
                if let Some(debug) = self.world.get_resource_mut::<DebugDraw>() {
                    debug.draw_line(
                        [-1.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [1.0, 1.0, 0.0],
                        Some(crate::render::debug::EndMarker::Cone {
                            length: 0.1,
                            radius: 0.05,
                        }),
                    );
                    debug.draw_box(
                        [0.0, 0.5, 0.0],
                        [0.2, 0.2, 0.2],
                        [0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.3],
                    );
                    debug.draw_sphere([-0.5, 0.0, 0.0], 0.2, [0.0, 0.5, 1.0, 0.3]);
                    debug.draw_capsule(
                        [0.5, -0.3, 0.0],
                        [0.5, 0.3, 0.0],
                        0.1,
                        [1.0, 0.0, 0.5, 0.3],
                    );
                    debug.draw_contact([0.0, -0.3, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
                    debug.draw_raycast([-1.0, -0.5, 0.0], [0.3, -0.5, 0.0], [1.0, 0.5, 0.0], true);
                }

                crate::render::render_system::render_system(&mut self.world);

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}
