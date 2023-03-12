#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(tuple_trait)]

mod graphics;
mod locker;
mod monitor;
mod screengrab;
mod utils;

use actix::Actor;
use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::reexports::client as wl;

use clap::Parser;

const DATADIR: &str = env!("DATADIR");
const SHADER_GLOB: &str = "shaders/*.frag";
const ICON_FILE: &str = "lock-icon.png";

#[derive(Parser)]
#[command(version, author, about)]
struct Args {
    /// Authentication always succeeds, for testing.
    #[arg(long, default_value_t = false)]
    skip_auth: bool,

    /// Shader applied to the lock screen background.
    #[arg(long, short)]
    shader_file: Option<String>,

    /// Icon to overlay on the lock screen.
    #[arg(long, default_value_t = format!("{}/{}", DATADIR, ICON_FILE))]
    icon_file: String,
}

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

#[actix_rt::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let shader_file = match args.shader_file {
        Some(s) => std::path::PathBuf::from(s),
        None => get_shader_file()?,
    };
    let icon_file = std::path::PathBuf::from(args.icon_file);

    use winit::platform::unix::{EventLoopBuilderExtUnix, EventLoopWindowTargetExtUnix};
    let event_loop = winit::event_loop::EventLoopBuilder::new()
        .with_wayland()
        .build();
    let display = unsafe {
        wl::Display::from_external_display(event_loop.wayland_display().unwrap() as *mut _)
    };

    let graphics_manager = graphics::Manager::new(&shader_file, &icon_file)
        .context("Failed to create graphics manager")?;

    let screengrabber = screengrab::Screengrabber::new(display.clone())
        .context("Failed to create screengrabber")?
        .start();

    let mut locker = locker::Locker::new(display.clone()).context("Failed to create locker")?;

    let mut monitor_manager = monitor::Manager::new(screengrabber, graphics_manager);
    for handle in event_loop.available_monitors() {
        monitor_manager
            .add_monitor(&event_loop, handle)
            .await
            .context("Failed to add monitor")?;
    }

    let skip_auth = args.skip_auth;

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
                            if skip_auth {
                                warn!("Skipping authentication, exiting");
                                *control_flow = ControlFlow::Exit;
                            } else {
                                match lock.authenticate() {
                                    Result::Ok(_) => *control_flow = ControlFlow::Exit,
                                    Result::Err(e) => warn!("Authentication failed: {}", e),
                                };
                            }
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
