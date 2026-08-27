//! Runtime for the versioned churn scene carried inside a Helio artifact.

use alloc::vec;
use alloc::vec::Vec;

use crate::retained_transform::{
    RetainedTransformProgram, RetainedTransformTemplate, TransformHierarchyFrame,
};
use crate::{Camera, DrawIndexedIndirectArgs, Error, Projector, linear_rgba_to_srgba8};

pub const SECTION_NAME: &str = "scene/churn-v1.bin";
pub const LIGHT_SECTION_NAME: &str = "scene/churn-light-v1.bin";
const MAGIC: &[u8; 8] = b"HCHURN\0\0";
const LIGHT_MAGIC: &[u8; 8] = b"HCHLIT\0\0";
const VERSION: u16 = 1;
const ENCODED_LEN: usize = 320;
const LIGHT_ENCODED_LEN: usize = 160;
pub const MATERIAL_COUNT: usize = 4;
pub const SHAPE_COUNT: usize = 3;
pub const LIGHT_COUNT: usize = 2;
pub const DRAW_GROUP_COUNT: usize = MATERIAL_COUNT * SHAPE_COUNT;
const FACE_COUNT: usize = 6;
const BATCH_COUNT: usize = MATERIAL_COUNT * FACE_COUNT;
const VERTICES_PER_OBJECT: usize = 24;
const INDICES_PER_OBJECT: usize = 36;
const VERTICES_PER_FACE: usize = VERTICES_PER_OBJECT / FACE_COUNT;
const INDICES_PER_FACE: usize = INDICES_PER_OBJECT / FACE_COUNT;
const HIDDEN: [f32; 3] = [2.0, 2.0, 0.999];
const ANIMATION_RATE_SCALE: f32 = 1.5;
const FLAT_LIGHT_RESPONSE_SCALE: f32 = 12.0;
const COLLISION_BURST_FRAMES: u32 = 75;
const COLLISION_BURST_DISTANCE: f32 = 12.0;

pub const INSTANCE_FLAG_CASTS_SHADOW: u32 = 1 << 0;
pub const INSTANCE_FLAG_RECEIVES_SHADOW: u32 = 1 << 1;
pub const MAX_RETAINED_TRANSFORM_ROWS: usize = 4_096;

const _: () = {
    assert!(MAX_RETAINED_TRANSFORM_ROWS <= u16::MAX as usize);
};

/// Helio's storage-buffer instance ABI, byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuInstanceData {
    pub model: [f32; 16],
    pub normal_mat: [f32; 12],
    pub bounds: [f32; 4],
    pub prev_model: [f32; 16],
    pub mesh_id: u32,
    pub material_id: u32,
    pub flags: u32,
    pub lightmap_index: u32,
}

impl GpuInstanceData {
    pub const BYTE_LEN: usize = 208;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_f32s(&mut bytes, 0, &self.model);
        write_f32s(&mut bytes, 64, &self.normal_mat);
        write_f32s(&mut bytes, 112, &self.bounds);
        write_f32s(&mut bytes, 128, &self.prev_model);
        write_u32(&mut bytes, 192, self.mesh_id);
        write_u32(&mut bytes, 196, self.material_id);
        write_u32(&mut bytes, 200, self.flags);
        write_u32(&mut bytes, 204, self.lightmap_index);
        bytes
    }
}

/// Compact CPU/simulation output consumed by Helio's retained GPU transform
/// pass. The GPU expands this seed into [`GpuInstanceData`] and writes the
/// compacted draw index, so a frame producer never has to build 208-byte
/// matrices on the CPU.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuRetainedTransformSeed {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// Quaternion in x, y, z, w order. The GPU normalizes it before use.
    pub rotation: [f32; 4],
    /// Radius of the retained local-space mesh before `scale` is applied.
    pub local_radius: f32,
    pub previous_translation: [f32; 3],
    /// Index into the frame's fixed draw-template array.
    pub draw_group: u32,
    /// High 16 bits are the deterministic slot within `draw_group`; low 16
    /// bits are the instance flags copied into [`GpuInstanceData`].
    pub flags: u32,
}

impl GpuRetainedTransformSeed {
    pub const BYTE_LEN: usize = 64;
    pub const DISABLED_DRAW_GROUP: u32 = u32::MAX;
    pub const COMPACT_SLOT_SHIFT: u32 = 16;
    pub const INSTANCE_FLAGS_MASK: u32 = u16::MAX as u32;
    pub const MAX_COMPACT_SLOT: u32 = u16::MAX as u32;

    pub const fn pack_slot_and_flags(compact_slot: u32, instance_flags: u32) -> Option<u32> {
        if compact_slot > Self::MAX_COMPACT_SLOT || instance_flags & !Self::INSTANCE_FLAGS_MASK != 0
        {
            return None;
        }
        Some((compact_slot << Self::COMPACT_SLOT_SHIFT) | instance_flags)
    }

    pub const fn compact_slot(self) -> u32 {
        self.flags >> Self::COMPACT_SLOT_SHIFT
    }

    pub const fn instance_flags(self) -> u32 {
        self.flags & Self::INSTANCE_FLAGS_MASK
    }

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_f32s(&mut bytes, 0, &self.translation);
        write_f32s(&mut bytes, 12, &self.scale);
        write_f32s(&mut bytes, 24, &self.rotation);
        write_f32(&mut bytes, 40, self.local_radius);
        write_f32s(&mut bytes, 44, &self.previous_translation);
        write_u32(&mut bytes, 56, self.draw_group);
        write_u32(&mut bytes, 60, self.flags);
        bytes
    }
}

/// Immutable indexed-draw fields plus one disjoint compacted-index slice.
///
/// `first_instance..first_instance + capacity` is reserved exclusively for
/// this group. Empty groups retain their mesh metadata with `capacity == 0`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpuRetainedDrawTemplate {
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
    pub capacity: u32,
    /// Low 16 bits are mesh ID; high 16 bits are material ID.
    pub packed_mesh_material: u32,
}

impl GpuRetainedDrawTemplate {
    pub const BYTE_LEN: usize = 24;

    pub const fn new(
        mesh: MeshDescriptor,
        group: DrawGroupDescriptor,
        first_instance: u32,
        capacity: u32,
    ) -> Option<Self> {
        if mesh.mesh_id != group.mesh_id
            || group.mesh_id > u16::MAX as u32
            || group.material_id > u16::MAX as u32
        {
            return None;
        }
        Some(Self {
            index_count: mesh.index_count,
            first_index: mesh.first_index,
            base_vertex: mesh.base_vertex,
            first_instance,
            capacity,
            packed_mesh_material: group.mesh_id | (group.material_id << 16),
        })
    }

    pub const fn mesh_id(self) -> u32 {
        self.packed_mesh_material & u16::MAX as u32
    }

    pub const fn material_id(self) -> u32 {
        self.packed_mesh_material >> 16
    }

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_u32(&mut bytes, 0, self.index_count);
        write_u32(&mut bytes, 4, self.first_index);
        write_i32(&mut bytes, 8, self.base_vertex);
        write_u32(&mut bytes, 12, self.first_instance);
        write_u32(&mut bytes, 16, self.capacity);
        write_u32(&mut bytes, 20, self.packed_mesh_material);
        bytes
    }
}

/// Helio's per-frame camera storage-buffer ABI, byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuCameraUniforms {
    pub view: [f32; 16],
    pub proj: [f32; 16],
    pub view_proj: [f32; 16],
    pub inv_view_proj: [f32; 16],
    pub position_near: [f32; 4],
    pub forward_far: [f32; 4],
    pub jitter_frame: [f32; 4],
    pub prev_view_proj: [f32; 16],
}

impl GpuCameraUniforms {
    pub const BYTE_LEN: usize = 368;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_f32s(&mut bytes, 0, &self.view);
        write_f32s(&mut bytes, 64, &self.proj);
        write_f32s(&mut bytes, 128, &self.view_proj);
        write_f32s(&mut bytes, 192, &self.inv_view_proj);
        write_f32s(&mut bytes, 256, &self.position_near);
        write_f32s(&mut bytes, 272, &self.forward_far);
        write_f32s(&mut bytes, 288, &self.jitter_frame);
        write_f32s(&mut bytes, 304, &self.prev_view_proj);
        bytes
    }
}

/// Helio's material storage-buffer ABI, byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuMaterial {
    pub base_color: [f32; 4],
    pub emissive: [f32; 4],
    pub roughness_metallic: [f32; 4],
    pub tex_base_color: u32,
    pub tex_normal: u32,
    pub tex_roughness: u32,
    pub tex_emissive: u32,
    pub tex_occlusion: u32,
    pub workflow: u32,
    pub flags: u32,
    pub material_class: u32,
    pub class_params: [f32; 4],
}

