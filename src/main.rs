mod ecs;
mod render;
mod app;

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = app::App::new();
    event_loop.run_app(&mut app).unwrap();
}

