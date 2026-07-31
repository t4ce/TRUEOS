//! Artifact-driven, allocation-stable port of Helio's shape battle scene.
//!
//! Hosted Helio uses Rapier for this example.  The TRUEOS runtime keeps the
//! visible contract while using a small deterministic fixed-step solver: the
//! kernel does not need to host Rapier, winit, or a wall clock.  Geometry is
//! retained in fixed-topology batches and only projected vertex positions are
//! rewritten from frame to frame.

use alloc::vec;
use alloc::vec::Vec;

use crate::churn::Batch;
use crate::{Camera, Error, Projector, linear_rgba_to_srgba8};
use trueos_helio_artifact::SectionKind;

pub const SECTION_NAME: &str = "scene/shape-battle-v1.bin";
const MAGIC: &[u8; 8] = b"HBATTLE\0";
const VERSION: u16 = 1;
const ENCODED_LEN: usize = 320;
const MATERIAL_COUNT: usize = 4;
const SHAPE_VARIANT_COUNT: usize = 4;
const MIN_SUPPORTED_SHAPES: usize = 4;
const MAX_SUPPORTED_SHAPES: usize = 16;
const MAX_PARTICLES_PER_BLAST: usize = 32;
const SHAPE_SLOTS_PER_MATERIAL: usize = MAX_SUPPORTED_SHAPES.div_ceil(MATERIAL_COUNT);
const PARTICLE_SLOTS_PER_MATERIAL: usize =
    (MAX_SUPPORTED_SHAPES * MAX_PARTICLES_PER_BLAST).div_ceil(MATERIAL_COUNT);
