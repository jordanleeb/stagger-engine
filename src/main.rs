mod archetype;
mod column;
mod component;
mod entity;
mod location;
mod query;
mod system;
mod world;
mod app;
mod renderer;
mod transform;
mod render_system;

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = app::App::new();
    event_loop.run_app(&mut app).unwrap();
}
