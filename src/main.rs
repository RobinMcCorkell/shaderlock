#![feature(unboxed_closures)]
#![feature(async_closure)]
#![feature(result_flattening)]
#![feature(fn_traits)]
#![feature(tuple_trait)]

mod async_winit;
mod graphics;
mod locker;
mod monitor;
mod screengrab;
mod utils;

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::reexports::client as wl;

use clap::Parser;
use tokio::task::LocalSet;
use wgpu::rwh::{HasDisplayHandle, RawDisplayHandle};

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

struct Monitor {
    handle: winit::monitor::MonitorHandle,
    window: winit::window::Window,
}

enum Event {
    Init {
        monitors: Vec<Monitor>,
        display: wl::Display,
    },
    WindowEvent(winit::window::WindowId, winit::event::WindowEvent),
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    LocalSet::new()
        .run_until(async move {
            let args = Args::parse();

            let shader_file = match args.shader_file {
                Some(s) => std::path::PathBuf::from(s),
                None => get_shader_file()?,
            };
            let icon_file = std::path::PathBuf::from(args.icon_file);

            let mut events = async_winit::run(
                || {
                    use winit::platform::wayland::EventLoopBuilderExtWayland;
                    winit::event_loop::EventLoop::builder()
                        .with_wayland()
                        .with_any_thread(true)
                        .build()
                        .unwrap()
                },
                |event, event_loop| match event {
                    winit::event::Event::Resumed => {
                        debug!("Resumed");
                        use winit::window::*;
                        let monitors = event_loop
                            .available_monitors()
                            .map(|handle| {
                                let attrs = Window::default_attributes().with_fullscreen(Some(
                                    Fullscreen::Borderless(Some(handle.clone())),
                                ));
                                let window = event_loop.create_window(attrs).unwrap();
                                window.set_cursor_visible(false);

                                Monitor { handle, window }
                            })
                            .collect();

                        let wdh = match event_loop.display_handle().unwrap().as_raw() {
                            RawDisplayHandle::Wayland(wdh) => wdh,
                            _ => panic!(),
                        };
                        let display = unsafe {
                            wl::Display::from_external_display(wdh.display.as_ptr() as *mut _)
                        };

                        Some(Event::Init { monitors, display })
                    }
                    winit::event::Event::WindowEvent { window_id, event } => {
                        info!("window event: {:?}", event);
                        Some(Event::WindowEvent(window_id, event))
                    }
                    _ => None,
                },
            );

            let (monitors, display) = match events.recv().await.context("Getting init message")? {
                Event::Init { monitors, display } => (monitors, display),
                _ => panic!("Unexpected event"),
            };

            let graphics_manager = graphics::Manager::new(&shader_file, &icon_file)
                .context("Failed to create graphics manager")?;

            let screengrabber = screengrab::Screengrabber::new(display.clone())
                .await
                .context("Failed to create screengrabber")?;

            let mut locker =
                locker::Locker::new(display.clone()).context("Failed to create locker")?;

            let mut monitor_manager = monitor::Manager::new(screengrabber, graphics_manager);
            let skip_auth = args.skip_auth;

            for Monitor { handle, window } in monitors {
                monitor_manager
                    .add_monitor(handle, window)
                    .await
                    .context("Failed to add monitor")?;
            }

            locker
                .with(async move |mut lock| {
                    sd_notify::notify(true, &[sd_notify::NotifyState::Ready])
                        .expect("Failed to notify readiness");

                    loop {
                        use winit::keyboard::*;
                        debug!("awaiting next event");
                        let event = events.recv().await.context("Getting event")?;
                        match event {
                            Event::Init { .. } => panic!("Unexpected init event"),
                            Event::WindowEvent(
                                _,
                                winit::event::WindowEvent::KeyboardInput {
                                    event:
                                        winit::event::KeyEvent {
                                            state: winit::event::ElementState::Pressed,
                                            ref logical_key,
                                            ..
                                        },
                                    ..
                                },
                            ) => match logical_key {
                                Key::Character(ss) => {
                                    info!("got character: {}", ss);
                                    for c in ss.chars() {
                                        lock.push(c);
                                    }
                                }
                                Key::Named(NamedKey::Escape) => {
                                    lock.clear();
                                }
                                Key::Named(NamedKey::Backspace) => {
                                    lock.pop();
                                }
                                Key::Named(NamedKey::Enter) => {
                                    if skip_auth {
                                        warn!("Skipping authentication, exiting");
                                        events.close();
                                        return Ok(());
                                    } else {
                                        match lock.authenticate() {
                                            Result::Ok(_) => {
                                                events.close();
                                                return Ok(());
                                            }
                                            Result::Err(e) => warn!("Authentication failed: {}", e),
                                        };
                                    }
                                }
                                _ => {}
                            },
                            Event::WindowEvent(id, event) => {
                                monitor_manager
                                    .handle_event(id, event)
                                    .expect("Handle event");
                            }
                        }
                    }
                })
                .await
                .flatten()
        })
        .await
}
