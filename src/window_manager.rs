use std::cell::RefCell;
use std::future::Future;
use std::os::fd::AsFd;
use std::ptr::NonNull;

use anyhow::*;
use futures::channel::mpsc;
use futures::FutureExt;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::compositor::*;
use sctk::output::*;
use sctk::reexports::client as wl;
use sctk::reexports::client::backend::WaylandError;
use sctk::reexports::client::globals::registry_queue_init;
use sctk::reexports::client::Proxy;
use sctk::registry::*;
use sctk::seat::keyboard::KeyboardHandler;
use sctk::seat::SeatHandler;
use sctk::seat::SeatState;
use sctk::session_lock::*;
use sctk::shm::*;
use tokio::io::unix::AsyncFd;
use tokio::time::timeout;
use wgpu::rwh;

use crate::screencopy::ScreencopyBuffer;
use crate::screencopy::ScreencopyHandler;
use crate::screencopy::ScreencopyState;

const RECEIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

pub struct WindowManagerState {
    pub output_state: OutputState,

    pub compositor_state: CompositorState,

    pub screencopy_state: ScreencopyState,

    pub registry_state: RegistryState,

    pub shm: Shm,
    pub buffer_pool: slot::SlotPool,

    pub session_lock_state: SessionLockState,
    // pub session_lock: SessionLock,
    // pub surfaces: Vec<(wl::protocol::wl_output::WlOutput, SessionLockSurface)>,
    pub seat_state: SeatState,

    pub events: mpsc::UnboundedSender<Event>,
}

impl WindowManagerState {
    pub fn queue_redraw(&mut self, surface: wl::protocol::wl_surface::WlSurface) {
        self.events
            .unbounded_send(Event::RedrawRequested(surface))
            .expect("send event");
    }
}

pub struct WindowManager {
    pub conn: wl::Connection,
    pub qh: wl::QueueHandle<WindowManagerState>,
    pub event_queue: wl::EventQueue<WindowManagerState>,

    pub state_cell: RefCell<WindowManagerState>,
    pub events: mpsc::UnboundedReceiver<Event>,
}

impl WindowManager {
    pub fn new() -> Result<Self> {
        let conn = wl::Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init::<WindowManagerState>(&conn)?;
        let qh = event_queue.handle();

        let output_state = OutputState::new(&globals, &qh);
        let compositor_state = CompositorState::bind(&globals, &qh)?;
        let session_lock_state = SessionLockState::new(&globals, &qh);
        let screencopy_state = ScreencopyState::new(&globals, &qh);
        let shm = Shm::bind(&globals, &qh)?;
        let registry_state = RegistryState::new(&globals);
        let seat_state = SeatState::new(&globals, &qh);

        let buffer_pool = slot::SlotPool::new(1024, &shm)?;

        let (tx, rx) = mpsc::unbounded();

        let state = WindowManagerState {
            output_state,

            compositor_state,

            screencopy_state,

            registry_state,

            shm,
            buffer_pool,

            session_lock_state,

            seat_state,

            events: tx,
        };

        let state_cell = RefCell::new(state);

        Ok(Self {
            conn,
            qh,
            event_queue,
            state_cell,
            events: rx,
        })
    }

    pub async fn run<'a, Fut>(
        &'a mut self,
        f: impl FnOnce(
            &'a wl::Connection,
            &'a wl::QueueHandle<WindowManagerState>,
            WindowManagerStateAccessor<'a>,
            &'a mut mpsc::UnboundedReceiver<Event>,
        ) -> Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>>,
    {
        let event_queue = &mut self.event_queue;
        let state_cell = &self.state_cell;
        let fd = AsyncFd::new(self.conn.as_fd()).unwrap();
        let receiver = async move {
            let mut state = WindowManagerStateAccessor::new(state_cell);
            loop {
                debug!("flushing event queue");
                event_queue.flush().unwrap();
                debug!("awaiting wayland socket read");
                let mut guard = match timeout(RECEIVE_TIMEOUT, fd.readable()).await {
                    Result::Ok(v) => v,
                    Result::Err(_) => continue,
                }?;
                guard.clear_ready();

                debug!("reading wayland socket");
                if let Some(read_guard) = event_queue.prepare_read() {
                    match read_guard.read() {
                        Result::Err(WaylandError::Io(err))
                            if err.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            continue
                        }
                        v => v,
                    }?;
                }
                debug!("dispatching pending events");
                state.access(|s| event_queue.dispatch_pending(s))?;
                debug!("dispatch complete");
            }
        };

