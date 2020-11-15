#![feature(async_closure)]

mod graphics;
mod monitor;
mod screengrab;

use log::{debug, error, info, warn};

#[async_std::main]
async fn main() {
    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::new();
    let mut monitor_manager = monitor::Manager::new();
    for handle in event_loop.available_monitors() {
        monitor_manager.add_monitor(&event_loop, handle).await;
    }

    use winit::event::*;
    use winit::event_loop::ControlFlow;
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::KeyboardInput { input, .. } => match input {
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Escape),
                        ..
                    } => *control_flow = ControlFlow::Exit,
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Q),
                        ..
                    } => *control_flow = ControlFlow::Exit,
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
        monitor_manager.handle_event(event);
    });
}
