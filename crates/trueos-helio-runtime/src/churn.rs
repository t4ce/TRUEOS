//! Runtime for the versioned churn scene carried inside a Helio artifact.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Camera, DrawIndexedIndirectArgs, Error, Projector, linear_rgba_to_srgba8};

pub const SECTION_NAME: &str = "scene/churn-v1.bin";
const MAGIC: &[u8; 8] = b"HCHURN\0\0";
const VERSION: u16 = 1;
const ENCODED_LEN: usize = 320;
const MATERIAL_COUNT: usize = 4;
const SHAPE_COUNT: usize = 3;
const VERTICES_PER_OBJECT: usize = 24;
const INDICES_PER_OBJECT: usize = 36;
const HIDDEN: [f32; 3] = [2.0, 2.0, 0.999];
const ANIMATION_RATE_SCALE: f32 = 1.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub max_objects: usize,
    pub spawn_rate: usize,
    pub spawn_interval_frames: u32,
    pub seed: u64,
    pub camera: Camera,
    pub clear_rgba: [u8; 4],
    pub floor_rgba: [u8; 4],
    pub floor_extent: f32,
    pub material_rgba: [[u8; 4]; MATERIAL_COUNT],
    shape_half_extents: [[f32; 3]; SHAPE_COUNT],
    time_step: f32,
    orbit_radius: f32,
    orbit_radius_amplitude: f32,
    height_base: f32,
    height_amplitude: f32,
    radius_phase_scale: f32,
    height_phase_scale: f32,
    rotation_scale: f32,
    scale_range: [f32; 2],
    speed_range: [f32; 2],
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let bytes = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingChurnScene)?
            .data;
        if bytes.len() != ENCODED_LEN
            || bytes.get(..8) != Some(MAGIC.as_slice())
            || read_u16(bytes, 8)? != VERSION
            || usize::from(read_u16(bytes, 10)?) != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 12)?).map_err(|_| Error::InvalidChurnScene)?
                != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 28)?).map_err(|_| Error::InvalidChurnScene)?
                != MATERIAL_COUNT
            || usize::try_from(read_u32(bytes, 32)?).map_err(|_| Error::InvalidChurnScene)?
                != SHAPE_COUNT
        {
            return Err(Error::InvalidChurnScene);
        }

        let max_objects =
            usize::try_from(read_u32(bytes, 16)?).map_err(|_| Error::InvalidChurnScene)?;
        let spawn_rate =
            usize::try_from(read_u32(bytes, 20)?).map_err(|_| Error::InvalidChurnScene)?;
        let spawn_interval_frames = read_u32(bytes, 24)?;
        if max_objects == 0
            || max_objects > 4_096
            || spawn_rate == 0
            || spawn_rate > max_objects
            || spawn_interval_frames == 0
        {
            return Err(Error::InvalidChurnScene);
        }

        let position = read_f32s::<3>(bytes, 48)?;
        let yaw = read_f32(bytes, 60)?;
        let pitch = read_f32(bytes, 64)?;
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
            vertical_fov_radians: read_f32(bytes, 68)?,
            near: read_f32(bytes, 72)?,
            far: read_f32(bytes, 76)?,
        };
        // Projector construction is the canonical camera validation.
        Projector::new(camera, 1.0)?;

        let mut material_rgba = [[0; 4]; MATERIAL_COUNT];
        for (index, rgba) in material_rgba.iter_mut().enumerate() {
            *rgba = linear_rgba_to_srgba8(read_f32s(bytes, 128 + index * 16)?)?;
        }
        let mut shape_half_extents = [[0.0; 3]; SHAPE_COUNT];
        for (index, extents) in shape_half_extents.iter_mut().enumerate() {
            *extents = read_f32s(bytes, 192 + index * 12)?;
            if extents.iter().any(|value| *value <= 0.0) {
                return Err(Error::InvalidChurnScene);
            }
        }

        let spec = Self {
            max_objects,
            spawn_rate,
            spawn_interval_frames,
            seed: read_u64(bytes, 40)?,
            camera,
            clear_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 80)?)?,
            floor_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 112)?)?,
            floor_extent: read_f32(bytes, 108)?,
            material_rgba,
            shape_half_extents,
            time_step: read_f32(bytes, 228)?,
            orbit_radius: read_f32(bytes, 232)?,
            orbit_radius_amplitude: read_f32(bytes, 236)?,
            height_base: read_f32(bytes, 240)?,
            height_amplitude: read_f32(bytes, 244)?,
            radius_phase_scale: read_f32(bytes, 248)?,
            height_phase_scale: read_f32(bytes, 252)?,
            rotation_scale: read_f32(bytes, 256)?,
            scale_range: [read_f32(bytes, 276)?, read_f32(bytes, 280)?],
            speed_range: [read_f32(bytes, 284)?, read_f32(bytes, 288)?],
        };
        if !spec.floor_extent.is_finite()
            || spec.floor_extent <= 0.0
            || spec.scale_range[0] <= 0.0
            || spec.scale_range[1] < spec.scale_range[0]
            || spec.speed_range[0] <= 0.0
            || spec.speed_range[1] < spec.speed_range[0]
            || [
                spec.time_step,
                spec.orbit_radius,
                spec.orbit_radius_amplitude,
                spec.height_base,
                spec.height_amplitude,
                spec.radius_phase_scale,
                spec.height_phase_scale,
                spec.rotation_scale,
            ]
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(Error::InvalidChurnScene);
        }
        Ok(spec)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub rgba: [u8; 4],
}