impl GpuMaterial {
    pub const BYTE_LEN: usize = 96;
    pub const NO_TEXTURE: u32 = u32::MAX;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_f32s(&mut bytes, 0, &self.base_color);
        write_f32s(&mut bytes, 16, &self.emissive);
        write_f32s(&mut bytes, 32, &self.roughness_metallic);
        write_u32(&mut bytes, 48, self.tex_base_color);
        write_u32(&mut bytes, 52, self.tex_normal);
        write_u32(&mut bytes, 56, self.tex_roughness);
        write_u32(&mut bytes, 60, self.tex_emissive);
        write_u32(&mut bytes, 64, self.tex_occlusion);
        write_u32(&mut bytes, 68, self.workflow);
        write_u32(&mut bytes, 72, self.flags);
        write_u32(&mut bytes, 76, self.material_class);
        write_f32s(&mut bytes, 80, &self.class_params);
        bytes
    }
}

/// Helio's light storage-buffer ABI, including its current feature tail.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuLight {
    pub position_range: [f32; 4],
    pub direction_outer: [f32; 4],
    pub color_intensity: [f32; 4],
    pub shadow_index: u32,
    pub light_type: u32,
    pub inner_angle: f32,
    pub _pad: u32,
    pub god_rays_enabled: u32,
    pub god_rays_density: f32,
    pub god_rays_weight: f32,
    pub god_rays_decay: f32,
    pub god_rays_exposure: f32,
    pub flare_enabled: u32,
    pub flare_type: u32,
    pub flare_intensity: f32,
    pub flare_scale: f32,
    pub flare_tint_r: f32,
    pub flare_tint_g: f32,
    pub flare_tint_b: f32,
    pub ies_profile_index: i32,
    pub light_function_index: i32,
    pub ies_angle_scale: f32,
    pub ies_angle_offset: f32,
}

impl GpuLight {
    pub const BYTE_LEN: usize = 128;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_f32s(&mut bytes, 0, &self.position_range);
        write_f32s(&mut bytes, 16, &self.direction_outer);
        write_f32s(&mut bytes, 32, &self.color_intensity);
        write_u32(&mut bytes, 48, self.shadow_index);
        write_u32(&mut bytes, 52, self.light_type);
        write_f32(&mut bytes, 56, self.inner_angle);
        write_u32(&mut bytes, 60, self._pad);
        write_u32(&mut bytes, 64, self.god_rays_enabled);
        write_f32(&mut bytes, 68, self.god_rays_density);
        write_f32(&mut bytes, 72, self.god_rays_weight);
        write_f32(&mut bytes, 76, self.god_rays_decay);
        write_f32(&mut bytes, 80, self.god_rays_exposure);
        write_u32(&mut bytes, 84, self.flare_enabled);
        write_u32(&mut bytes, 88, self.flare_type);
        write_f32(&mut bytes, 92, self.flare_intensity);
        write_f32(&mut bytes, 96, self.flare_scale);
        write_f32(&mut bytes, 100, self.flare_tint_r);
        write_f32(&mut bytes, 104, self.flare_tint_g);
        write_f32(&mut bytes, 108, self.flare_tint_b);
        write_i32(&mut bytes, 112, self.ies_profile_index);
        write_i32(&mut bytes, 116, self.light_function_index);
        write_f32(&mut bytes, 120, self.ies_angle_scale);
        write_f32(&mut bytes, 124, self.ies_angle_offset);
        bytes
    }
}

impl Default for GpuLight {
    fn default() -> Self {
        Self {
            position_range: [0.0; 4],
            direction_outer: [0.0, -1.0, 0.0, 0.0],
            color_intensity: [1.0; 4],
            shadow_index: u32::MAX,
            light_type: 1,
            inner_angle: 0.0,
            _pad: 0,
            god_rays_enabled: 0,
            god_rays_density: 1.0,
            god_rays_weight: 0.6,
            god_rays_decay: 1.0,
            god_rays_exposure: 0.7,
            flare_enabled: 0,
            flare_type: 0,
            flare_intensity: 1.0,
            flare_scale: 1.0,
            flare_tint_r: 1.0,
            flare_tint_g: 1.0,
            flare_tint_b: 1.0,
            ies_profile_index: -1,
            light_function_index: -1,
            ies_angle_scale: 1.0,
            ies_angle_offset: 0.0,
        }
    }
}

/// The exact globals layout consumed by Helio's forward-lit pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuForwardLitGlobals {
    pub frame: u32,
    pub delta_time: f32,
    pub light_count: u32,
    pub ambient_intensity: f32,
    pub ambient_color: [f32; 4],
    pub num_tiles_x: u32,
    pub num_tiles_y: u32,
    pub screen_width: f32,
    pub screen_height: f32,
}

impl GpuForwardLitGlobals {
    pub const BYTE_LEN: usize = 48;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_u32(&mut bytes, 0, self.frame);
        write_f32(&mut bytes, 4, self.delta_time);
        write_u32(&mut bytes, 8, self.light_count);
        write_f32(&mut bytes, 12, self.ambient_intensity);
        write_f32s(&mut bytes, 16, &self.ambient_color);
        write_u32(&mut bytes, 32, self.num_tiles_x);
        write_u32(&mut bytes, 36, self.num_tiles_y);
        write_f32(&mut bytes, 40, self.screen_width);
        write_f32(&mut bytes, 44, self.screen_height);
        bytes
    }
}

