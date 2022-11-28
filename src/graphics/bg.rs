use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use wgpu::util::DeviceExt;

pub const VS_MAIN: &str = "main";
pub const FS_MAIN: &str = "main";

const MIPMAP_LEVELS: u32 = 8;

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
        shader: wgpu::ShaderModuleSource,
        screenshot: crate::screengrab::Buffer,
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
                module: &device
                    .create_shader_module(wgpu::include_spirv!("../../resources/bg.vert.spv")),
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
        let texture_descriptor = wgpu::TextureDescriptor {
            label: Some("Screenshot"),
            size: texture_size,
            mip_level_count: MIPMAP_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format_from_sctk(screenshot.format()),
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
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
            wgpu::TextureCopyView {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            screenshot.as_bytes(),
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

        use wgpu_mipmap::MipmapGenerator;
        let mipmap_generator = wgpu_mipmap::RecommendedMipmapGenerator::new(&device);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        mipmap_generator.generate(&device, &mut encoder, &texture, &texture_descriptor)?;
        queue.submit(std::iter::once(encoder.finish()));

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
        frame: &wgpu::SwapChainTexture,
        time: f32,
    ) {
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
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind_group, &[]); // NEW!
        rp.set_push_constants(
            wgpu::ShaderStage::FRAGMENT,
            0,
            bytemuck::cast_slice(&[FrameUniforms { time }]),
        );
        rp.draw(0..4, 0..1);
    }
}

fn texture_format_from_sctk(f: sctk::shm::Format) -> wgpu::TextureFormat {
    use sctk::shm::Format::*;
    use wgpu::TextureFormat::*;
    match f {
        Argb8888 | Xrgb8888 => Bgra8UnormSrgb,
        Xbgr8888 | Abgr8888 => Rgba8UnormSrgb,
        _ => panic!("Unsupported format: {:?}", f),
    }
}
