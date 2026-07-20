use std::{
    num::NonZeroU64,
    sync::{Mutex, mpsc::Receiver},
};

use bytemuck::{Pod, Zeroable};
use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt as _;

use crate::{
    mesh::{self, MeshData, Vertex},
    video::{PlaybackStats, VIDEO_HEIGHT, VIDEO_WIDTH, VideoFrame},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Sphere,
    Cube,
}

#[derive(Clone, Copy)]
pub struct SceneParameters {
    pub yaw: f32,
    pub tilt: f32,
    pub size: f32,
    pub camera_distance: f32,
    pub exposure: f32,
    pub lighting: f32,
    pub saturation: f32,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneUniform {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    tuning: [f32; 4],
}

pub struct SceneCallback {
    pub mesh: MeshKind,
    pub parameters: SceneParameters,
    pub playing: bool,
}

impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(resources) = resources.get_mut::<SceneRenderResources>() {
            resources.prepare(queue, self.parameters, self.playing);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(resources) = resources.get::<SceneRenderResources>() {
            resources.paint(render_pass, self.mesh);
        }
    }
}

struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, label: &str, mesh: MeshData) -> Self {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label} vertices")),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label} indices")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        }
    }
}

pub struct SceneRenderResources {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    video_texture: wgpu::Texture,
    sphere: GpuMesh,
    cube: GpuMesh,
    frames: Mutex<Receiver<VideoFrame>>,
    stats: std::sync::Arc<PlaybackStats>,
    uploaded_frames: u64,
}

impl SceneRenderResources {
    pub fn new(
        render_state: &egui_wgpu::RenderState,
        frames: Receiver<VideoFrame>,
        stats: std::sync::Arc<PlaybackStats>,
    ) -> Self {
        let device = &render_state.device;
        let queue = &render_state.queue;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video mesh uniforms"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let video_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("looping video texture"),
            size: wgpu::Extent3d {
                width: VIDEO_WIDTH,
                height: VIDEO_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = video_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video texture sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        write_placeholder(queue, &video_texture);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video mesh bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<SceneUniform>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video mesh bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video mesh pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video mesh pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(render_state.target_format.into())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            video_texture,
            sphere: GpuMesh::new(device, "sphere", mesh::uv_sphere(64, 40)),
            cube: GpuMesh::new(device, "cube", mesh::cube()),
            frames: Mutex::new(frames),
            stats,
            uploaded_frames: 0,
        }
    }

    fn prepare(&mut self, queue: &wgpu::Queue, parameters: SceneParameters, playing: bool) {
        let model = Mat4::from_rotation_y(parameters.yaw)
            * Mat4::from_rotation_x(parameters.tilt)
            * Mat4::from_scale(Vec3::splat(parameters.size));
        let view =
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, parameters.camera_distance), Vec3::ZERO, Vec3::Y);
        let projection = Mat4::perspective_rh(
            44.0_f32.to_radians(),
            parameters.aspect_ratio.max(0.05),
            0.1,
            100.0,
        );
        let uniform = SceneUniform {
            mvp: (projection * view * model).to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            tuning: [
                parameters.exposure,
                parameters.lighting,
                parameters.saturation,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        if playing {
            let mut newest_frame = None;
            if let Ok(frames) = self.frames.get_mut() {
                while let Ok(frame) = frames.try_recv() {
                    newest_frame = Some(frame);
                }
            }
            if let Some(frame) = newest_frame {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.video_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &frame.rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(VIDEO_WIDTH * 4),
                        rows_per_image: Some(VIDEO_HEIGHT),
                    },
                    wgpu::Extent3d {
                        width: VIDEO_WIDTH,
                        height: VIDEO_HEIGHT,
                        depth_or_array_layers: 1,
                    },
                );
                self.uploaded_frames += 1;
                self.stats.set_uploaded_frames(self.uploaded_frames);
            }
        }
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'_>, kind: MeshKind) {
        let mesh = match kind {
            MeshKind::Sphere => &self.sphere,
            MeshKind::Cube => &self.cube,
        };

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..mesh.index_count, 0, 0..1);
    }
}

fn write_placeholder(queue: &wgpu::Queue, texture: &wgpu::Texture) {
    let mut rgba = vec![0_u8; (VIDEO_WIDTH * VIDEO_HEIGHT * 4) as usize];
    for y in 0..VIDEO_HEIGHT {
        for x in 0..VIDEO_WIDTH {
            let checker = ((x / 48) + (y / 48)) % 2;
            let color = if checker == 0 {
                [17, 28, 48, 255]
            } else {
                [24, 44, 72, 255]
            };
            let offset = ((y * VIDEO_WIDTH + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&color);
        }
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(VIDEO_WIDTH * 4),
            rows_per_image: Some(VIDEO_HEIGHT),
        },
        wgpu::Extent3d {
            width: VIDEO_WIDTH,
            height: VIDEO_HEIGHT,
            depth_or_array_layers: 1,
        },
    );
}
