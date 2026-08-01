//! Runtime for Helio's versioned big pendulum-cloth scene.
//!
//! The artifact carries the authored scene parameters.  TRUEOS keeps the
//! visible topology fixed and advances a compact Verlet/PBD equivalent of the
//! demo's ball-joint lattice, allowing the resident GPU mesh to be updated in
//! place for every frame.

use alloc::vec;
use alloc::vec::Vec;

use crate::churn::{
    DRAW_GROUP_COUNT, DirtyRange, DrawGroupDescriptor, GpuCameraUniforms, GpuForwardLitGlobals,
    GpuInstanceData, GpuLight, GpuMaterial, INSTANCE_FLAG_CASTS_SHADOW,
    INSTANCE_FLAG_RECEIVES_SHADOW, InstanceFrame, LIGHT_COUNT, MATERIAL_COUNT, MeshDescriptor,
    SHAPE_COUNT, gpu_camera_uniforms,
};
use crate::{
    Camera, DrawIndexedIndirectArgs, Error, Projector, churn::Batch, linear_rgba_to_srgba8,
};
use trueos_helio_artifact::SectionKind;

pub const SECTION_NAME: &str = "scene/pendulum-bigcloth-v1.bin";
const MAGIC: &[u8; 8] = b"HPENDUL\0";
const VERSION: u16 = 1;
const ENCODED_LEN: usize = 192;
const CHAIN_COUNT: usize = 14;
const CHAIN_LENGTH: usize = 24;
const SEGMENT_COUNT: usize = CHAIN_COUNT * CHAIN_LENGTH;
const VERTICES_PER_SEGMENT: usize = 24;
const INDICES_PER_SEGMENT: usize = 36;
const FLOOR_VERTICES: usize = 4;
const NATIVE_INSTANCE_COUNT: usize = SEGMENT_COUNT + 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub camera: Camera,
    pub clear_rgba: [u8; 4],
    pub segment_rgba: [u8; 4],
    pub floor_rgba: [u8; 4],
    pub floor_extent: f32,
    chain_spacing: f32,
    segment_length: f32,
    pivot_y: f32,
    box_half_extent: f32,
    visual_scale: f32,
    gravity: f32,
    fixed_dt: f32,
    damping: f32,
    ground_y: f32,
    collider_radius: f32,
    restitution: f32,
    constraint_iterations: usize,
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let section = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingPendulumScene)?;
        if section.kind != SectionKind::Unknown(u16::MAX) {
            return Err(Error::InvalidPendulumScene);
        }
        Self::decode(section.data)
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != ENCODED_LEN
            || bytes.get(..8) != Some(MAGIC.as_slice())
            || read_u16(bytes, 8)? != VERSION
            || usize::from(read_u16(bytes, 10)?) != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 12)?).map_err(|_| Error::InvalidPendulumScene)?
                != ENCODED_LEN
            || usize::from(read_u16(bytes, 16)?) != CHAIN_COUNT
            || usize::from(read_u16(bytes, 18)?) != CHAIN_LENGTH
            || read_u16(bytes, 22)? != 0
            || bytes[56..60].iter().any(|byte| *byte != 0)
            || bytes[156..].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidPendulumScene);
        }

        let constraint_iterations = usize::from(read_u16(bytes, 20)?);
        let position = read_f32s::<3>(bytes, 24)?;
        let yaw = read_f32(bytes, 36)?;
        let pitch = read_f32(bytes, 40)?;
        let cos_pitch = libm::cosf(pitch);
        let forward = [
            libm::sinf(yaw) * cos_pitch,
            libm::sinf(pitch),
            -libm::cosf(yaw) * cos_pitch,
        ];
        let camera = Camera {
            position,
            target: [
                position[0] + forward[0],
                position[1] + forward[1],
                position[2] + forward[2],
            ],
            up: [0.0, 1.0, 0.0],
            vertical_fov_radians: read_f32(bytes, 44)?,
            near: read_f32(bytes, 48)?,
            far: read_f32(bytes, 52)?,
        };
        Projector::new(camera, 1.0)?;

        let spec = Self {
            camera,
            chain_spacing: read_f32(bytes, 60)?,
            segment_length: read_f32(bytes, 64)?,
            pivot_y: read_f32(bytes, 68)?,
            box_half_extent: read_f32(bytes, 72)?,
            visual_scale: read_f32(bytes, 76)?,
            gravity: read_f32(bytes, 80)?,
            fixed_dt: read_f32(bytes, 84)?,
            damping: read_f32(bytes, 88)?,
            ground_y: read_f32(bytes, 92)?,
            collider_radius: read_f32(bytes, 96)?,
            restitution: read_f32(bytes, 100)?,
            segment_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 104)?)?,
            floor_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 120)?)?,
            floor_extent: read_f32(bytes, 136)?,
            clear_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 140)?)?,
            constraint_iterations,
        };
        if spec.constraint_iterations == 0
            || spec.constraint_iterations > 32
            || spec.chain_spacing <= 0.0
            || spec.segment_length <= 0.0
            || spec.pivot_y <= spec.ground_y
            || spec.box_half_extent <= 0.0
            || spec.visual_scale <= 0.0
            || spec.gravity >= 0.0
            || spec.fixed_dt <= 0.0
            || spec.fixed_dt > 0.1
            || !(0.0..=1.0).contains(&spec.damping)
            || spec.collider_radius <= 0.0
            || !(0.0..=1.0).contains(&spec.restitution)
            || spec.floor_extent <= 0.0
        {
            return Err(Error::InvalidPendulumScene);
        }
        Ok(spec)
    }
}

