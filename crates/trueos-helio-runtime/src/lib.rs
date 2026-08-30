#![no_std]
#![forbid(unsafe_code)]

//! Narrow runtime lowering from Helio's normalized render IR to the retained
//! triangle contract already submitted by TRUEOS's Intel Render/GuC path.

extern crate alloc;

pub mod churn;
pub mod picasso_scene;
pub mod retained_transform;
pub mod scene_db;

use alloc::vec::Vec;
use trueos_helio_artifact::render_ir::{
    BindingKind, CompareFunction, CullMode, DrawIndexed, FrontFace, IndexFormat, PrimitiveTopology,
    Program, ShaderStages, TextureFormat, VertexAttribute,
};
use trueos_helio_artifact::replay::ReplayPlan;

const MAX_TRIANGLES: usize = 4_096;

/// The fixed camera used by Helio's build-time `simple_graph` capture.
///
/// It fills the dynamic `camera.view_proj` slot without baking projected cube
/// coordinates into the artifact or into TRUEOS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub vertical_fov_radians: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub const fn helio_simple_graph() -> Self {
        Self {
            position: [0.0, 0.8, 4.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_radians: core::f32::consts::FRAC_PI_4,
            near: 0.01,
            far: 100.0,
        }
    }
}

/// One artifact triangle after dynamic-camera projection.
///
/// TRUEOS's existing renderer accepts one uniform color per resident draw, so
/// normalized IR triangles are kept independent. No scene-specific geometry
/// or topology is synthesized here.
#[derive(Clone, Debug, PartialEq)]
pub struct Triangle {
    pub vertices: [[f32; 3]; 3],
    pub rgba: [u8; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub triangles: Vec<Triangle>,
    pub clear_rgba: [u8; 4],
    /// The one authoritative indexed draw from the normalized Helio program.
    /// When `draw_source` is `ArtifactReplayV1`, this is the record carried by
    /// `render/replay-v1.bin`, after it has been cross-checked against the IR.
    pub source_draw_indexed_indirect: DrawIndexedIndirectArgs,
    pub draw_source: DrawCommandSource,
}

impl Scene {
    /// Lower one source triangle to the current constant-color Intel backend.
    ///
    /// The artifact replay record remains authoritative for selecting and
    /// validating the source index range. The retained renderer currently
    /// specializes one uniform color per draw, so each decoded triangle owns a
    /// rebased three-index resident mesh and therefore a local three-index
    /// indirect record. This can disappear once per-vertex color reaches the
    /// retained backend.
    pub fn resident_triangle_draw_indexed_indirect(
        &self,
        triangle_index: usize,
    ) -> Result<DrawIndexedIndirectArgs, Error> {
        let source_indices = usize::try_from(self.source_draw_indexed_indirect.index_count)
            .map_err(|_| Error::IndexOutOfRange)?;
        let source_triangles = source_indices / 3;
        if triangle_index >= self.triangles.len() || triangle_index >= source_triangles {
            return Err(Error::IndexOutOfRange);
        }
        Ok(DrawIndexedIndirectArgs::new(3))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawCommandSource {
    /// Compatibility path for older artifacts without a replay section.
    RenderIrFallback,
    /// Strictly validated `render/replay-v1.bin` supplied the source record.
    ArtifactReplayV1,
}

impl DrawCommandSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::RenderIrFallback => "render-ir-fallback",
            Self::ArtifactReplayV1 => "artifact-replay-v1",
        }
    }
}

/// Helio's native GPU draw record, byte-for-byte compatible with
/// `wgpu::util::DrawIndexedIndirectArgs` and the output of Helio's
/// `IndirectDispatchPass`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawIndexedIndirectArgs {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

impl DrawIndexedIndirectArgs {
    pub const BYTE_LEN: usize = 5 * core::mem::size_of::<u32>();

    pub const fn new(index_count: u32) -> Self {
        Self {
            index_count,
            instance_count: 1,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        }
    }

