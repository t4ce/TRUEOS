use bytemuck::{Pod, Zeroable};

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
            vertices.push(Vertex {
                position,
                normal: position,
                uv: [u, v],
            });
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

    MeshData { vertices, indices }
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

    MeshData { vertices, indices }
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
        vertices.push(Vertex {
            position: [
                normal[0] + tangent[0] * u + bitangent[0] * v,
                normal[1] + tangent[1] * u + bitangent[1] * v,
                normal[2] + tangent[2] * u + bitangent[2] * v,
            ],
            normal,
            uv,
        });
    }

    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_mesh_sizes_are_stable() {
        let sphere = uv_sphere(64, 40);
        assert_eq!(sphere.vertices.len(), 65 * 41);
        assert_eq!(sphere.indices.len(), 64 * 40 * 6);

        let cube = cube();
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
    }
}
