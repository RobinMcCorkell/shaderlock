use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use wgpu::util::DeviceExt;

pub const VS_MAIN: &str = "main";
pub const FS_MAIN: &str = "main";

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

pub struct State {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms_handle: UniformsHandle,
}

impl State {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        swapchain_format: wgpu::TextureFormat,
        surface_size: (u32, u32),
        icon: &image::RgbaImage,
    ) -> Result<Self> {
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
                    .create_shader_module(wgpu::include_spirv!("../../resources/icon.vert.spv")),
                entry_point: VS_MAIN,
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &device
                    .create_shader_module(wgpu::include_spirv!("../../resources/icon.frag.spv")),
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

        Ok(Self {
            pipeline,
            bind_group,
            uniforms_handle,
        })
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, new_size: winit::dpi::PhysicalSize<u32>) {
        let resolution_transform = cgmath::Matrix4::from_nonuniform_scale(
            1.0 / new_size.width as f32,
            1.0 / new_size.height as f32,
            1.0,
        );

        self.uniforms_handle.data.transform =
            self.uniforms_handle.texture_transform * resolution_transform;
        queue.write_buffer(
            &self.uniforms_handle.buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms_handle.data]),
        );
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::SwapChainTexture,
    ) {
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
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind_group, &[]); // NEW!
        rp.draw(0..4, 0..1);
    }
}