    pub const fn from_render_ir(draw: DrawIndexed) -> Self {
        Self {
            index_count: draw.index_count,
            instance_count: draw.instance_count,
            first_index: draw.first_index,
            base_vertex: draw.base_vertex,
            first_instance: draw.first_instance,
        }
    }

    pub const fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let fields = [
            self.index_count,
            self.instance_count,
            self.first_index,
            self.base_vertex as u32,
            self.first_instance,
        ];
        let mut bytes = [0; Self::BYTE_LEN];
        let mut field = 0usize;
        while field < fields.len() {
            let encoded = fields[field].to_le_bytes();
            let mut byte = 0usize;
            while byte < encoded.len() {
                bytes[field * 4 + byte] = encoded[byte];
                byte += 1;
            }
            field += 1;
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Artifact,
    UnsupportedPipeline,
    UnsupportedBinding,
    MissingPosition,
    MissingColor,
    DrawNotTriangleList,
    TooManyTriangles,
    IndexOutOfRange,
    NonFiniteVertex,
    NonUniformTriangleColor,
    ColorOutOfRange,
    InvalidCamera,
    VertexBehindCamera,
    DegenerateTriangle,
    MissingReplay,
    ReplayCommandCount,
    ReplayChecksumMismatch,
    ReplayResourceMismatch,
    ReplayDrawMismatch,
    MissingChurnScene,
    InvalidChurnScene,
    MissingChurnLighting,
    InvalidChurnLighting,
    MissingPortalRoomsScene,
    InvalidPortalRoomsScene,
    MissingRetainedTransformTemplate,
    InvalidRetainedTransformTemplate,
}

pub fn decode_artifact(bytes: &[u8], aspect: f32, camera: Camera) -> Result<Scene, Error> {
    let artifact = trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
    let program = artifact.render_program().map_err(|_| Error::Artifact)?;
    let Some(_) = artifact.section(trueos_helio_artifact::replay::SECTION_NAME) else {
        return decode_program(&program, aspect, camera);
    };
    let render_ir = artifact
        .section(trueos_helio_artifact::render_ir::SECTION_NAME)
        .ok_or(Error::Artifact)?;
    let replay = artifact.replay_plan().map_err(|_| Error::Artifact)?;
    let draw = validate_replay_plan(&program, render_ir.data, replay)?;
    decode_program_with_draw(&program, aspect, camera, draw, DrawCommandSource::ArtifactReplayV1)
}

/// Decode only artifacts that carry the validated Helio replay contract.
///
/// `decode_artifact` intentionally retains an explicit Render-IR fallback for
/// old build products. The kernel demo uses this strict entry point so its
/// telemetry cannot claim GPU-owned indirect parameters when the section is
/// absent.
pub fn decode_artifact_with_replay(
    bytes: &[u8],
    aspect: f32,
    camera: Camera,
) -> Result<Scene, Error> {
    let artifact = trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
    let program = artifact.render_program().map_err(|_| Error::Artifact)?;
    let render_ir = artifact
        .section(trueos_helio_artifact::render_ir::SECTION_NAME)
        .ok_or(Error::Artifact)?;
    if artifact
        .section(trueos_helio_artifact::replay::SECTION_NAME)
        .is_none()
    {
        return Err(Error::MissingReplay);
    }
    let replay = artifact.replay_plan().map_err(|_| Error::Artifact)?;
    let draw = validate_replay_plan(&program, render_ir.data, replay)?;
    decode_program_with_draw(&program, aspect, camera, draw, DrawCommandSource::ArtifactReplayV1)
}

pub fn decode_program(program: &Program<'_>, aspect: f32, camera: Camera) -> Result<Scene, Error> {
    decode_program_with_draw(
        program,
        aspect,
        camera,
        DrawIndexedIndirectArgs::from_render_ir(program.draw),
        DrawCommandSource::RenderIrFallback,
    )
}

fn decode_program_with_draw(
    program: &Program<'_>,
    aspect: f32,
    camera: Camera,
    source_draw: DrawIndexedIndirectArgs,
    draw_source: DrawCommandSource,
) -> Result<Scene, Error> {
    validate_contract(program)?;
    let position = attribute(program, 0).ok_or(Error::MissingPosition)?;
    let color = attribute(program, 2).ok_or(Error::MissingColor)?;
    let draw_count =
        usize::try_from(source_draw.index_count).map_err(|_| Error::DrawNotTriangleList)?;
    if !draw_count.is_multiple_of(3) {
        return Err(Error::DrawNotTriangleList);
    }
    let triangle_count = draw_count / 3;
    if triangle_count > MAX_TRIANGLES {
        return Err(Error::TooManyTriangles);
    }

    let projector = Projector::new(camera, aspect)?;
    let first_index =
        usize::try_from(source_draw.first_index).map_err(|_| Error::IndexOutOfRange)?;
    let mut triangles = Vec::with_capacity(triangle_count);
    for triangle_index in 0..triangle_count {
        let first = first_index
            .checked_add(
                triangle_index
                    .checked_mul(3)
                    .ok_or(Error::IndexOutOfRange)?,
            )
            .ok_or(Error::IndexOutOfRange)?;
        let mut projected = [[0.0; 3]; 3];
        let mut colors = [[0.0; 3]; 3];
        for corner in 0..3 {
            let source_index = read_index(program, first + corner)?;
            let adjusted = i64::from(source_index) + i64::from(source_draw.base_vertex);
            let vertex_index = usize::try_from(adjusted).map_err(|_| Error::IndexOutOfRange)?;
            let position = read_f32x3(program, vertex_index, position)?;
            let color = read_f32x3(program, vertex_index, color)?;
            if position
                .iter()
                .chain(color.iter())
                .any(|value| !value.is_finite())
            {
                return Err(Error::NonFiniteVertex);
            }
            projected[corner] = projector.project(position)?;
            colors[corner] = color;
        }
        if colors[1] != colors[0] || colors[2] != colors[0] {
            return Err(Error::NonUniformTriangleColor);
        }
        let area = signed_area(projected);
        if !area.is_finite() || area.abs() <= 1.0e-9 {
            return Err(Error::DegenerateTriangle);
        }
        // The resident scene backend's fixed front-end contract consumes
        // counter-clockwise projected triangles. Normalize winding here while
        // preserving the IR's indexed draw range and per-triangle material.
        if area < 0.0 {
            projected.swap(1, 2);
        }
        triangles.push(Triangle {
            vertices: projected,
            rgba: linear_rgb_to_srgba8(colors[0])?,
        });
    }

    Ok(Scene {
        triangles,
        clear_rgba: linear_rgba_to_srgba8(program.pipeline.clear_color)?,
        source_draw_indexed_indirect: source_draw,
        draw_source,
    })
}

fn validate_replay_plan(
    program: &Program<'_>,
    render_ir_bytes: &[u8],
    replay: ReplayPlan<'_>,
) -> Result<DrawIndexedIndirectArgs, Error> {
    if replay.command_count() != 1 {
        return Err(Error::ReplayCommandCount);
    }
    if replay.source_render_ir_crc32() != crc32(render_ir_bytes) {
        return Err(Error::ReplayChecksumMismatch);
    }
    if replay.vertex_buffer_id() != program.vertex.id
        || replay.index_buffer_id() != program.index.id
    {
        return Err(Error::ReplayResourceMismatch);
    }
    let replay_draw = replay.commands().next().ok_or(Error::ReplayCommandCount)?;
    if replay_draw != program.draw {
        return Err(Error::ReplayDrawMismatch);
    }
    Ok(DrawIndexedIndirectArgs::from_render_ir(replay_draw))
}

/// Table-free IEEE CRC32 keeps the no-std replay/IR binding self-contained.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn validate_contract(program: &Program<'_>) -> Result<(), Error> {
    let pipeline = program.pipeline;
    if pipeline.color_format != TextureFormat::Bgra8UnormSrgb
        || pipeline.depth_format != TextureFormat::Depth32Float
        || pipeline.topology != PrimitiveTopology::TriangleList
        || pipeline.front_face != FrontFace::Ccw
        || pipeline.cull_mode != CullMode::Back
        || pipeline.depth_compare != CompareFunction::Less
        || pipeline.color_write_mask != 0xf
        || pipeline.flags.bits() != 0b11_1111
        || program.draw.instance_count != 1
        || program.draw.first_instance != 0
    {
        return Err(Error::UnsupportedPipeline);
    }
    if program.camera.dynamic_slot != "camera.view_proj"
        || program.output_dynamic_slot != "output.surface"
        || program.camera.group != 0
        || program.camera.binding != 0
        || program.camera.kind != BindingKind::StorageBuffer
        || !program.camera.visibility.contains(ShaderStages::VERTEX)
        || !program.camera.read_only
        || program.camera.minimum_size < 64
    {
        return Err(Error::UnsupportedBinding);
    }
    Ok(())
}

