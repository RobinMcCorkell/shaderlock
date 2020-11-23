#![feature(async_closure)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]

mod callback_utils;
mod graphics;
mod locker;
mod monitor;
mod screengrab;

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::reexports::client as wl;

fn get_shader_file() -> Result<std::path::PathBuf> {
    use rand::seq::IteratorRandom;
    let mut rng = rand::thread_rng();
    let file = glob::glob("shaders/*.frag")
        .expect("Failed to parse shader file glob")
        .choose(&mut rng)
        .context("Failed to randomly pick a shader file")?
        .context("Failed to get the path to the shader")?;

    info!("Chosen shader {}", file.to_string_lossy());
    Ok(file)
}

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    use winit::platform::unix::{EventLoopExtUnix, EventLoopWindowTargetExtUnix};
    let event_loop = winit::event_loop::EventLoop::<()>::new_wayland();
    let display = unsafe {
        wl::Display::from_external_display(event_loop.wayland_display().unwrap() as *mut _)
    };

    let graphics_manager =
        graphics::Manager::new(get_shader_file()?).context("Failed to create graphics manager")?;

    let screengrabber = screengrab::Screengrabber::new(display.clone())
        .context("Failed to create screengrabber")?;

    let mut locker = locker::Locker::new(display.clone()).context("Failed to create locker")?;

    let mut monitor_manager = monitor::Manager::new(screengrabber, graphics_manager);
    for handle in event_loop.available_monitors() {
        monitor_manager
            .add_monitor(&event_loop, handle)
            .await
            .context("Failed to add monitor")?;
    }

    locker.with(move || {
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
    })
}
