use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::environment::SimpleGlobal;
use sctk::reexports::client as wl;
use sctk::reexports::{
    protocols::wlr::unstable::input_inhibitor::v1::client::zwlr_input_inhibit_manager_v1::ZwlrInputInhibitManagerV1,
};

const PAM_SERVICE: &str = env!("PAM_SERVICE");
const PASSWORD_SIZE: usize = 256;

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
        F: FnOnce(LockContext<'static>) -> O,
    {
        debug!("Starting input inhibitor");
        let input_inhibit_manager = self.env.require_global::<ZwlrInputInhibitManagerV1>();
        let _input_inhibitor = input_inhibit_manager.get_inhibitor();

        self.communicate()?;

        Ok(f(LockContext::new()?))
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

pub struct LockContext<'a> {
    auth: pam::Authenticator<'a, pam::PasswordConv>,
    username: String,
    password: arrayvec::ArrayString<{ PASSWORD_SIZE }>,
}

impl<'a> LockContext<'a> {
    fn new() -> Result<Self> {
        let auth =
            pam::Authenticator::with_password(PAM_SERVICE).context("Failed to initialize PAM")?;
        let username = users::get_current_username()
            .context("Failed to get username")?
            .into_string()
            .map_err(|oss| anyhow!("Failed to parse username {:?}", oss))?;
        info!("My username: {}", username);

        Ok(Self {
            auth,
            username,
            password: arrayvec::ArrayString::new(),
        })
    }

    pub fn push(&mut self, c: char) {
        self.password
            .try_push(c)
            .unwrap_or_else(|_| error!("Overflowed password field"))
    }

    pub fn pop(&mut self) -> Option<char> {
        self.password.pop()
    }

    pub fn clear(&mut self) {
        debug!("Clearing password buffer");
        self.password.clear()
    }

    pub fn authenticate(&mut self) -> pam::PamResult<()> {
        debug!("Beginning authentication");
        self.auth
            .get_handler()
            .set_credentials(&self.username, self.password.as_str());
        self.clear();
        let result = self.auth.authenticate();
        info!("Authentication result: {:?}", result);
        result
    }
}