        let state_accessor = WindowManagerStateAccessor::new(&self.state_cell);
        let handler = f(&self.conn, &self.qh, state_accessor, &mut self.events);

        futures::select! {
            receiver_res = receiver.fuse() => receiver_res,
            handler_res = handler.fuse() => handler_res,
        }
    }
}

pub struct WindowManagerStateAccessor<'a>(&'a RefCell<WindowManagerState>);

impl<'a> WindowManagerStateAccessor<'a> {
    pub fn new(inner: &'a RefCell<WindowManagerState>) -> Self {
        Self(inner)
    }

    pub fn access<R>(&mut self, f: impl FnOnce(&mut WindowManagerState) -> R) -> R {
        let mut state = self.0.borrow_mut();
        f(&mut state)
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    NewOutput(wl::protocol::wl_output::WlOutput),
    RedrawRequested(wl::protocol::wl_surface::WlSurface),

    NewSeatCapability(wl::protocol::wl_seat::WlSeat, sctk::seat::Capability),
    RemoveSeatCapability(wl::protocol::wl_seat::WlSeat, sctk::seat::Capability),
    KeyPressed(sctk::seat::keyboard::KeyEvent),

    SessionLocked,
    SessionLockFinished,
    ConfigureLockSurface(SessionLockSurface, (u32, u32)),

    ExitSync,
}

pub struct ExitSync;

#[derive(Clone, Debug)]
pub struct Window {
    pub display: wl::protocol::wl_display::WlDisplay,
    pub surface: wl::protocol::wl_surface::WlSurface,
}

impl rwh::HasWindowHandle for Window {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let wwh = rwh::WaylandWindowHandle::new(
            NonNull::new(self.surface.id().as_ptr() as *mut _).unwrap(),
        );
        unsafe { Result::Ok(rwh::WindowHandle::borrow_raw(wwh.into())) }
    }
}

impl rwh::HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let wdh = rwh::WaylandDisplayHandle::new(
            NonNull::new(self.display.id().as_ptr() as *mut _).unwrap(),
        );
        unsafe { Result::Ok(rwh::DisplayHandle::borrow_raw(wdh.into())) }
    }
}

sctk::delegate_output!(WindowManagerState);

impl OutputHandler for WindowManagerState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        output: wl::protocol::wl_output::WlOutput,
    ) {
        debug!("new output: {:?}", output);
        self.events
            .unbounded_send(Event::NewOutput(output))
            .expect("send event");
    }

    fn update_output(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _output: wl::protocol::wl_output::WlOutput,
    ) {
        unimplemented!()
    }

    fn output_destroyed(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _output: wl::protocol::wl_output::WlOutput,
    ) {
        unimplemented!()
    }
}

sctk::delegate_compositor!(WindowManagerState);

impl CompositorHandler for WindowManagerState {
    fn scale_factor_changed(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _new_transform: wl::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        surface: &wl::protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
        debug!("got frame event for surface: {:?}", surface);
        self.events
            .unbounded_send(Event::RedrawRequested(surface.clone()))
            .expect("send event");
    }

    fn surface_enter(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _output: &wl::protocol::wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _output: &wl::protocol::wl_output::WlOutput,
    ) {
    }
}

sctk::delegate_session_lock!(WindowManagerState);

impl SessionLockHandler for WindowManagerState {
    fn locked(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _session_lock: SessionLock,
    ) {
        info!("locked");
        self.events
            .unbounded_send(Event::SessionLocked)
            .expect("send event");
    }

    fn finished(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _session_lock: SessionLock,
    ) {
        info!("lock finished");
        self.events
            .unbounded_send(Event::SessionLockFinished)
            .expect("send event");
    }

    fn configure(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        debug!("configure lock surface: {:?}", surface);
        self.events
            .unbounded_send(Event::ConfigureLockSurface(surface, configure.new_size))
            .expect("send event");
        // let (output, _) = self
        //     .surfaces
        //     .iter()
        //     .find(|(_, s)| surface.wl_surface() == s.wl_surface())
        //     .unwrap();
        // self.events
        //     .unbounded_send(Event::ConfigureSurface(
        //         Window {
        //             display: conn.display(),
        //             output: output.clone(),
        //             surface: surface.wl_surface().clone(),
        //         },
        //         configure.new_size,
        //     ))
        //     .expect("send event");
    }
}