pub struct Engine {
    spec: Spec,
    positions: Vec<[f32; 3]>,
    previous: Vec<[f32; 3]>,
    batches: Vec<Batch>,
    enabled: bool,
    gpu_camera: GpuCameraUniforms,
    gpu_globals: GpuForwardLitGlobals,
    gpu_lights: [GpuLight; LIGHT_COUNT],
    gpu_materials: [GpuMaterial; MATERIAL_COUNT],
    gpu_meshes: [MeshDescriptor; SHAPE_COUNT],
    gpu_groups: [DrawGroupDescriptor; DRAW_GROUP_COUNT],
    gpu_instances: Vec<GpuInstanceData>,
    gpu_compacted_indices: Vec<u32>,
    gpu_draws: [DrawIndexedIndirectArgs; DRAW_GROUP_COUNT],
    previous_view_proj: Option<[f32; 16]>,
    frame: u32,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let vertex_count = SEGMENT_COUNT
            .checked_mul(VERTICES_PER_SEGMENT)
            .ok_or(Error::InvalidPendulumScene)?;
        let index_count = SEGMENT_COUNT
            .checked_mul(INDICES_PER_SEGMENT)
            .ok_or(Error::InvalidPendulumScene)?;
        let mut indices = Vec::with_capacity(index_count);
        for segment in 0..SEGMENT_COUNT {
            append_box_indices(&mut indices, segment * VERTICES_PER_SEGMENT)?;
        }
        let batches = vec![
            Batch {
                vertices: vec![[0.0; 3]; vertex_count],
                indices,
                rgba: spec.segment_rgba,
            },
            Batch {
                vertices: vec![[0.0; 3]; FLOOR_VERTICES],
                indices: vec![0, 1, 2, 0, 2, 3],
                rgba: spec.floor_rgba,
            },
        ];
        let gpu_meshes = core::array::from_fn(|mesh| MeshDescriptor {
            mesh_id: mesh as u32,
            half_extents: [1.0; 3],
            first_vertex: (mesh * VERTICES_PER_SEGMENT) as u32,
            vertex_count: VERTICES_PER_SEGMENT as u32,
            first_index: (mesh * INDICES_PER_SEGMENT) as u32,
            index_count: INDICES_PER_SEGMENT as u32,
            base_vertex: (mesh * VERTICES_PER_SEGMENT) as i32,
        });
        let gpu_groups = core::array::from_fn(|group| DrawGroupDescriptor {
            mesh_id: (group / MATERIAL_COUNT) as u32,
            material_id: (group % MATERIAL_COUNT) as u32,
        });
        let mut engine = Self {
            spec,
            positions: vec![[0.0; 3]; SEGMENT_COUNT],
            previous: vec![[0.0; 3]; SEGMENT_COUNT],
            batches,
            enabled: true,
            gpu_camera: GpuCameraUniforms::default(),
            gpu_globals: GpuForwardLitGlobals::default(),
            gpu_lights: core::array::from_fn(|_| GpuLight::default()),
            gpu_materials: core::array::from_fn(|_| GpuMaterial::default()),
            gpu_meshes,
            gpu_groups,
            gpu_instances: Vec::with_capacity(NATIVE_INSTANCE_COUNT),
            gpu_compacted_indices: Vec::with_capacity(NATIVE_INSTANCE_COUNT),
            gpu_draws: core::array::from_fn(|group| {
                let mesh = gpu_meshes[group / MATERIAL_COUNT];
                DrawIndexedIndirectArgs {
                    index_count: mesh.index_count,
                    instance_count: 0,
                    first_index: mesh.first_index,
                    base_vertex: mesh.base_vertex,
                    first_instance: 0,
                }
            }),
            previous_view_proj: None,
            frame: 0,
        };
        engine.reset();
        Ok(engine)
    }

    pub fn camera(&self) -> Camera {
        self.spec.camera
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), Error> {
        Projector::new(camera, 1.0)?;
        self.spec.camera = camera;
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
    }

    pub fn reset(&mut self) {
        for chain in 0..CHAIN_COUNT {
            let z = (chain as f32 - (CHAIN_COUNT - 1) as f32 * 0.5) * self.spec.chain_spacing;
            for segment in 0..CHAIN_LENGTH {
                let index = node_index(chain, segment);
                let point = [
                    (segment as f32 + 1.0) * self.spec.segment_length,
                    self.spec.pivot_y,
                    z,
                ];
                self.positions[index] = point;
                self.previous[index] = point;
            }
        }
    }

    pub const fn segment_count(&self) -> usize {
        SEGMENT_COUNT
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn step(&mut self, aspect: f32) -> Result<&[Batch], Error> {
        self.advance_simulation();
        self.project_batches(aspect)?;
        Ok(&self.batches)
    }

    /// Advance the authored cloth while retaining one immutable local-space
    /// box. Only compact transforms and draw ranges are returned; Helio's
    /// native vertex shader owns local -> world -> clip transformation.
    pub fn step_instances(&mut self, aspect: f32) -> Result<InstanceFrame<'_>, Error> {
        Projector::new(self.spec.camera, aspect)?;
        self.advance_simulation();

        self.gpu_instances.clear();
        self.gpu_compacted_indices.clear();
        let extent = self.spec.box_half_extent * self.spec.visual_scale;
        let segment_scale = [extent; 3];
        let segment_radius = libm::sqrtf(3.0) * extent;
        for (index, center) in self.positions.iter().copied().enumerate() {
            let previous = self.previous[index];
            self.gpu_instances.push(GpuInstanceData {
                model: translation_scale_matrix(center, segment_scale),
                normal_mat: inverse_scale_normal_matrix(segment_scale),
                bounds: [center[0], center[1], center[2], segment_radius],
                prev_model: translation_scale_matrix(previous, segment_scale),
                mesh_id: 0,
                material_id: 0,
                flags: INSTANCE_FLAG_CASTS_SHADOW | INSTANCE_FLAG_RECEIVES_SHADOW,
                lightmap_index: u32::MAX,
            });
            self.gpu_compacted_indices.push(index as u32);
        }

        let floor_scale = [self.spec.floor_extent, 0.01, self.spec.floor_extent];
        let floor_center = [0.0, self.spec.ground_y - floor_scale[1], 0.0];
        self.gpu_instances.push(GpuInstanceData {
            model: translation_scale_matrix(floor_center, floor_scale),
            normal_mat: inverse_scale_normal_matrix(floor_scale),
            bounds: [
                floor_center[0],
                floor_center[1],
                floor_center[2],
                libm::sqrtf(
                    floor_scale[0] * floor_scale[0]
                        + floor_scale[1] * floor_scale[1]
                        + floor_scale[2] * floor_scale[2],
                ),
            ],
            prev_model: translation_scale_matrix(floor_center, floor_scale),
            mesh_id: 0,
            material_id: 1,
            flags: INSTANCE_FLAG_RECEIVES_SHADOW,
            lightmap_index: u32::MAX,
        });
        self.gpu_compacted_indices.push(SEGMENT_COUNT as u32);

        self.gpu_draws = core::array::from_fn(|group| {
            let mesh = self.gpu_meshes[group / MATERIAL_COUNT];
            DrawIndexedIndirectArgs {
                index_count: mesh.index_count,
                instance_count: 0,
                first_index: mesh.first_index,
                base_vertex: mesh.base_vertex,
                first_instance: 0,
            }
        });
        self.gpu_draws[0] = DrawIndexedIndirectArgs {
            index_count: INDICES_PER_SEGMENT as u32,
            instance_count: SEGMENT_COUNT as u32,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        };
        self.gpu_draws[1] = DrawIndexedIndirectArgs {
            index_count: INDICES_PER_SEGMENT as u32,
            instance_count: 1,
            first_index: 0,
            base_vertex: 0,
            first_instance: SEGMENT_COUNT as u32,
        };

        let camera =
            gpu_camera_uniforms(self.spec.camera, aspect, self.frame, self.previous_view_proj)?;
        self.previous_view_proj = Some(camera.view_proj);
        self.gpu_camera = camera;
        self.gpu_globals = GpuForwardLitGlobals {
            frame: self.frame,
            delta_time: self.spec.fixed_dt,
            light_count: LIGHT_COUNT as u32,
            ambient_intensity: 0.25,
            ambient_color: [0.3, 0.4, 0.55, 1.0],
            num_tiles_x: 1,
            num_tiles_y: 1,
            screen_width: aspect,
            screen_height: 1.0,
        };
        self.frame = self.frame.wrapping_add(1);
        let dirty = DirtyRange {
            first: 0,
            count: NATIVE_INSTANCE_COUNT as u32,
        };
        Ok(InstanceFrame {
            camera: &self.gpu_camera,
            globals: &self.gpu_globals,
            lights: &self.gpu_lights,
            materials: &self.gpu_materials,
            meshes: &self.gpu_meshes,
            groups: &self.gpu_groups,
            instances: &self.gpu_instances,
            compacted_indices: &self.gpu_compacted_indices,
            draws: &self.gpu_draws,
            instance_dirty: dirty,
            compacted_indices_dirty: dirty,
        })
    }

    fn advance_simulation(&mut self) {
        if self.enabled {
            self.integrate();
            for _ in 0..self.spec.constraint_iterations {
                self.solve_lengthwise_constraints();
                self.solve_cross_constraints();
                self.solve_ground();
            }
        }
    }

    fn integrate(&mut self) {
        let acceleration = self.spec.gravity * self.spec.fixed_dt * self.spec.fixed_dt;
        for (position, previous) in self.positions.iter_mut().zip(&mut self.previous) {
            let current = *position;
            let velocity = scale(sub(current, *previous), self.spec.damping);
            *position = add(add(current, velocity), [0.0, acceleration, 0.0]);
            *previous = current;
        }
    }

    fn solve_lengthwise_constraints(&mut self) {
        for chain in 0..CHAIN_COUNT {
            let pivot = [
                0.0,
                self.spec.pivot_y,
                (chain as f32 - (CHAIN_COUNT - 1) as f32 * 0.5) * self.spec.chain_spacing,
            ];
            let first = node_index(chain, 0);
            constrain_to_anchor(&mut self.positions[first], pivot, self.spec.segment_length);
            for segment in 1..CHAIN_LENGTH {
                let left = node_index(chain, segment - 1);
                let right = node_index(chain, segment);
                constrain_pair(&mut self.positions, left, right, self.spec.segment_length);
            }
        }
    }

    fn solve_cross_constraints(&mut self) {
        for segment in 0..CHAIN_LENGTH {
            for chain in 0..CHAIN_COUNT - 1 {
                let near = node_index(chain, segment);
                let far = node_index(chain + 1, segment);
                constrain_pair(&mut self.positions, near, far, self.spec.chain_spacing);
            }
        }
    }

    fn solve_ground(&mut self) {
        let minimum_y = self.spec.ground_y + self.spec.collider_radius;
        for (position, previous) in self.positions.iter_mut().zip(&mut self.previous) {
            if position[1] >= minimum_y {
                continue;
            }
            let falling_velocity = position[1] - previous[1];
            position[1] = minimum_y;
            if falling_velocity < 0.0 {
                previous[1] = minimum_y + falling_velocity * self.spec.restitution;
                // Approximate the original collider friction without adding a
                // separate artifact field to the stable v1 contract.
                previous[0] = position[0] - (position[0] - previous[0]) * 0.6;
                previous[2] = position[2] - (position[2] - previous[2]) * 0.6;
            }
        }
    }

    fn project_batches(&mut self, aspect: f32) -> Result<(), Error> {
        let projector = Projector::new(self.spec.camera, aspect)?;
        let half_extent = self.spec.box_half_extent * self.spec.visual_scale;
        let local = box_vertices(half_extent);
        let segment_batch = &mut self.batches[0];
        for (segment, center) in self.positions.iter().copied().enumerate() {
            let start = segment * VERTICES_PER_SEGMENT;
            for (offset, point) in local.iter().copied().enumerate() {
                segment_batch.vertices[start + offset] = projector.project(add(center, point))?;
            }
            normalize_slot_winding(segment_batch, segment);
        }

        let extent = self.spec.floor_extent;
        let near_z = self.spec.camera.position[2] - self.spec.camera.near - 1.0;
        let floor_points = [
            [-extent, self.spec.ground_y, -extent],
            [extent, self.spec.ground_y, -extent],
            [extent, self.spec.ground_y, near_z],
            [-extent, self.spec.ground_y, near_z],
        ];
        let floor = &mut self.batches[1];
        for (vertex, point) in floor.vertices.iter_mut().zip(floor_points) {
            *vertex = projector.project(point)?;
        }
        normalize_all_winding(floor);
        Ok(())
    }
}