impl Batch {
    /// The command record Helio owns for this material batch. Geometry is
    /// expanded today, so each batch is one indexed instance; moving object
    /// transforms to a storage buffer later changes the instance fields but
    /// not this ABI or the Intel consumer.
    pub fn draw_indexed_indirect(&self) -> Result<DrawIndexedIndirectArgs, Error> {
        let index_count =
            u32::try_from(self.indices.len()).map_err(|_| Error::InvalidChurnScene)?;
        if index_count == 0 {
            return Err(Error::InvalidChurnScene);
        }
        Ok(DrawIndexedIndirectArgs::new(index_count))
    }
}

#[derive(Clone, Copy, Debug)]
struct Object {
    seed: f32,
    speed: f32,
    scale: f32,
    shape: usize,
}

#[derive(Clone, Copy, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u32(&mut self) -> u32 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        (value >> 32) as u32
    }

    fn next_f32(&mut self, range: [f32; 2]) -> f32 {
        let fraction = self.next_u32() as f32 / u32::MAX as f32;
        range[0] + (range[1] - range[0]) * fraction
    }
}

pub struct Engine {
    spec: Spec,
    objects: Vec<Object>,
    batches: Vec<Batch>,
    rng: Rng,
    frame: u64,
    recycle: usize,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let slots_per_batch = spec.max_objects.div_ceil(MATERIAL_COUNT);
        let mut batches = Vec::with_capacity(MATERIAL_COUNT);
        for rgba in spec.material_rgba {
            let vertex_count = slots_per_batch
                .checked_mul(VERTICES_PER_OBJECT)
                .ok_or(Error::InvalidChurnScene)?;
            let index_count = slots_per_batch
                .checked_mul(INDICES_PER_OBJECT)
                .ok_or(Error::InvalidChurnScene)?;
            let mut indices = Vec::with_capacity(index_count);
            for slot in 0..slots_per_batch {
                append_box_indices(&mut indices, slot * VERTICES_PER_OBJECT)?;
            }
            batches.push(Batch {
                vertices: vec![HIDDEN; vertex_count],
                indices,
                rgba,
            });
        }
        Ok(Self {
            rng: Rng::new(spec.seed),
            spec,
            objects: Vec::new(),
            batches,
            frame: 0,
            recycle: 0,
        })
    }

    pub fn active_objects(&self) -> usize {
        self.objects.len()
    }

    pub fn camera(&self) -> Camera {
        self.spec.camera
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), Error> {
        // Use the same validation as projection before making the new camera
        // visible to a frame.
        Projector::new(camera, 1.0)?;
        self.spec.camera = camera;
        Ok(())
    }

    pub fn spawn_rate(&self) -> usize {
        self.spec.spawn_rate
    }

    pub fn adjust_spawn_rate(&mut self, delta: i32) {
        self.spec.spawn_rate = if delta >= 0 {
            self.spec
                .spawn_rate
                .saturating_add(delta as usize)
                .min(self.spec.max_objects)
        } else {
            self.spec
                .spawn_rate
                .saturating_sub(delta.unsigned_abs() as usize)
                .max(1)
        };
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn step(&mut self, aspect: f32) -> Result<&[Batch], Error> {
        if self.frame % u64::from(self.spec.spawn_interval_frames) == 0 {
            self.spawn();
        }
        for batch in &mut self.batches {
            batch.vertices.fill(HIDDEN);
        }
        let projector = Projector::new(self.spec.camera, aspect)?;
        // Speed up only the visual clock. Spawn cadence and retained geometry
        // remain driven by their baked frame/count contracts.
        let time = self.frame as f32 * self.spec.time_step * ANIMATION_RATE_SCALE;
        for (object_index, object) in self.objects.iter().copied().enumerate() {
            let phase = object.seed + time * object.speed;
            let radius = self.spec.orbit_radius
                + libm::sinf(phase * self.spec.radius_phase_scale)
                    * self.spec.orbit_radius_amplitude;
            let center = [
                libm::cosf(phase) * radius,
                self.spec.height_base
                    + libm::sinf(phase * self.spec.height_phase_scale) * self.spec.height_amplitude,
                libm::sinf(phase) * radius,
            ];
            let angle = phase * self.spec.rotation_scale;
            let (sin_angle, cos_angle) = (libm::sinf(angle), libm::cosf(angle));
            let batch_index = object_index % MATERIAL_COUNT;
            let slot = object_index / MATERIAL_COUNT;
            let start = slot * VERTICES_PER_OBJECT;
            let local = box_vertices(self.spec.shape_half_extents[object.shape]);
            for (offset, point) in local.into_iter().enumerate() {
                let scaled = [
                    point[0] * object.scale,
                    point[1] * object.scale,
                    point[2] * object.scale,
                ];
                let world = [
                    center[0] + scaled[0] * cos_angle + scaled[2] * sin_angle,
                    center[1] + scaled[1],
                    center[2] - scaled[0] * sin_angle + scaled[2] * cos_angle,
                ];
                self.batches[batch_index].vertices[start + offset] = projector.project(world)?;
            }
            normalize_slot_winding(&mut self.batches[batch_index], slot);
        }
        self.frame = self.frame.wrapping_add(1);
        Ok(&self.batches)
    }

    pub fn floor(&self, aspect: f32) -> Result<Batch, Error> {
        let projector = Projector::new(self.spec.camera, aspect)?;
        let extent = self.spec.floor_extent;
        // Keep the near edge in front of the fixed camera while retaining the
        // baked floor's full depth away from it.
        let near_z = self.spec.camera.position[2] - self.spec.camera.near - 1.0;
        let points = [
            [-extent, -0.01, -extent],
            [extent, -0.01, -extent],
            [extent, -0.01, near_z],
            [-extent, -0.01, near_z],
        ];
        let mut vertices = Vec::with_capacity(4);
        for point in points {
            vertices.push(projector.project(point)?);
        }
        let mut batch = Batch {
            vertices,
            indices: vec![0, 1, 2, 0, 2, 3],
            rgba: self.spec.floor_rgba,
        };
        normalize_all_winding(&mut batch);
        Ok(batch)
    }

    fn spawn(&mut self) {
        for _ in 0..self.spec.spawn_rate {
            let object = Object {
                shape: self.rng.next_u32() as usize % SHAPE_COUNT,
                seed: self.rng.next_f32([0.0, core::f32::consts::TAU]),
                speed: self.rng.next_f32(self.spec.speed_range),
                scale: self.rng.next_f32(self.spec.scale_range),
            };
            if self.objects.len() < self.spec.max_objects {
                self.objects.push(object);
            } else {
                self.objects[self.recycle] = object;
                self.recycle = (self.recycle + 1) % self.spec.max_objects;
            }
        }
    }
}

