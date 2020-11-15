use log::{debug, error, info, warn};
use std::collections::HashMap;
use winit::event_loop::EventLoop;
use winit::monitor::*;
use winit::window::*;

use sctk::output::OutputHandler;
use sctk::reexports::client as wl;
use sctk::reexports::{
    protocols::wlr::unstable::screencopy::v1::client::zwlr_screencopy_frame_v1,
    protocols::wlr::unstable::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};
use sctk::shm::ShmHandler;
use wl::protocol::wl_buffer::WlBuffer;
use wl::protocol::wl_output::WlOutput;
use wl::protocol::wl_shm::WlShm;

pub struct State {
    window: Window,
    graphics: crate::graphics::State,
}

pub struct WaylandEnv {
    screencopy: sctk::environment::SimpleGlobal<ZwlrScreencopyManagerV1>,
    shm: ShmHandler,
    outputs: OutputHandler,
}

impl Default for WaylandEnv {
    fn default() -> Self {
        Self {
            screencopy: sctk::environment::SimpleGlobal::new(),
            shm: ShmHandler::new(),
            outputs: OutputHandler::new(),
        }
    }
}

sctk::environment!(
    WaylandEnv,
    singles = [
        ZwlrScreencopyManagerV1 => screencopy,
        WlShm => shm,
    ],
    multis = [
        WlOutput => outputs,
    ],
);

struct Screengrabber {
    event_queue: wl::EventQueue,
    env: sctk::environment::Environment<WaylandEnv>,
}

impl Screengrabber {
    fn new() -> Self {
        let display = wl::Display::connect_to_env().expect("Failed to connect to Wayland");
        let mut event_queue = display.create_event_queue();

        let env = sctk::environment::Environment::new(
            &wl::Proxy::clone(&display).attach(event_queue.token()),
            &mut event_queue,
            WaylandEnv::default(),
        )
        .expect("Failed to create Wayland environment");

        Self { event_queue, env }
    }

    fn grab_screen(&mut self, output_id: u32) -> WlBuffer {
        let screencopy = self.env.require_global::<ZwlrScreencopyManagerV1>();
        let output = self
            .env
            .get_all_globals::<WlOutput>()
            .into_iter()
            .find(|o| sctk::output::with_output_info(o, |info| info.id) == Some(output_id))
            .expect("Failed to find Wayland output for monitor");

        let shm = self.env.require_global::<WlShm>();
        let mempool = sctk::shm::MemPool::new(shm, |_| {}).unwrap();

        screencopy
            .capture_output(0, &*output)
            .quick_assign(move |frame, event, mut data| match event {
                zwlr_screencopy_frame_v1::Event::Buffer {
                    format,
                    width,
                    height,
                    stride,
                } => {
                    debug!(
                        "Creating {:?} buffer with dimensions {}x{}",
                        format, width, height
                    );
                    let buffer = data.get::<Option<WlBuffer>>().unwrap();
                    *buffer =
                        Some(mempool.buffer(0, width as i32, height as i32, stride as i32, format));
                    frame.copy(buffer.as_ref().unwrap());
                }
                _ => {}
            });

        let mut buffer: Option<WlBuffer> = None;
        self.event_queue
            .sync_roundtrip(&mut buffer, |_, _, _| unreachable!())
            .unwrap();

        buffer.unwrap()
    }
}

pub struct Manager {
    screengrabber: Screengrabber,
    state: HashMap<WindowId, State>,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            screengrabber: Screengrabber::new(),
            state: HashMap::new(),
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
        let graphics = crate::graphics::State::new(&window).await;
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
                graphics.render();
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