const DYNAMIC_SLOTS_PER_MATERIAL: usize = SHAPE_SLOTS_PER_MATERIAL + PARTICLE_SLOTS_PER_MATERIAL;
const VERTICES_PER_BOX: usize = 24;
const INDICES_PER_BOX: usize = 36;
const HIDDEN: [f32; 3] = [2.0, 2.0, 0.999];

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    start_shapes: usize,
    min_shapes: usize,
    max_shapes: usize,
    particles_per_blast: usize,
    seed: u64,
    camera: Camera,
    clear_rgba: [u8; 4],
    floor_rgba: [u8; 4],
    arena_radius: f32,
    wall_height: f32,
    wall_thickness: f32,
    restitution: f32,
    material_rgba: [[u8; 4]; MATERIAL_COUNT],
    shape_half_extents: [[f32; 3]; SHAPE_VARIANT_COUNT],
    collider_radii: [f32; SHAPE_VARIANT_COUNT],
    fixed_dt: f32,
    launch_speed: f32,
    launch_up_speed: f32,
    gravity: f32,
    elimination_speed: f32,
    particle_speed: f32,
    particle_lifetime_frames: u32,
    round_reset_frames: u32,
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let section = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingBattleScene)?;
        if section.kind != SectionKind::Unknown(u16::MAX) {
            return Err(Error::InvalidBattleScene);
        }
        let bytes = section.data;
        if bytes.len() != ENCODED_LEN
            || bytes.get(..8) != Some(MAGIC.as_slice())
            || read_u16(bytes, 8)? != VERSION
            || usize::from(read_u16(bytes, 10)?) != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 12)?).map_err(|_| Error::InvalidBattleScene)?
                != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 28)?).map_err(|_| Error::InvalidBattleScene)?
                != MATERIAL_COUNT
            || usize::try_from(read_u32(bytes, 32)?).map_err(|_| Error::InvalidBattleScene)?
                != SHAPE_VARIANT_COUNT
            || bytes[288..].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidBattleScene);
        }

        let start_shapes = read_usize(bytes, 16)?;
        let min_shapes = read_usize(bytes, 20)?;
        let max_shapes = read_usize(bytes, 24)?;
        let particles_per_blast = read_usize(bytes, 36)?;
        if min_shapes < MIN_SUPPORTED_SHAPES
            || min_shapes > start_shapes
            || start_shapes > max_shapes
            || max_shapes > MAX_SUPPORTED_SHAPES
            || particles_per_blast == 0
            || particles_per_blast > MAX_PARTICLES_PER_BLAST
        {
            return Err(Error::InvalidBattleScene);
        }

        let position = read_f32s::<3>(bytes, 48)?;
        let yaw = read_f32(bytes, 60)?;
        let pitch = read_f32(bytes, 64)?;
        let cos_pitch = libm::cosf(pitch);
        let camera = Camera {
            position,
            target: [
                position[0] + libm::sinf(yaw) * cos_pitch,
                position[1] + libm::sinf(pitch),
                position[2] - libm::cosf(yaw) * cos_pitch,
            ],
            up: [0.0, 1.0, 0.0],
            vertical_fov_radians: read_f32(bytes, 68)?,
            near: read_f32(bytes, 72)?,
            far: read_f32(bytes, 76)?,
        };
        Projector::new(camera, 1.0)?;

        let mut material_rgba = [[0; 4]; MATERIAL_COUNT];
        for (index, rgba) in material_rgba.iter_mut().enumerate() {
            *rgba = linear_rgba_to_srgba8(read_f32s(bytes, 128 + index * 16)?)?;
        }
        let mut shape_half_extents = [[0.0; 3]; SHAPE_VARIANT_COUNT];
        for (index, extents) in shape_half_extents.iter_mut().enumerate() {
            *extents = read_f32s(bytes, 192 + index * 12)?;
            if extents.iter().any(|value| *value <= 0.0) {
                return Err(Error::InvalidBattleScene);
            }
        }
        let collider_radii = read_f32s::<SHAPE_VARIANT_COUNT>(bytes, 240)?;
        if collider_radii.iter().any(|radius| *radius <= 0.0) {
            return Err(Error::InvalidBattleScene);
        }

        let spec = Self {
            start_shapes,
            min_shapes,
            max_shapes,
            particles_per_blast,
            seed: read_u64(bytes, 40)?,
            camera,
            clear_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 80)?)?,
            floor_rgba: linear_rgba_to_srgba8(read_f32s(bytes, 96)?)?,
            arena_radius: read_f32(bytes, 112)?,
            wall_height: read_f32(bytes, 116)?,
            wall_thickness: read_f32(bytes, 120)?,
            restitution: read_f32(bytes, 124)?,
            material_rgba,
            shape_half_extents,
            collider_radii,
            fixed_dt: read_f32(bytes, 256)?,
            launch_speed: read_f32(bytes, 260)?,
            launch_up_speed: read_f32(bytes, 264)?,
            gravity: read_f32(bytes, 268)?,
            elimination_speed: read_f32(bytes, 272)?,
            particle_speed: read_f32(bytes, 276)?,
            particle_lifetime_frames: read_u32(bytes, 280)?,
            round_reset_frames: read_u32(bytes, 284)?,
        };
        if spec.arena_radius <= 1.0
            || spec.wall_height <= 0.0
            || spec.wall_thickness <= 0.0
            || !(0.0..=1.0).contains(&spec.restitution)
            || spec.fixed_dt <= 0.0
            || spec.fixed_dt > 0.1
            || spec.launch_speed <= 0.0
            || spec.launch_up_speed < 0.0
            || spec.gravity > 0.0
            || spec.elimination_speed < 0.0
            || spec.elimination_speed >= spec.launch_speed
            || spec.particle_speed <= 0.0
            || spec.particle_lifetime_frames == 0
            || spec.round_reset_frames == 0
        {
            return Err(Error::InvalidBattleScene);
        }
        Ok(spec)
    }
}

#[derive(Clone, Copy, Debug)]
struct Shape {
    position: [f32; 3],
    velocity: [f32; 3],
    angle: f32,
    angular_speed: f32,
    scale: f32,
    variant: usize,
    material: usize,
    eliminated: bool,
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    position: [f32; 3],
    velocity: [f32; 3],
    age_frames: u32,
    material: usize,
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

