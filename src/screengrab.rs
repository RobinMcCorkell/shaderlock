use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::output::OutputHandler;
use sctk::reexports::client as wl;
use sctk::reexports::{
    protocols::wlr::unstable::screencopy::v1::client::zwlr_screencopy_frame_v1,
    protocols::wlr::unstable::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};
use sctk::shm::ShmHandler;
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

#[derive(Debug)]
struct BufferInfo {
    width: u32,
    height: u32,
    stride: u32,
    format: sctk::shm::Format,
}

pub struct Buffer {
    mempool: sctk::shm::MemPool,
    info: BufferInfo,
    transform: sctk::output::Transform,
}

impl Buffer {
    pub fn as_bgra(&mut self) -> &[u8] {
        use sctk::shm::Format::*;
        if self.info.format != Argb8888 {
            panic!("Unsupported format: {:?}", self.info.format);
        }
        debug!("Buffer size = {}", self.mempool.mmap().len());
        self.mempool.mmap()
    }

    pub fn width(&self) -> u32 {
        self.info.width
    }

    pub fn height(&self) -> u32 {
        self.info.height
    }

    pub fn stride(&self) -> u32 {
        self.info.stride
    }

    pub fn transform_matrix(&self) -> cgmath::Matrix4<f32> {
        use cgmath::{Angle, Matrix4, Rad};
        use sctk::output::Transform::*;
        let angle = Rad::turn_div_4()
            * match self.transform {
                Normal | Flipped => 0.0,
                _90 | Flipped90 => 1.0,
                _180 | Flipped180 => 2.0,
                _270 | Flipped270 => 3.0,
                _ => panic!("Unsupported transform"),
            };
        let flip = match self.transform {
            Flipped | Flipped90 | Flipped180 | Flipped270 => true,
            _ => false,
        };
        Matrix4::from_angle_z(angle)
            * Matrix4::from_nonuniform_scale(if flip { -1.0 } else { 1.0 }, 1.0, 1.0)
    }
}

pub struct Screengrabber {
    event_queue: wl::EventQueue,
    env: sctk::environment::Environment<WaylandEnv>,
}

impl Screengrabber {
    pub fn new() -> Result<Self> {
        let display = wl::Display::connect_to_env().context("Failed to connect to Wayland")?;
        let mut event_queue = display.create_event_queue();

        let env = sctk::environment::Environment::new(
            &wl::Proxy::clone(&display).attach(event_queue.token()),
            &mut event_queue,
            WaylandEnv::default(),
        )
        .context("Failed to create Wayland environment")?;

        Ok(Self { event_queue, env })
    }

    pub async fn grab_screen(&mut self, output_id: u32) -> Result<Buffer> {
        let screencopy = self.env.require_global::<ZwlrScreencopyManagerV1>();
        let output = self
            .env
            .get_all_globals::<WlOutput>()
            .into_iter()
            .find(|o| sctk::output::with_output_info(o, |info| info.id) == Some(output_id))
            .context("Failed to find Wayland output for monitor")?;

        let transform = sctk::output::with_output_info(&output, |oi| oi.transform)
            .context("Failed to get window transform state")?;
        let shm = self.env.require_global::<WlShm>();

        let (tx, rx) = futures::channel::oneshot::channel();
        let (donetx, donerx) = futures::channel::oneshot::channel();
        let mut do_copy = crate::callback_utils::CallOnce::new(
            move |frame: sctk::reexports::client::Main<
                zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
            >,
                  format,
                  width,
                  height,
                  stride| {
                let info = BufferInfo {
                    width,
                    height,
                    stride,
                    format,
                };
                debug!("Creating buffer with info {:?}", info);
                let mut mempool =
                    sctk::shm::MemPool::new(shm, |_| {}).expect("Failed to create mempool");
                mempool
                    .resize((height * stride) as usize)
                    .expect("Failed to resize buffer");
                let buffer = mempool.buffer(0, width as i32, height as i32, stride as i32, format);
                frame.copy(&buffer);

                let buf = Buffer {
                    mempool,
                    info,
                    transform,
                };
                tx.send(buf)
                    .map_err(|_| ())
                    .expect("Failed to send buffer from callback");
            },
        );
        let mut do_ready = crate::callback_utils::CallOnce::new(move || {
            debug!("Copy completed");
            donetx
                .send(())
                .expect("Failed to signal done from callback");
        });

        debug!("Starting screengrab");
        use zwlr_screencopy_frame_v1::Event;
        screencopy
            .capture_output(0, &*output)
            .quick_assign(move |frame, event, _| match event {
                Event::Buffer {
                    format,
                    width,
                    height,
                    stride,
                } => {
                    do_copy(frame, format, width, height, stride);
                }
                Event::BufferDone => {}
                Event::Ready { .. } => {
                    do_ready();
                }
                Event::Flags { .. } => {}
                Event::LinuxDmabuf { .. } => {}
                Event::Failed => panic!("Failed to copy buffer"),
                ev => panic!("Unexpected event {:?}", ev),
            });

        self.communicate()?;

        debug!("Waiting for screengrab buffer");
        let buf = rx.await?;
        debug!("Waiting for buffer ready");
        donerx.await?;
        Ok(buf)
    }

    pub fn communicate(&mut self) -> Result<()> {
        debug!("Communicating with Wayland");
        self.event_queue
            .sync_roundtrip(&mut (), |_, _, _| unreachable!())
            .context("Failed to tx/rx with Wayland")?;
        self.event_queue
            .sync_roundtrip(&mut (), |_, _, _| unreachable!())
            .context("Failed to tx/rx with Wayland")?;
        Ok(())
    }
}
