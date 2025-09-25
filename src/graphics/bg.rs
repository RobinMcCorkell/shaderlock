use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use sctk::reexports::client::protocol::wl_shm::Format;
use wgpu::util::DeviceExt;

use crate::screencopy::ScreencopyBuffer;

use super::RenderContext;

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

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct FrameUniforms {
    elapsed: f32,
    fade_amount: f32,
}
unsafe impl bytemuck::Pod for FrameUniforms {}
unsafe impl bytemuck::Zeroable for FrameUniforms {}

impl From<RenderContext> for FrameUniforms {
    fn from(ctx: RenderContext) -> Self {
        Self {
            elapsed: ctx.elapsed.as_secs_f32(),
            fade_amount: ctx.fade_amount,
        }
    }
}

pub const PUSH_CONSTANTS_SIZE: u32 = std::mem::size_of::<FrameUniforms>() as u32;

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
        shader: wgpu::ShaderSource,
        screenshot: ScreencopyBuffer,
    ) -> Result<Self> {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
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
                stages: wgpu::ShaderStages::FRAGMENT,
                range: 0..PUSH_CONSTANTS_SIZE,
            }],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("BG Render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &device
                    .create_shader_module(wgpu::include_spirv!("../../resources/bg.vert.spv")),
                entry_point: Some(VS_MAIN),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("shader"),
                    source: shader,
                }),
                entry_point: Some(FS_MAIN),
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            depth_stencil: None,
            multiview: None,
            cache: None,
        });

        let texture_size = wgpu::Extent3d {
            width: screenshot.width(),
            height: screenshot.height(),
            depth_or_array_layers: 1,
        };
        let texture_descriptor = wgpu::TextureDescriptor {
            label: Some("Screenshot"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format_from_sctk(screenshot.format()),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let texture = device.create_texture(&texture_descriptor);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            address_mode_w: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let stride = screenshot.stride();
        let height = screenshot.height();
        let width = screenshot.width();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            screenshot.bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let texture_transform =
            cgmath::Matrix4::from_translation(cgmath::Vector3::new(0.5, 0.5, 0.0))
                * screenshot.transform_matrix()
                * cgmath::Matrix4::from_translation(cgmath::Vector3::new(-0.5, -0.5, 0.0));
        let uniforms = Uniforms {
            transform: texture_transform,
        };

        let uniforms_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniforms Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
                    resource: uniforms_buffer.as_entire_binding(),
                },
            ],
            label: Some("bg bind group"),
        });

        let uniforms_handle = UniformsHandle {
            data: uniforms,
            texture_transform,
            buffer: uniforms_buffer,
        };

        Ok(Self {
            pipeline,
            bind_group,
            uniforms_handle,
        })
    }
    pub fn resize(&mut self, queue: &wgpu::Queue, resolution_transform: cgmath::Matrix4<f32>) {
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
        view: &wgpu::TextureView,
        ctx: RenderContext,
    ) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("BG render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind_group, &[]); // NEW!
        rp.set_push_constants(
            wgpu::ShaderStages::FRAGMENT,
            0,
            bytemuck::cast_slice(&[FrameUniforms::from(ctx)]),
        );
        rp.draw(0..4, 0..1);
    }
}

fn texture_format_from_sctk(f: Format) -> wgpu::TextureFormat {
    use wgpu::TextureFormat::*;
    use Format::*;
    match f {
        Argb8888 | Xrgb8888 => Bgra8UnormSrgb,
        Xbgr8888 | Abgr8888 => Rgba8UnormSrgb,
        _ => panic!("Unsupported format: {:?}", f),
    }
}
