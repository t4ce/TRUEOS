//! Shared retained-renderer ABI retained after the Churn demo moved to its
//! own Blueprint. The scene simulation and compatibility batches do not live
//! in TRUEOS; Picasso and the Intel renderer consume only these layouts.

use crate::DrawIndexedIndirectArgs;
use crate::retained_transform::TransformHierarchyFrame;

pub const MATERIAL_COUNT: usize = 4;
pub const SHAPE_COUNT: usize = 3;
pub const DRAW_GROUP_COUNT: usize = MATERIAL_COUNT * SHAPE_COUNT;
pub const INSTANCE_FLAG_CASTS_SHADOW: u32 = 1 << 0;
pub const INSTANCE_FLAG_RECEIVES_SHADOW: u32 = 1 << 1;
pub const MAX_RETAINED_TRANSFORM_ROWS: usize = 4_096;

const _: () = assert!(MAX_RETAINED_TRANSFORM_ROWS <= u16::MAX as usize);

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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuRetainedTransformSeed {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    pub local_radius: f32,
    pub previous_translation: [f32; 3],
    pub draw_group: u32,
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpuRetainedDrawTemplate {
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
    pub capacity: u32,
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
    pub materials: &'a [GpuMaterial; MATERIAL_COUNT],
    pub meshes: &'a [MeshDescriptor; SHAPE_COUNT],
    pub instances: &'a [GpuInstanceData],
    pub compacted_indices: &'a [u32],
    pub draws: &'a [DrawIndexedIndirectArgs; DRAW_GROUP_COUNT],
    pub instance_dirty: DirtyRange,
    pub compacted_indices_dirty: DirtyRange,
}

pub struct TransformFrame<'a> {
    pub camera: &'a GpuCameraUniforms,
    pub materials: &'a [GpuMaterial; MATERIAL_COUNT],
    pub meshes: &'a [MeshDescriptor; SHAPE_COUNT],
    pub groups: &'a [DrawGroupDescriptor; DRAW_GROUP_COUNT],
    pub seeds: &'a [GpuRetainedTransformSeed],
    pub draw_templates: &'a [GpuRetainedDrawTemplate; DRAW_GROUP_COUNT],
    pub seed_dirty: DirtyRange,
    pub draw_templates_dirty: DirtyRange,
    pub hierarchy: TransformHierarchyFrame<'a>,
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
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, translation) == 0);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, scale) == 12);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, rotation) == 24);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, local_radius) == 40);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, previous_translation) == 44);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, draw_group) == 56);
    assert!(core::mem::offset_of!(GpuRetainedTransformSeed, flags) == 60);
    assert!(core::mem::size_of::<GpuRetainedDrawTemplate>() == GpuRetainedDrawTemplate::BYTE_LEN);
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
    assert!(core::mem::size_of::<GpuMaterial>() == GpuMaterial::BYTE_LEN);
    assert!(core::mem::offset_of!(GpuMaterial, tex_base_color) == 48);
    assert!(core::mem::offset_of!(GpuMaterial, class_params) == 80);
};

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
