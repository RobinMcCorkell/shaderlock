mod bg;
mod icon;

use std::{sync::Arc, time::Duration};

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

pub struct Manager {
    instance: wgpu::Instance,
    shader: wgpu::ShaderSource<'static>,
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
        let shader = wgpu::ShaderSource::SpirV(data.into());

        let icon = image::open(icon_file).context("Failed to read icon file")?;

        Ok(Manager {
            instance: wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..Default::default()
            }),
            shader,
            icon: icon.into_rgba8(),
        })
    }

    pub async fn init_window(
        &self,
        window: Arc<winit::window::Window>,
        screenshot: crate::screengrab::Buffer,
    ) -> Result<State<'static>> {
        let size = window.inner_size();

        let surface = self.instance.create_surface(window).context("Failed to create surface")?;
        let adapter = self
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("Failed to get graphics adapter")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::PUSH_CONSTANTS,
                    required_limits: wgpu::Limits {
                        max_push_constant_size: self::bg::PUSH_CONSTANTS_SIZE,
                        ..wgpu::Limits::default()
                    },
                    memory_hints: Default::default(),
                },
                None, // Trace path
            )
            .await
            .context("Failed to get device")?;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let bg = self::bg::State::new(
            &device,
            &queue,
            surface_config.format,
            self.shader.clone(),
            screenshot,
        )?;
        let icon = self::icon::State::new(&device, &queue, surface_config.format, &self.icon)?;

        let mut me = State {
            surface,
            device,
            queue,
            surface_config,

            bg,
            icon,
        };

        me.resize(size);
        Ok(me)
    }
}
pub struct State<'window> {
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,

    bg: self::bg::State,
    icon: self::icon::State,
}

impl State<'_> {
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);

        let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
            1.0 / new_size.width as f32,
            1.0 / new_size.height as f32,
            1.0,
        );

        self.bg.resize(&self.queue, resolution_transform);
        self.icon.resize(&self.queue, resolution_transform);
    }

    pub fn render(&mut self, ctx: RenderContext) -> wgpu::SurfaceTexture {
        let frame = self.surface.get_current_texture().expect("Timeout getting texture");
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        self.bg.render(&mut encoder, &view, ctx);
        self.icon.render(&mut encoder, &view);

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));

        frame
    }
}

pub struct RenderContext {
    pub elapsed: Duration,
    pub fade_amount: f32,
}