fn attribute(program: &Program<'_>, location: u32) -> Option<VertexAttribute> {
    program
        .attributes()
        .iter()
        .copied()
        .find(|attribute| attribute.shader_location == location)
}

fn read_index(program: &Program<'_>, index: usize) -> Result<u32, Error> {
    let width = program.index.format.byte_width();
    let offset = index.checked_mul(width).ok_or(Error::IndexOutOfRange)?;
    let bytes = program
        .index
        .data
        .get(offset..offset + width)
        .ok_or(Error::IndexOutOfRange)?;
    Ok(match program.index.format {
        IndexFormat::Uint16 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        IndexFormat::Uint32 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

fn read_f32x3(
    program: &Program<'_>,
    vertex: usize,
    attribute: VertexAttribute,
) -> Result<[f32; 3], Error> {
    let stride = usize::try_from(program.vertex.stride).map_err(|_| Error::IndexOutOfRange)?;
    let attribute_offset = usize::try_from(attribute.offset).map_err(|_| Error::IndexOutOfRange)?;
    let offset = vertex
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(attribute_offset))
        .ok_or(Error::IndexOutOfRange)?;
    let bytes = program
        .vertex
        .data
        .get(offset..offset + 12)
        .ok_or(Error::IndexOutOfRange)?;
    Ok([
        f32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        f32::from_le_bytes(bytes[8..12].try_into().unwrap()),
    ])
}

pub struct Projector {
    position: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    near: f32,
    far: f32,
    tan_half_fov: f32,
    aspect: f32,
}

impl Projector {
    pub fn new(camera: Camera, aspect: f32) -> Result<Self, Error> {
        if !aspect.is_finite()
            || aspect <= 0.0
            || !camera.vertical_fov_radians.is_finite()
            || camera.vertical_fov_radians <= 0.0
            || camera.vertical_fov_radians >= core::f32::consts::PI
            || !camera.near.is_finite()
            || !camera.far.is_finite()
            || camera.near <= 0.0
            || camera.far <= camera.near
        {
            return Err(Error::InvalidCamera);
        }
        let forward = normalize(sub(camera.target, camera.position))?;
        let right = normalize(cross(forward, camera.up))?;
        let up = normalize(cross(right, forward))?;
        let tan_half_fov = libm::tanf(camera.vertical_fov_radians * 0.5);
        if !tan_half_fov.is_finite() || tan_half_fov <= 0.0 {
            return Err(Error::InvalidCamera);
        }
        Ok(Self {
            position: camera.position,
            right,
            up,
            forward,
            near: camera.near,
            far: camera.far,
            tan_half_fov,
            aspect,
        })
    }

    pub fn project(&self, point: [f32; 3]) -> Result<[f32; 3], Error> {
        let relative = sub(point, self.position);
        let depth = dot(relative, self.forward);
        if !depth.is_finite() || depth < self.near || depth > self.far {
            return Err(Error::VertexBehindCamera);
        }
        let inverse_y = 1.0 / (depth * self.tan_half_fov);
        let projected = [
            dot(relative, self.right) * inverse_y / self.aspect,
            dot(relative, self.up) * inverse_y,
            (depth - self.near) / (self.far - self.near),
        ];
        if projected.iter().any(|value| !value.is_finite()) {
            return Err(Error::NonFiniteVertex);
        }
        Ok(projected)
    }
}

fn linear_rgb_to_srgba8(rgb: [f32; 3]) -> Result<[u8; 4], Error> {
    Ok([
        linear_channel_to_srgb8(rgb[0])?,
        linear_channel_to_srgb8(rgb[1])?,
        linear_channel_to_srgb8(rgb[2])?,
        u8::MAX,
    ])
}

pub fn linear_rgba_to_srgba8(rgba: [f32; 4]) -> Result<[u8; 4], Error> {
    if rgba
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(Error::ColorOutOfRange);
    }
    let alpha = quantize(rgba[3]);
    let alpha_scale = f32::from(alpha) / 255.0;
    Ok([
        quantize(linear_to_srgb(rgba[0]) * alpha_scale),
        quantize(linear_to_srgb(rgba[1]) * alpha_scale),
        quantize(linear_to_srgb(rgba[2]) * alpha_scale),
        alpha,
    ])
}

fn linear_channel_to_srgb8(value: f32) -> Result<u8, Error> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::ColorOutOfRange);
    }
    Ok(quantize(linear_to_srgb(value)))
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * libm::powf(value, 1.0 / 2.4) - 0.055
    }
}

