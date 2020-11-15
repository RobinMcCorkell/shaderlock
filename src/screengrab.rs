use log::{debug, error, info, warn};

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

struct WaylandEnv {
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

pub struct BufferInfo {
    width: u32,
    height: u32,
    stride: u32,
    format: sctk::shm::Format,
}

pub struct ScreengrabBuffer(sctk::shm::MemPool, Option<BufferInfo>);

pub struct Screengrabber {
    event_queue: wl::EventQueue,
    env: sctk::environment::Environment<WaylandEnv>,
}

impl Screengrabber {
    pub fn new() -> Self {
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

    pub fn grab_screen(&mut self, output_id: u32) -> ScreengrabBuffer {
        let screencopy = self.env.require_global::<ZwlrScreencopyManagerV1>();
        let output = self
            .env
            .get_all_globals::<WlOutput>()
            .into_iter()
            .find(|o| sctk::output::with_output_info(o, |info| info.id) == Some(output_id))
            .expect("Failed to find Wayland output for monitor");

        screencopy
            .capture_output(0, &*output)
            .quick_assign(|frame, event, mut data| match event {
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
                    let ScreengrabBuffer(ref mempool, ref mut bi) =
                        data.get::<ScreengrabBuffer>().unwrap();
                    *bi = Some(BufferInfo {
                        width,
                        height,
                        stride,
                        format,
                    });
                    let buf = mempool.buffer(0, width as i32, height as i32, stride as i32, format);
                    frame.copy(&buf);
                }
                _ => {}
            });

        let shm = self.env.require_global::<WlShm>();
        let mempool = sctk::shm::MemPool::new(shm, |_| {}).unwrap();
        let bi: Option<BufferInfo> = None;
        let mut context = ScreengrabBuffer(mempool, bi);

        self.event_queue
            .sync_roundtrip(&mut context, |_, _, _| unreachable!())
            .unwrap();

        context
    }
}