const _: () = {
    assert!(core::mem::size_of::<GpuInstanceData>() == GpuInstanceData::BYTE_LEN);
    assert!(core::mem::offset_of!(GpuInstanceData, model) == 0);
    assert!(core::mem::offset_of!(GpuInstanceData, normal_mat) == 64);
    assert!(core::mem::offset_of!(GpuInstanceData, bounds) == 112);
    assert!(core::mem::offset_of!(GpuInstanceData, prev_model) == 128);
    assert!(core::mem::offset_of!(GpuInstanceData, mesh_id) == 192);
    assert!(core::mem::offset_of!(GpuInstanceData, material_id) == 196);
    assert!(core::mem::offset_of!(GpuInstanceData, flags) == 200);
    assert!(core::mem::offset_of!(GpuInstanceData, lightmap_index) == 204);

    assert!(core::mem::size_of::<GpuRetainedTransformSeed>() == GpuRetainedTransformSeed::BYTE_LEN);
    assert!(core::mem::align_of::<GpuRetainedTransformSeed>() == 4);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, translation) == 0);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, scale) == 12);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, rotation) == 24);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, local_radius) == 40);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, previous_translation) == 44);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, draw_group) == 56);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, flags) == 60);

    assert!(core::mem::size_of::<GpuRetainedDrawTemplate>() == GpuRetainedDrawTemplate::BYTE_LEN);
    assert!(core::mem::align_of::<GpuRetainedDrawTemplate>() == 4);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, index_count) == 0);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, first_index) == 4);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, base_vertex) == 8);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, first_instance) == 12);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, capacity) == 16);
    assert!(core::mem::offset_of!(GpuRetainedDrawTemplate, packed_mesh_material) == 20);

    assert!(core::mem::size_of::<GpuCameraUniforms>() == GpuCameraUniforms::BYTE_LEN);
    assert!(core::mem::offset_of!(GpuCameraUniforms, view) == 0);
    assert!(core::mem::offset_of!(GpuCameraUniforms, proj) == 64);
    assert!(core::mem::offset_of!(GpuCameraUniforms, view_proj) == 128);
    assert!(core::mem::offset_of!(GpuCameraUniforms, inv_view_proj) == 192);
    assert!(core::mem::offset_of!(GpuCameraUniforms, position_near) == 256);
    assert!(core::mem::offset_of!(GpuCameraUniforms, forward_far) == 272);
    assert!(core::mem::offset_of!(GpuCameraUniforms, jitter_frame) == 288);
    assert!(core::mem::offset_of!(GpuCameraUniforms, prev_view_proj) == 304);

    assert!(core::mem::size_of::<GpuLight>() == GpuLight::BYTE_LEN);
    assert!(core::mem::offset_of!(GpuLight, shadow_index) == 48);
    assert!(core::mem::offset_of!(GpuLight, god_rays_enabled) == 64);
    assert!(core::mem::offset_of!(GpuLight, flare_enabled) == 84);
    assert!(core::mem::offset_of!(GpuLight, ies_profile_index) == 112);

    assert!(core::mem::size_of::<GpuMaterial>() == GpuMaterial::BYTE_LEN);
    assert!(core::mem::offset_of!(GpuMaterial, tex_base_color) == 48);
    assert!(core::mem::offset_of!(GpuMaterial, class_params) == 80);
    assert!(core::mem::size_of::<GpuForwardLitGlobals>() == GpuForwardLitGlobals::BYTE_LEN);
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirtyRange {
    pub first: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshDescriptor {
    pub mesh_id: u32,
    pub half_extents: [f32; 3],
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawGroupDescriptor {
    pub mesh_id: u32,
    pub material_id: u32,
}

pub struct InstanceFrame<'a> {
    pub camera: &'a GpuCameraUniforms,
    pub globals: &'a GpuForwardLitGlobals,
    pub lights: &'a [GpuLight; LIGHT_COUNT],
    pub materials: &'a [GpuMaterial; MATERIAL_COUNT],
    pub meshes: &'a [MeshDescriptor; SHAPE_COUNT],
    pub groups: &'a [DrawGroupDescriptor; DRAW_GROUP_COUNT],
    pub instances: &'a [GpuInstanceData],
    pub compacted_indices: &'a [u32],
    pub draws: &'a [DrawIndexedIndirectArgs; DRAW_GROUP_COUNT],
    pub instance_dirty: DirtyRange,
    pub compacted_indices_dirty: DirtyRange,
}

/// GPU-transform input for one retained Helio frame.
///
/// Seeds stay in simulation order. Each draw template owns a disjoint,
/// prefix-counted output slice; the GPU pass expands and compacts seeds into
/// those slices before the existing indexed-indirect Render submission.
pub struct TransformFrame<'a> {
    pub camera: &'a GpuCameraUniforms,
    pub globals: &'a GpuForwardLitGlobals,
    pub lights: &'a [GpuLight; LIGHT_COUNT],
    pub materials: &'a [GpuMaterial; MATERIAL_COUNT],
    pub meshes: &'a [MeshDescriptor; SHAPE_COUNT],
    pub groups: &'a [DrawGroupDescriptor; DRAW_GROUP_COUNT],
    pub seeds: &'a [GpuRetainedTransformSeed],
    pub draw_templates: &'a [GpuRetainedDrawTemplate; DRAW_GROUP_COUNT],
    pub seed_dirty: DirtyRange,
    pub draw_templates_dirty: DirtyRange,
    /// Levelized retained affine graph consumed by Helio's three GPU passes:
    /// dynamic local authoring, dirty world resolution, then row emission.
    pub hierarchy: TransformHierarchyFrame<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PointLight {
    position: [f32; 3],
    range: f32,
    color: [f32; 3],
    intensity: f32,
}

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
    material_linear_rgba: [[f32; 4]; MATERIAL_COUNT],
    material_surface: [[f32; 2]; MATERIAL_COUNT],
    ambient_rgb: [f32; 3],
    ambient_intensity: f32,
    lights: [PointLight; LIGHT_COUNT],
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
    retained_transform_template: RetainedTransformTemplate,
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let retained_transform_template = RetainedTransformTemplate::decode(&artifact)?;
        let bytes = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingChurnScene)?
            .data;
        let light_bytes = artifact
            .section(LIGHT_SECTION_NAME)
            .ok_or(Error::MissingChurnLighting)?
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
            || max_objects > MAX_RETAINED_TRANSFORM_ROWS
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
        let mut material_linear_rgba = [[0.0; 4]; MATERIAL_COUNT];
        for (index, (rgba, linear)) in material_rgba
            .iter_mut()
            .zip(material_linear_rgba.iter_mut())
            .enumerate()
        {
            *linear = read_f32s(bytes, 128 + index * 16)?;
            *rgba = linear_rgba_to_srgba8(*linear)?;
        }
        let mut shape_half_extents = [[0.0; 3]; SHAPE_COUNT];
        for (index, extents) in shape_half_extents.iter_mut().enumerate() {
            *extents = read_f32s(bytes, 192 + index * 12)?;
            if extents.iter().any(|value| *value <= 0.0) {
                return Err(Error::InvalidChurnScene);
            }
        }

        let (ambient_rgb, ambient_intensity, lights, material_surface) =
            decode_lighting(light_bytes)?;
        if ambient_rgb != read_f32s::<3>(bytes, 96)? {
            return Err(Error::InvalidChurnLighting);
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
            material_linear_rgba,
            material_surface,
            ambient_rgb,
            ambient_intensity,
            lights,
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
            retained_transform_template,
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

fn decode_lighting(
    bytes: &[u8],
) -> Result<([f32; 3], f32, [PointLight; LIGHT_COUNT], [[f32; 2]; MATERIAL_COUNT]), Error> {
    if bytes.len() != LIGHT_ENCODED_LEN
        || bytes.get(..8) != Some(LIGHT_MAGIC.as_slice())
        || read_u16(bytes, 8).map_err(|_| Error::InvalidChurnLighting)? != VERSION
        || usize::from(read_u16(bytes, 10).map_err(|_| Error::InvalidChurnLighting)?)
            != LIGHT_ENCODED_LEN
        || usize::try_from(read_u32(bytes, 12).map_err(|_| Error::InvalidChurnLighting)?)
            .map_err(|_| Error::InvalidChurnLighting)?
            != LIGHT_ENCODED_LEN
        || usize::try_from(read_u32(bytes, 16).map_err(|_| Error::InvalidChurnLighting)?)
            .map_err(|_| Error::InvalidChurnLighting)?
            != LIGHT_COUNT
        || usize::try_from(read_u32(bytes, 20).map_err(|_| Error::InvalidChurnLighting)?)
            .map_err(|_| Error::InvalidChurnLighting)?
            != MATERIAL_COUNT
        || bytes[136..].iter().any(|byte| *byte != 0)
    {
        return Err(Error::InvalidChurnLighting);
    }

    let ambient_rgb = read_f32s(bytes, 24).map_err(|_| Error::InvalidChurnLighting)?;
    let ambient_intensity = read_f32(bytes, 36).map_err(|_| Error::InvalidChurnLighting)?;
    let mut lights = [PointLight {
        position: [0.0; 3],
        range: 0.0,
        color: [0.0; 3],
        intensity: 0.0,
    }; LIGHT_COUNT];
    for (index, light) in lights.iter_mut().enumerate() {
        let offset = 40 + index * 32;
        *light = PointLight {
            position: read_f32s(bytes, offset).map_err(|_| Error::InvalidChurnLighting)?,
            range: read_f32(bytes, offset + 12).map_err(|_| Error::InvalidChurnLighting)?,
            color: read_f32s(bytes, offset + 16).map_err(|_| Error::InvalidChurnLighting)?,
            intensity: read_f32(bytes, offset + 28).map_err(|_| Error::InvalidChurnLighting)?,
        };
    }
    let mut material_surface = [[0.0; 2]; MATERIAL_COUNT];
    for (index, surface) in material_surface.iter_mut().enumerate() {
        *surface = read_f32s(bytes, 104 + index * 8).map_err(|_| Error::InvalidChurnLighting)?;
    }
    if ambient_intensity < 0.0
        || ambient_rgb.iter().any(|value| *value < 0.0)
        || lights.iter().any(|light| {
            light.range <= 0.0
                || light.intensity < 0.0
                || light.color.iter().any(|value| *value < 0.0)
        })
        || material_surface
            .iter()
            .flatten()
            .any(|value| !(0.0..=1.0).contains(value))
    {
        return Err(Error::InvalidChurnLighting);
    }
    Ok((ambient_rgb, ambient_intensity, lights, material_surface))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub rgba: [u8; 4],
}

impl Batch {
    /// The command record Helio owns for this compatibility material/face
    /// batch. The native path uses the same WGPU record layout for retained
    /// meshes and dense instance ranges; only this fallback expands geometry.
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
    previous_model: Option<[f32; 16]>,
    previous_translation: Option<[f32; 3]>,
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
    collisions_enabled: bool,
    collision_burst_frame: u32,
    gpu_camera: GpuCameraUniforms,
    gpu_globals: GpuForwardLitGlobals,
    gpu_lights: [GpuLight; LIGHT_COUNT],
    gpu_materials: [GpuMaterial; MATERIAL_COUNT],
    gpu_meshes: [MeshDescriptor; SHAPE_COUNT],
    gpu_groups: [DrawGroupDescriptor; DRAW_GROUP_COUNT],
    gpu_instances: Vec<GpuInstanceData>,
    gpu_compacted_indices: Vec<u32>,
    gpu_draws: [DrawIndexedIndirectArgs; DRAW_GROUP_COUNT],
    gpu_transform_seeds: Vec<GpuRetainedTransformSeed>,
    gpu_draw_templates: [GpuRetainedDrawTemplate; DRAW_GROUP_COUNT],
    gpu_transform_hierarchy: RetainedTransformProgram,
    previous_view_proj: Option<[f32; 16]>,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let retained_transform_template = spec.retained_transform_template;
        let slots_per_batch = spec.max_objects.div_ceil(MATERIAL_COUNT);
        let mut batches = Vec::with_capacity(BATCH_COUNT);
        for material in 0..MATERIAL_COUNT {
            for face in 0..FACE_COUNT {
                let vertex_count = slots_per_batch
                    .checked_mul(VERTICES_PER_FACE)
                    .ok_or(Error::InvalidChurnScene)?;
                let index_count = slots_per_batch
                    .checked_mul(INDICES_PER_FACE)
                    .ok_or(Error::InvalidChurnScene)?;
                let mut indices = Vec::with_capacity(index_count);
                for slot in 0..slots_per_batch {
                    append_face_indices(&mut indices, slot * VERTICES_PER_FACE)?;
                }
                batches.push(Batch {
                    vertices: vec![HIDDEN; vertex_count],
                    indices,
                    rgba: initial_face_rgba(&spec, material, face)?,
                });
            }
        }
        let gpu_meshes = core::array::from_fn(|shape| MeshDescriptor {
            mesh_id: shape as u32,
            half_extents: spec.shape_half_extents[shape],
            first_vertex: (shape * VERTICES_PER_OBJECT) as u32,
            vertex_count: VERTICES_PER_OBJECT as u32,
            first_index: (shape * INDICES_PER_OBJECT) as u32,
            index_count: INDICES_PER_OBJECT as u32,
            base_vertex: (shape * VERTICES_PER_OBJECT) as i32,
        });
        let gpu_groups = core::array::from_fn(|group| DrawGroupDescriptor {
            mesh_id: (group / MATERIAL_COUNT) as u32,
            material_id: (group % MATERIAL_COUNT) as u32,
        });
        let gpu_draws = core::array::from_fn(|group| {
            let mesh = gpu_meshes[group / MATERIAL_COUNT];
            DrawIndexedIndirectArgs {
                index_count: mesh.index_count,
                instance_count: 0,
                first_index: mesh.first_index,
                base_vertex: mesh.base_vertex,
                first_instance: 0,
            }
        });
        let gpu_materials = core::array::from_fn(|material| {
            let [roughness, metallic] = spec.material_surface[material];
            GpuMaterial {
                base_color: spec.material_linear_rgba[material],
                emissive: [0.0; 4],
                roughness_metallic: [roughness, metallic, 1.5, 0.5],
                tex_base_color: GpuMaterial::NO_TEXTURE,
                tex_normal: GpuMaterial::NO_TEXTURE,
                tex_roughness: GpuMaterial::NO_TEXTURE,
                tex_emissive: GpuMaterial::NO_TEXTURE,
                tex_occlusion: GpuMaterial::NO_TEXTURE,
                workflow: 0,
                flags: 0,
                material_class: 0,
                class_params: [0.0; 4],
            }
        });
        let gpu_lights = core::array::from_fn(|index| {
            let source = spec.lights[index];
            GpuLight {
                position_range: [
                    source.position[0],
                    source.position[1],
                    source.position[2],
                    source.range,
                ],
                color_intensity: [
                    source.color[0],
                    source.color[1],
                    source.color[2],
                    source.intensity,
                ],
                ..GpuLight::default()
            }
        });
        let max_objects = spec.max_objects;
        Ok(Self {
            rng: Rng::new(spec.seed),
            spec,
            objects: Vec::new(),
            batches,
            frame: 0,
            recycle: 0,
            collisions_enabled: false,
            collision_burst_frame: 0,
            gpu_camera: GpuCameraUniforms::default(),
            gpu_globals: GpuForwardLitGlobals::default(),
            gpu_lights,
            gpu_materials,
            gpu_meshes,
            gpu_groups,
            gpu_instances: Vec::with_capacity(max_objects),
            gpu_compacted_indices: Vec::with_capacity(max_objects),
            gpu_draws,
            gpu_transform_seeds: Vec::with_capacity(max_objects),
            gpu_draw_templates: [GpuRetainedDrawTemplate::default(); DRAW_GROUP_COUNT],
            gpu_transform_hierarchy: RetainedTransformProgram::from_template(
                retained_transform_template,
                0,
            )
            .map_err(|_| Error::InvalidChurnScene)?,
            previous_view_proj: None,
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

    pub fn collisions_enabled(&self) -> bool {
        self.collisions_enabled
    }

    /// Match hosted Helio's `C` control without importing a heavyweight
    /// physics runtime into this fixed-allocation port. Dense orbit objects get
    /// a deterministic, bounded three-dimensional separation impulse; toggling
    /// it off returns exactly to the procedural orbit on the next frame.
    pub fn toggle_collisions(&mut self) -> bool {
        self.collisions_enabled = !self.collisions_enabled;
        self.collision_burst_frame = 0;
        self.collisions_enabled
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
        let mut light_sums = [[0.0f32; 3]; BATCH_COUNT];
        let mut light_counts = [0u32; BATCH_COUNT];
        // Speed up only the visual clock. Spawn cadence and retained geometry
        // remain driven by their baked frame/count contracts.
        let time = self.frame as f32 * self.spec.time_step * ANIMATION_RATE_SCALE;
        let burst_progress = if self.collisions_enabled {
            let frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
            frame as f32 / COLLISION_BURST_FRAMES as f32
        } else {
            0.0
        };
        for (object_index, object) in self.objects.iter().copied().enumerate() {
            let pose = object_pose(
                &self.spec,
                object,
                object_index,
                time,
                self.collisions_enabled,
                burst_progress,
            );
            let center = pose.center;
            let (sin_angle, cos_angle) = (libm::sinf(pose.angle), libm::cosf(pose.angle));
            let material_index = object_index % MATERIAL_COUNT;
            let slot = object_index / MATERIAL_COUNT;
            let local = box_vertices(self.spec.shape_half_extents[object.shape]);
            for face in 0..FACE_COUNT {
                let batch_index = material_index * FACE_COUNT + face;
                let start = slot * VERTICES_PER_FACE;
                let mut world_face = [[0.0f32; 3]; VERTICES_PER_FACE];
                let mut projected_face = [HIDDEN; VERTICES_PER_FACE];
                let mut visible = true;
                for (face_offset, world) in world_face.iter_mut().enumerate() {
                    let point = local[face * VERTICES_PER_FACE + face_offset];
                    let scaled = [
                        point[0] * object.scale,
                        point[1] * object.scale,
                        point[2] * object.scale,
                    ];
                    *world = [
                        center[0] + scaled[0] * cos_angle + scaled[2] * sin_angle,
                        center[1] + scaled[1],
                        center[2] - scaled[0] * sin_angle + scaled[2] * cos_angle,
                    ];
                    match projector.project(*world) {
                        Ok(projected) => projected_face[face_offset] = projected,
                        // A burst or a moved fly camera can put an object across
                        // the near/far plane. The fixed resident draw stays valid;
                        // collapse that face outside NDC for this frame instead of
                        // treating ordinary frustum clipping as a scene failure.
                        Err(Error::VertexBehindCamera) => visible = false,
                        Err(error) => return Err(error),
                    }
                }
                if !visible {
                    continue;
                }
                self.batches[batch_index].vertices[start..start + VERTICES_PER_FACE]
                    .copy_from_slice(&projected_face);
                let lit = shade_face(&self.spec, material_index, world_face);
                for channel in 0..3 {
                    light_sums[batch_index][channel] += lit[channel];
                }
                light_counts[batch_index] = light_counts[batch_index].saturating_add(1);
                normalize_face_slot_winding(&mut self.batches[batch_index], slot);
            }
        }
        for batch_index in 0..BATCH_COUNT {
            let count = light_counts[batch_index];
            if count != 0 {
                let inverse = 1.0 / count as f32;
                self.batches[batch_index].rgba = linear_rgba_to_srgba8([
                    light_sums[batch_index][0] * inverse,
                    light_sums[batch_index][1] * inverse,
                    light_sums[batch_index][2] * inverse,
                    1.0,
                ])?;
            }
        }
        if self.collisions_enabled {
            self.collision_burst_frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
        }
        self.frame = self.frame.wrapping_add(1);
        Ok(&self.batches)
    }

    /// Advance Churn into compact retained-transform seeds. The simulation CPU
    /// writes 64 bytes per object plus twelve prefix-counted templates; matrix,
    /// normal, bounds, compaction, and indirect-count expansion stay on GPU.
    pub fn step_transform_frame(&mut self, aspect: f32) -> Result<TransformFrame<'_>, Error> {
        Projector::new(self.spec.camera, aspect)?;
        if self.frame % u64::from(self.spec.spawn_interval_frames) == 0 {
            self.spawn();
        }

        let time = self.frame as f32 * self.spec.time_step * ANIMATION_RATE_SCALE;
        let burst_progress = if self.collisions_enabled {
            let frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
            frame as f32 / COLLISION_BURST_FRAMES as f32
        } else {
            0.0
        };

        let object_count = self.objects.len();
        let object_count_u32 = u32::try_from(object_count).map_err(|_| Error::InvalidChurnScene)?;
        let mut group_counts = [0u32; DRAW_GROUP_COUNT];
        for (object_index, object) in self.objects.iter().enumerate() {
            let group = object.shape * MATERIAL_COUNT + object_index % MATERIAL_COUNT;
            group_counts[group] = group_counts[group]
                .checked_add(1)
                .ok_or(Error::InvalidChurnScene)?;
        }
        let mut next_start = 0u32;
        for group in 0..DRAW_GROUP_COUNT {
            let mesh = self.gpu_meshes[group / MATERIAL_COUNT];
            self.gpu_draw_templates[group] = GpuRetainedDrawTemplate::new(
                mesh,
                self.gpu_groups[group],
                next_start,
                group_counts[group],
            )
            .ok_or(Error::InvalidChurnScene)?;
            next_start = next_start
                .checked_add(group_counts[group])
                .ok_or(Error::InvalidChurnScene)?;
        }
        if next_start != object_count_u32 {
            return Err(Error::InvalidChurnScene);
        }

        self.gpu_transform_seeds.clear();
        let mut group_slots = [0u32; DRAW_GROUP_COUNT];
        for object_index in 0..object_count {
            let object = self.objects[object_index];
            let pose = object_pose(
                &self.spec,
                object,
                object_index,
                time,
                self.collisions_enabled,
                burst_progress,
            );
            let half_angle = pose.angle * 0.5;
            let previous_translation = object
                .previous_translation
                .or_else(|| {
                    object
                        .previous_model
                        .map(|model| [model[12], model[13], model[14]])
                })
                .unwrap_or(pose.center);
            let extents = self.spec.shape_half_extents[object.shape];
            let draw_group = object.shape * MATERIAL_COUNT + object_index % MATERIAL_COUNT;
            let compact_slot = group_slots[draw_group];
            group_slots[draw_group] = compact_slot
                .checked_add(1)
                .ok_or(Error::InvalidChurnScene)?;
            self.gpu_transform_seeds.push(GpuRetainedTransformSeed {
                translation: pose.center,
                scale: [object.scale; 3],
                rotation: [0.0, libm::sinf(half_angle), 0.0, libm::cosf(half_angle)],
                local_radius: libm::sqrtf(
                    extents[0] * extents[0] + extents[1] * extents[1] + extents[2] * extents[2],
                ),
                previous_translation,
                draw_group: draw_group as u32,
                flags: GpuRetainedTransformSeed::pack_slot_and_flags(
                    compact_slot,
                    INSTANCE_FLAG_CASTS_SHADOW | INSTANCE_FLAG_RECEIVES_SHADOW,
                )
                .ok_or(Error::InvalidChurnScene)?,
            });
            self.objects[object_index].previous_translation = Some(pose.center);
            // A later CPU fallback reconstructs its temporal model from the
            // compact translation instead of consuming a stale matrix.
            self.objects[object_index].previous_model = None;
        }
        if group_slots != group_counts {
            return Err(Error::InvalidChurnScene);
        }

        // The graph is persistent while topology is stable. A growing Churn
        // scene recompiles once with generation 1 and publishes those initial
        // worklists directly; this avoids bumping a never-emitted row to
        // generation 2. Ordinary frames only mark their changing TRS rows.
        if self.gpu_transform_hierarchy.authored_leaf_nodes.len() != object_count + 1 {
            self.gpu_transform_hierarchy = RetainedTransformProgram::from_template(
                self.spec.retained_transform_template,
                object_count,
            )
            .map_err(|_| Error::InvalidChurnScene)?;
        } else {
            self.gpu_transform_hierarchy.begin_update();
            for row in 0..object_count_u32 {
                if !self
                    .gpu_transform_hierarchy
                    .mark_dynamic_slot_dirty(row)
                    .map_err(|_| Error::InvalidChurnScene)?
                {
                    return Err(Error::InvalidChurnScene);
                }
            }
            self.gpu_transform_hierarchy.propagate_dirty();
        }
        let camera = gpu_camera_uniforms(
            self.spec.camera,
            aspect,
            self.frame as u32,
            self.previous_view_proj,
        )?;
        self.previous_view_proj = Some(camera.view_proj);
        self.gpu_camera = camera;
        self.gpu_globals = GpuForwardLitGlobals {
            frame: self.frame as u32,
            delta_time: self.spec.time_step * ANIMATION_RATE_SCALE,
            light_count: LIGHT_COUNT as u32,
            ambient_intensity: self.spec.ambient_intensity,
            ambient_color: [
                self.spec.ambient_rgb[0],
                self.spec.ambient_rgb[1],
                self.spec.ambient_rgb[2],
                1.0,
            ],
            num_tiles_x: 1,
            num_tiles_y: 1,
            screen_width: aspect,
            screen_height: 1.0,
        };

        if self.collisions_enabled {
            self.collision_burst_frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
        }
        self.frame = self.frame.wrapping_add(1);
        Ok(TransformFrame {
            camera: &self.gpu_camera,
            globals: &self.gpu_globals,
            lights: &self.gpu_lights,
            materials: &self.gpu_materials,
            meshes: &self.gpu_meshes,
            groups: &self.gpu_groups,
            seeds: &self.gpu_transform_seeds,
            draw_templates: &self.gpu_draw_templates,
            seed_dirty: DirtyRange {
                first: 0,
                count: object_count_u32,
            },
            draw_templates_dirty: DirtyRange {
                first: 0,
                count: DRAW_GROUP_COUNT as u32,
            },
            hierarchy: TransformHierarchyFrame {
                nodes: &self.gpu_transform_hierarchy.nodes,
                local_affines: &self.gpu_transform_hierarchy.local_affines,
                dynamic_bindings: &self.gpu_transform_hierarchy.dynamic_bindings,
                level_indices: &self.gpu_transform_hierarchy.level_indices,
                levels: &self.gpu_transform_hierarchy.levels,
                dirty_local_nodes: &self.gpu_transform_hierarchy.dirty_local_node_ids,
                dirty_world_nodes: &self.gpu_transform_hierarchy.dirty_world_node_ids,
                dirty_rows: &self.gpu_transform_hierarchy.dirty_row_ids,
                row_leaf_nodes: &self.gpu_transform_hierarchy.row_leaf_nodes,
                report: self.gpu_transform_hierarchy.report(),
            },
        })
    }

    /// Advance the same Churn simulation while retaining Helio's GPU-native
    /// scene shape. This path updates only object matrices and compact draw
    /// ranges; it deliberately performs no CPU vertex projection or lighting.
    pub fn step_instances(&mut self, aspect: f32) -> Result<InstanceFrame<'_>, Error> {
        // Keep camera validation identical to the compatibility path, without
        // using Projector::project for any geometry.
        Projector::new(self.spec.camera, aspect)?;
        if self.frame % u64::from(self.spec.spawn_interval_frames) == 0 {
            self.spawn();
        }

        let time = self.frame as f32 * self.spec.time_step * ANIMATION_RATE_SCALE;
        let burst_progress = if self.collisions_enabled {
            let frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
            frame as f32 / COLLISION_BURST_FRAMES as f32
        } else {
            0.0
        };

        let object_count = self.objects.len();
        let object_count_u32 = u32::try_from(object_count).map_err(|_| Error::InvalidChurnScene)?;
        let mut group_counts = [0usize; DRAW_GROUP_COUNT];
        for (object_index, object) in self.objects.iter().enumerate() {
            let group = object.shape * MATERIAL_COUNT + object_index % MATERIAL_COUNT;
            group_counts[group] += 1;
        }
        let mut group_starts = [0usize; DRAW_GROUP_COUNT];
        let mut next_start = 0usize;
        for group in 0..DRAW_GROUP_COUNT {
            group_starts[group] = next_start;
            next_start += group_counts[group];
            let mesh = self.gpu_meshes[group / MATERIAL_COUNT];
            self.gpu_draws[group] = DrawIndexedIndirectArgs {
                index_count: mesh.index_count,
                instance_count: group_counts[group] as u32,
                first_index: mesh.first_index,
                base_vertex: mesh.base_vertex,
                first_instance: group_starts[group] as u32,
            };
        }

        self.gpu_instances
            .resize(object_count, GpuInstanceData::default());
        self.gpu_compacted_indices.resize(object_count, 0);
        let mut group_cursors = group_starts;
        for object_index in 0..object_count {
            let object = self.objects[object_index];
            let pose = object_pose(
                &self.spec,
                object,
                object_index,
                time,
                self.collisions_enabled,
                burst_progress,
            );
            let model = model_matrix(pose.center, pose.angle, object.scale);
            let previous_model = object.previous_model.unwrap_or_else(|| {
                model_matrix(
                    object.previous_translation.unwrap_or(pose.center),
                    pose.angle,
                    object.scale,
                )
            });
            let group = object.shape * MATERIAL_COUNT + object_index % MATERIAL_COUNT;
            let packed_index = group_cursors[group];
            group_cursors[group] += 1;
            let extents = self.spec.shape_half_extents[object.shape];
            let radius = libm::sqrtf(
                extents[0] * extents[0] + extents[1] * extents[1] + extents[2] * extents[2],
            ) * object.scale;
            self.gpu_instances[packed_index] = GpuInstanceData {
                model,
                normal_mat: normal_matrix_y(pose.angle, object.scale),
                bounds: [pose.center[0], pose.center[1], pose.center[2], radius],
                prev_model: previous_model,
                mesh_id: object.shape as u32,
                material_id: (object_index % MATERIAL_COUNT) as u32,
                flags: INSTANCE_FLAG_CASTS_SHADOW | INSTANCE_FLAG_RECEIVES_SHADOW,
                lightmap_index: u32::MAX,
            };
            self.gpu_compacted_indices[packed_index] = packed_index as u32;
            self.objects[object_index].previous_model = Some(model);
            self.objects[object_index].previous_translation = Some(pose.center);
        }

        let camera = gpu_camera_uniforms(
            self.spec.camera,
            aspect,
            self.frame as u32,
            self.previous_view_proj,
        )?;
        self.previous_view_proj = Some(camera.view_proj);
        self.gpu_camera = camera;
        self.gpu_globals = GpuForwardLitGlobals {
            frame: self.frame as u32,
            delta_time: self.spec.time_step * ANIMATION_RATE_SCALE,
            light_count: LIGHT_COUNT as u32,
            ambient_intensity: self.spec.ambient_intensity,
            ambient_color: [
                self.spec.ambient_rgb[0],
                self.spec.ambient_rgb[1],
                self.spec.ambient_rgb[2],
                1.0,
            ],
            num_tiles_x: 1,
            num_tiles_y: 1,
            screen_width: aspect,
            screen_height: 1.0,
        };

        if self.collisions_enabled {
            self.collision_burst_frame = self
                .collision_burst_frame
                .saturating_add(1)
                .min(COLLISION_BURST_FRAMES);
        }
        self.frame = self.frame.wrapping_add(1);
        let dirty = DirtyRange {
            first: 0,
            count: object_count_u32,
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
                previous_model: None,
                previous_translation: None,
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

#[derive(Clone, Copy)]
struct ObjectPose {
    center: [f32; 3],
    angle: f32,
}

fn object_pose(
    spec: &Spec,
    object: Object,
    object_index: usize,
    time: f32,
    collisions_enabled: bool,
    burst_progress: f32,
) -> ObjectPose {
    let phase = object.seed + time * object.speed;
    let radius = spec.orbit_radius
        + libm::sinf(phase * spec.radius_phase_scale) * spec.orbit_radius_amplitude;
    let mut center = [
        libm::cosf(phase) * radius,
        spec.height_base + libm::sinf(phase * spec.height_phase_scale) * spec.height_amplitude,
        libm::sinf(phase) * radius,
    ];
    let mut angle = phase * spec.rotation_scale;
    if collisions_enabled {
        let direction = collision_burst_direction(object.seed, object_index);
        let eased = smoothstep01(burst_progress);
        for axis in 0..3 {
            center[axis] += direction[axis] * COLLISION_BURST_DISTANCE * eased;
        }
        angle += direction[1] * core::f32::consts::TAU * eased;
    }
    ObjectPose { center, angle }
}

fn model_matrix(center: [f32; 3], angle: f32, scale: f32) -> [f32; 16] {
    let sin = libm::sinf(angle);
    let cos = libm::cosf(angle);
    [
        cos * scale,
        0.0,
        -sin * scale,
        0.0,
        0.0,
        scale,
        0.0,
        0.0,
        sin * scale,
        0.0,
        cos * scale,
        0.0,
        center[0],
        center[1],
        center[2],
        1.0,
    ]
}

fn normal_matrix_y(angle: f32, scale: f32) -> [f32; 12] {
    let inverse_scale = 1.0 / scale;
    let sin = libm::sinf(angle);
    let cos = libm::cosf(angle);
    [
        cos * inverse_scale,
        0.0,
        -sin * inverse_scale,
        0.0,
        0.0,
        inverse_scale,
        0.0,
        0.0,
        sin * inverse_scale,
        0.0,
        cos * inverse_scale,
        0.0,
    ]
}

pub(crate) fn gpu_camera_uniforms(
    camera: Camera,
    aspect: f32,
    frame: u32,
    previous_view_proj: Option<[f32; 16]>,
) -> Result<GpuCameraUniforms, Error> {
    let forward = normalize3(subtract3(camera.target, camera.position));
    let right = normalize3(cross3(forward, camera.up));
    let up = normalize3(cross3(right, forward));
    let view = [
        right[0],
        up[0],
        -forward[0],
        0.0,
        right[1],
        up[1],
        -forward[1],
        0.0,
        right[2],
        up[2],
        -forward[2],
        0.0,
        -dot3(right, camera.position),
        -dot3(up, camera.position),
        dot3(forward, camera.position),
        1.0,
    ];
    let f = 1.0 / libm::tanf(camera.vertical_fov_radians * 0.5);
    let depth = camera.far / (camera.near - camera.far);
    let proj = [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        depth,
        -1.0,
        0.0,
        0.0,
        camera.near * depth,
        0.0,
    ];
    let view_proj = matrix4_mul(proj, view);
    let inv_view_proj = matrix4_inverse(view_proj).ok_or(Error::InvalidCamera)?;
    Ok(GpuCameraUniforms {
        view,
        proj,
        view_proj,
        inv_view_proj,
        position_near: [
            camera.position[0],
            camera.position[1],
            camera.position[2],
            camera.near,
        ],
        forward_far: [forward[0], forward[1], forward[2], camera.far],
        jitter_frame: [0.0, 0.0, frame as f32, 0.0],
        prev_view_proj: previous_view_proj.unwrap_or(view_proj),
    })
}

fn matrix4_mul(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut product = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            for inner in 0..4 {
                product[column * 4 + row] += left[inner * 4 + row] * right[column * 4 + inner];
            }
        }
    }
    product
}

fn matrix4_inverse(matrix: [f32; 16]) -> Option<[f32; 16]> {
    let mut augmented = [[0.0f32; 8]; 4];
    for row in 0..4 {
        for column in 0..4 {
            augmented[row][column] = matrix[column * 4 + row];
        }
        augmented[row][row + 4] = 1.0;
    }
    for column in 0..4 {
        let mut pivot = column;
        for row in column + 1..4 {
            if augmented[row][column].abs() > augmented[pivot][column].abs() {
                pivot = row;
            }
        }
        if !augmented[pivot][column].is_finite() || augmented[pivot][column].abs() <= 1.0e-12 {
            return None;
        }
        augmented.swap(column, pivot);
        let divisor = augmented[column][column];
        for value in &mut augmented[column] {
            *value /= divisor;
        }
        for row in 0..4 {
            if row == column {
                continue;
            }
            let factor = augmented[row][column];
            for entry in 0..8 {
                augmented[row][entry] -= factor * augmented[column][entry];
            }
        }
    }
    let mut inverse = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            inverse[column * 4 + row] = augmented[row][column + 4];
        }
    }
    inverse
        .iter()
        .all(|value| value.is_finite())
        .then_some(inverse)
}

