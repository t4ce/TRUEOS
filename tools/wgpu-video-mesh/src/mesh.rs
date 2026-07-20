use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::suzanne_data;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

const VERTEX_ATTRIBUTES: [eframe::wgpu::VertexAttribute; 3] =
    eframe::wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

impl Vertex {
    pub fn layout() -> eframe::wgpu::VertexBufferLayout<'static> {
        eframe::wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as eframe::wgpu::BufferAddress,
            step_mode: eframe::wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRIBUTES,
        }
    }
}

pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn normalized(mut self) -> Self {
        let radius = self
            .vertices
            .iter()
            .map(|vertex| Vec3::from_array(vertex.position).length())
            .fold(0.0_f32, f32::max);
        if radius > f32::EPSILON {
            for vertex in &mut self.vertices {
                vertex.position = (Vec3::from_array(vertex.position) / radius).to_array();
            }
        }
        self
    }
}

pub fn plane() -> MeshData {
    let vertices = vec![
        vertex([-1.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0]),
        vertex([1.0, -1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0]),
        vertex([1.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0]),
        vertex([-1.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0]),
    ];
    MeshData {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
    }
    .normalized()
}

pub fn cube() -> MeshData {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    push_face(&mut vertices, &mut indices, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    push_face(&mut vertices, &mut indices, [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    push_face(&mut vertices, &mut indices, [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    push_face(&mut vertices, &mut indices, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    push_face(&mut vertices, &mut indices, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
    push_face(&mut vertices, &mut indices, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    MeshData { vertices, indices }.normalized()
}

pub fn circle(segments: u32) -> MeshData {
    let mut vertices = Vec::with_capacity((segments + 2) as usize);
    let mut indices = Vec::with_capacity((segments * 3) as usize);
    vertices.push(vertex([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.5, 0.5]));

    for segment in 0..=segments {
        let angle = segment as f32 / segments as f32 * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        vertices.push(vertex([cos, sin, 0.0], [0.0, 0.0, 1.0], [cos * 0.5 + 0.5, 0.5 - sin * 0.5]));
    }
    for segment in 0..segments {
        indices.extend_from_slice(&[0, segment + 1, segment + 2]);
    }

    MeshData { vertices, indices }.normalized()
}

pub fn uv_sphere(segments: u32, rings: u32) -> MeshData {
    let mut vertices = Vec::with_capacity(((segments + 1) * (rings + 1)) as usize);
    let mut indices = Vec::with_capacity((segments * rings * 6) as usize);

    for ring in 0..=rings {
        let v = ring as f32 / rings as f32;
        let theta = v * std::f32::consts::PI;
        let y = theta.cos();
        let radius = theta.sin();

        for segment in 0..=segments {
            let u = segment as f32 / segments as f32;
            let phi = u * std::f32::consts::TAU;
            let position = [radius * phi.sin(), y, radius * phi.cos()];
            vertices.push(vertex(position, position, [u, v]));
        }
    }

    let row = segments + 1;
    for ring in 0..rings {
        for segment in 0..segments {
            let top_left = ring * row + segment;
            let bottom_left = (ring + 1) * row + segment;
            indices.extend_from_slice(&[
                top_left,
                bottom_left,
                top_left + 1,
                top_left + 1,
                bottom_left,
                bottom_left + 1,
            ]);
        }
    }

    MeshData { vertices, indices }.normalized()
}

pub fn icosphere(subdivisions: u32) -> MeshData {
    let golden = (1.0 + 5.0_f32.sqrt()) * 0.5;
    let mut positions = vec![
        [-1.0, golden, 0.0],
        [1.0, golden, 0.0],
        [-1.0, -golden, 0.0],
        [1.0, -golden, 0.0],
        [0.0, -1.0, golden],
        [0.0, 1.0, golden],
        [0.0, -1.0, -golden],
        [0.0, 1.0, -golden],
        [golden, 0.0, -1.0],
        [golden, 0.0, 1.0],
        [-golden, 0.0, -1.0],
        [-golden, 0.0, 1.0],
    ];
    for position in &mut positions {
        *position = Vec3::from_array(*position).normalize().to_array();
    }

    let mut triangles = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];

    for _ in 0..subdivisions {
        let mut midpoint_cache = HashMap::new();
        let mut refined = Vec::with_capacity(triangles.len() * 4);
        for [a, b, c] in triangles {
            let ab = midpoint(&mut positions, &mut midpoint_cache, a, b);
            let bc = midpoint(&mut positions, &mut midpoint_cache, b, c);
            let ca = midpoint(&mut positions, &mut midpoint_cache, c, a);
            refined.extend_from_slice(&[[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        triangles = refined;
    }

    smooth_spherical_mesh(positions, triangles)
}

pub fn cylinder(segments: u32) -> MeshData {
    capped_cone(segments, 1.0, 1.0)
}

pub fn cone(segments: u32) -> MeshData {
    capped_cone(segments, 1.0, 0.0)
}

pub fn torus(major_segments: u32, minor_segments: u32) -> MeshData {
    let major_radius = 0.65;
    let minor_radius = 0.35;
    let row = minor_segments + 1;
    let mut vertices = Vec::with_capacity(((major_segments + 1) * row) as usize);
    let mut indices = Vec::with_capacity((major_segments * minor_segments * 6) as usize);

    for major in 0..=major_segments {
        let u = major as f32 / major_segments as f32;
        let major_angle = u * std::f32::consts::TAU;
        let (major_sin, major_cos) = major_angle.sin_cos();
        for minor in 0..=minor_segments {
            let v = minor as f32 / minor_segments as f32;
            let minor_angle = v * std::f32::consts::TAU;
            let (minor_sin, minor_cos) = minor_angle.sin_cos();
            let ring_radius = major_radius + minor_radius * minor_cos;
            vertices.push(vertex(
                [
                    ring_radius * major_cos,
                    ring_radius * major_sin,
                    minor_radius * minor_sin,
                ],
                [minor_cos * major_cos, minor_cos * major_sin, minor_sin],
                [u, v],
            ));
        }
    }
    for major in 0..major_segments {
        for minor in 0..minor_segments {
            let a = major * row + minor;
            let b = (major + 1) * row + minor;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }

    MeshData { vertices, indices }.normalized()
}

pub fn grid(columns: u32, rows: u32) -> MeshData {
    let mut vertices = Vec::with_capacity(((columns + 1) * (rows + 1)) as usize);
    let mut indices = Vec::with_capacity((columns * rows * 6) as usize);

    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let y = v * 2.0 - 1.0;
        for column in 0..=columns {
            let u = column as f32 / columns as f32;
            let x = u * 2.0 - 1.0;
            vertices.push(vertex([x, y, 0.0], [0.0, 0.0, 1.0], [u, 1.0 - v]));
        }
    }
    let stride = columns + 1;
    for row in 0..rows {
        for column in 0..columns {
            let a = row * stride + column;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }

    MeshData { vertices, indices }.normalized()
}

pub fn suzanne() -> MeshData {
    const FACE_OFFSET: i32 = 4;
    let mut positions = Vec::with_capacity(suzanne_data::VERTICES.len() * 2);
    for raw in suzanne_data::VERTICES {
        positions.push([
            (f32::from(raw[0]) + 127.0) / 128.0,
            f32::from(raw[1]) / 128.0,
            f32::from(raw[2]) / 128.0,
        ]);
    }

    let mut mirrored = Vec::with_capacity(suzanne_data::VERTICES.len());
    for index in 0..suzanne_data::VERTICES.len() {
        let source = positions[index];
        if source[0].abs() < 0.001 {
            mirrored.push(index as u32);
        } else {
            mirrored.push(positions.len() as u32);
            positions.push([-source[0], source[1], source[2]]);
        }
    }

    let mut triangles = Vec::with_capacity(suzanne_data::FACES.len() * 4);
    for (face_index, face) in suzanne_data::FACES.into_iter().enumerate() {
        let decode =
            |value: i8| -> u32 { (i32::from(value) + face_index as i32 - FACE_OFFSET) as u32 };
        let [a, b, c, d] = face.map(decode);
        triangles.push([a, b, c]);
        if d != c {
            triangles.push([a, c, d]);
        }

        let [ma, mb, mc, md] = [a, b, c, d].map(|index| mirrored[index as usize]);
        triangles.push([mc, mb, ma]);
        if md != mc {
            triangles.push([mc, ma, md]);
        }
    }

    smooth_spherical_mesh(positions, triangles)
}

fn vertex(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> Vertex {
    Vertex {
        position,
        normal,
        uv,
    }
}

fn push_face(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
) {
    let base = vertices.len() as u32;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    for ((u, v), uv) in corners.into_iter().zip(uvs) {
        vertices.push(vertex(
            [
                normal[0] + tangent[0] * u + bitangent[0] * v,
                normal[1] + tangent[1] * u + bitangent[1] * v,
                normal[2] + tangent[2] * u + bitangent[2] * v,
            ],
            normal,
            uv,
        ));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn capped_cone(segments: u32, bottom_radius: f32, top_radius: f32) -> MeshData {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let slope = (bottom_radius - top_radius) * 0.5;

    for segment in 0..=segments {
        let u = segment as f32 / segments as f32;
        let angle = u * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        let normal = Vec3::new(cos, slope, sin).normalize().to_array();
        vertices.push(vertex([bottom_radius * cos, -1.0, bottom_radius * sin], normal, [u, 1.0]));
        vertices.push(vertex([top_radius * cos, 1.0, top_radius * sin], normal, [u, 0.0]));
    }
    for segment in 0..segments {
        let bottom = segment * 2;
        let top = bottom + 1;
        let next_bottom = bottom + 2;
        let next_top = bottom + 3;
        if top_radius <= f32::EPSILON {
            indices.extend_from_slice(&[bottom, top, next_bottom]);
        } else {
            indices.extend_from_slice(&[bottom, top, next_bottom, next_bottom, top, next_top]);
        }
    }

    push_cap(&mut vertices, &mut indices, segments, bottom_radius, -1.0, false);
    if top_radius > f32::EPSILON {
        push_cap(&mut vertices, &mut indices, segments, top_radius, 1.0, true);
    }

    MeshData { vertices, indices }.normalized()
}

fn push_cap(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    segments: u32,
    radius: f32,
    y: f32,
    top: bool,
) {
    let normal = if top {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, -1.0, 0.0]
    };
    let center = vertices.len() as u32;
    vertices.push(vertex([0.0, y, 0.0], normal, [0.5, 0.5]));
    for segment in 0..=segments {
        let angle = segment as f32 / segments as f32 * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        vertices.push(vertex(
            [radius * cos, y, radius * sin],
            normal,
            [cos * 0.5 + 0.5, 0.5 - sin * 0.5],
        ));
    }
    for segment in 0..segments {
        let current = center + segment + 1;
        let next = current + 1;
        if top {
            indices.extend_from_slice(&[center, next, current]);
        } else {
            indices.extend_from_slice(&[center, current, next]);
        }
    }
}

fn midpoint(
    positions: &mut Vec<[f32; 3]>,
    cache: &mut HashMap<(u32, u32), u32>,
    a: u32,
    b: u32,
) -> u32 {
    let key = if a < b { (a, b) } else { (b, a) };
    if let Some(index) = cache.get(&key) {
        return *index;
    }
    let point = (Vec3::from_array(positions[a as usize]) + Vec3::from_array(positions[b as usize]))
        .normalize()
        .to_array();
    let index = positions.len() as u32;
    positions.push(point);
    cache.insert(key, index);
    index
}

fn smooth_spherical_mesh(positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> MeshData {
    let mut normal_sums = vec![Vec3::ZERO; positions.len()];
    for triangle in &triangles {
        let a = Vec3::from_array(positions[triangle[0] as usize]);
        let b = Vec3::from_array(positions[triangle[1] as usize]);
        let c = Vec3::from_array(positions[triangle[2] as usize]);
        let face_normal = (b - a).cross(c - a);
        for index in triangle {
            normal_sums[*index as usize] += face_normal;
        }
    }

    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        let mut uvs =
            triangle.map(|index| spherical_uv(Vec3::from_array(positions[index as usize])));
        let min_u = uvs.iter().map(|uv| uv[0]).fold(f32::INFINITY, f32::min);
        let max_u = uvs.iter().map(|uv| uv[0]).fold(f32::NEG_INFINITY, f32::max);
        if max_u - min_u > 0.5 {
            for uv in &mut uvs {
                if uv[0] < 0.5 {
                    uv[0] += 1.0;
                }
            }
        }

        for (index, uv) in triangle.into_iter().zip(uvs) {
            let position = Vec3::from_array(positions[index as usize]);
            let normal = normal_sums[index as usize].normalize_or(position.normalize_or_zero());
            indices.push(vertices.len() as u32);
            vertices.push(vertex(position.to_array(), normal.to_array(), uv));
        }
    }

    MeshData { vertices, indices }.normalized()
}

fn spherical_uv(position: Vec3) -> [f32; 2] {
    let direction = position.normalize_or_zero();
    [
        0.5 + direction.x.atan2(direction.z) / std::f32::consts::TAU,
        direction.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated_meshes() -> Vec<(&'static str, MeshData)> {
        vec![
            ("Plane", plane()),
            ("Cube", cube()),
            ("Circle", circle(64)),
            ("UV Sphere", uv_sphere(64, 40)),
            ("Icosphere", icosphere(2)),
            ("Cylinder", cylinder(64)),
            ("Cone", cone(64)),
            ("Torus", torus(64, 24)),
            ("Grid", grid(12, 12)),
            ("Monkey", suzanne()),
        ]
    }

    #[test]
    fn every_mesh_is_non_empty_and_normalized_to_one() {
        for (name, mesh) in generated_meshes() {
            assert!(!mesh.vertices.is_empty(), "{name}");
            assert!(!mesh.indices.is_empty(), "{name}");
            assert_eq!(mesh.indices.len() % 3, 0, "{name}");
            assert!(
                mesh.indices
                    .iter()
                    .all(|index| *index < mesh.vertices.len() as u32),
                "{name}"
            );
            let radius = mesh
                .vertices
                .iter()
                .map(|vertex| Vec3::from_array(vertex.position).length())
                .fold(0.0_f32, f32::max);
            assert!((radius - 1.0).abs() < 1.0e-5, "{name}: {radius}");
            assert!(
                mesh.vertices.iter().all(|vertex| {
                    let normal = Vec3::from_array(vertex.normal);
                    normal.is_finite() && (normal.length() - 1.0).abs() < 1.0e-4
                }),
                "{name}"
            );
        }
    }

    #[test]
    fn expected_topology_sizes_are_stable() {
        assert_eq!(plane().indices.len(), 6);
        assert_eq!(cube().indices.len(), 36);
        assert_eq!(circle(64).indices.len(), 64 * 3);
        assert_eq!(uv_sphere(64, 40).indices.len(), 64 * 40 * 6);
        assert_eq!(icosphere(2).indices.len(), 20 * 4 * 4 * 3);
        assert_eq!(grid(12, 12).indices.len(), 12 * 12 * 6);
    }
}
