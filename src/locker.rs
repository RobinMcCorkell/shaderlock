use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::environment::SimpleGlobal;
use sctk::reexports::client as wl;
use sctk::reexports::{
    protocols::wlr::unstable::input_inhibitor::v1::client::zwlr_input_inhibit_manager_v1::ZwlrInputInhibitManagerV1,
};

struct WaylandEnv {
    input_inhibit: SimpleGlobal<ZwlrInputInhibitManagerV1>,
}

impl Default for WaylandEnv {
    fn default() -> Self {
        Self {
            input_inhibit: SimpleGlobal::new(),
        }
    }
}

sctk::environment!(
    WaylandEnv,
    singles = [
        ZwlrInputInhibitManagerV1 => input_inhibit,
    ],
    multis = [
    ],
);
pub struct Locker {
    event_queue: wl::EventQueue,
    env: sctk::environment::Environment<WaylandEnv>,
}

impl Locker {
    pub fn new(display: wl::Display) -> Result<Self> {
        let mut event_queue = display.create_event_queue();

        let env = sctk::environment::Environment::new(
            &display.attach(event_queue.token()),
            &mut event_queue,
            WaylandEnv::default(),
        )
        .context("Failed to create Wayland environment")?;

        Ok(Self { event_queue, env })
    }

    pub fn with<F, O>(&mut self, f: F) -> Result<O>
    where
        F: FnOnce() -> O,
    {
        debug!("Starting input inhibitor");
        let input_inhibit_manager = self.env.require_global::<ZwlrInputInhibitManagerV1>();
        let _input_inhibitor = input_inhibit_manager.get_inhibitor();

        self.communicate()?;

        Ok(f())
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
