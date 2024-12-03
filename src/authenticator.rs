use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

const PAM_SERVICE: &str = env!("PAM_SERVICE");
const PASSWORD_SIZE: usize = 256;

pub trait AuthenticatorBackend {
    fn authenticate(&mut self, password: &str) -> Result<()>;
}

pub struct PamAuthenticatorBackend {
    username: String,
    auth: pam::Authenticator<'static, pam::PasswordConv>,
}

impl PamAuthenticatorBackend {
    pub fn new() -> Result<Self> {
        let username = users::get_current_username()
            .context("Failed to get username")?
            .into_string()
            .map_err(|oss| anyhow!("Failed to parse username {:?}", oss))?;
        info!("My username: {}", username);

        let auth =
            pam::Authenticator::with_password(PAM_SERVICE).context("Failed to initialize PAM")?;

        Ok(Self { username, auth })
    }
}

impl AuthenticatorBackend for PamAuthenticatorBackend {
    fn authenticate(&mut self, password: &str) -> Result<()> {
        self.auth
            .get_handler()
            .set_credentials(&self.username, password);
        self.auth.authenticate().context("PAM auth failed")
    }
}

pub struct NullAuthenticatorBackend;

impl NullAuthenticatorBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NullAuthenticatorBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthenticatorBackend for NullAuthenticatorBackend {
    fn authenticate(&mut self, _: &str) -> Result<()> {
        warn!("null authentication = success");
        Ok(())
    }
}

pub struct Authenticator<'a> {
    backend: &'a mut dyn AuthenticatorBackend,
    password: arrayvec::ArrayString<{ PASSWORD_SIZE }>,
}

impl<'a> Authenticator<'a> {
    pub fn new(backend: &'a mut dyn AuthenticatorBackend) -> Result<Self> {
        Ok(Self {
            backend,
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

    pub fn authenticate(&mut self) -> Result<()> {
        debug!("Beginning authentication");
        let result = self.backend.authenticate(&self.password);
        self.clear();
        info!("Authentication result: {:?}", result);
        result
    }
}
