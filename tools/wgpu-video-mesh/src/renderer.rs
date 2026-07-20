use std::{
    num::NonZeroU64,
    sync::{Mutex, mpsc::Receiver},
    time::{Duration, Instant},
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
    video::{
        MAX_PLAYBACK_SPEED, MIN_PLAYBACK_SPEED, PlaybackStats, VIDEO_FPS, VIDEO_HEIGHT,
        VIDEO_WIDTH, VideoFrame,
    },
};

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Plane,
    Cube,
    Circle,
    UvSphere,
    Icosphere,
    Cylinder,
    Cone,
    Torus,
    Grid,
    Monkey,
}

impl MeshKind {
    pub const ALL: [Self; 10] = [
        Self::Plane,
        Self::Cube,
        Self::Circle,
        Self::UvSphere,
        Self::Icosphere,
        Self::Cylinder,
        Self::Cone,
        Self::Torus,
        Self::Grid,
        Self::Monkey,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Plane => "Plane",
            Self::Cube => "Cube",
            Self::Circle => "Circle",
            Self::UvSphere => "UV Sphere",
            Self::Icosphere => "Icosphere",
            Self::Cylinder => "Cylinder",
            Self::Cone => "Cone",
            Self::Torus => "Torus",
            Self::Grid => "Grid",
            Self::Monkey => "Monkey (Suzanne)",
        }
    }

    fn build(self) -> MeshData {
        match self {
            Self::Plane => mesh::plane(),
            Self::Cube => mesh::cube(),
            Self::Circle => mesh::circle(64),
            Self::UvSphere => mesh::uv_sphere(64, 40),
            Self::Icosphere => mesh::icosphere(2),
            Self::Cylinder => mesh::cylinder(64),
            Self::Cone => mesh::cone(64),
            Self::Torus => mesh::torus(64, 24),
            Self::Grid => mesh::grid(12, 12),
            Self::Monkey => mesh::suzanne(),
        }
    }
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
    pub noise_weight: f32,
    pub noise_time: f32,
    pub object_tint: [f32; 3],
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneUniform {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    tuning: [f32; 4],
    tint: [f32; 4],
    warp: [f32; 4],
}

pub struct SceneCallback {
    pub mesh: MeshKind,
    pub parameters: SceneParameters,
    pub playing: bool,
    pub playback_speed: f32,
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
            resources.prepare(queue, self.parameters, self.playing, self.playback_speed);
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
    meshes: Vec<GpuMesh>,
    frames: Mutex<Receiver<VideoFrame>>,
    stats: std::sync::Arc<PlaybackStats>,
    uploaded_frames: u64,
    next_frame_due: Option<Instant>,
    playback_speed: f32,
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

        let meshes = MeshKind::ALL
            .into_iter()
            .map(|kind| GpuMesh::new(device, kind.label(), kind.build()))
            .collect();

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            video_texture,
            meshes,
            frames: Mutex::new(frames),
            stats,
            uploaded_frames: 0,
            next_frame_due: None,
            playback_speed: 1.0,
        }
    }

    pub fn replace_video(
        &mut self,
        queue: &wgpu::Queue,
        frames: Receiver<VideoFrame>,
        stats: std::sync::Arc<PlaybackStats>,
    ) {
        self.frames = Mutex::new(frames);
        self.stats = stats;
        self.uploaded_frames = 0;
        self.next_frame_due = None;
        write_placeholder(queue, &self.video_texture);
    }

    fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        parameters: SceneParameters,
        playing: bool,
        playback_speed: f32,
    ) {
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
            tint: [
                parameters.object_tint[0],
                parameters.object_tint[1],
                parameters.object_tint[2],
                1.0,
            ],
            warp: [parameters.noise_weight, parameters.noise_time, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        let now = Instant::now();
        let speed = playback_speed.clamp(MIN_PLAYBACK_SPEED, MAX_PLAYBACK_SPEED);
        let frame_interval = Duration::from_secs_f32(1.0 / (VIDEO_FPS as f32 * speed));
        if (speed - self.playback_speed).abs() > f32::EPSILON {
            self.playback_speed = speed;
            self.next_frame_due = Some(now);
        }
        let frame_is_due = self.next_frame_due.is_none_or(|deadline| now >= deadline);

        if playing && frame_is_due {
            let mut next_frame = None;
            if let Ok(frames) = self.frames.get_mut() {
                next_frame = frames.try_recv().ok();
            }
            if let Some(frame) = next_frame {
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
                let scheduled_next = self.next_frame_due.unwrap_or(now) + frame_interval;
                self.next_frame_due =
                    Some(if now.saturating_duration_since(scheduled_next) >= frame_interval {
                        now + frame_interval
                    } else {
                        scheduled_next
                    });
            }
        }
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'_>, kind: MeshKind) {
        let mesh = &self.meshes[kind as usize];

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
