mod bg;
mod icon;

use std::time::Duration;

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
        let compiler = shaderc::Compiler::new().context("Failed to create shader compiler")?;
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

        let surface = unsafe { self.instance.create_surface(window) };
        let adapter = self
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
            })
            .await
            .context("Failed to get graphics adapter")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::PUSH_CONSTANTS,
                    limits: wgpu::Limits {
                        max_push_constant_size: self::bg::PUSH_CONSTANTS_SIZE,
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
            self.shader.shallow_copy(),
            screenshot,
        )?;
        let icon = self::icon::State::new(&device, &queue, sc_desc.format, &self.icon)?;

        let mut me = State {
            surface,
            device,
            queue,
            sc_desc,
            swap_chain,

            bg,
            icon,
        };

        me.resize(size);
        Ok(me)
    }
}

pub struct State {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    sc_desc: wgpu::SwapChainDescriptor,
    swap_chain: wgpu::SwapChain,

    bg: self::bg::State,
    icon: self::icon::State,
}

impl State {
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.sc_desc.width = new_size.width;
        self.sc_desc.height = new_size.height;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);

        let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
            1.0 / new_size.width as f32,
            1.0 / new_size.height as f32,
            1.0,
        );

        self.bg.resize(&self.queue, resolution_transform);
        self.icon.resize(&self.queue, resolution_transform);
    }

    pub fn render(&mut self, ctx: RenderContext) {
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

        self.bg.render(&mut encoder, &frame, ctx);
        self.icon.render(&mut encoder, &frame);

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}

pub struct RenderContext {
    pub elapsed: Duration,
    pub fade_amount: f32,
}
