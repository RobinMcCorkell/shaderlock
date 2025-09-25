use std::{error::Error, sync::Mutex};

use anyhow::Result;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::{
    globals::GlobalData,
    reexports::{
        client::{
            globals::GlobalList,
            protocol::{
                wl_buffer::WlBuffer,
                wl_output::{Transform, WlOutput},
                wl_shm::Format,
            },
            Connection, Dispatch, QueueHandle, WEnum,
        },
        protocols_wlr::screencopy::v1::client::{
            zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
        },
    },
    registry::GlobalProxy,
};

pub trait HasWlBuffer {
    fn wl_buffer(&self) -> &WlBuffer;
}

impl HasWlBuffer for sctk::shm::slot::Buffer {
    fn wl_buffer(&self) -> &WlBuffer {
        self.wl_buffer()
    }
}

pub trait ScreencopyHandler: Sized {
    type ShmBuffer: HasWlBuffer + Send + Sync + std::fmt::Debug;
    type CreateBufferError: Error;

    fn screencopy_state(&mut self) -> &mut ScreencopyState;

    /// Create an SHM buffer to which Wayland can write the screenshot.
    fn create_buffer(
        &mut self,
        info: &BufferInfo,
    ) -> Result<Self::ShmBuffer, Self::CreateBufferError>;

    /// Get the raw buffer bytes for a screenshot buffer.
    fn get_buffer_data(
        &mut self,
        handle: ScreencopyBufferHandle<Self::ShmBuffer>,
    ) -> ScreencopyBuffer;
}

#[derive(Debug)]
pub struct ScreencopyState {
    manager: GlobalProxy<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
}

impl ScreencopyState {
    pub fn new<D>(globals: &GlobalList, qh: &QueueHandle<D>) -> Self
    where
        D: Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, GlobalData> + 'static,
    {
        // Version 3 of the screencopy protocol sends the BufferDone event.
        let manager = GlobalProxy::from(globals.bind(qh, 3..=3, GlobalData));
        Self { manager }
    }

    pub fn capture_output<D>(
        &self,
        output: &WlOutput,
        qh: &QueueHandle<D>,
    ) -> Result<futures::channel::oneshot::Receiver<Result<ScreencopyBufferHandle<D::ShmBuffer>>>>
    where
        D: Dispatch<
                zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
                ScreencopyCaptureOutputData<D::ShmBuffer>,
            > + ScreencopyHandler
            + 'static,
    {
        let manager = self.manager.get()?;

        let (tx, rx) = futures::channel::oneshot::channel();
        manager.capture_output(0, output, qh, ScreencopyCaptureOutputData::new(tx));
        Ok(rx)
    }
}

#[macro_export]
macro_rules! delegate_screencopy {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        sctk::reexports::client::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty:
            [
                sctk::reexports::protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1: sctk::globals::GlobalData
            ] => $crate::screencopy::ScreencopyState
        );
        sctk::reexports::client::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty:
            [
                sctk::reexports::protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1: $crate::screencopy::ScreencopyCaptureOutputData<sctk::shm::slot::Buffer>
            ] => $crate::screencopy::ScreencopyState
        );
    };
}

#[derive(Default)]
pub struct ScreencopyCaptureOutputData<ShmBuffer: HasWlBuffer> {
    on_done:
        Mutex<Option<futures::channel::oneshot::Sender<Result<ScreencopyBufferHandle<ShmBuffer>>>>>,
    info: Mutex<Option<BufferInfo>>,
    flags: Mutex<Option<zwlr_screencopy_frame_v1::Flags>>,
    buffer: Mutex<Option<ShmBuffer>>,
}

impl<ShmBuffer: HasWlBuffer> ScreencopyCaptureOutputData<ShmBuffer> {
    fn new(
        on_done: futures::channel::oneshot::Sender<Result<ScreencopyBufferHandle<ShmBuffer>>>,
    ) -> Self {
        Self {
            on_done: Mutex::new(Some(on_done)),
            info: Mutex::new(None),
            flags: Mutex::new(None),
            buffer: Mutex::new(None),
        }
    }
}

