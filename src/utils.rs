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
    F: FnOnce<Args>, Args: std::marker::Tuple,
{
    type Output = Option<F::Output>;
    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        self.inner.map(|f| f.call_once(args))
    }
}

impl<F, Args> FnMut<Args> for CallOnce<F>
where
    F: FnOnce<Args>, Args: std::marker::Tuple,
{
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        self.inner.take().map(|f| f.call_once(args))
    }
}

pub trait ShallowCopy<'b> {
    type Output: 'b;
    fn shallow_copy(&'b self) -> Self::Output;
}

impl<'a, 'b, B: std::borrow::ToOwned + ?Sized + 'b> ShallowCopy<'b> for std::borrow::Cow<'a, B> {
    type Output = std::borrow::Cow<'b, B>;

    fn shallow_copy(&'b self) -> Self::Output {
        use std::borrow::Borrow;
        std::borrow::Cow::Borrowed(self.borrow())
    }
}

impl<'a, 'b> ShallowCopy<'b> for wgpu::ShaderModuleSource<'a> {
    type Output = wgpu::ShaderModuleSource<'b>;

    fn shallow_copy(&'b self) -> Self::Output {
        match *self {
            wgpu::ShaderModuleSource::SpirV(ref inner) => {
                wgpu::ShaderModuleSource::SpirV(inner.shallow_copy())
            }
            wgpu::ShaderModuleSource::Wgsl(ref inner) => {
                wgpu::ShaderModuleSource::Wgsl(inner.shallow_copy())
            }
        }
    }
}
