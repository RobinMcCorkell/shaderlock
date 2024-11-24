use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use std::collections::HashMap;
use std::sync::Arc;
use winit::event::*;
use winit::monitor::*;
use winit::window::*;

use crate::graphics::RenderContext;
use crate::screengrab::Screengrabber;

const FREEZE_AFTER_INACTIVITY: std::time::Duration = std::time::Duration::from_secs(10);
const FADE_BEFORE_FREEZE: std::time::Duration = std::time::Duration::from_secs(5);

pub struct State {
    window: Arc<Window>,
    graphics: crate::graphics::State<'static>,
}

pub struct Manager {
    screengrabber: Screengrabber,
    graphics: crate::graphics::Manager,
    state: HashMap<WindowId, State>,
    init_time: std::time::Instant,
    last_keypress_time: std::time::Instant,
    frozen: bool,
}

impl Manager {
    pub fn new(screengrabber: Screengrabber, graphics: crate::graphics::Manager) -> Self {
        Self {
            screengrabber,
            graphics,
            state: HashMap::new(),
            init_time: std::time::Instant::now(),
            last_keypress_time: std::time::Instant::now(),
            frozen: false,
        }
    }

    pub async fn add_monitor(&mut self, handle: MonitorHandle, window: Window) -> Result<()> {
        use winit::platform::wayland::MonitorHandleExtWayland;
        debug!("Grabbing screen on {:?}", handle.name());
        let frame = self
            .screengrabber
            .grab_screen(handle.native_id())
            .await
            .expect("Failed to grab screen");

        debug!("Initialising graphics on {:?}", window.id());
        let windowrc = Arc::new(window);
        let graphics = self
            .graphics
            .init_window(windowrc.clone(), frame)
            .await
            .expect("Init window");

        debug!("Added window {:?} on {:?}", windowrc.id(), handle.name());

        self.state.insert(
            windowrc.id(),
            State {
                graphics,
                window: windowrc,
            },
        );

        Ok(())
    }

    pub fn handle_event(&mut self, window_id: WindowId, event: WindowEvent) -> Result<()> {
        match self.state.get_mut(&window_id) {
            None => Ok(()),
            Some(state) => {
                let State {
                    ref mut graphics,
                    ref window,
                } = *state;

                match event {
                    WindowEvent::Resized(physical_size) => {
                        graphics.resize(physical_size);
                    }
                    // WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    //     // new_inner_size is &&mut so we have to dereference it twice
                    //     graphics.resize(**new_inner_size);
                    // }
                    WindowEvent::KeyboardInput { .. } => {
                        self.last_keypress_time = std::time::Instant::now();
                        self.frozen = false;
                    }
                    WindowEvent::RedrawRequested => {
                        let ctx = RenderContext {
                            elapsed: self.init_time.elapsed(),
                            fade_amount: (self.last_keypress_time.elapsed() + FADE_BEFORE_FREEZE)
                                .saturating_sub(FREEZE_AFTER_INACTIVITY)
                                .as_secs_f32()
                                / FADE_BEFORE_FREEZE.as_secs_f32(),
                        };
                        let frame = graphics.render(ctx);
                        window.pre_present_notify();
                        frame.present();

                        if self.last_keypress_time.elapsed() < FREEZE_AFTER_INACTIVITY {
                            window.request_redraw();
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
        }
    }
}