impl<D> Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, GlobalData, D>
    for ScreencopyState
where
    D: Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, GlobalData>,
{
    fn event(
        _state: &mut D,
        _proxy: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _event: zwlr_screencopy_manager_v1::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<D>,
    ) {
        unreachable!()
    }
}

impl<D>
    Dispatch<
        zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        ScreencopyCaptureOutputData<D::ShmBuffer>,
        D,
    > for ScreencopyState
where
    D: Dispatch<
            zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
            ScreencopyCaptureOutputData<D::ShmBuffer>,
        > + ScreencopyHandler,
{
    fn event(
        state: &mut D,
        proxy: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        data: &ScreencopyCaptureOutputData<D::ShmBuffer>,
        conn: &Connection,
        _: &QueueHandle<D>,
    ) {
        debug!("got screencopy event: {:?}", event);
        match event {
            // Step 1: one or more Buffer events inform the client of available buffer formats.
            zwlr_screencopy_frame_v1::Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } => {
                data.info.lock().unwrap().replace(BufferInfo {
                    width,
                    height,
                    stride,
                    format,
                });
            }
            // Step 1b: zero or more LinuxDmabuf events inform the client of available DMA buffer formats.
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf { .. } => {}
            // Step 2: one Flags event informs the client of any flags.
            zwlr_screencopy_frame_v1::Event::Flags {
                flags: WEnum::Value(flags),
            } => {
                data.flags.lock().unwrap().replace(flags);
            }
            // Step 3: one BufferDone event informs the client all Buffer events have been sent,
            // and the client should start a copy.
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                let info_guard = data.info.lock().unwrap();
                let info = info_guard.as_ref().unwrap();
                debug!("Creating buffer with info {:?}", info);
                let buffer = state.create_buffer(info).unwrap();
                proxy.copy(buffer.wl_buffer());
                conn.flush().unwrap();
                data.buffer.lock().unwrap().replace(buffer);
            }
            // Step 4: one Ready event informs the client the copy is successful.
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let info = data.info.lock().unwrap().take().unwrap();
                let flags = data.flags.lock().unwrap().take().unwrap();
                let buffer = data.buffer.lock().unwrap().take().unwrap();

                let handle = ScreencopyBufferHandle {
                    buffer,
                    info,
                    transform: Transform::Normal,
                    y_invert: flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
                };
                data.on_done
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(Ok(handle))
                    .unwrap();
            }
            // Step 4b: one Failed event informs the client the copy has failed.
            zwlr_screencopy_frame_v1::Event::Failed => {
                data.on_done
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(Err(anyhow::anyhow!("screencopy failed")))
                    .unwrap();
            }
            zwlr_screencopy_frame_v1::Event::Damage { .. } => unimplemented!(),
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub struct BufferInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: Format,
}

#[derive(Debug)]
pub struct ScreencopyBufferHandle<ShmBuffer: HasWlBuffer> {
    pub buffer: ShmBuffer,
    pub info: BufferInfo,
    pub transform: Transform,
    pub y_invert: bool,
}

pub struct ScreencopyBuffer {
    info: BufferInfo,
    transform: Transform,
    y_invert: bool,
    data: Vec<u8>,
}

impl ScreencopyBuffer {
    pub fn new<ShmBuffer: HasWlBuffer>(
        ScreencopyBufferHandle {
            buffer,
            info,
            transform,
            y_invert,
        }: ScreencopyBufferHandle<ShmBuffer>,
        data: &[u8],
    ) -> Self {
        let data = data.to_vec();
        drop(buffer);
        Self {
            info,
            transform,
            y_invert,
            data,
        }
    }

    pub fn bytes(&self) -> &[u8] {
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

    pub fn format(&self) -> Format {
        self.info.format
    }

    pub fn transform_matrix(&self) -> cgmath::Matrix4<f32> {
        use cgmath::{Angle, Matrix4, Rad};
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