fn quantize(value: f32) -> u8 {
    libm::roundf(value.clamp(0.0, 1.0) * 255.0) as u8
}

fn signed_area(vertices: [[f32; 3]; 3]) -> f32 {
    let [a, b, c] = vertices;
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(value: [f32; 3]) -> Result<[f32; 3], Error> {
    let length = libm::sqrtf(dot(value, value));
    if !length.is_finite() || length <= 1.0e-9 {
        return Err(Error::InvalidCamera);
    }
    Ok([value[0] / length, value[1] / length, value[2] / length])
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos_helio_artifact::render_ir::{
        CameraBinding, DrawIndexed, IndexBuffer, PipelineState, ResourceId, Shader, StateFlags,
        VertexBuffer, VertexFormat,
    };
    use trueos_helio_artifact::replay::{COMMAND_STRIDE, HEADER_LEN, MAGIC, VERSION};

    fn f32s(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn program<'a>(vertices: &'a [u8], indices: &'a [u8]) -> Program<'a> {
        Program {
            vertex: VertexBuffer {
                id: ResourceId(1),
                data: vertices,
                stride: 24,
            },
            index: IndexBuffer {
                id: ResourceId(2),
                data: indices,
                format: IndexFormat::Uint16,
            },
            attributes: [
                VertexAttribute {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
                VertexAttribute {
                    shader_location: 2,
                    format: VertexFormat::Float32x3,
                    offset: 12,
                },
                VertexAttribute {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
            ],
            attribute_count: 2,
            shader: Shader {
                wgsl: "shader",
                vertex_entry: "vs_main",
                fragment_entry: "fs_main",
            },
            camera: CameraBinding {
                buffer_id: ResourceId(3),
                minimum_size: 192,
                dynamic_slot: "camera.view_proj",
                group: 0,
                binding: 0,
                kind: BindingKind::StorageBuffer,
                visibility: ShaderStages::VERTEX,
                read_only: true,
            },
            pipeline: PipelineState {
                color_format: TextureFormat::Bgra8UnormSrgb,
                depth_format: TextureFormat::Depth32Float,
                topology: PrimitiveTopology::TriangleList,
                front_face: FrontFace::Ccw,
                cull_mode: CullMode::Back,
                depth_compare: CompareFunction::Less,
                flags: state_flags(),
                color_write_mask: 0xf,
                clear_color: [0.01, 0.01, 0.02, 1.0],
                clear_depth: 1.0,
            },
            draw: DrawIndexed {
                index_count: 3,
                instance_count: 1,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            output_dynamic_slot: "output.surface",
            pass_label: "fixture",
        }
    }

    fn state_flags() -> StateFlags {
        StateFlags::from_bits(0b11_1111).unwrap()
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn replay_bytes(
        render_ir: &[u8],
        vertex_id: ResourceId,
        index_id: ResourceId,
        commands: &[DrawIndexed],
    ) -> Vec<u8> {
        let mut bytes = alloc::vec![0u8; HEADER_LEN + commands.len() * COMMAND_STRIDE];
        bytes[..8].copy_from_slice(&MAGIC);
        put_u16(&mut bytes, 8, VERSION);
        put_u16(&mut bytes, 10, HEADER_LEN as u16);
        let total_len = bytes.len() as u32;
        put_u32(&mut bytes, 12, total_len);
        put_u32(&mut bytes, 16, commands.len() as u32);
        put_u32(&mut bytes, 20, COMMAND_STRIDE as u32);
        put_u32(&mut bytes, 28, crc32(render_ir));
        put_u32(&mut bytes, 32, vertex_id.0);
        put_u32(&mut bytes, 36, index_id.0);
        for (index, command) in commands.iter().enumerate() {
            let base = HEADER_LEN + index * COMMAND_STRIDE;
            put_u32(&mut bytes, base, command.index_count);
            put_u32(&mut bytes, base + 4, command.instance_count);
            put_u32(&mut bytes, base + 8, command.first_index);
            put_u32(&mut bytes, base + 12, command.base_vertex as u32);
            put_u32(&mut bytes, base + 16, command.first_instance);
        }
        bytes
    }

    #[test]
    fn fixed_camera_projects_generic_triangle() {
        let vertices = f32s(&[
            -0.5, -0.5, 0.0, 1.0, 0.2, 0.1, 0.5, -0.5, 0.0, 1.0, 0.2, 0.1, 0.0, 0.5, 0.0, 1.0, 0.2,
            0.1,
        ]);
        let indices = [0, 0, 1, 0, 2, 0];
        let program = program(&vertices, &indices);
        let scene = decode_program(&program, 16.0 / 9.0, Camera::helio_simple_graph()).unwrap();
        assert_eq!(scene.triangles.len(), 1);
        assert_eq!(scene.triangles[0].rgba, [255, 124, 89, 255]);
        let center_x = scene.triangles[0]
            .vertices
            .iter()
            .map(|vertex| vertex[0])
            .sum::<f32>()
            / 3.0;
        assert!(center_x.abs() < 1.0e-6);
        assert!(
            scene.triangles[0]
                .vertices
                .iter()
                .all(|vertex| (0.0..1.0).contains(&vertex[2]))
        );
        assert_eq!(scene.draw_source, DrawCommandSource::RenderIrFallback);
        assert_eq!(scene.source_draw_indexed_indirect, DrawIndexedIndirectArgs::new(3));
        assert_eq!(
            scene.resident_triangle_draw_indexed_indirect(0).unwrap(),
            DrawIndexedIndirectArgs::new(3)
        );
        assert_eq!(scene.resident_triangle_draw_indexed_indirect(1), Err(Error::IndexOutOfRange));
    }

    #[test]
    fn draw_indexed_indirect_abi_is_exactly_wgpu_order() {
        let draw = DrawIndexedIndirectArgs {
            index_count: 0x0102_0304,
            instance_count: 0x1112_1314,
            first_index: 0x2122_2324,
            base_vertex: -2,
            first_instance: 0x4142_4344,
        };
        assert_eq!(DrawIndexedIndirectArgs::BYTE_LEN, 20);
        assert_eq!(
            draw.to_le_bytes(),
            [
                0x04, 0x03, 0x02, 0x01, 0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21, 0xfe, 0xff,
                0xff, 0xff, 0x44, 0x43, 0x42, 0x41,
            ]
        );
    }

    #[test]
    fn replay_is_bound_to_ir_crc_resources_and_exact_draw() {
        let vertices = f32s(&[
            -0.5, -0.5, 0.0, 1.0, 0.2, 0.1, 0.5, -0.5, 0.0, 1.0, 0.2, 0.1, 0.0, 0.5, 0.0, 1.0, 0.2,
            0.1,
        ]);
        let indices = [0, 0, 1, 0, 2, 0];
        let program = program(&vertices, &indices);
        let render_ir = b"normalized-render-ir";

        let valid = replay_bytes(render_ir, program.vertex.id, program.index.id, &[program.draw]);
        assert_eq!(
            validate_replay_plan(&program, render_ir, ReplayPlan::parse(&valid).unwrap()),
            Ok(DrawIndexedIndirectArgs::new(3))
        );

        let mut bad_crc = valid.clone();
        put_u32(&mut bad_crc, 28, crc32(render_ir) ^ 1);
        assert_eq!(
            validate_replay_plan(&program, render_ir, ReplayPlan::parse(&bad_crc).unwrap()),
            Err(Error::ReplayChecksumMismatch)
        );

        let wrong_resource =
            replay_bytes(render_ir, ResourceId(9), program.index.id, &[program.draw]);
        assert_eq!(
            validate_replay_plan(&program, render_ir, ReplayPlan::parse(&wrong_resource).unwrap()),
            Err(Error::ReplayResourceMismatch)
        );

        let different_draw = DrawIndexed {
            index_count: program.draw.index_count,
            instance_count: program.draw.instance_count,
            first_index: program.draw.first_index,
            base_vertex: program.draw.base_vertex,
            first_instance: 1,
        };
        let wrong_draw =
            replay_bytes(render_ir, program.vertex.id, program.index.id, &[different_draw]);
        assert_eq!(
            validate_replay_plan(&program, render_ir, ReplayPlan::parse(&wrong_draw).unwrap()),
            Err(Error::ReplayDrawMismatch)
        );

        let two_draws = replay_bytes(
            render_ir,
            program.vertex.id,
            program.index.id,
            &[program.draw, program.draw],
        );
        assert_eq!(
            validate_replay_plan(&program, render_ir, ReplayPlan::parse(&two_draws).unwrap()),
            Err(Error::ReplayCommandCount)
        );
    }

    #[test]
    fn crc32_matches_ieee_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn srgb_and_premultiplied_clear_are_explicit() {
        assert_eq!(linear_rgb_to_srgba8([1.0, 0.0, 0.0]).unwrap(), [255, 0, 0, 255]);
        assert_eq!(linear_rgb_to_srgba8([0.5, 0.5, 0.5]).unwrap(), [188, 188, 188, 255]);
        assert_eq!(linear_rgba_to_srgba8([1.0, 0.0, 0.0, 0.5]).unwrap(), [128, 0, 0, 128]);
    }

    #[test]
    fn uniform_triangle_color_is_enforced() {
        let vertices = f32s(&[
            -0.5, -0.5, 0.0, 1.0, 0.0, 0.0, 0.5, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0,
            0.0,
        ]);
        let program = program(&vertices, &[0, 0, 1, 0, 2, 0]);
        let position = attribute(&program, 0).unwrap();
        let color = attribute(&program, 2).unwrap();
        assert_eq!(read_f32x3(&program, 1, position).unwrap(), [0.5, -0.5, 0.0]);
        assert_ne!(
            read_f32x3(&program, 0, color).unwrap(),
            read_f32x3(&program, 1, color).unwrap()
        );
    }
}
