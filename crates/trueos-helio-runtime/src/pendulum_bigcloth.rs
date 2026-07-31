//! Runtime for Helio's versioned big pendulum-cloth scene.
//!
//! The artifact carries the authored scene parameters.  TRUEOS keeps the
//! visible topology fixed and advances a compact Verlet/PBD equivalent of the
//! demo's ball-joint lattice, allowing the resident GPU mesh to be updated in
//! place for every frame.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Camera, Error, Projector, churn::Batch, linear_rgba_to_srgba8};
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
        let mut engine = Self {
            spec,
            positions: vec![[0.0; 3]; SEGMENT_COUNT],
            previous: vec![[0.0; 3]; SEGMENT_COUNT],
            batches,
            enabled: true,
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
        if self.enabled {
            self.integrate();
            for _ in 0..self.spec.constraint_iterations {
                self.solve_lengthwise_constraints();
                self.solve_cross_constraints();
                self.solve_ground();
            }
        }
        self.project_batches(aspect)?;
        Ok(&self.batches)
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
        assert!(engine.batches().iter().all(|batch| {
            batch
                .vertices
                .iter()
                .flatten()
                .all(|component| component.is_finite())
        }));
        assert_eq!(
            engine
                .batches()
                .iter()
                .map(|batch| batch.indices.len() / 3)
                .sum::<usize>(),
            4_034
        );

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