    fn unit_f32(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}

pub struct Engine {
    spec: Spec,
    shape_count: usize,
    shapes: Vec<Shape>,
    particles: Vec<Particle>,
    batches: Vec<Batch>,
    rng: Rng,
    round_active: bool,
    reset_frames: u32,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let mut batches = Vec::with_capacity(2 + MATERIAL_COUNT);
        batches.push(Batch {
            vertices: vec![HIDDEN; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            rgba: spec.floor_rgba,
        });
        let mut wall_indices = Vec::with_capacity(4 * INDICES_PER_BOX);
        for slot in 0..4 {
            append_box_indices(&mut wall_indices, slot * VERTICES_PER_BOX)?;
        }
        batches.push(Batch {
            vertices: vec![HIDDEN; 4 * VERTICES_PER_BOX],
            indices: wall_indices,
            rgba: spec.material_rgba[0],
        });
        for rgba in spec.material_rgba {
            let mut indices = Vec::with_capacity(DYNAMIC_SLOTS_PER_MATERIAL * INDICES_PER_BOX);
            for slot in 0..DYNAMIC_SLOTS_PER_MATERIAL {
                append_box_indices(&mut indices, slot * VERTICES_PER_BOX)?;
            }
            batches.push(Batch {
                vertices: vec![HIDDEN; DYNAMIC_SLOTS_PER_MATERIAL * VERTICES_PER_BOX],
                indices,
                rgba,
            });
        }
        let shape_count = spec.start_shapes;
        let seed = spec.seed;
        let particle_capacity = spec.max_shapes * spec.particles_per_blast;
        let mut engine = Self {
            spec,
            shape_count,
            shapes: Vec::with_capacity(MAX_SUPPORTED_SHAPES),
            particles: Vec::with_capacity(particle_capacity),
            batches,
            rng: Rng::new(seed),
            round_active: false,
            reset_frames: 0,
        };
        engine.start_new_round();
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

    pub fn shape_count(&self) -> usize {
        self.shape_count
    }

    pub fn adjust_shape_count(&mut self, delta: i32) {
        self.shape_count = if delta >= 0 {
            self.shape_count
                .saturating_add(delta as usize)
                .min(self.spec.max_shapes)
        } else {
            self.shape_count
                .saturating_sub(delta.unsigned_abs() as usize)
                .max(self.spec.min_shapes)
        };
        self.start_new_round();
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn step(&mut self, aspect: f32) -> Result<&[Batch], Error> {
        if self.round_active {
            self.step_shapes();
        } else if self.reset_frames > 0 {
            self.reset_frames -= 1;
            if self.reset_frames == 0 {
                self.start_new_round();
            }
        }
        self.step_particles();
        self.rebuild_batches(aspect)?;
        Ok(&self.batches)
    }

    fn start_new_round(&mut self) {
        self.shapes.clear();
        self.particles.clear();
        let radius = self.spec.arena_radius * 0.75;
        for index in 0..self.shape_count {
            let angle = index as f32 * core::f32::consts::TAU / self.shape_count as f32;
            let position = [libm::cosf(angle) * radius, 1.0, libm::sinf(angle) * radius];
            let direction = normalize_or([-position[0], 0.2, -position[2]], [0.0, 0.0, -1.0]);
            self.shapes.push(Shape {
                position,
                velocity: [
                    direction[0] * self.spec.launch_speed,
                    self.spec.launch_up_speed,
                    direction[2] * self.spec.launch_speed,
                ],
                angle: angle,
                angular_speed: 4.0 + self.rng.unit_f32() * 2.0,
                scale: 1.0 + index as f32 * 0.05,
                variant: index % SHAPE_VARIANT_COUNT,
                material: index % MATERIAL_COUNT,
                eliminated: false,
            });
        }
        self.round_active = true;
        self.reset_frames = 0;
    }

    fn step_shapes(&mut self) {
        let dt = self.spec.fixed_dt;
        for shape in &mut self.shapes {
            if shape.eliminated {
                continue;
            }
            shape.velocity[1] += self.spec.gravity * dt;
            add_scaled(&mut shape.position, shape.velocity, dt);
            shape.angle += shape.angular_speed * dt;
            let radius = self.spec.collider_radii[shape.variant] * shape.scale;
            if shape.position[1] < radius {
                shape.position[1] = radius;
                if shape.velocity[1] < 0.0 {
                    shape.velocity[1] = -shape.velocity[1] * self.spec.restitution;
                }
            }
            let wall = self.spec.arena_radius - radius;
            for axis in [0usize, 2] {
                if shape.position[axis] > wall {
                    shape.position[axis] = wall;
                    shape.velocity[axis] = -shape.velocity[axis].abs() * self.spec.restitution;
                } else if shape.position[axis] < -wall {
                    shape.position[axis] = -wall;
                    shape.velocity[axis] = shape.velocity[axis].abs() * self.spec.restitution;
                }
            }
            for velocity in &mut shape.velocity {
                *velocity *= 0.998;
            }
        }

        for left_index in 0..self.shapes.len() {
            let (left_slice, right_slice) = self.shapes.split_at_mut(left_index + 1);
            let left = &mut left_slice[left_index];
            if left.eliminated {
                continue;
            }
            for (offset, right) in right_slice.iter_mut().enumerate() {
                if right.eliminated {
                    continue;
                }
                let delta = sub(right.position, left.position);
                let distance_sq = dot(delta, delta);
                let radii = self.spec.collider_radii[left.variant] * left.scale
                    + self.spec.collider_radii[right.variant] * right.scale;
                if distance_sq >= radii * radii {
                    continue;
                }
                let normal = if distance_sq > 1.0e-8 {
                    scale(delta, 1.0 / libm::sqrtf(distance_sq))
                } else if (left_index + offset).is_multiple_of(2) {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                };
                let distance = libm::sqrtf(distance_sq).max(1.0e-4);
                let correction = (radii - distance) * 0.5;
                add_scaled(&mut left.position, normal, -correction);
                add_scaled(&mut right.position, normal, correction);
                let relative_speed = dot(sub(right.velocity, left.velocity), normal);
                if relative_speed < 0.0 {
                    let impulse = -(1.0 + self.spec.restitution) * relative_speed * 0.5;
                    add_scaled(&mut left.velocity, normal, -impulse);
                    add_scaled(&mut right.velocity, normal, impulse);
                    left.angular_speed = -left.angular_speed;
                    right.angular_speed = -right.angular_speed;
                }
            }
        }

        let mut explosions = Vec::new();
        let mut alive = 0usize;
        for shape in &mut self.shapes {
            if shape.eliminated {
                continue;
            }
            let radial = libm::sqrtf(
                shape.position[0] * shape.position[0] + shape.position[2] * shape.position[2],
            );
            let speed = libm::sqrtf(dot(shape.velocity, shape.velocity));
            if radial > self.spec.arena_radius || speed < self.spec.elimination_speed {
                shape.eliminated = true;
                explosions.push(shape.position);
            } else {
                alive += 1;
            }
        }
        for position in explosions {
            self.create_explosion(position);
        }
        if alive <= 1 {
            self.round_active = false;
            self.reset_frames = self.spec.round_reset_frames;
        }
    }

    fn create_explosion(&mut self, position: [f32; 3]) {
        for index in 0..self.spec.particles_per_blast {
            let angle =
                index as f32 * core::f32::consts::TAU / self.spec.particles_per_blast as f32;
            let direction =
                normalize_or([libm::cosf(angle), 0.3, libm::sinf(angle)], [1.0, 0.3, 0.0]);
            let speed = self.spec.particle_speed * (1.0 + index as f32 * 0.025);
            self.particles.push(Particle {
                position: [
                    position[0] + direction[0] * 0.2,
                    position[1] + direction[1] * 0.2,
                    position[2] + direction[2] * 0.2,
                ],
                velocity: scale(direction, speed),
                age_frames: 0,
                material: index % MATERIAL_COUNT,
            });
        }
    }

    fn step_particles(&mut self) {
        let dt = self.spec.fixed_dt;
        let lifetime = self.spec.particle_lifetime_frames;
        self.particles.retain_mut(|particle| {
            particle.age_frames = particle.age_frames.saturating_add(1);
            if particle.age_frames >= lifetime {
                return false;
            }
            for velocity in &mut particle.velocity {
                *velocity *= 0.94;
            }
            add_scaled(&mut particle.position, particle.velocity, dt);
            true
        });
    }

    fn rebuild_batches(&mut self, aspect: f32) -> Result<(), Error> {
        let projector = Projector::new(self.spec.camera, aspect)?;
        for batch in &mut self.batches {
            batch.vertices.fill(HIDDEN);
        }

        let extent = self.spec.arena_radius;
        let floor = [
            [-extent, 0.0, -extent],
            [extent, 0.0, -extent],
            [extent, 0.0, extent],
            [-extent, 0.0, extent],
        ];
        for (target, point) in self.batches[0].vertices.iter_mut().zip(floor) {
            *target = projector.project(point)?;
        }
        normalize_all_winding(&mut self.batches[0]);

        let wall_half = self.spec.wall_thickness * 0.5;
        let wall_y = self.spec.wall_height * 0.5;
        let walls = [
            ([0.0, wall_y, extent + wall_half], [extent, wall_y, wall_half]),
            ([0.0, wall_y, -extent - wall_half], [extent, wall_y, wall_half]),
            ([extent + wall_half, wall_y, 0.0], [wall_half, wall_y, extent]),
            ([-extent - wall_half, wall_y, 0.0], [wall_half, wall_y, extent]),
        ];
        for (slot, (center, extents)) in walls.into_iter().enumerate() {
            write_projected_box(&projector, &mut self.batches[1], slot, center, extents, 0.0)?;
        }

        for (index, shape) in self.shapes.iter().copied().enumerate() {
            if shape.eliminated {
                continue;
            }
            let material = shape.material;
            let slot = index / MATERIAL_COUNT;
            let extents = scale(self.spec.shape_half_extents[shape.variant], shape.scale);
            write_projected_box(
                &projector,
                &mut self.batches[2 + material],
                slot,
                shape.position,
                extents,
                shape.angle,
            )?;
        }
        let mut particle_slots = [0usize; MATERIAL_COUNT];
        for particle in self.particles.iter().copied() {
            let material = particle.material;
            let slot = SHAPE_SLOTS_PER_MATERIAL + particle_slots[material];
            particle_slots[material] += 1;
            if slot >= DYNAMIC_SLOTS_PER_MATERIAL {
                return Err(Error::InvalidBattleScene);
            }
            write_projected_box(
                &projector,
                &mut self.batches[2 + material],
                slot,
                particle.position,
                [0.12; 3],
                0.0,
            )?;
        }
        Ok(())
    }
}

fn write_projected_box(
    projector: &Projector,
    batch: &mut Batch,
    slot: usize,
    center: [f32; 3],
    extents: [f32; 3],
    angle: f32,
) -> Result<(), Error> {
    let start = slot
        .checked_mul(VERTICES_PER_BOX)
        .ok_or(Error::InvalidBattleScene)?;
    let end = start
        .checked_add(VERTICES_PER_BOX)
        .ok_or(Error::InvalidBattleScene)?;
    let target = batch
        .vertices
        .get_mut(start..end)
        .ok_or(Error::InvalidBattleScene)?;
    let (sin_angle, cos_angle) = (libm::sinf(angle), libm::cosf(angle));
    for (output, local) in target.iter_mut().zip(box_vertices(extents)) {
        let world = [
            center[0] + local[0] * cos_angle + local[2] * sin_angle,
            center[1] + local[1],
            center[2] - local[0] * sin_angle + local[2] * cos_angle,
        ];
        *output = projector.project(world)?;
    }
    normalize_slot_winding(batch, slot);
    Ok(())
}

fn box_vertices(extents: [f32; 3]) -> [[f32; 3]; VERTICES_PER_BOX] {
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
    let base = u32::try_from(vertex_start).map_err(|_| Error::InvalidBattleScene)?;
    for face in 0..6u32 {
        let first = base + face * 4;
        indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    Ok(())
}

fn normalize_slot_winding(batch: &mut Batch, slot: usize) {
    let first = slot * INDICES_PER_BOX;
    for triangle in (first..first + INDICES_PER_BOX).step_by(3) {
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

fn add_scaled(target: &mut [f32; 3], value: [f32; 3], factor: f32) {
    for index in 0..3 {
        target[index] += value[index] * factor;
    }
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(value: [f32; 3], factor: f32) -> [f32; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn normalize_or(value: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length = libm::sqrtf(dot(value, value));
    if length.is_finite() && length > 1.0e-8 {
        scale(value, 1.0 / length)
    } else {
        fallback
    }
}

fn read_usize(bytes: &[u8], offset: usize) -> Result<usize, Error> {
    usize::try_from(read_u32(bytes, offset)?).map_err(|_| Error::InvalidBattleScene)
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
    value
        .is_finite()
        .then_some(value)
        .ok_or(Error::InvalidBattleScene)
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
        .ok_or(Error::InvalidBattleScene)?
        .try_into()
        .map_err(|_| Error::InvalidBattleScene)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTIFACT: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");

    #[test]
    fn embedded_battle_has_fixed_topology() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        assert_eq!(engine.shape_count(), 4);
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.batches().len(), 6);
        assert_eq!(engine.batches()[0].indices.len(), 6);
        assert_eq!(engine.batches()[1].indices.len(), 4 * INDICES_PER_BOX);
        assert!(
            engine.batches()[2..]
                .iter()
                .all(|batch| batch.indices.len() == DYNAMIC_SLOTS_PER_MATERIAL * INDICES_PER_BOX)
        );
        let topology: Vec<usize> = engine
            .batches()
            .iter()
            .map(|batch| batch.indices.len())
            .collect();
        // Cross collisions, eliminations, particle expiry, and at least one
        // automatic round reset without ever changing resident topology.
        for _ in 0..1_200 {
            engine.step(16.0 / 9.0).unwrap();
        }
        assert_eq!(
            topology,
            engine
                .batches()
                .iter()
                .map(|batch| batch.indices.len())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn count_adjustment_clamps_and_restarts() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        for _ in 0..32 {
            engine.adjust_shape_count(1);
        }
        assert_eq!(engine.shape_count(), 16);
        for _ in 0..32 {
            engine.adjust_shape_count(-1);
        }
        assert_eq!(engine.shape_count(), 4);
    }
}
