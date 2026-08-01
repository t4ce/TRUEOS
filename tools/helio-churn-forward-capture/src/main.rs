//! Build-time capture of a minimal, genuinely instanced Helio forward frame.
//!
//! Unlike the SimpleCube capture this records Helio's actual camera, instance,
//! compacted-index, and indexed-indirect contracts. Geometry remains immutable;
//! only model matrices and the canonical indirect record describe the scene.

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use helio_artifact::{Artifact, Builder, DynamicSlot, Manifest, SectionKind};
use libhelio::{DrawIndexedIndirectArgs, GpuCameraUniforms, GpuInstanceData};
use wgpu::util::DeviceExt;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const INSTANCE_COUNT: usize = 32;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADER_SECTION: &str = "render/churn-forward.wgsl";
const SCENE_SECTION: &str = "scene/churn-forward-v1.bin";
const SHADER: &str = include_str!("../shaders/churn_forward.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

fn main() -> Result<(), Box<dyn Error>> {
    validate_host_abis()?;

    let output = output_path()?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let trace_dir = parent.join(format!(".churn-forward-wgpu-trace-{}", std::process::id()));
    if trace_dir.exists() {
        return Err(format!("temporary trace path already exists: {}", trace_dir.display()).into());
    }
    fs::create_dir(&trace_dir)?;

    let scene = SceneSeed::new();
    let adapter = capture_one_frame(&trace_dir, &scene)?;
    let trace_files = collect_files(&trace_dir)?;
    if trace_files.is_empty() {
        return Err("wgpu produced an empty trace".into());
    }
    verify_trace(&trace_files)?;

    let manifest = Manifest {
        schema: 1,
        engine: "Helio".into(),
        program: "churn-forward".into(),
        graph: "Helio ForwardLit-derived single pass".into(),
        capture: "wgpu-native-trace-v30".into(),
        target_api: "trueos-render".into(),
        target_architecture: "intel-xe-lp".into(),
        surface_format: format!("{FORMAT:?}"),
        width: WIDTH,
        height: HEIGHT,
        dynamic_slots: vec![
            DynamicSlot {
                name: "camera".into(),
                kind: "libhelio::GpuCameraUniforms[1]".into(),
            },
            DynamicSlot {
                name: "scene.instances".into(),
                kind: "libhelio::GpuInstanceData[]".into(),
            },
            DynamicSlot {
                name: "scene.compacted_indices".into(),
                kind: "u32[]".into(),
            },
            DynamicSlot {
                name: "draw.indexed_indirect".into(),
                kind: "libhelio::DrawIndexedIndirectArgs".into(),
            },
            DynamicSlot {
                name: "output.surface".into(),
                kind: "ui4-bgra8-srgb-alpha".into(),
            },
        ],
    };

    let mut builder = Builder::new(&manifest)?;
    builder.add(
        SectionKind::CompilerMetadata,
        "capture/adapter.txt",
        adapter.as_bytes().to_vec(),
    )?;
    builder.add(SectionKind::ShaderSource, SHADER_SECTION, SHADER.as_bytes().to_vec())?;
    builder.add(SectionKind::Other, SCENE_SECTION, scene.encode())?;
    for (relative, data) in trace_files {
        builder.add(
            SectionKind::WgpuTrace,
            format!("wgpu/{}", relative.replace('\\', "/")),
            data,
        )?;
    }

    let bytes = builder.finish()?;
    let parsed = Artifact::parse(&bytes)?;
    for required in [SHADER_SECTION, SCENE_SECTION, "wgpu/trace.ron"] {
        if parsed.section(required).is_none() {
            return Err(format!("captured artifact is missing {required}").into());
        }
    }
    let section_count = parsed.sections().count();
    fs::write(&output, bytes)?;
    fs::remove_dir_all(&trace_dir)?;

    println!(
        "captured {} ({} sections, {} instances, adapter: {})",
        output.display(),
        section_count,
        INSTANCE_COUNT,
        adapter.lines().next().unwrap_or("unknown")
    );
    Ok(())
}

fn validate_host_abis() -> Result<(), Box<dyn Error>> {
    let actual = [
        ("GpuCameraUniforms", std::mem::size_of::<GpuCameraUniforms>(), 368),
        ("GpuInstanceData", std::mem::size_of::<GpuInstanceData>(), 208),
        ("DrawIndexedIndirectArgs", std::mem::size_of::<DrawIndexedIndirectArgs>(), 20),
        ("Vertex", std::mem::size_of::<Vertex>(), 24),
    ];
    for (name, found, expected) in actual {
        if found != expected {
            return Err(
                format!("{name} ABI drift: expected {expected} bytes, found {found}").into()
            );
        }
    }
    Ok(())
}

fn output_path() -> Result<PathBuf, Box<dyn Error>> {
    let mut args = env::args_os();
    let _program = args.next();
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/helio-artifacts/churn-forward.trueos.helio"));
    if args.next().is_some() {
        return Err("usage: helio-churn-forward-capture [output.helio]".into());
    }
    Ok(output)
}

struct SceneSeed {
    vertices: [Vertex; 24],
    indices: [u32; 36],
    camera: GpuCameraUniforms,
    instances: [GpuInstanceData; INSTANCE_COUNT],
    compacted_indices: [u32; INSTANCE_COUNT],
    indirect: DrawIndexedIndirectArgs,
}

impl SceneSeed {
    fn new() -> Self {
        let (vertices, indices) = unit_cube();
        let eye = Vec3::new(0.0, 3.2, 8.0);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let proj = Mat4::perspective_rh(
            std::f32::consts::FRAC_PI_4,
            WIDTH as f32 / HEIGHT as f32,
            0.05,
            100.0,
        );
        let camera =
            GpuCameraUniforms::new(view, proj, eye, 0.05, 100.0, 0, [0.0, 0.0], proj * view);
        let instances = std::array::from_fn(|index| {
            let lane = index % 16;
            let layer = index / 16;
            let angle = lane as f32 * std::f32::consts::TAU / 16.0;
            let radius = 2.15 + layer as f32 * 0.65;
            let position = Vec3::new(
                angle.cos() * radius,
                (angle * 2.0).sin() * 0.28 + (layer as f32 - 0.5) * 0.55,
                angle.sin() * radius,
            );
            let rotation =
                Mat4::from_rotation_y(-angle + 0.55) * Mat4::from_rotation_x(angle * 0.37);
            let scale = Mat4::from_scale(Vec3::splat(0.36));
            let model = Mat4::from_translation(position) * rotation * scale;
            gpu_instance(model, index as u32 & 3)
        });
        let compacted_indices = std::array::from_fn(|index| index as u32);
        let indirect = DrawIndexedIndirectArgs {
            index_count: indices.len() as u32,
            instance_count: INSTANCE_COUNT as u32,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        };
        Self {
            vertices,
            indices,
            camera,
            instances,
            compacted_indices,
            indirect,
        }
    }

    /// Pointer-free, little-endian seed image used to validate the runtime ABI.
    /// Header (96 bytes) carries strides/counts followed by six absolute offsets.
    fn encode(&self) -> Vec<u8> {
        const HEADER_LEN: usize = 96;
        let mut output = vec![0u8; HEADER_LEN];
        output[0..8].copy_from_slice(b"HCFWD1\0\0");
        put_u16(&mut output, 8, 1);
        put_u16(&mut output, 10, HEADER_LEN as u16);
        put_u32(&mut output, 12, WIDTH);
        put_u32(&mut output, 16, HEIGHT);

        let layouts = [
            (std::mem::size_of::<Vertex>(), self.vertices.len()),
            (std::mem::size_of::<u32>(), self.indices.len()),
            (std::mem::size_of::<GpuCameraUniforms>(), 1),
            (std::mem::size_of::<GpuInstanceData>(), self.instances.len()),
            (std::mem::size_of::<u32>(), self.compacted_indices.len()),
            (std::mem::size_of::<DrawIndexedIndirectArgs>(), 1),
        ];
        for (index, (stride, count)) in layouts.into_iter().enumerate() {
            put_u32(&mut output, 20 + index * 8, stride as u32);
            put_u32(&mut output, 24 + index * 8, count as u32);
        }

        let payloads: [&[u8]; 6] = [
            bytemuck::cast_slice(&self.vertices),
            bytemuck::cast_slice(&self.indices),
            bytemuck::bytes_of(&self.camera),
            bytemuck::cast_slice(&self.instances),
            bytemuck::cast_slice(&self.compacted_indices),
            bytemuck::bytes_of(&self.indirect),
        ];
        for (index, payload) in payloads.into_iter().enumerate() {
            let offset = output.len();
            put_u32(&mut output, 68 + index * 4, offset as u32);
            output.extend_from_slice(payload);
            if index + 1 != payloads.len() {
                output.resize(align_16(output.len()), 0);
            }
        }
        output
    }
}

fn capture_one_frame(trace_dir: &Path, scene: &SceneSeed) -> Result<String, Box<dyn Error>> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Helio Churn Forward Capture Device"),
        trace: wgpu::Trace::Directory(trace_dir.to_path_buf()),
        ..Default::default()
    }))?;
    device.on_uncaptured_error(std::sync::Arc::new(|error| {
        panic!("Helio Churn forward capture GPU error: {error:?}");
    }));

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Helio Churn Forward Shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Helio Churn Forward BGL 0"),
        entries: &[
            storage_layout_entry(0, wgpu::ShaderStages::VERTEX),
            storage_layout_entry(1, wgpu::ShaderStages::VERTEX),
            storage_layout_entry(2, wgpu::ShaderStages::VERTEX),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Helio Churn Forward Pipeline Layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Helio Churn Forward Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let vertex_buffer = buffer_init(
        &device,
        "Helio Churn Forward Unit Cube VB",
        bytemuck::cast_slice(&scene.vertices),
        wgpu::BufferUsages::VERTEX,
    );
    let index_buffer = buffer_init(
        &device,
        "Helio Churn Forward Unit Cube IB",
        bytemuck::cast_slice(&scene.indices),
        wgpu::BufferUsages::INDEX,
    );
    let camera_buffer = buffer_init(
        &device,
        "Helio Churn Forward Camera",
        bytemuck::bytes_of(&scene.camera),
        wgpu::BufferUsages::STORAGE,
    );
    let instance_buffer = buffer_init(
        &device,
        "Helio Churn Forward GpuInstanceData",
        bytemuck::cast_slice(&scene.instances),
        wgpu::BufferUsages::STORAGE,
    );
    let compacted_buffer = buffer_init(
        &device,
        "Helio Churn Forward Compacted Indices",
        bytemuck::cast_slice(&scene.compacted_indices),
        wgpu::BufferUsages::STORAGE,
    );
    let indirect_buffer = buffer_init(
        &device,
        "Helio Churn Forward DrawIndexedIndirectArgs",
        bytemuck::bytes_of(&scene.indirect),
        wgpu::BufferUsages::INDIRECT,
    );
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Helio Churn Forward BG 0"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instance_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: compacted_buffer.as_entire_binding(),
            },
        ],
    });

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Helio Churn Forward Transparent Output"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Helio Churn Forward Depth"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Helio Churn Forward Encoder"),
    });
    {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &target_view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Helio Churn Forward Pass"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed_indirect(&indirect_buffer, 0);
    }
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::wait_indefinitely())?;

    drop(depth_view);
    drop(target_view);
    drop(depth);
    drop(target);
    drop(bind_group);
    drop(indirect_buffer);
    drop(compacted_buffer);
    drop(instance_buffer);
    drop(camera_buffer);
    drop(index_buffer);
    drop(vertex_buffer);
    drop(pipeline);
    drop(pipeline_layout);
    drop(bind_group_layout);
    drop(shader);
    drop(queue);
    drop(device);
    drop(adapter);
    drop(instance);

    Ok(format!(
        "name={}\nbackend={:?}\ndevice_type={:?}\ndriver={}\ncamera_stride={}\ninstance_stride={}\nindirect_stride={}\n",
        info.name,
        info.backend,
        info.device_type,
        info.driver,
        std::mem::size_of::<GpuCameraUniforms>(),
        std::mem::size_of::<GpuInstanceData>(),
        std::mem::size_of::<DrawIndexedIndirectArgs>(),
    ))
}