fn box_vertices(extents: [f32; 3]) -> [[f32; 3]; VERTICES_PER_OBJECT] {
    let [x, y, z] = extents;
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
    let base = u32::try_from(vertex_start).map_err(|_| Error::InvalidChurnScene)?;
    for face in 0..6u32 {
        let first = base + face * 4;
        indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    Ok(())
}

fn normalize_slot_winding(batch: &mut Batch, slot: usize) {
    let first = slot * INDICES_PER_OBJECT;
    for triangle in (first..first + INDICES_PER_OBJECT).step_by(3) {
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

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(read_array(bytes, offset)?))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, Error> {
    let value = f32::from_le_bytes(read_array(bytes, offset)?);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidChurnScene)
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
        .ok_or(Error::InvalidChurnScene)?
        .try_into()
        .map_err(|_| Error::InvalidChurnScene)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTIFACT: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");

    #[test]
    fn embedded_artifact_drives_fixed_retained_batches() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        assert_eq!(spec.max_objects, 2_200);
        assert_eq!(spec.spawn_rate, 8);
        assert_eq!(spec.spawn_interval_frames, 2);
        assert!((spec.time_step - 0.01).abs() <= f32::EPSILON);
        assert!((spec.time_step * ANIMATION_RATE_SCALE - 0.015).abs() <= f32::EPSILON);

        let mut engine = Engine::new(spec).unwrap();
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 8);
        assert_eq!(engine.batches().len(), 4);
        assert!(engine.batches().iter().all(|batch| {
            batch.vertices.len() == 13_200
                && batch.indices.len() == 19_800
                && batch
                    .vertices
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
        }));
        assert!(engine.batches().iter().all(|batch| {
            batch.draw_indexed_indirect()
                == Ok(DrawIndexedIndirectArgs {
                    index_count: 19_800,
                    instance_count: 1,
                    first_index: 0,
                    base_vertex: 0,
                    first_instance: 0,
                })
        }));
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 8);
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 16);
        let floor = engine.floor(16.0 / 9.0).unwrap();
        assert_eq!(floor.indices.len(), 6);
    }
}
