pub struct CallOnce<F> {
    inner: Option<F>,
}

impl<F> CallOnce<F> {
    pub fn new(f: F) -> Self {
        CallOnce { inner: Some(f) }
    }
}

impl<F, Args> FnOnce<Args> for CallOnce<F>
where
    F: FnOnce<Args>,
{
    type Output = Option<F::Output>;
    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        self.inner.map(|f| f.call_once(args))
    }
}

impl<F, Args> FnMut<Args> for CallOnce<F>
where
    F: FnOnce<Args>,
{
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        self.inner.take().map(|f| f.call_once(args))
    }
}
