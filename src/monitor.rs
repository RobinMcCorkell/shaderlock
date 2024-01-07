use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use actix::prelude::*;

use std::collections::HashMap;
use winit::event_loop::EventLoop;
use winit::monitor::*;
use winit::window::*;

use crate::graphics::RenderContext;
use crate::screengrab::Screengrabber;

pub struct State {
    window: Window,
    graphics: crate::graphics::State,
}

pub struct Manager {
    screengrabber: Addr<Screengrabber>,
    graphics: crate::graphics::Manager,
    state: HashMap<WindowId, State>,
    init_time: std::time::Instant,
    last_keypress_time: std::time::Instant,
    frozen: bool,
    freeze_fade: std::time::Duration,
    freeze_timeout: std::time::Duration,
}

impl Manager {
    pub fn new(
        screengrabber: Addr<Screengrabber>,
        graphics: crate::graphics::Manager,
        freeze_fade: std::time::Duration,
        freeze_timeout: std::time::Duration,
    ) -> Self {
        Self {
            screengrabber,
            graphics,
            freeze_fade,
            freeze_timeout,
            state: HashMap::new(),
            init_time: std::time::Instant::now(),
            last_keypress_time: std::time::Instant::now(),
            frozen: false,
        }
    }

    pub async fn add_monitor<EventT>(
        &mut self,
        event_loop: &EventLoop<EventT>,
        handle: MonitorHandle,
    ) -> Result<()> {
        use winit::platform::unix::MonitorHandleExtUnix;
        debug!("Grabbing screen on {:?}", handle.name());
        let frame = self
            .screengrabber
            .send(crate::screengrab::GrabScreen {
                output_id: handle.native_id(),
            })
            .await
            .context("Failed to send grab screen request")?
            .context("Failed to grab screen")?;

        debug!("Creating window on {:?}", handle.name());
        let window = WindowBuilder::new()
            .with_fullscreen(Some(Fullscreen::Borderless(Some(handle.clone()))))
            .build(event_loop)
            .context("Failed to build window")?;

        window.set_cursor_visible(false);

        debug!("Initialising graphics on {:?}", window.id());
        let graphics = self
            .graphics
            .init_window(&window, frame)
            .await
            .context("Failed to create graphics context")?;

        debug!("Added window {:?} on {:?}", window.id(), handle.name());
        self.state.insert(window.id(), State { window, graphics });

        Ok(())
    }

    pub fn handle_event<EventT>(&mut self, event: winit::event::Event<EventT>) {
        use winit::event::*;
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if self.state.contains_key(&window_id) => match event {
                WindowEvent::Resized(physical_size) => {
                    let State {
                        ref mut graphics, ..
                    } = self
                        .state
                        .get_mut(&window_id)
                        .expect("Missing window ID in state");
                    graphics.resize(*physical_size);
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    let State {
                        ref mut graphics, ..
                    } = self
                        .state
                        .get_mut(&window_id)
                        .expect("Missing window ID in state");
                    // new_inner_size is &&mut so we have to dereference it twice
                    graphics.resize(**new_inner_size);
                }
                WindowEvent::KeyboardInput { .. } => {
                    self.last_keypress_time = std::time::Instant::now();
                    self.frozen = false;
                }
                _ => {}
            },
            Event::RedrawRequested(window_id) if self.state.contains_key(&window_id) => {
                let State {
                    ref mut graphics, ..
                } = self
                    .state
                    .get_mut(&window_id)
                    .expect("Missing window ID in state");
                let ctx = RenderContext {
                    elapsed: self.init_time.elapsed(),
                    fade_amount: (self.last_keypress_time.elapsed() + self.freeze_fade)
                        .saturating_sub(self.freeze_timeout)
                        .as_secs_f32()
                        / self.freeze_fade.as_secs_f32(),
                };
                graphics.render(ctx);
                if self.last_keypress_time.elapsed() > self.freeze_timeout {
                    self.frozen = true;
                }
            }
            Event::MainEventsCleared => {
                if !self.frozen {
                    for State { window, .. } in self.state.values() {
                        window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}
