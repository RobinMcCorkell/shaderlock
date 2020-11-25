mod bg;
mod icon;

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

pub struct Manager {
    instance: wgpu::Instance,
    shader: wgpu::ShaderModuleSource<'static>,
    icon: image::RgbaImage,
}

impl Manager {
    pub fn new(shader_file: &std::path::Path, icon_file: &std::path::Path) -> Result<Self> {
        let shader_source =
            std::fs::read_to_string(shader_file).context("Failed to read shader")?;
        let mut compiler = shaderc::Compiler::new().context("Failed to create shader compiler")?;
        let spirv = compiler
            .compile_into_spirv(
                &shader_source,
                shaderc::ShaderKind::Fragment,
                &shader_file.to_string_lossy(),
                bg::FS_MAIN,
                None,
            )
            .context("Failed to compile shader")?;

        let data = Vec::from(spirv.as_binary());
        let shader = wgpu::ShaderModuleSource::SpirV(data.into());

        let icon = image::open(icon_file).context("Failed to read icon file")?;

        Ok(Manager {
            instance: wgpu::Instance::new(wgpu::BackendBit::PRIMARY),
            shader,
            icon: icon.into_rgba8(),
        })
    }

    pub async fn init_window(
        &self,
        window: &winit::window::Window,
        screenshot: crate::screengrab::Buffer,
    ) -> Result<State> {
        let size = window.inner_size();
        let surface_size = (size.width, size.height);

        let surface = unsafe { self.instance.create_surface(window) };
        let adapter = self
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::Default,
                compatible_surface: Some(&surface),
            })
            .await
            .context("Failed to get graphics adapter")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS,
                    limits: wgpu::Limits {
                        max_push_constant_size: 4,
                        ..wgpu::Limits::default()
                    },
                    shader_validation: true,
                },
                None, // Trace path
            )
            .await
            .context("Failed to get device")?;

        let sc_desc = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
        };
        let swap_chain = device.create_swap_chain(&surface, &sc_desc);

        use crate::utils::ShallowCopy;
        let bg = self::bg::State::new(
            &device,
            &queue,
            sc_desc.format,
            surface_size,
            self.shader.shallow_copy(),
            screenshot,
        )?;
        let icon =
            self::icon::State::new(&device, &queue, sc_desc.format, surface_size, &self.icon)?;

        Ok(State {
            surface,
            device,
            queue,
            sc_desc,
            swap_chain,
            size,

            bg,
            icon,
        })
    }
}

pub struct State {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    sc_desc: wgpu::SwapChainDescriptor,
    swap_chain: wgpu::SwapChain,
    size: winit::dpi::PhysicalSize<u32>,

    bg: self::bg::State,
    icon: self::icon::State,
}

impl State {
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.sc_desc.width = new_size.width;
        self.sc_desc.height = new_size.height;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);

        self.bg.resize(&self.queue, new_size);
        self.icon.resize(&self.queue, new_size);
    }

    pub fn render(&mut self, time: f32) {
        let frame = self
            .swap_chain
            .get_current_frame()
            .expect("Timeout getting texture")
            .output;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        self.bg.render(&mut encoder, &frame, time);
        self.icon.render(&mut encoder, &frame);

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