fn initial_face_rgba(spec: &Spec, material: usize, face: usize) -> Result<[u8; 4], Error> {
    const NORMAL_Y: [f32; FACE_COUNT] = [0.0, 0.0, 0.0, 0.0, 1.0, -1.0];
    let sky_mix = NORMAL_Y[face] * 0.5 + 0.5;
    let base = spec.material_linear_rgba[material];
    let mut lit = [0.0; 4];
    for channel in 0..3 {
        let sky = spec.ambient_rgb[channel] * spec.ambient_intensity;
        let ambient = sky * (0.15 + 0.85 * sky_mix);
        lit[channel] = (base[channel] * ambient).clamp(0.0, 1.0);
    }
    lit[3] = base[3];
    linear_rgba_to_srgba8(lit)
}

fn shade_face(spec: &Spec, material: usize, world: [[f32; 3]; VERTICES_PER_FACE]) -> [f32; 3] {
    let mut center = [0.0f32; 3];
    for point in world {
        for axis in 0..3 {
            center[axis] += point[axis] * 0.25;
        }
    }
    let edge_a = subtract3(world[1], world[0]);
    let edge_b = subtract3(world[2], world[0]);
    let normal = normalize3(cross3(edge_a, edge_b));
    let base = spec.material_linear_rgba[material];
    let [roughness, metallic] = spec.material_surface[material];
    let sky_mix = normal[1] * 0.5 + 0.5;
    let mut lit = [0.0f32; 3];
    for channel in 0..3 {
        let sky = spec.ambient_rgb[channel] * spec.ambient_intensity;
        lit[channel] = base[channel] * sky * (0.15 + 0.85 * sky_mix);
    }

    let view = normalize3(subtract3(spec.camera.position, center));
    for light in spec.lights {
        let to_light = subtract3(light.position, center);
        let distance_squared = dot3(to_light, to_light);
        if distance_squared <= f32::EPSILON || distance_squared > light.range * light.range {
            continue;
        }
        let distance = libm::sqrtf(distance_squared);
        let light_direction = scale3(to_light, 1.0 / distance);
        let normal_dot_light = dot3(normal, light_direction).max(0.0);
        if normal_dot_light <= 0.0 {
            continue;
        }
        let normalized_distance = distance / light.range;
        let attenuation = (1.0 / (distance_squared + 0.0001))
            * (1.0
                - normalized_distance
                    * normalized_distance
                    * normalized_distance
                    * normalized_distance)
                .max(0.0);
        let half_vector = normalize3(add3(view, light_direction));
        let normal_dot_half = dot3(normal, half_vector).max(0.0);
        let shininess = (2.0 / (roughness * roughness).max(0.01) - 2.0).clamp(2.0, 128.0);
        let specular =
            libm::powf(normal_dot_half, shininess) * normal_dot_light * (1.0 - roughness * 0.5);
        for channel in 0..3 {
            let radiance =
                light.color[channel] * light.intensity * attenuation * FLAT_LIGHT_RESPONSE_SCALE;
            let f0 = 0.04 * (1.0 - metallic) + base[channel] * metallic;
            lit[channel] +=
                radiance * (base[channel] * (1.0 - metallic) * normal_dot_light + f0 * specular);
        }
    }
    for channel in &mut lit {
        *channel = channel.clamp(0.0, 1.0);
    }
    lit
}