fn storage_layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn buffer_init(
    device: &wgpu::Device,
    label: &'static str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage,
    })
}

fn gpu_instance(model: Mat4, material_id: u32) -> GpuInstanceData {
    let normal = Mat3::from_mat4(model).inverse().transpose().to_cols_array();
    let translation = model.w_axis.truncate();
    GpuInstanceData {
        model: model.to_cols_array(),
        normal_mat: [
            normal[0], normal[1], normal[2], 0.0, normal[3], normal[4], normal[5], 0.0, normal[6],
            normal[7], normal[8], 0.0,
        ],
        bounds: [translation.x, translation.y, translation.z, 0.312],
        prev_model: model.to_cols_array(),
        mesh_id: 0,
        material_id,
        flags: 0,
        lightmap_index: u32::MAX,
    }
}

fn unit_cube() -> ([Vertex; 24], [u32; 36]) {
    let p = 0.5;
    let mut vertices = [Vertex::zeroed(); 24];
    let faces = [
        ([0.0, 0.0, 1.0], [[-p, -p, p], [p, -p, p], [p, p, p], [-p, p, p]]),
        ([0.0, 0.0, -1.0], [[p, -p, -p], [-p, -p, -p], [-p, p, -p], [p, p, -p]]),
        ([1.0, 0.0, 0.0], [[p, -p, p], [p, -p, -p], [p, p, -p], [p, p, p]]),
        ([-1.0, 0.0, 0.0], [[-p, -p, -p], [-p, -p, p], [-p, p, p], [-p, p, -p]]),
        ([0.0, 1.0, 0.0], [[-p, p, p], [p, p, p], [p, p, -p], [-p, p, -p]]),
        ([0.0, -1.0, 0.0], [[-p, -p, -p], [p, -p, -p], [p, -p, p], [-p, -p, p]]),
    ];
    for (face_index, (normal, positions)) in faces.into_iter().enumerate() {
        for (corner, position) in positions.into_iter().enumerate() {
            vertices[face_index * 4 + corner] = Vertex { position, normal };
        }
    }
    let mut indices = [0u32; 36];
    for face in 0..6 {
        let vertex = (face * 4) as u32;
        let offset = face * 6;
        indices[offset..offset + 6].copy_from_slice(&[
            vertex,
            vertex + 1,
            vertex + 2,
            vertex,
            vertex + 2,
            vertex + 3,
        ]);
    }
    (vertices, indices)
}

fn collect_files(root: &Path) -> Result<Vec<(String, Vec<u8>)>, Box<dyn Error>> {
    fn visit(
        root: &Path,
        current: &Path,
        output: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<(), Box<dyn Error>> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                visit(root, &path, output)?;
            } else if file_type.is_file() {
                let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
                output.push((relative, fs::read(path)?));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn verify_trace(files: &[(String, Vec<u8>)]) -> Result<(), Box<dyn Error>> {
    let trace = files
        .iter()
        .find(|(name, _)| name == "trace.ron")
        .ok_or("wgpu trace has no trace.ron")?;
    let trace = std::str::from_utf8(&trace.1)?;
    for required in [
        "Helio Churn Forward Shader",
        "Helio Churn Forward Pipeline",
        "Helio Churn Forward GpuInstanceData",
        "Helio Churn Forward DrawIndexedIndirectArgs",
        "DrawIndirect",
        "DrawIndexed",
    ] {
        if !trace.contains(required) {
            return Err(format!("wgpu trace is missing required event: {required}").into());
        }
    }
    Ok(())
}

fn align_16(value: usize) -> usize {
    (value + 15) & !15
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