fn translation_scale_matrix(center: [f32; 3], scale: [f32; 3]) -> [f32; 16] {
    [
        scale[0], 0.0, 0.0, 0.0, 0.0, scale[1], 0.0, 0.0, 0.0, 0.0, scale[2], 0.0, center[0],
        center[1], center[2], 1.0,
    ]
}

fn inverse_scale_normal_matrix(scale: [f32; 3]) -> [f32; 12] {
    [
        1.0 / scale[0],
        0.0,
        0.0,
        0.0,
        0.0,
        1.0 / scale[1],
        0.0,
        0.0,
        0.0,
        0.0,
        1.0 / scale[2],
        0.0,
    ]
}

const fn node_index(chain: usize, segment: usize) -> usize {
    chain * CHAIN_LENGTH + segment
}

fn constrain_to_anchor(position: &mut [f32; 3], anchor: [f32; 3], rest: f32) {
    let delta = sub(*position, anchor);
    let length = magnitude(delta);
    if length > f32::EPSILON {
        *position = sub(*position, scale(delta, (length - rest) / length));
    }
}

fn constrain_pair(positions: &mut [[f32; 3]], left: usize, right: usize, rest: f32) {
    let delta = sub(positions[right], positions[left]);
    let length = magnitude(delta);
    if length <= f32::EPSILON {
        return;
    }
    let correction = scale(delta, (length - rest) * 0.5 / length);
    positions[left] = add(positions[left], correction);
    positions[right] = sub(positions[right], correction);
}