fn collision_burst_direction(seed: f32, object_index: usize) -> [f32; 3] {
    let index = object_index as f32;
    let azimuth = seed * 1.618_034 + index * 2.399_963;
    let vertical = (0.15 + libm::sinf(seed * 2.173 + index * 0.754_877) * 0.65).clamp(-0.75, 0.85);
    let horizontal = libm::sqrtf((1.0 - vertical * vertical).max(0.0));
    [
        libm::cosf(azimuth) * horizontal,
        vertical,
        libm::sinf(azimuth) * horizontal,
    ]
}

fn smoothstep01(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn subtract3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length_squared = dot3(value, value);
    if length_squared <= f32::EPSILON {
        [0.0, 1.0, 0.0]
    } else {
        scale3(value, 1.0 / libm::sqrtf(length_squared))
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

fn append_face_indices(indices: &mut Vec<u32>, vertex_start: usize) -> Result<(), Error> {
    let base = u32::try_from(vertex_start).map_err(|_| Error::InvalidChurnScene)?;
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    Ok(())
}

fn normalize_face_slot_winding(batch: &mut Batch, slot: usize) {
    let first = slot * INDICES_PER_FACE;
    for triangle in (first..first + INDICES_PER_FACE).step_by(3) {
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

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_f32s(bytes: &mut [u8], offset: usize, values: &[f32]) {
    for (index, value) in values.iter().enumerate() {
        write_f32(bytes, offset + index * 4, *value);
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

    const ARTIFACT: &[u8] = include_bytes!("../../../picasso/simple-cube.trueos.intel.helio");

    #[test]
    fn embedded_artifact_drives_fixed_retained_batches() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        assert_eq!(spec.max_objects, 2_200);
        assert_eq!(spec.spawn_rate, 8);
        assert_eq!(spec.spawn_interval_frames, 2);
        assert_eq!(spec.ambient_rgb, [0.12, 0.12, 0.14]);
        assert_eq!(spec.ambient_intensity, 1.0);
        assert_eq!(spec.lights[0].position, [-20.0, 5.0, -20.0]);
        assert_eq!(spec.lights[1].position, [20.0, 5.0, 20.0]);
        assert_eq!(spec.lights[0].intensity, 7.0);
        assert_eq!(spec.lights[1].range, 40.0);
        assert!((spec.time_step - 0.01).abs() <= f32::EPSILON);
        assert!((spec.time_step * ANIMATION_RATE_SCALE - 0.015).abs() <= f32::EPSILON);

        let mut engine = Engine::new(spec).unwrap();
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 8);
        assert_eq!(engine.batches().len(), 24);
        assert!(engine.batches().iter().all(|batch| {
            batch.vertices.len() == 2_200
                && batch.indices.len() == 3_300
                && batch
                    .vertices
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
        }));
        assert!(engine.batches().iter().all(|batch| {
            batch.draw_indexed_indirect()
                == Ok(DrawIndexedIndirectArgs {
                    index_count: 3_300,
                    instance_count: 1,
                    first_index: 0,
                    base_vertex: 0,
                    first_instance: 0,
                })
        }));
        assert_eq!(
            engine
                .batches()
                .iter()
                .map(|batch| batch.indices.len() / 3)
                .sum::<usize>(),
            26_400,
        );
        assert!(
            engine.batches()[..FACE_COUNT]
                .windows(2)
                .any(|faces| faces[0].rgba != faces[1].rgba)
        );
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 8);
        engine.step(16.0 / 9.0).unwrap();
        assert_eq!(engine.active_objects(), 16);
        let floor = engine.floor(16.0 / 9.0).unwrap();
        assert_eq!(floor.indices.len(), 6);
    }

    #[test]
    fn collision_toggle_bursts_then_returns_exactly_to_orbit() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut burst = Engine::new(spec.clone()).unwrap();
        let mut orbit = Engine::new(spec).unwrap();
        burst.step(16.0 / 9.0).unwrap();
        orbit.step(16.0 / 9.0).unwrap();
        assert_eq!(burst.batches(), orbit.batches());

        assert!(burst.toggle_collisions());
        burst.step(16.0 / 9.0).unwrap();
        orbit.step(16.0 / 9.0).unwrap();
        assert_ne!(burst.batches(), orbit.batches());
        assert!(burst.batches().iter().all(|batch| {
            batch.draw_indexed_indirect() == Ok(DrawIndexedIndirectArgs::new(3_300))
        }));

        assert!(!burst.toggle_collisions());
        burst.step(16.0 / 9.0).unwrap();
        orbit.step(16.0 / 9.0).unwrap();
        assert_eq!(burst.batches(), orbit.batches());
    }

    #[test]
    fn collision_burst_reaching_full_separation_does_not_abort_the_scene() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine.step(16.0 / 9.0).unwrap();
        assert!(engine.toggle_collisions());

        for burst_frame in 0..=COLLISION_BURST_FRAMES {
            engine
                .step(16.0 / 9.0)
                .unwrap_or_else(|error| panic!("collision burst frame {burst_frame}: {error:?}"));
        }

        assert_eq!(engine.collision_burst_frame, COLLISION_BURST_FRAMES);
        assert!(engine.batches().iter().all(|batch| {
            batch
                .vertices
                .iter()
                .flatten()
                .all(|coordinate| coordinate.is_finite())
        }));
    }

    #[test]
    fn camera_moving_past_geometry_depth_clips_faces_without_aborting() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine.step(16.0 / 9.0).unwrap();

        let mut camera = engine.camera();
        camera.target = [
            camera.position[0],
            camera.position[1],
            camera.position[2] + 1.0,
        ];
        engine.set_camera(camera).unwrap();
        engine
            .step(16.0 / 9.0)
            .expect("geometry outside the camera depth range must be clipped");

        assert!(
            engine
                .batches()
                .iter()
                .flat_map(|batch| batch.vertices.chunks_exact(VERTICES_PER_FACE))
                .all(|face| face.iter().all(|vertex| *vertex == HIDDEN))
        );
    }

    #[test]
    fn helio_gpu_abi_lengths_offsets_and_little_endian_encoding_are_exact() {
        assert_eq!(core::mem::size_of::<GpuInstanceData>(), 208);
        assert_eq!(core::mem::size_of::<GpuRetainedTransformSeed>(), 64);
        assert_eq!(core::mem::size_of::<GpuRetainedDrawTemplate>(), 24);
        assert_eq!(core::mem::size_of::<GpuCameraUniforms>(), 368);
        assert_eq!(core::mem::size_of::<GpuLight>(), 128);
        assert_eq!(core::mem::size_of::<GpuMaterial>(), 96);
        assert_eq!(core::mem::size_of::<GpuForwardLitGlobals>(), 48);

        let instance = GpuInstanceData {
            model: core::array::from_fn(|index| index as f32 + 0.25),
            normal_mat: core::array::from_fn(|index| index as f32 + 20.25),
            bounds: [40.25, 41.25, 42.25, 43.25],
            prev_model: core::array::from_fn(|index| index as f32 + 50.25),
            mesh_id: 0x1122_3344,
            material_id: 0x5566_7788,
            flags: 0x99aa_bbcc,
            lightmap_index: 0xddee_ff00,
        };
        let bytes = instance.to_le_bytes();
        assert_eq!(&bytes[0..4], &0.25f32.to_le_bytes());
        assert_eq!(&bytes[64..68], &20.25f32.to_le_bytes());
        assert_eq!(&bytes[112..116], &40.25f32.to_le_bytes());
        assert_eq!(&bytes[128..132], &50.25f32.to_le_bytes());
        assert_eq!(&bytes[192..196], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&bytes[204..208], &0xddee_ff00u32.to_le_bytes());

        let mut camera = GpuCameraUniforms::default();
        camera.proj[0] = 2.5;
        camera.position_near[3] = 0.125;
        camera.prev_view_proj[15] = 7.5;
        let bytes = camera.to_le_bytes();
        assert_eq!(&bytes[64..68], &2.5f32.to_le_bytes());
        assert_eq!(&bytes[268..272], &0.125f32.to_le_bytes());
        assert_eq!(&bytes[364..368], &7.5f32.to_le_bytes());

        let mut light = GpuLight::default();
        light.flare_enabled = 0x1020_3040;
        light.ies_profile_index = -7;
        let bytes = light.to_le_bytes();
        assert_eq!(&bytes[84..88], &0x1020_3040u32.to_le_bytes());
        assert_eq!(&bytes[112..116], &(-7i32).to_le_bytes());

        let mut material = GpuMaterial::default();
        material.tex_base_color = 0x1234_5678;
        material.class_params[0] = 3.25;
        let bytes = material.to_le_bytes();
        assert_eq!(&bytes[48..52], &0x1234_5678u32.to_le_bytes());
        assert_eq!(&bytes[80..84], &3.25f32.to_le_bytes());

        let seed = GpuRetainedTransformSeed {
            translation: [1.25, 2.25, 3.25],
            scale: [4.25, 5.25, 6.25],
            rotation: [7.25, 8.25, 9.25, 10.25],
            local_radius: 11.25,
            previous_translation: [12.25, 13.25, 14.25],
            draw_group: 0x1122_3344,
            flags: 0x5566_7788,
        };
        let bytes = seed.to_le_bytes();
        assert_eq!(&bytes[0..4], &1.25f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &4.25f32.to_le_bytes());
        assert_eq!(&bytes[24..28], &7.25f32.to_le_bytes());
        assert_eq!(&bytes[40..44], &11.25f32.to_le_bytes());
        assert_eq!(&bytes[44..48], &12.25f32.to_le_bytes());
        assert_eq!(&bytes[56..60], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&bytes[60..64], &0x5566_7788u32.to_le_bytes());
        assert_eq!(seed.compact_slot(), 0x5566);
        assert_eq!(seed.instance_flags(), 0x7788);
        assert_eq!(
            GpuRetainedTransformSeed::pack_slot_and_flags(0x5566, 0x7788),
            Some(0x5566_7788)
        );
        assert_eq!(GpuRetainedTransformSeed::pack_slot_and_flags(0x1_0000, 0), None);
        assert_eq!(GpuRetainedTransformSeed::pack_slot_and_flags(0, 0x1_0000), None);

        let template = GpuRetainedDrawTemplate::new(
            MeshDescriptor {
                mesh_id: 2,
                half_extents: [1.0; 3],
                first_vertex: 0,
                vertex_count: 24,
                first_index: 72,
                index_count: 36,
                base_vertex: -7,
            },
            DrawGroupDescriptor {
                mesh_id: 2,
                material_id: 3,
            },
            19,
            23,
        )
        .unwrap();
        assert_eq!(template.mesh_id(), 2);
        assert_eq!(template.material_id(), 3);
        let bytes = template.to_le_bytes();
        assert_eq!(&bytes[0..4], &36u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &72u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &(-7i32).to_le_bytes());
        assert_eq!(&bytes[12..16], &19u32.to_le_bytes());
        assert_eq!(&bytes[16..20], &23u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &(2u32 | (3u32 << 16)).to_le_bytes());
    }

    #[test]
    fn transform_frame_is_compact_prefix_counted_and_temporal() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        let (first_translations, first_view_proj) = {
            let frame = engine.step_transform_frame(16.0 / 9.0).unwrap();
            assert_eq!(frame.seeds.len(), 8);
            assert_eq!(frame.seed_dirty, DirtyRange { first: 0, count: 8 });
            assert_eq!(frame.hierarchy.nodes.len(), 9);
            assert_eq!(frame.hierarchy.local_affines.len(), 9);
            assert_eq!(frame.hierarchy.dynamic_bindings[0], u32::MAX);
            assert_eq!(frame.hierarchy.dynamic_bindings[1..], [0, 1, 2, 3, 4, 5, 6, 7]);
            assert_eq!(frame.hierarchy.row_leaf_nodes, &[1, 2, 3, 4, 5, 6, 7, 8]);
            assert_eq!(frame.hierarchy.dirty_local_nodes, &[1, 2, 3, 4, 5, 6, 7, 8]);
            assert_eq!(frame.hierarchy.dirty_world_nodes, &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
            assert_eq!(frame.hierarchy.dirty_rows, &[0, 1, 2, 3, 4, 5, 6, 7]);
            assert_eq!(frame.hierarchy.report.authored_ops, 10);
            assert_eq!(frame.hierarchy.report.constant_ops_folded, 2);
            assert_eq!(frame.hierarchy.report.runtime_nodes, 9);
            assert_eq!(frame.hierarchy.report.max_depth, 2);
            assert_eq!(frame.hierarchy.report.dirty_local, 8);
            assert_eq!(frame.hierarchy.report.dirty_world, 9);
            assert!(
                frame
                    .hierarchy
                    .nodes
                    .iter()
                    .all(|node| node.local_generation == 1 && node.world_generation == 1)
            );
            assert_eq!(frame.draw_templates.len(), DRAW_GROUP_COUNT);
            assert_eq!(
                frame.draw_templates_dirty,
                DirtyRange {
                    first: 0,
                    count: DRAW_GROUP_COUNT as u32,
                }
            );
            assert_eq!(
                frame
                    .draw_templates
                    .iter()
                    .map(|template| template.capacity)
                    .sum::<u32>(),
                frame.seeds.len() as u32
            );

            let mut expected_first = 0u32;
            for (group_index, template) in frame.draw_templates.iter().copied().enumerate() {
                let group = frame.groups[group_index];
                let mesh = frame.meshes[group_index / MATERIAL_COUNT];
                assert_eq!(template.first_instance, expected_first);
                assert_eq!(template.mesh_id(), group.mesh_id);
                assert_eq!(template.material_id(), group.material_id);
                assert_eq!(template.index_count, mesh.index_count);
                assert_eq!(template.first_index, mesh.first_index);
                assert_eq!(template.base_vertex, mesh.base_vertex);
                assert_eq!(
                    template.capacity as usize,
                    frame
                        .seeds
                        .iter()
                        .filter(|seed| seed.draw_group == group_index as u32)
                        .count()
                );
                expected_first += template.capacity;
            }
            assert_eq!(expected_first, frame.seeds.len() as u32);
            let mut expected_slots = [0u32; DRAW_GROUP_COUNT];
            for seed in frame.seeds {
                let group = seed.draw_group as usize;
                assert_eq!(seed.compact_slot(), expected_slots[group]);
                assert_eq!(
                    seed.instance_flags(),
                    INSTANCE_FLAG_CASTS_SHADOW | INSTANCE_FLAG_RECEIVES_SHADOW
                );
                expected_slots[group] += 1;
            }
            assert_eq!(expected_slots, frame.draw_templates.map(|template| template.capacity));
            assert!(frame.seeds.iter().all(|seed| {
                seed.translation.iter().all(|value| value.is_finite())
                    && seed.scale.iter().all(|value| *value > 0.0)
                    && seed.rotation.iter().all(|value| value.is_finite())
                    && seed.local_radius > 0.0
                    && seed.previous_translation == seed.translation
            }));
            (
                frame
                    .seeds
                    .iter()
                    .map(|seed| seed.translation)
                    .collect::<Vec<_>>(),
                frame.camera.view_proj,
            )
        };
        assert!(engine.gpu_instances.is_empty());

        let frame = engine.step_transform_frame(16.0 / 9.0).unwrap();
        assert_eq!(frame.camera.prev_view_proj, first_view_proj);
        assert!(
            frame.hierarchy.nodes[1..]
                .iter()
                .all(|node| node.local_generation == 2 && node.world_generation == 2)
        );
        assert!(
            frame
                .seeds
                .iter()
                .zip(first_translations.iter())
                .all(|(seed, previous)| seed.previous_translation == *previous)
        );
        assert!(
            frame
                .seeds
                .iter()
                .zip(first_translations.iter())
                .any(|(seed, previous)| seed.translation != *previous)
        );
    }

    #[test]
    fn instance_step_groups_contiguously_and_keeps_temporal_models() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        let (first_models, first_view_proj) = {
            let frame = engine.step_instances(16.0 / 9.0).unwrap();
            assert_eq!(frame.instances.len(), 8);
            assert_eq!(frame.instance_dirty, DirtyRange { first: 0, count: 8 });
            assert_eq!(frame.compacted_indices, &[0, 1, 2, 3, 4, 5, 6, 7]);
            assert_eq!(frame.meshes.len(), SHAPE_COUNT);
            assert_eq!(frame.groups.len(), DRAW_GROUP_COUNT);
            assert_eq!(
                frame
                    .draws
                    .iter()
                    .map(|draw| draw.instance_count)
                    .sum::<u32>(),
                8
            );

            let mut expected_first = 0u32;
            for (group_index, (group, draw)) in
                frame.groups.iter().zip(frame.draws.iter()).enumerate()
            {
                assert_eq!(draw.first_instance, expected_first);
                let start = draw.first_instance as usize;
                let end = start + draw.instance_count as usize;
                assert!(frame.instances[start..end].iter().all(|instance| {
                    instance.mesh_id == group.mesh_id
                        && instance.material_id == group.material_id
                        && instance.prev_model == instance.model
                }));
                assert_eq!(
                    draw.first_index,
                    frame.meshes[group_index / MATERIAL_COUNT].first_index
                );
                expected_first += draw.instance_count;
            }

            let identity = matrix4_mul(frame.camera.view_proj, frame.camera.inv_view_proj);
            for (index, value) in identity.iter().enumerate() {
                let expected = if index % 5 == 0 { 1.0 } else { 0.0 };
                assert!((value - expected).abs() < 2.0e-4, "identity[{index}]={value}");
            }
            assert!(frame.camera.view_proj.iter().all(|value| value.is_finite()));
            assert_eq!(frame.camera.prev_view_proj, frame.camera.view_proj);
            (
                frame
                    .instances
                    .iter()
                    .map(|instance| instance.model)
                    .collect::<Vec<_>>(),
                frame.camera.view_proj,
            )
        };

        let frame = engine.step_instances(16.0 / 9.0).unwrap();
        assert_eq!(frame.instances.len(), first_models.len());
        assert_eq!(frame.camera.prev_view_proj, first_view_proj);
        assert!(
            frame
                .instances
                .iter()
                .zip(first_models.iter())
                .all(|(instance, previous)| instance.prev_model == *previous)
        );
        assert!(
            frame
                .instances
                .iter()
                .zip(first_models.iter())
                .any(|(instance, previous)| instance.model != *previous)
        );
    }
}
