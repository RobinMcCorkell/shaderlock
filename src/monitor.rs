use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use std::collections::HashMap;
use winit::event_loop::EventLoop;
use winit::monitor::*;
use winit::window::*;

use crate::screengrab::Screengrabber;

pub struct State {
    window: Window,
    graphics: crate::graphics::State,
}

pub struct Manager {
    screengrabber: Screengrabber,
    graphics: crate::graphics::Manager,
    state: HashMap<WindowId, State>,
    init_time: std::time::Instant,
}

impl Manager {
    pub fn new(screengrabber: Screengrabber, graphics: crate::graphics::Manager) -> Self {
        Self {
            screengrabber,
            graphics,
            state: HashMap::new(),
            init_time: std::time::Instant::now(),
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
            .grab_screen(handle.native_id())
            .await
            .context("Failed to grab screen")?;

        debug!("Creating window on {:?}", handle.name());
        let window = WindowBuilder::new()
            .with_fullscreen(Some(Fullscreen::Borderless(Some(handle.clone()))))
            .build(event_loop)
            .context("Failed to build window")?;

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
                _ => {}
            },
            Event::RedrawRequested(window_id) if self.state.contains_key(&window_id) => {
                let State {
                    ref mut graphics, ..
                } = self
                    .state
                    .get_mut(&window_id)
                    .expect("Missing window ID in state");
                let time = self.init_time.elapsed().as_secs_f32();
                graphics.render(time);
            }
            Event::MainEventsCleared => {
                for State { window, .. } in self.state.values() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