sctk::delegate_shm!(WindowManagerState);

impl ShmHandler for WindowManagerState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

sctk::delegate_registry!(WindowManagerState);

impl ProvidesRegistryState for WindowManagerState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    sctk::registry_handlers![OutputState,];
}

crate::delegate_screencopy!(WindowManagerState);

impl ScreencopyHandler for WindowManagerState {
    type ShmBuffer = slot::Buffer;
    type CreateBufferError = slot::CreateBufferError;

    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        &mut self.screencopy_state
    }

    fn create_buffer(
        &mut self,
        info: &crate::screencopy::BufferInfo,
    ) -> Result<Self::ShmBuffer, Self::CreateBufferError> {
        debug!("creating buffer: {:?}", info);
        let (buffer, _) = self.buffer_pool.create_buffer(
            info.width as i32,
            info.height as i32,
            info.stride as i32,
            info.format,
        )?;
        buffer.activate().unwrap();
        std::result::Result::Ok(buffer)
    }

    fn get_buffer_data(
        &mut self,
        handle: crate::screencopy::ScreencopyBufferHandle<Self::ShmBuffer>,
    ) -> crate::screencopy::ScreencopyBuffer {
        let bytes = self
            .buffer_pool
            .canvas(&handle.buffer)
            .expect("get buffer bytes");
        ScreencopyBuffer::new(handle, bytes)
    }
}

sctk::delegate_keyboard!(WindowManagerState);

impl KeyboardHandler for WindowManagerState {
    fn enter(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _keyboard: &wl::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[sctk::seat::keyboard::Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _keyboard: &wl::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl::protocol::wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _keyboard: &wl::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: sctk::seat::keyboard::KeyEvent,
    ) {
        debug!("press key event: {:?}", event);
        self.events
            .unbounded_send(Event::KeyPressed(event))
            .expect("send event");
    }

    fn release_key(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _keyboard: &wl::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: sctk::seat::keyboard::KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _keyboard: &wl::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: sctk::seat::keyboard::Modifiers,
        _layout: u32,
    ) {
    }
}

sctk::delegate_seat!(WindowManagerState);

impl SeatHandler for WindowManagerState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _seat: wl::protocol::wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        seat: wl::protocol::wl_seat::WlSeat,
        capability: sctk::seat::Capability,
    ) {
        debug!("new seat capability: {:?}", capability);
        self.events
            .unbounded_send(Event::NewSeatCapability(seat, capability))
            .expect("send event");

        // if capability == Capability::Keyboard && self.keyboard.is_none() {
        //     println!("Set keyboard capability");
        //     let keyboard = self
        //         .seat_state
        //         .get_keyboard_with_repeat(
        //             qh,
        //             &seat,
        //             None,
        //             self.loop_handle.clone(),
        //             Box::new(|_state, _wl_kbd, event| {
        //                 println!("Repeat: {:?} ", event);
        //             }),
        //         )
        //         .expect("Failed to create keyboard");

        //     self.keyboard = Some(keyboard);
        // }
    }

    fn remove_capability(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        seat: wl::protocol::wl_seat::WlSeat,
        capability: sctk::seat::Capability,
    ) {
        debug!("remove seat capability: {:?}", capability);
        self.events
            .unbounded_send(Event::RemoveSeatCapability(seat, capability))
            .expect("send event");
    }

    fn remove_seat(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _seat: wl::protocol::wl_seat::WlSeat,
    ) {
    }
}

wl::delegate_noop!(WindowManagerState: ignore wl::protocol::wl_buffer::WlBuffer);

impl wl::Dispatch<wl::protocol::wl_callback::WlCallback, ExitSync> for WindowManagerState {
    fn event(
        state: &mut Self,
        _proxy: &wl::protocol::wl_callback::WlCallback,
        _event: <wl::protocol::wl_callback::WlCallback as Proxy>::Event,
        _data: &ExitSync,
        _conn: &wl::Connection,
        _qhandle: &wl::QueueHandle<Self>,
    ) {
        state
            .events
            .unbounded_send(Event::ExitSync)
            .expect("send event");
    }
}
