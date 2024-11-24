use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::environment::SimpleGlobal;
use sctk::output::OutputHandler;
use sctk::reexports::client as wl;
use sctk::reexports::{
    protocols::wlr::unstable::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    protocols::wlr::unstable::screencopy::v1::client::*,
};
use sctk::shm::ShmHandler;
use tokio::task::spawn_local;
use wl::protocol::wl_output::WlOutput;
use wl::protocol::wl_shm::WlShm;

struct WaylandEnv {
    screencopy: SimpleGlobal<ZwlrScreencopyManagerV1>,
    shm: ShmHandler,
    outputs: OutputHandler,
}

impl Default for WaylandEnv {
    fn default() -> Self {
        Self {
            screencopy: SimpleGlobal::new(),
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

struct BufferMempool {
    mempool: sctk::shm::MemPool,
    info: BufferInfo,
    transform: sctk::output::Transform,
    y_invert: bool,
}

impl Into<Buffer> for BufferMempool {
    fn into(mut self) -> Buffer {
        debug!("Buffer size = {}", self.mempool.mmap().len());
        Buffer {
            data: self.mempool.mmap().to_owned(),
            info: self.info,
            transform: self.transform,
            y_invert: self.y_invert,
        }
    }
}

pub struct Buffer {
    data: Vec<u8>,
    info: BufferInfo,
    transform: sctk::output::Transform,
    y_invert: bool,
}

impl Buffer {
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
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

    pub fn format(&self) -> sctk::shm::Format {
        self.info.format
    }

    pub fn transform_matrix(&self) -> cgmath::Matrix4<f32> {
        use cgmath::{Angle, Matrix4, Rad};
        use sctk::output::Transform;
        let angle = Rad::turn_div_4()
            * match self.transform {
                Transform::Normal | Transform::Flipped => 0.0,
                Transform::_90 | Transform::Flipped90 => 1.0,
                Transform::_180 | Transform::Flipped180 => 2.0,
                Transform::_270 | Transform::Flipped270 => 3.0,
                _ => panic!("Unsupported transform"),
            };
        let flip = matches!(
            self.transform,
            Transform::Flipped
                | Transform::Flipped90
                | Transform::Flipped180
                | Transform::Flipped270
        ) ^ self.y_invert;
        Matrix4::from_angle_z(angle)
            * Matrix4::from_nonuniform_scale(if flip { -1.0 } else { 1.0 }, 1.0, 1.0)
    }
}

pub struct Screengrabber {
    display: wl::Display,
    env: sctk::environment::Environment<WaylandEnv>,
}

impl Screengrabber {
    pub async fn new(display: wl::Display) -> Result<Self> {
        let mut event_queue = display.create_event_queue();

        let env = sctk::environment::Environment::new(
            &wl::Proxy::clone(&display).attach(event_queue.token()),
            &mut event_queue,
            WaylandEnv::default(),
        )
        .context("Failed to create Wayland environment")?;

        let fd = tokio::io::unix::AsyncFd::new(event_queue.display().get_connection_fd()).unwrap();

        debug!("Spawning screengrab event dispatch");
        spawn_local(async move {
            loop {
                debug!("awaiting socket readiness");
                let mut rg = fd.readable().await.unwrap();
                rg.clear_ready();
                // if let Some(guard) = event_queue.prepare_read() {
                //     debug!("reading events");
                //     if let Err(e) = guard.read_events() {
                //         if e.kind() != std::io::ErrorKind::WouldBlock {
                //             continue;
                //         }
                //     }
                // }

                debug!("dispatching screengrab events");
                event_queue
                    .dispatch_pending(&mut (), |_, _, _| unreachable!())
                    .expect("Dispatch pending events");
                debug!("dispatch complete");
            }
        });

        Ok(Self { display, env })
    }

    pub async fn grab_screen(&self, output_id: u32) -> Result<Buffer> {
        let screencopy = self.env.require_global::<ZwlrScreencopyManagerV1>();
        let shm = self.env.require_global::<WlShm>();
        let outputs = self.env.get_all_globals::<WlOutput>();
        let display = self.display.clone();

        let output = outputs
            .into_iter()
            .find(|o| sctk::output::with_output_info(o, |info| info.id) == Some(output_id))
            .context("Failed to find Wayland output for monitor")?;

        let transform = sctk::output::with_output_info(&output, |oi| oi.transform)
            .context("Failed to get window transform state")?;

        let (tx, rx) = futures::channel::oneshot::channel();
        let (donetx, donerx) = futures::channel::oneshot::channel();
        let (flagstx, flagsrx) = futures::channel::oneshot::channel();
        let copydisplay = display.clone();
        let mut do_copy = crate::utils::CallOnce::new(
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
                copydisplay.flush().unwrap();

                let buf = BufferMempool {
                    mempool,
                    info,
                    transform,
                    y_invert: false, // Filled in later.
                };
                tx.send(buf)
                    .map_err(|_| ())
                    .expect("Failed to send buffer from callback");
            },
        );
        let mut do_ready = crate::utils::CallOnce::new(move || {
            debug!("Copy completed");
            donetx
                .send(())
                .expect("Failed to signal done from callback");
        });

        let mut do_flags =
            crate::utils::CallOnce::new(move |flags: zwlr_screencopy_frame_v1::Flags| {
                flagstx
                    .send(flags)
                    .expect("Failed to send flags from callback");
            });

        debug!("Starting screengrab");
        use zwlr_screencopy_frame_v1::Event;
        screencopy
            .capture_output(0, &output)
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
                Event::Flags { flags } => {
                    do_flags(flags);
                }
                Event::LinuxDmabuf { .. } => {}
                Event::Failed => panic!("Failed to copy buffer"),
                ev => panic!("Unexpected event {:?}", ev),
            });
        display.flush()?;

        debug!("Waiting for screengrab buffer");
        let (buf, flags) = futures::join!(rx, flagsrx);
        let mut buf = buf?;
        let flags = flags?;
        buf.y_invert = flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
        debug!("Waiting for buffer ready");
        donerx.await?;
        Ok(buf.into())
    }
}
