use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use wgpu::util::DeviceExt;

const VS_MAIN: &str = "main";
const FS_MAIN: &str = "main";

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct Uniforms {
    transform: cgmath::Matrix4<f32>,
}
unsafe impl bytemuck::Pod for Uniforms {}
unsafe impl bytemuck::Zeroable for Uniforms {}

struct UniformsHandle {
    data: Uniforms,
    texture_transform: cgmath::Matrix4<f32>,
    buffer: wgpu::Buffer,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct FrameUniforms {
    time: f32,
}
unsafe impl bytemuck::Pod for FrameUniforms {}
unsafe impl bytemuck::Zeroable for FrameUniforms {}

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
                FS_MAIN,
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
        mut screenshot: crate::screengrab::Buffer,
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
        let (bg_render_pipeline, bg_bind_group, bg_uniforms_handle) = create_bg_pipeline(
            &device,
            &queue,
            sc_desc.format,
            surface_size,
            self.shader.shallow_copy(),
            screenshot,
        )?;

        let (icon_render_pipeline, icon_bind_group, icon_uniforms_handle) =
            create_icon_pipeline(&device, &queue, sc_desc.format, surface_size, &self.icon)?;

        Ok(State {
            surface,
            device,
            queue,
            sc_desc,
            swap_chain,
            size,

            bg_render_pipeline,
            bg_bind_group,
            bg_uniforms_handle,

            icon_render_pipeline,
            icon_bind_group,
            icon_uniforms_handle,
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

    bg_render_pipeline: wgpu::RenderPipeline,
    bg_bind_group: wgpu::BindGroup,
    bg_uniforms_handle: UniformsHandle,

    icon_render_pipeline: wgpu::RenderPipeline,
    icon_bind_group: wgpu::BindGroup,
    icon_uniforms_handle: UniformsHandle,
}

impl State {
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.sc_desc.width = new_size.width;
        self.sc_desc.height = new_size.height;
        self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);

        let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
            1.0 / new_size.width as f32,
            1.0 / new_size.height as f32,
            1.0,
        );
        self.bg_uniforms_handle.data.transform =
            self.bg_uniforms_handle.texture_transform * resolution_transform;
        self.queue.write_buffer(
            &self.bg_uniforms_handle.buffer,
            0,
            bytemuck::cast_slice(&[self.bg_uniforms_handle.data]),
        );

        self.icon_uniforms_handle.data.transform =
            self.icon_uniforms_handle.texture_transform * resolution_transform;
        self.queue.write_buffer(
            &self.icon_uniforms_handle.buffer,
            0,
            bytemuck::cast_slice(&[self.icon_uniforms_handle.data]),
        );
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

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });
            rp.set_pipeline(&self.bg_render_pipeline);
            rp.set_bind_group(0, &self.bg_bind_group, &[]); // NEW!
            rp.set_push_constants(
                wgpu::ShaderStage::FRAGMENT,
                0,
                bytemuck::cast_slice(&[FrameUniforms { time }]),
            );
            rp.draw(0..4, 0..1);
        }

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });
            rp.set_pipeline(&self.icon_render_pipeline);
            rp.set_bind_group(0, &self.icon_bind_group, &[]); // NEW!
            rp.draw(0..4, 0..1);
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}

