use std::collections::HashMap;

use anyhow::*;
use either::Either;
use futures::StreamExt;
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::protocol::wl_output::WlOutput;
use sctk::reexports::client::Proxy;
use sctk::session_lock::*;
use shaderlock::authenticator::{
    Authenticator, AuthenticatorBackend, NullAuthenticatorBackend, PamAuthenticatorBackend,
};
use shaderlock::graphics::RenderContext;
use shaderlock::screencopy::ScreencopyBuffer;
use shaderlock::window_manager::ExitSync;

use clap::Parser;
use sctk::seat::keyboard::{KeyEvent, Keysym};
use shaderlock::screencopy::ScreencopyHandler;
use shaderlock::window_manager::WindowManager;
use shaderlock::window_manager::{Event, Window};
use tokio::task::LocalSet;

const DATADIR: &str = env!("DATADIR");
const SHADER_GLOB: &str = "shaders/*.frag";
const ICON_FILE: &str = "lock-icon.png";
const FREEZE_AFTER_INACTIVITY: std::time::Duration = std::time::Duration::from_secs(10);
const FADE_BEFORE_FREEZE: std::time::Duration = std::time::Duration::from_secs(5);

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

            let graphics_manager = shaderlock::graphics::Manager::new(&shader_file, &icon_file)
                .context("Failed to create graphics manager")?;

            let mut authenticator_backend = if args.skip_auth {
                Either::Left(NullAuthenticatorBackend::new())
            } else {
                Either::Right(PamAuthenticatorBackend::new()?)
            };

            let mut auth = Authenticator::new(
                authenticator_backend
                    .as_mut()
                    .map_either(
                        |v| v as &mut dyn AuthenticatorBackend,
                        |v| v as &mut dyn AuthenticatorBackend,
                    )
                    .into_inner(),
            )?;

            let mut wm = WindowManager::new()?;

            let mut keyboard = None;
            let init_time = std::time::Instant::now();
            let mut last_keypress_time = std::time::Instant::now();

            let mut output_by_surface = HashMap::<ObjectId, WlOutput>::new();
            let mut frame_by_output = HashMap::<ObjectId, ScreencopyBuffer>::new();
            let mut lock_surface_by_surface = HashMap::<ObjectId, SessionLockSurface>::new();
            let mut graphics_by_surface = HashMap::<ObjectId, shaderlock::graphics::State>::new();

            wm.run(|conn, qh, mut state, events| async move {
                let outputs: Vec<_> = state.access(|s| s.output_state.outputs().collect());
                for output in outputs {
                    let frame_handle = state
                        .access(|s| {
                            debug!("capture frame on output: {:?}", output);
                            let res = s.screencopy_state().capture_output(&output, qh);
                            conn.flush()?;
                            res
                        })?
                        .await??;
                    debug!("capture complete, getting buffer data");
                    let frame = state.access(|s| s.get_buffer_data(frame_handle));

                    frame_by_output.insert(output.id(), frame);
                }

                let session_lock = state.access(|s| s.session_lock_state.lock(qh))?;
                sd_notify::notify(true, &[sd_notify::NotifyState::Ready])
                    .context("Failed to notify readiness")?;

                loop {
                    debug!("awaiting events");
                    let event = events.next().await.context("events stream was closed")?;
                    debug!("got event: {:?}", event);
                    match event {
                        Event::NewOutput(output) => {
                            let lock_surface = state.access(|s| {
                                let surface = s.compositor_state.create_surface(qh);
                                session_lock.create_lock_surface(surface, &output, qh)
                            });
                            conn.flush()?;
                            debug!("created lock surface: {:?}", lock_surface);
                            output_by_surface.insert(lock_surface.wl_surface().id(), output);
                            lock_surface_by_surface
                                .insert(lock_surface.wl_surface().id(), lock_surface);
                        }
                        Event::SessionLocked => {}
                        Event::SessionLockFinished => {
                            error!("session lock failed!");
                            bail!("session lock failed!");
                        }
                        Event::ConfigureLockSurface(lock_surface, (width, height)) => {
                            let surface = lock_surface.wl_surface();
                            let output = output_by_surface.get(&surface.id()).unwrap();
                            let frame = frame_by_output.remove(&output.id()).unwrap();

                            let window = Window {
                                display: conn.display(),
                                surface: surface.clone(),
                            };

                            debug!("initializing graphics on output: {:?}", output);
                            let graphics = graphics_manager
                                .init_window(window, frame, (width, height))
                                .await?;
                            debug!("graphics initialized");

                            graphics_by_surface.insert(surface.id(), graphics);

                            state.access(|s| s.queue_redraw(lock_surface.wl_surface().clone()));
                        }
                        Event::RedrawRequested(surface) => {
                            debug!("redraw requested on surface: {:?}", surface);
                            let graphics = graphics_by_surface.get_mut(&surface.id()).unwrap();
                            let ctx = RenderContext {
                                elapsed: init_time.elapsed(),
                                fade_amount: (last_keypress_time.elapsed() + FADE_BEFORE_FREEZE)
                                    .saturating_sub(FREEZE_AFTER_INACTIVITY)
                                    .as_secs_f32()
                                    / FADE_BEFORE_FREEZE.as_secs_f32(),
                            };
                            let frame = graphics.render(ctx);
                            if last_keypress_time.elapsed() < FREEZE_AFTER_INACTIVITY {
                                debug!("requesting next frame");
                                surface.frame(qh, surface.clone());
                                conn.flush()?;
                            }
                            debug!("scheduling present of current frame");
                            frame.present();
                        }
                        Event::NewSeatCapability(seat, capability) => {
                            if capability == sctk::seat::Capability::Keyboard {
                                debug!("configure keyboard");
                                keyboard.replace(
                                    state.access(|s| s.seat_state.get_keyboard(qh, &seat, None))?,
                                );
                            }
                        }
                        Event::RemoveSeatCapability(_seat, capability) => {
                            if capability == sctk::seat::Capability::Keyboard {
                                debug!("deconfigure keyboard");
                                keyboard.take();
                            }
                        }
                        Event::KeyPressed(key_event) => {
                            last_keypress_time = std::time::Instant::now();
                            match key_event {
                                KeyEvent {
                                    keysym: Keysym::Escape,
                                    ..
                                } => {
                                    auth.clear();
                                }
                                KeyEvent {
                                    keysym: Keysym::BackSpace | Keysym::Delete | Keysym::KP_Delete,
                                    ..
                                } => {
                                    auth.pop();
                                }
                                KeyEvent {
                                    keysym: Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter,
                                    ..
                                } => {
                                    match auth.authenticate() {
                                        Result::Ok(_) => {
                                            session_lock.unlock();
                                            conn.display().sync(qh, ExitSync);
                                            conn.flush()?;
                                        }
                                        Result::Err(e) => warn!("Authentication failed: {}", e),
                                    };
                                }
                                KeyEvent {
                                    utf8: Some(text), ..
                                } => {
                                    debug!("got input: {}", text);
                                    for c in text.chars() {
                                        auth.push(c);
                                    }
                                }
                                KeyEvent { keysym, .. } => {
                                    debug!("unknown key pressed: {:?}", keysym);
                                }
                            };
                        }
                        Event::ExitSync => {
                            info!("exiting");
                            return Ok(());
                        }
                    };
                }
            })
            .await
        })
        .await
}
