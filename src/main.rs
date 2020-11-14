#![feature(async_closure)]
mod graphics;

use log::{debug, error, info, warn};
use std::collections::HashMap;

struct State {
    window: winit::window::Window,
    graphics: graphics::State,
}

#[async_std::main]
async fn main() {
    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::new();
    use futures::stream::StreamExt;
    let p_event_loop = &event_loop;
    let mut state: HashMap<_, _> = futures::stream::iter(event_loop.available_monitors())
        .then(async move |handle| {
            use winit::window::*;
            debug!("Creating window on {}", handle.name().unwrap());
            let window = WindowBuilder::new()
                .with_fullscreen(Some(Fullscreen::Borderless(Some(handle))))
                .build(p_event_loop)
                .unwrap();
            let graphics = graphics::State::new(&window).await;
            (window.id(), State { window, graphics })
        })
        .collect()
        .await;

    use winit::event::*;
    use winit::event_loop::ControlFlow;
    event_loop.run(move |event, _, control_flow| match event {
        Event::WindowEvent {
            ref event,
            window_id,
        } if state.contains_key(&window_id) => match event {
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
            WindowEvent::Resized(physical_size) => {
                let State {
                    ref mut graphics, ..
                } = state.get_mut(&window_id).unwrap();
                graphics.resize(*physical_size);
            }
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                let State {
                    ref mut graphics, ..
                } = state.get_mut(&window_id).unwrap();
                // new_inner_size is &&mut so we have to dereference it twice
                graphics.resize(**new_inner_size);
            }
            _ => {}
        },
        Event::RedrawRequested(window_id) if state.contains_key(&window_id) => {
            let State {
                ref mut graphics, ..
            } = state.get_mut(&window_id).unwrap();
            graphics.update();
            graphics.render();
        }
        Event::MainEventsCleared => {
            for State { window, .. } in state.values() {
                window.request_redraw();
            }
        }
        _ => {}
    });
}
