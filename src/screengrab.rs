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

#[derive(Debug)]
struct BufferInfo {
    width: u32,
    height: u32,
    stride: u32,
    format: sctk::shm::Format,
}

pub struct Buffer {
    buffer: wl::protocol::wl_buffer::WlBuffer,
    pool: sctk::shm::MemPool,
    info: BufferInfo,
    transform: sctk::output::Transform,
}

impl Buffer {
    pub fn as_rgba(&mut self) -> &[u8] {
        use sctk::shm::Format::*;
        if self.info.format != Rgba8888 {
            info!("Format is {:?}, converting", self.info.format);
            let shufb = match self.info.format {
                Argb8888 => &[2, 1, 0, 3],
                _ => panic!("Unsupported format"),
            };
            for chunk in self.pool.mmap().chunks_exact_mut(4) {
                let new_chunk = [
                    chunk[shufb[0]],
                    chunk[shufb[1]],
                    chunk[shufb[2]],
                    chunk[shufb[3]],
                ];
                for i in 0..3 {
                    chunk[i] = new_chunk[i];
                }
            }
            self.info.format = Rgba8888;
        }
        debug!("Buffer size = {}", self.pool.mmap().len());
        self.pool.mmap()
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

    pub fn grab_screen(&mut self, output_id: u32) -> Buffer {
        let screencopy = self.env.require_global::<ZwlrScreencopyManagerV1>();
        let output = self
            .env
            .get_all_globals::<WlOutput>()
            .into_iter()
            .find(|o| sctk::output::with_output_info(o, |info| info.id) == Some(output_id))
            .expect("Failed to find Wayland output for monitor");

        struct PartialBuffer {
            buffer: Option<wl::protocol::wl_buffer::WlBuffer>,
            pool: sctk::shm::MemPool,
            info: Option<BufferInfo>,
        };
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
                    let b = data.get::<PartialBuffer>().unwrap();
                    b.info = Some(BufferInfo {
                        width,
                        height,
                        stride,
                        format,
                    });
                    b.pool.resize((height * stride) as usize).unwrap();
                    let buf = b
                        .pool
                        .buffer(0, width as i32, height as i32, stride as i32, format);
                    frame.copy(&buf);
                    b.buffer = Some(buf);
                }
                _ => {}
            });

        let shm = self.env.require_global::<WlShm>();
        let mempool = sctk::shm::MemPool::new(shm, |_| {}).unwrap();
        let mut context = PartialBuffer {
            buffer: None,
            pool: mempool,
            info: None,
        };

        self.event_queue
            .sync_roundtrip(&mut context, |_, _, _| unreachable!())
            .unwrap();
        self.event_queue
            .sync_roundtrip(&mut context, |_, _, _| unreachable!())
            .unwrap();

        let transform = sctk::output::with_output_info(&output, |oi| oi.transform)
            .unwrap_or(sctk::output::Transform::Normal);

        debug!("Took screenshot with info {:?}", context.info,);
        Buffer {
            buffer: context.buffer.unwrap(),
            pool: context.pool,
            info: context.info.unwrap(),
            transform,
        }
    }
}
