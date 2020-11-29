#![feature(async_closure)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]

mod graphics;
mod locker;
mod monitor;
mod screengrab;
mod utils;

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::reexports::client as wl;

const DATADIR: &str = env!("DATADIR");
const SHADER_GLOB: &str = "shaders/*.frag";
const ICON_FILE: &str = "lock-icon.png";

fn get_shader_file() -> Result<std::path::PathBuf> {
    use rand::seq::IteratorRandom;
    let mut rng = rand::thread_rng();
    let file = glob::glob(&format!("{}/{}", DATADIR, SHADER_GLOB))
        .expect("Failed to parse shader file glob")
        .choose(&mut rng)
        .context("Failed to randomly pick a shader file")?
        .context("Failed to get the path to the shader")?;

    info!("Chosen shader {}", file.to_string_lossy());
    Ok(file)
}

fn get_icon_file() -> Result<std::path::PathBuf> {
    Ok(format!("{}/{}", DATADIR, ICON_FILE).into())
}

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();

    use clap::*;
    let app = app_from_crate!()
        .arg(
            Arg::with_name("shader")
                .long("shader")
                .help("Shader applied to the lock screen background")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("icon")
                .long("icon")
                .help("Icon to overlay on the lock screen")
                .takes_value(true),
        );
    let matches = app.get_matches();

    let shader_file = match matches.value_of("shader") {
        Some(s) => std::path::PathBuf::from(s),
        None => get_shader_file()?,
    };
    let icon_file = match matches.value_of("icon") {
        Some(s) => std::path::PathBuf::from(s),
        None => get_icon_file()?,
    };

    use winit::platform::unix::{EventLoopExtUnix, EventLoopWindowTargetExtUnix};
    let event_loop = winit::event_loop::EventLoop::<()>::new_wayland();
    let display = unsafe {
        wl::Display::from_external_display(event_loop.wayland_display().unwrap() as *mut _)
    };

    let graphics_manager = graphics::Manager::new(&shader_file, &icon_file)
        .context("Failed to create graphics manager")?;

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

    locker.with(move |mut lock| {
        sd_notify::notify(true, &[sd_notify::NotifyState::Ready])
            .expect("Failed to notify readiness");
        use winit::event::*;
        use winit::event_loop::ControlFlow;
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::WindowEvent { ref event, .. } => match event {
                    WindowEvent::ReceivedCharacter(c) => {
                        if !c.is_control() {
                            lock.push(*c);
                        }
                    }
                    WindowEvent::KeyboardInput { input, .. } => match input {
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Escape),
                            ..
                        } => {
                            lock.clear();
                        }
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Back),
                            ..
                        } => {
                            lock.pop();
                        }
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Return),
                            ..
                        } => {
                            match lock.authenticate() {
                                Ok(_) => *control_flow = ControlFlow::Exit,
                                Err(e) => warn!("Authentication failed: {}", e),
                            };
                        }
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