fn magnitude(value: [f32; 3]) -> f32 {
    libm::sqrtf(value[0] * value[0] + value[1] * value[1] + value[2] * value[2])
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn box_vertices(extent: f32) -> [[f32; 3]; VERTICES_PER_SEGMENT] {
    let (x, y, z) = (extent, extent, extent);
    [
        [-x, -y, z],
        [x, -y, z],
        [x, y, z],
        [-x, y, z],
        [x, -y, -z],
        [-x, -y, -z],
        [-x, y, -z],
        [x, y, -z],
        [-x, -y, -z],
        [-x, -y, z],
        [-x, y, z],
        [-x, y, -z],
        [x, -y, z],
        [x, -y, -z],
        [x, y, -z],
        [x, y, z],
        [-x, y, z],
        [x, y, z],
        [x, y, -z],
        [-x, y, -z],
        [-x, -y, -z],
        [x, -y, -z],
        [x, -y, z],
        [-x, -y, z],
    ]
}

fn append_box_indices(indices: &mut Vec<u32>, vertex_start: usize) -> Result<(), Error> {
    let base = u32::try_from(vertex_start).map_err(|_| Error::InvalidPendulumScene)?;
    for face in 0..6u32 {
        let first = base + face * 4;
        indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    Ok(())
}

fn normalize_slot_winding(batch: &mut Batch, slot: usize) {
    let first = slot * INDICES_PER_SEGMENT;
    for triangle in (first..first + INDICES_PER_SEGMENT).step_by(3) {
        normalize_triangle_winding(batch, triangle);
    }
}

fn normalize_all_winding(batch: &mut Batch) {
    for triangle in (0..batch.indices.len()).step_by(3) {
        normalize_triangle_winding(batch, triangle);
    }
}

fn normalize_triangle_winding(batch: &mut Batch, triangle: usize) {
    let a = batch.vertices[batch.indices[triangle] as usize];
    let b = batch.vertices[batch.indices[triangle + 1] as usize];
    let c = batch.vertices[batch.indices[triangle + 2] as usize];
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area < 0.0 {
        batch.indices.swap(triangle + 1, triangle + 2);
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, Error> {
    let value = f32::from_le_bytes(read_array(bytes, offset)?);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidPendulumScene)
    }
}

fn read_f32s<const N: usize>(bytes: &[u8], offset: usize) -> Result<[f32; N], Error> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_f32(bytes, offset + index * 4)?;
    }
    Ok(values)
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], Error> {
    bytes
        .get(offset..offset + N)
        .ok_or(Error::InvalidPendulumScene)?
        .try_into()
        .map_err(|_| Error::InvalidPendulumScene)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTIFACT: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");

    #[test]
    fn embedded_artifact_drives_the_full_fixed_cloth_topology() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        assert_eq!(engine.segment_count(), 336);
        assert_eq!(engine.batches().len(), 2);
        assert_eq!(engine.batches()[0].vertices.len(), 8_064);
        assert_eq!(engine.batches()[0].indices.len(), 12_096);
        assert_eq!(engine.batches()[1].vertices.len(), 4);
        assert_eq!(engine.batches()[1].indices.len(), 6);

        let initial = engine.positions.clone();
        engine.step(16.0 / 9.0).unwrap();
        assert_ne!(engine.positions, initial);
        assert!(engine.batches()[0].vertices.iter().any(|vertex| {
            vertex[0].abs() <= 1.0 && vertex[1].abs() <= 1.0 && (0.0..=1.0).contains(&vertex[2])
        }));
        assert!(engine.batches().iter().all(|batch| {
            batch
                .vertices
                .iter()
                .flatten()
                .all(|component| component.is_finite())
        }));
        let mut visible_triangles = 0usize;
        let mut visible_area = 0.0f32;
        for batch in engine.batches() {
            assert_ne!(batch.rgba[3], 0);
            for triangle in batch.indices.chunks_exact(3) {
                let a = batch.vertices[triangle[0] as usize];
                let b = batch.vertices[triangle[1] as usize];
                let c = batch.vertices[triangle[2] as usize];
                let area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
                let x_min = a[0].min(b[0]).min(c[0]);
                let x_max = a[0].max(b[0]).max(c[0]);
                let y_min = a[1].min(b[1]).min(c[1]);
                let y_max = a[1].max(b[1]).max(c[1]);
                let z_visible = [a[2], b[2], c[2]]
                    .into_iter()
                    .any(|z| (0.0..=1.0).contains(&z));
                if area > f32::EPSILON
                    && x_max >= -1.0
                    && x_min <= 1.0
                    && y_max >= -1.0
                    && y_min <= 1.0
                    && z_visible
                {
                    visible_triangles += 1;
                    visible_area += area;
                }
            }
        }
        assert!(visible_triangles >= 1_000);
        assert!(visible_area >= 0.1);
        assert_eq!(
            engine
                .batches()
                .iter()
                .map(|batch| batch.indices.len() / 3)
                .sum::<usize>(),
            4_034
        );

        let native = engine.step_instances(16.0 / 9.0).unwrap();
        assert_eq!(native.instances.len(), NATIVE_INSTANCE_COUNT);
        assert_eq!(native.compacted_indices.len(), NATIVE_INSTANCE_COUNT);
        assert_eq!(native.draws[0].index_count, 36);
        assert_eq!(native.draws[0].instance_count, SEGMENT_COUNT as u32);
        assert_eq!(native.draws[1].instance_count, 1);
        assert_eq!(native.draws[1].first_instance, SEGMENT_COUNT as u32);
        assert_eq!(native.meshes[0].vertex_count, VERTICES_PER_SEGMENT as u32);
        for (group, draw) in native.draws.iter().enumerate() {
            let mesh = native.meshes[group / MATERIAL_COUNT];
            assert_eq!(draw.index_count, mesh.index_count);
            assert_eq!(draw.first_index, mesh.first_index);
            assert_eq!(draw.base_vertex, mesh.base_vertex);
            if group >= 2 {
                assert_eq!(draw.instance_count, 0);
            }
        }
        assert!(native.instances.iter().all(|instance| {
            instance.model.iter().all(|component| component.is_finite())
                && instance
                    .normal_mat
                    .iter()
                    .all(|component| component.is_finite())
                && instance
                    .bounds
                    .iter()
                    .all(|component| component.is_finite())
        }));

        engine.toggle_enabled();
        assert!(!engine.enabled());
        engine.reset();
        assert_eq!(engine.positions, initial);
    }

    #[test]
    fn decoder_rejects_nonzero_reserved_bytes() {
        let artifact = trueos_helio_artifact::Artifact::parse(ARTIFACT).unwrap();
        let mut encoded = artifact.section(SECTION_NAME).unwrap().data.to_vec();
        encoded[56] = 1;
        assert_eq!(Spec::decode(&encoded), Err(Error::InvalidPendulumScene));
        encoded[56] = 0;
        encoded[191] = 1;
        assert_eq!(Spec::decode(&encoded), Err(Error::InvalidPendulumScene));
    }
}