fn create_bg_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    swapchain_format: wgpu::TextureFormat,
    surface_size: (u32, u32),
    shader: wgpu::ShaderModuleSource,
    mut screenshot: crate::screengrab::Buffer,
) -> Result<(wgpu::RenderPipeline, wgpu::BindGroup, UniformsHandle)> {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::SampledTexture {
                    multisampled: false,
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Uint,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::Sampler { comparison: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
        label: Some("bind_group_layout"),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("BG Render pipeline layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[wgpu::PushConstantRange {
            stages: wgpu::ShaderStage::FRAGMENT,
            range: 0..4,
        }],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("BG Render pipeline"),
        layout: Some(&pipeline_layout),
        vertex_stage: wgpu::ProgrammableStageDescriptor {
            module: &device.create_shader_module(wgpu::include_spirv!("../resources/bg.vert.spv")),
            entry_point: VS_MAIN,
        },
        fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
            module: &device.create_shader_module(shader),
            entry_point: FS_MAIN,
        }),
        rasterization_state: Some(wgpu::RasterizationStateDescriptor {
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: wgpu::CullMode::None,
            ..wgpu::RasterizationStateDescriptor::default()
        }),
        primitive_topology: wgpu::PrimitiveTopology::TriangleStrip,
        color_states: &[wgpu::ColorStateDescriptor {
            format: swapchain_format,
            color_blend: wgpu::BlendDescriptor::REPLACE,
            alpha_blend: wgpu::BlendDescriptor::REPLACE,
            write_mask: wgpu::ColorWrite::ALL,
        }],
        depth_stencil_state: None,
        vertex_state: wgpu::VertexStateDescriptor {
            index_format: wgpu::IndexFormat::Uint16,
            vertex_buffers: &[],
        },
        sample_count: 1,
        sample_mask: !0,
        alpha_to_coverage_enabled: false,
    });

    let texture_size = wgpu::Extent3d {
        width: screenshot.width(),
        height: screenshot.height(),
        depth: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Screenshot"),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let stride = screenshot.stride();
    let height = screenshot.height();
    let width = screenshot.width();
    queue.write_texture(
        wgpu::TextureCopyView {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
        },
        screenshot.as_bgra(),
        wgpu::TextureDataLayout {
            offset: 0,
            bytes_per_row: stride,
            rows_per_image: height,
        },
        wgpu::Extent3d {
            width: width,
            height: height,
            depth: 1,
        },
    );

    let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
        1.0 / surface_size.0 as f32,
        1.0 / surface_size.1 as f32,
        1.0,
    );
    let texture_transform = cgmath::Matrix4::from_translation(cgmath::Vector3::new(0.5, 0.5, 0.0))
        * screenshot.transform_matrix()
        * cgmath::Matrix4::from_nonuniform_scale(1.0, -1.0, 1.0)
        * cgmath::Matrix4::from_translation(cgmath::Vector3::new(-0.5, -0.5, 0.0));
    let uniforms = Uniforms {
        transform: texture_transform * resolution_transform,
    };

    let uniforms_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Uniforms Buffer"),
        contents: bytemuck::cast_slice(&[uniforms]),
        usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(uniforms_buffer.slice(..)),
            },
        ],
        label: Some("bg bind group"),
    });

    let uniforms_handle = UniformsHandle {
        data: uniforms,
        texture_transform: texture_transform,
        buffer: uniforms_buffer,
    };

    Ok((pipeline, bind_group, uniforms_handle))
}

fn create_icon_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    swapchain_format: wgpu::TextureFormat,
    surface_size: (u32, u32),
    icon: &image::RgbaImage,
) -> Result<(wgpu::RenderPipeline, wgpu::BindGroup, UniformsHandle)> {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::SampledTexture {
                    multisampled: false,
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Uint,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::Sampler { comparison: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStage::VERTEX,
                ty: wgpu::BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
        label: Some("icon bind_group_layout"),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Icon Render pipeline layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Icon Render pipeline"),
        layout: Some(&pipeline_layout),
        vertex_stage: wgpu::ProgrammableStageDescriptor {
            module: &device
                .create_shader_module(wgpu::include_spirv!("../resources/icon.vert.spv")),
            entry_point: VS_MAIN,
        },
        fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
            module: &device
                .create_shader_module(wgpu::include_spirv!("../resources/icon.frag.spv")),
            entry_point: FS_MAIN,
        }),
        rasterization_state: Some(wgpu::RasterizationStateDescriptor {
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: wgpu::CullMode::None,
            ..wgpu::RasterizationStateDescriptor::default()
        }),
        primitive_topology: wgpu::PrimitiveTopology::TriangleStrip,
        color_states: &[wgpu::ColorStateDescriptor {
            format: swapchain_format,
            color_blend: wgpu::BlendDescriptor {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha_blend: wgpu::BlendDescriptor::REPLACE,
            write_mask: wgpu::ColorWrite::ALL,
        }],
        depth_stencil_state: None,
        vertex_state: wgpu::VertexStateDescriptor {
            index_format: wgpu::IndexFormat::Uint16,
            vertex_buffers: &[],
        },
        sample_count: 1,
        sample_mask: !0,
        alpha_to_coverage_enabled: false,
    });

    let texture_size = wgpu::Extent3d {
        width: icon.width(),
        height: icon.height(),
        depth: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Icon"),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    queue.write_texture(
        wgpu::TextureCopyView {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
        },
        icon,
        wgpu::TextureDataLayout {
            offset: 0,
            bytes_per_row: 4 * icon.width(),
            rows_per_image: icon.height(),
        },
        wgpu::Extent3d {
            width: icon.width(),
            height: icon.height(),
            depth: 1,
        },
    );

    let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
        1.0 / surface_size.0 as f32,
        1.0 / surface_size.1 as f32,
        1.0,
    );
    let texture_transform =
        cgmath::Matrix4::from_nonuniform_scale(icon.width() as f32, icon.height() as f32, 1.0);
    let uniforms = Uniforms {
        transform: texture_transform * resolution_transform,
    };

    let uniforms_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Uniforms Buffer"),
        contents: bytemuck::cast_slice(&[uniforms]),
        usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(uniforms_buffer.slice(..)),
            },
        ],
        label: Some("icon bind group"),
    });

    let uniforms_handle = UniformsHandle {
        data: uniforms,
        texture_transform: texture_transform,
        buffer: uniforms_buffer,
    };

    Ok((pipeline, bind_group, uniforms_handle))
}
