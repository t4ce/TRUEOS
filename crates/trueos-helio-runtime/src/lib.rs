#![no_std]
#![forbid(unsafe_code)]

//! Narrow runtime lowering from Helio's normalized render IR to the retained
//! triangle contract already submitted by TRUEOS's Intel Render/GuC path.

extern crate alloc;

pub mod churn;

use alloc::vec::Vec;
use trueos_helio_artifact::render_ir::{
    BindingKind, CompareFunction, CullMode, FrontFace, IndexFormat, PrimitiveTopology, Program,
    ShaderStages, TextureFormat, VertexAttribute,
};

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
    MissingChurnScene,
    InvalidChurnScene,
}

pub fn decode_artifact(bytes: &[u8], aspect: f32, camera: Camera) -> Result<Scene, Error> {
    let artifact = trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
    let program = artifact.render_program().map_err(|_| Error::Artifact)?;
    decode_program(&program, aspect, camera)
}

pub fn decode_program(program: &Program<'_>, aspect: f32, camera: Camera) -> Result<Scene, Error> {
    validate_contract(program)?;
    let position = attribute(program, 0).ok_or(Error::MissingPosition)?;
    let color = attribute(program, 2).ok_or(Error::MissingColor)?;
    let draw_count =
        usize::try_from(program.draw.index_count).map_err(|_| Error::DrawNotTriangleList)?;
    if !draw_count.is_multiple_of(3) {
        return Err(Error::DrawNotTriangleList);
    }
    let triangle_count = draw_count / 3;
    if triangle_count > MAX_TRIANGLES {
        return Err(Error::TooManyTriangles);
    }

    let projector = Projector::new(camera, aspect)?;
    let first_index =
        usize::try_from(program.draw.first_index).map_err(|_| Error::IndexOutOfRange)?;
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
            let adjusted = i64::from(source_index) + i64::from(program.draw.base_vertex);
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
    })
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

pub(crate) struct Projector {
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
    pub(crate) fn new(camera: Camera, aspect: f32) -> Result<Self, Error> {
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

    pub(crate) fn project(&self, point: [f32; 3]) -> Result<[f32; 3], Error> {
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

pub(crate) fn linear_rgba_to_srgba8(rgba: [f32; 4]) -> Result<[u8; 4], Error> {
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
