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
    state: HashMap<WindowId, State>,
    init_time: std::time::Instant,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            screengrabber: Screengrabber::new(),
            state: HashMap::new(),
            init_time: std::time::Instant::now(),
        }
    }

    pub async fn add_monitor<EventT>(
        &mut self,
        event_loop: &EventLoop<EventT>,
        handle: MonitorHandle,
    ) {
        use winit::platform::unix::MonitorHandleExtUnix;
        let frame = self.screengrabber.grab_screen(handle.native_id());

        debug!("Creating window on {}", handle.name().unwrap());
        let window = WindowBuilder::new()
            .with_fullscreen(Some(Fullscreen::Borderless(Some(handle))))
            .build(event_loop)
            .unwrap();
        let graphics = crate::graphics::State::new(&window, frame).await;
        self.state.insert(window.id(), State { window, graphics });
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
                    } = self.state.get_mut(&window_id).unwrap();
                    graphics.resize(*physical_size);
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    let State {
                        ref mut graphics, ..
                    } = self.state.get_mut(&window_id).unwrap();
                    // new_inner_size is &&mut so we have to dereference it twice
                    graphics.resize(**new_inner_size);
                }
                _ => {}
            },
            Event::RedrawRequested(window_id) if self.state.contains_key(&window_id) => {
                let State {
                    ref mut graphics, ..
                } = self.state.get_mut(&window_id).unwrap();
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
