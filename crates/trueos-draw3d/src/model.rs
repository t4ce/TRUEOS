use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::protocol::Command;

pub type MeshId = u64;
pub type InstanceId = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn component_mul(self, rhs: Self) -> Self {
        Self::new(self.x * rhs.x, self.y * rhs.y, self.z * rhs.z)
    }

    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    pub fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }
}

impl core::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl core::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl core::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Rgba8 {
    fn default() -> Self {
        Self::WHITE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
}

impl Edge {
    pub const fn new(a: u32, b: u32) -> Self {
        Self { a, b }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Face {
    /// Vertex indices in winding order. Polygons are preserved; a renderer may triangulate them.
    pub vertices: Vec<u32>,
}

impl Face {
    pub fn new(vertices: Vec<u32>) -> Self {
        Self { vertices }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    /// One compact RGBA attribute for the whole mesh.
    pub color: Rgba8,
}

impl Mesh {
    pub fn new(vertices: Vec<Vec3>, edges: Vec<Edge>, faces: Vec<Face>, color: Rgba8) -> Self {
        Self {
            vertices,
            edges,
            faces,
            color,
        }
    }

    pub fn estimated_bytes(&self) -> u64 {
        let face_indices = self
            .faces
            .iter()
            .map(|face| face.vertices.len() as u64)
            .sum::<u64>();
        (self.vertices.len() as u64) * 12
            + (self.edges.len() as u64) * 8
            + (self.faces.len() as u64) * 2
            + face_indices * 4
            + 4
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Transform {
    pub location: Vec3,
    /// Euler rotation in radians, applied by the eventual renderer.
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        location: Vec3::ZERO,
        rotation: Vec3::ZERO,
        scale: Vec3::ONE,
    };

    pub fn is_finite(self) -> bool {
        self.location.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }

    /// Applies scale, then intrinsic X/Y/Z Euler rotation, then translation.
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        let scaled = point.component_mul(self.scale);
        let (sin_x, cos_x) = libm::sincosf(self.rotation.x);
        let (sin_y, cos_y) = libm::sincosf(self.rotation.y);
        let (sin_z, cos_z) = libm::sincosf(self.rotation.z);
        let x_rotated = Vec3::new(
            scaled.x,
            scaled.y * cos_x - scaled.z * sin_x,
            scaled.y * sin_x + scaled.z * cos_x,
        );
        let y_rotated = Vec3::new(
            x_rotated.x * cos_y + x_rotated.z * sin_y,
            x_rotated.y,
            -x_rotated.x * sin_y + x_rotated.z * cos_y,
        );
        Vec3::new(
            y_rotated.x * cos_z - y_rotated.y * sin_z,
            y_rotated.x * sin_z + y_rotated.y * cos_z,
            y_rotated.z,
        ) + self.location
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ViewCamera {
    pub position: Vec3,
    pub view_direction: Vec3,
    pub up_axis: Vec3,
    pub near_plane: f32,
    pub far_plane: f32,
    /// Vertical field of view in radians.
    pub vertical_fov: f32,
}

impl ViewCamera {
    pub const DEFAULT: Self = Self {
        position: Vec3::new(0.0, 0.0, 5.0),
        view_direction: Vec3::new(0.0, 0.0, -1.0),
        up_axis: Vec3::new(0.0, 1.0, 0.0),
        near_plane: 0.1,
        far_plane: 1_000.0,
        vertical_fov: core::f32::consts::FRAC_PI_3,
    };

    pub fn is_finite(self) -> bool {
        self.position.is_finite()
            && self.view_direction.is_finite()
            && self.up_axis.is_finite()
            && self.near_plane.is_finite()
            && self.far_plane.is_finite()
            && self.vertical_fov.is_finite()
    }
}

impl Default for ViewCamera {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub mesh_id: MeshId,
    pub transform: Transform,
}

impl Instance {
    pub const fn new(mesh_id: MeshId, transform: Transform) -> Self {
        Self { mesh_id, transform }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneLimits {
    pub max_meshes: usize,
    pub max_instances: usize,
    pub max_vertices_per_mesh: usize,
    pub max_edges_per_mesh: usize,
    /// Maximum triangle count after polygon fan triangulation.
    pub max_faces_per_mesh: usize,
    pub max_vertices_per_face: usize,
}

impl Default for SceneLimits {
    fn default() -> Self {
        Self {
            // The experimental render service deliberately has a small,
            // predictable residency envelope.
            max_meshes: 100,
            max_instances: 100,
            max_vertices_per_mesh: 1_000,
            max_edges_per_mesh: 3_000,
            max_faces_per_mesh: 2_000,
            max_vertices_per_face: 1_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneStats {
    pub mesh_count: u32,
    pub instance_count: u32,
    pub vertex_count: u64,
    pub edge_count: u64,
    pub face_count: u64,
    pub mesh_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyError {
    MeshMissing,
    InstanceMissing,
    TargetExists,
    MeshInUse,
    MeshLimit,
    InstanceLimit,
    VertexLimit,
    EdgeLimit,
    FaceLimit,
    FaceVertexLimit,
    FaceTooSmall,
    VertexIndexOutOfRange,
    NonFiniteVector,
    InvalidClipPlanes,
    InvalidFieldOfView,
    ZeroViewDirection,
    ZeroUpAxis,
    ParallelCameraAxes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplyOutcome {
    pub affected: u32,
    pub stats: SceneStats,
}

pub struct Scene {
    meshes: BTreeMap<MeshId, Mesh>,
    instances: BTreeMap<InstanceId, Instance>,
    camera: ViewCamera,
    limits: SceneLimits,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new(SceneLimits::default())
    }
}

impl Scene {
    pub fn new(limits: SceneLimits) -> Self {
        Self {
            meshes: BTreeMap::new(),
            instances: BTreeMap::new(),
            camera: ViewCamera::default(),
            limits,
        }
    }

    pub fn mesh(&self, id: MeshId) -> Option<&Mesh> {
        self.meshes.get(&id)
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(&id)
    }

    pub const fn camera(&self) -> ViewCamera {
        self.camera
    }

    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &Mesh)> {
        self.meshes.iter().map(|(&id, mesh)| (id, mesh))
    }

    pub fn instances(&self) -> impl Iterator<Item = (InstanceId, &Instance)> {
        self.instances.iter().map(|(&id, instance)| (id, instance))
    }

    pub fn stats(&self) -> SceneStats {
        let mut stats = SceneStats {
            mesh_count: self.meshes.len().min(u32::MAX as usize) as u32,
            instance_count: self.instances.len().min(u32::MAX as usize) as u32,
            ..SceneStats::default()
        };
        for mesh in self.meshes.values() {
            stats.vertex_count = stats
                .vertex_count
                .saturating_add(mesh.vertices.len() as u64);
            stats.edge_count = stats.edge_count.saturating_add(mesh.edges.len() as u64);
            stats.face_count = stats.face_count.saturating_add(mesh.faces.len() as u64);
            stats.mesh_bytes = stats.mesh_bytes.saturating_add(mesh.estimated_bytes());
        }
        stats
    }

    pub fn apply(&mut self, command: Command) -> Result<ApplyOutcome, ApplyError> {
        let affected = match command {
            Command::PutMesh { mesh_id, mesh } => {
                self.validate_mesh(&mesh)?;
                if !self.meshes.contains_key(&mesh_id)
                    && self.meshes.len() >= self.limits.max_meshes
                {
                    return Err(ApplyError::MeshLimit);
                }
                self.meshes.insert(mesh_id, mesh);
                1
            }
            Command::DeleteMesh { mesh_id, cascade } => {
                if !self.meshes.contains_key(&mesh_id) {
                    return Err(ApplyError::MeshMissing);
                }
                let referenced = self
                    .instances
                    .values()
                    .any(|instance| instance.mesh_id == mesh_id);
                if referenced && !cascade {
                    return Err(ApplyError::MeshInUse);
                }
                let before = self.instances.len();
                if cascade {
                    self.instances
                        .retain(|_, instance| instance.mesh_id != mesh_id);
                }
                self.meshes.remove(&mesh_id);
                1usize.saturating_add(before.saturating_sub(self.instances.len()))
            }
            Command::CopyMesh {
                source_id,
                target_id,
            } => {
                if self.meshes.contains_key(&target_id) {
                    return Err(ApplyError::TargetExists);
                }
                if self.meshes.len() >= self.limits.max_meshes {
                    return Err(ApplyError::MeshLimit);
                }
                let mesh = self
                    .meshes
                    .get(&source_id)
                    .ok_or(ApplyError::MeshMissing)?
                    .clone();
                self.meshes.insert(target_id, mesh);
                1
            }
            Command::SetVertices { mesh_id, vertices } => {
                if vertices.len() > self.limits.max_vertices_per_mesh {
                    return Err(ApplyError::VertexLimit);
                }
                validate_vectors(&vertices)?;
                let mesh = self.meshes.get(&mesh_id).ok_or(ApplyError::MeshMissing)?;
                validate_indices(
                    vertices.len(),
                    &mesh.edges,
                    &mesh.faces,
                    self.limits.max_vertices_per_face,
                )?;
                self.meshes.get_mut(&mesh_id).unwrap().vertices = vertices;
                1
            }
            Command::SetEdges { mesh_id, edges } => {
                if edges.len() > self.limits.max_edges_per_mesh {
                    return Err(ApplyError::EdgeLimit);
                }
                let vertex_count = self
                    .meshes
                    .get(&mesh_id)
                    .ok_or(ApplyError::MeshMissing)?
                    .vertices
                    .len();
                validate_edges(vertex_count, &edges)?;
                self.meshes.get_mut(&mesh_id).unwrap().edges = edges;
                1
            }
            Command::SetFaces { mesh_id, faces } => {
                if triangulated_face_count(&faces) > self.limits.max_faces_per_mesh {
                    return Err(ApplyError::FaceLimit);
                }
                let vertex_count = self
                    .meshes
                    .get(&mesh_id)
                    .ok_or(ApplyError::MeshMissing)?
                    .vertices
                    .len();
                validate_faces(vertex_count, &faces, self.limits.max_vertices_per_face)?;
                self.meshes.get_mut(&mesh_id).unwrap().faces = faces;
                1
            }
            Command::SetColor { mesh_id, color } => {
                self.meshes
                    .get_mut(&mesh_id)
                    .ok_or(ApplyError::MeshMissing)?
                    .color = color;
                1
            }
            Command::PutInstance {
                instance_id,
                instance,
            } => {
                self.validate_instance(&instance)?;
                if !self.instances.contains_key(&instance_id)
                    && self.instances.len() >= self.limits.max_instances
                {
                    return Err(ApplyError::InstanceLimit);
                }
                self.instances.insert(instance_id, instance);
                1
            }
            Command::DeleteInstance { instance_id } => {
                self.instances
                    .remove(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?;
                1
            }
            Command::CopyInstance {
                source_id,
                target_id,
            } => {
                if self.instances.contains_key(&target_id) {
                    return Err(ApplyError::TargetExists);
                }
                if self.instances.len() >= self.limits.max_instances {
                    return Err(ApplyError::InstanceLimit);
                }
                let instance = *self
                    .instances
                    .get(&source_id)
                    .ok_or(ApplyError::InstanceMissing)?;
                self.instances.insert(target_id, instance);
                1
            }
            Command::SetInstanceMesh {
                instance_id,
                mesh_id,
            } => {
                if !self.meshes.contains_key(&mesh_id) {
                    return Err(ApplyError::MeshMissing);
                }
                self.instances
                    .get_mut(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?
                    .mesh_id = mesh_id;
                1
            }
            Command::SetTransform {
                instance_id,
                transform,
            } => {
                validate_transform(transform)?;
                self.instances
                    .get_mut(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?
                    .transform = transform;
                1
            }
            Command::SetLocation {
                instance_id,
                location,
            } => {
                validate_vec3(location)?;
                self.instances
                    .get_mut(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?
                    .transform
                    .location = location;
                1
            }
            Command::SetRotation {
                instance_id,
                rotation,
            } => {
                validate_vec3(rotation)?;
                self.instances
                    .get_mut(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?
                    .transform
                    .rotation = rotation;
                1
            }
            Command::SetScale { instance_id, scale } => {
                validate_vec3(scale)?;
                self.instances
                    .get_mut(&instance_id)
                    .ok_or(ApplyError::InstanceMissing)?
                    .transform
                    .scale = scale;
                1
            }
            Command::SetViewCamera { camera } => {
                validate_camera(camera)?;
                self.camera = camera;
                1
            }
            Command::Clear => {
                let affected = self.meshes.len().saturating_add(self.instances.len());
                self.meshes.clear();
                self.instances.clear();
                affected
            }
            Command::GetStats | Command::Ping { .. } | Command::RequestRender => 0,
        };
        Ok(ApplyOutcome {
            affected: affected.min(u32::MAX as usize) as u32,
            stats: self.stats(),
        })
    }

    fn validate_mesh(&self, mesh: &Mesh) -> Result<(), ApplyError> {
        if mesh.vertices.len() > self.limits.max_vertices_per_mesh {
            return Err(ApplyError::VertexLimit);
        }
        if mesh.edges.len() > self.limits.max_edges_per_mesh {
            return Err(ApplyError::EdgeLimit);
        }
        if triangulated_face_count(&mesh.faces) > self.limits.max_faces_per_mesh {
            return Err(ApplyError::FaceLimit);
        }
        validate_vectors(&mesh.vertices)?;
        validate_indices(
            mesh.vertices.len(),
            &mesh.edges,
            &mesh.faces,
            self.limits.max_vertices_per_face,
        )
    }

    fn validate_instance(&self, instance: &Instance) -> Result<(), ApplyError> {
        if !self.meshes.contains_key(&instance.mesh_id) {
            return Err(ApplyError::MeshMissing);
        }
        validate_transform(instance.transform)
    }
}

fn validate_vec3(value: Vec3) -> Result<(), ApplyError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ApplyError::NonFiniteVector)
    }
}

fn validate_vectors(values: &[Vec3]) -> Result<(), ApplyError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(ApplyError::NonFiniteVector)
    }
}

fn validate_transform(transform: Transform) -> Result<(), ApplyError> {
    if transform.is_finite() {
        Ok(())
    } else {
        Err(ApplyError::NonFiniteVector)
    }
}

fn validate_camera(camera: ViewCamera) -> Result<(), ApplyError> {
    const MIN_AXIS_LENGTH_SQUARED: f32 = 1.0e-12;
    const MIN_CROSS_LENGTH_SQUARED: f32 = 1.0e-12;

    if !camera.is_finite() {
        return Err(ApplyError::NonFiniteVector);
    }
    if camera.near_plane <= 0.0 || camera.far_plane <= camera.near_plane {
        return Err(ApplyError::InvalidClipPlanes);
    }
    if camera.vertical_fov <= 0.0 || camera.vertical_fov >= core::f32::consts::PI {
        return Err(ApplyError::InvalidFieldOfView);
    }
    if camera.view_direction.length_squared() <= MIN_AXIS_LENGTH_SQUARED {
        return Err(ApplyError::ZeroViewDirection);
    }
    if camera.up_axis.length_squared() <= MIN_AXIS_LENGTH_SQUARED {
        return Err(ApplyError::ZeroUpAxis);
    }
    if camera.view_direction.cross(camera.up_axis).length_squared()
        <= MIN_CROSS_LENGTH_SQUARED
            * camera.view_direction.length_squared()
            * camera.up_axis.length_squared()
    {
        return Err(ApplyError::ParallelCameraAxes);
    }
    Ok(())
}

fn validate_edges(vertex_count: usize, edges: &[Edge]) -> Result<(), ApplyError> {
    if edges
        .iter()
        .any(|edge| edge.a as usize >= vertex_count || edge.b as usize >= vertex_count)
    {
        Err(ApplyError::VertexIndexOutOfRange)
    } else {
        Ok(())
    }
}

fn validate_faces(
    vertex_count: usize,
    faces: &[Face],
    max_vertices_per_face: usize,
) -> Result<(), ApplyError> {
    for face in faces {
        if face.vertices.len() < 3 {
            return Err(ApplyError::FaceTooSmall);
        }
        if face.vertices.len() > max_vertices_per_face {
            return Err(ApplyError::FaceVertexLimit);
        }
        if face
            .vertices
            .iter()
            .any(|&index| index as usize >= vertex_count)
        {
            return Err(ApplyError::VertexIndexOutOfRange);
        }
    }
    Ok(())
}

fn triangulated_face_count(faces: &[Face]) -> usize {
    faces
        .iter()
        .fold(0usize, |total, face| total.saturating_add(face.vertices.len().saturating_sub(2)))
}

fn validate_indices(
    vertex_count: usize,
    edges: &[Edge],
    faces: &[Face],
    max_vertices_per_face: usize,
) -> Result<(), ApplyError> {
    validate_edges(vertex_count, edges)?;
    validate_faces(vertex_count, faces, max_vertices_per_face)
}
