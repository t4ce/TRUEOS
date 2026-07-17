use alloc::vec::Vec;

use crate::{InstanceId, MeshId, Rgba8, Scene, Vec3, ViewCamera};

/// One scene instance converted to indexed clip-space triangles.
///
/// The source scene still owns and de-duplicates meshes. This is the bounded
/// renderer-facing cache which can be uploaded once and reused until the
/// scene or camera changes.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedMesh {
    pub instance_id: InstanceId,
    pub mesh_id: MeshId,
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub color: Rgba8,
    /// Positive camera-space distance used for pass-local draw ordering:
    /// opaque front-to-back, then blended back-to-front.
    pub depth: f32,
}

#[derive(Clone, Copy)]
struct CameraBasis {
    position: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    near: f32,
    far: f32,
    tan_half_fov: f32,
    aspect: f32,
}

impl CameraBasis {
    fn from_camera(camera: ViewCamera, aspect: f32) -> Self {
        let forward = normalize(camera.view_direction);
        let right = normalize(forward.cross(camera.up_axis));
        let up = normalize(right.cross(forward));
        Self {
            position: camera.position,
            right,
            up,
            forward,
            near: camera.near_plane,
            far: camera.far_plane,
            tan_half_fov: libm::tanf(camera.vertical_fov * 0.5),
            aspect: aspect.max(1.0e-6),
        }
    }

    fn project(self, point: Vec3) -> Option<([f32; 3], f32)> {
        let relative = point - self.position;
        let depth = relative.dot(self.forward);
        if depth < self.near || depth > self.far {
            return None;
        }
        let inverse_y = 1.0 / (depth * self.tan_half_fov);
        let ndc_x = relative.dot(self.right) * inverse_y / self.aspect;
        let ndc_y = relative.dot(self.up) * inverse_y;
        let ndc_z = (depth - self.near) / (self.far - self.near);
        Some(([ndc_x, ndc_y, ndc_z], depth))
    }
}

/// Materialize the current instances as renderer-ready triangle jobs.
///
/// Polygon faces use a compact fan triangulation. A triangle crossing a near
/// or far clip plane is omitted for now; X/Y clipping remains GPU-owned.
pub fn project_scene(scene: &Scene, aspect: f32) -> Vec<ProjectedMesh> {
    project_scene_with_camera(scene, aspect, scene.camera())
}

/// Project a scene after evaluating its optional camera orbit at `angle`.
///
/// `angle` is in radians. For a camera without an orbit this is identical to
/// [`project_scene`]. Keeping time outside the model makes protocol/state tests
/// deterministic and lets the kernel render loop choose its own clock.
pub fn project_scene_at(scene: &Scene, aspect: f32, angle: f32) -> Vec<ProjectedMesh> {
    project_scene_with_camera(scene, aspect, scene.camera_at(angle))
}

/// Project a scene through an explicit view without mutating the scene's
/// protocol-owned camera.
///
/// This is the renderer-facing boundary used by independent consumers such
/// as a local fly camera and an off-screen screenshot view.
pub fn project_scene_with_camera(
    scene: &Scene,
    aspect: f32,
    view_camera: ViewCamera,
) -> Vec<ProjectedMesh> {
    let camera = CameraBasis::from_camera(view_camera, aspect);
    let mut projected = Vec::with_capacity(scene.stats().instance_count as usize);

    for (instance_id, instance) in scene.instances() {
        let Some(mesh) = scene.mesh(instance.mesh_id) else {
            continue;
        };
        // Straight-alpha source-over with a constant zero-alpha mesh has no
        // color or alpha contribution. Reject it before transform/projection,
        // residency, and GPU submission.
        if mesh.color.a == 0 {
            continue;
        }
        let mut vertices = Vec::with_capacity(mesh.vertices.len());
        let mut visible = Vec::with_capacity(mesh.vertices.len());
        let mut depth_sum = 0.0;
        let mut depth_count = 0usize;
        for source in &mesh.vertices {
            let world = instance.transform.transform_point(*source);
            if let Some((point, depth)) = camera.project(world) {
                vertices.push(point);
                visible.push(true);
                depth_sum += depth;
                depth_count += 1;
            } else {
                vertices.push([0.0, 0.0, 0.0]);
                visible.push(false);
            }
        }

        let mut indices = Vec::new();
        for face in &mesh.faces {
            let first = face.vertices[0];
            for pair in face.vertices[1..].windows(2) {
                let triangle = [first, pair[0], pair[1]];
                if triangle.iter().all(|index| visible[*index as usize]) {
                    let a = vertices[triangle[0] as usize];
                    let b = vertices[triangle[1] as usize];
                    let c = vertices[triangle[2] as usize];
                    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                    if area.abs() > 1.0e-9 {
                        if area < 0.0 {
                            indices.extend_from_slice(&[triangle[0], triangle[2], triangle[1]]);
                        } else {
                            indices.extend_from_slice(&triangle);
                        }
                    } else {
                        indices.extend_from_slice(&[first, first, first]);
                    }
                } else {
                    // Preserve a stable index-buffer size across camera and
                    // transform changes so the resident job can be updated in
                    // place. A degenerate triangle has no raster coverage.
                    indices.extend_from_slice(&[first, first, first]);
                }
            }
        }
        if !indices.is_empty() {
            projected.push(ProjectedMesh {
                instance_id,
                mesh_id: instance.mesh_id,
                vertices,
                indices,
                color: mesh.color,
                depth: depth_sum / depth_count.max(1) as f32,
            });
        }
    }

    projected.sort_by(compare_projected_draw_order);
    projected
}

fn compare_projected_draw_order(
    left: &ProjectedMesh,
    right: &ProjectedMesh,
) -> core::cmp::Ordering {
    let left_opaque = left.color.a == u8::MAX;
    let right_opaque = right.color.a == u8::MAX;
    match (left_opaque, right_opaque) {
        // Opaque geometry establishes the depth buffer before any blended
        // draw. Front-to-back improves rejection without changing the result.
        (true, true) => left.depth.partial_cmp(&right.depth),
        (true, false) => Some(core::cmp::Ordering::Less),
        (false, true) => Some(core::cmp::Ordering::Greater),
        // Blended geometry retains the existing painter order while testing
        // read-only against the completed opaque depth buffer.
        (false, false) => right.depth.partial_cmp(&left.depth),
    }
    .unwrap_or(core::cmp::Ordering::Equal)
    .then_with(|| left.instance_id.cmp(&right.instance_id))
}

fn normalize(value: Vec3) -> Vec3 {
    let inverse = 1.0 / libm::sqrtf(value.length_squared());
    value * inverse
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::{Command, Face, Instance, Mesh, Transform};

    #[test]
    fn projects_and_triangulates_an_instance() {
        let mut scene = Scene::default();
        scene
            .apply(Command::PutMesh {
                mesh_id: 1,
                mesh: Mesh::new(
                    vec![
                        Vec3::new(-1.0, -1.0, 0.0),
                        Vec3::new(1.0, -1.0, 0.0),
                        Vec3::new(1.0, 1.0, 0.0),
                        Vec3::new(-1.0, 1.0, 0.0),
                    ],
                    Vec::new(),
                    vec![Face::new(vec![0, 1, 2, 3])],
                    Rgba8::new(1, 2, 3, 4),
                ),
            })
            .unwrap();
        scene
            .apply(Command::PutInstance {
                instance_id: 9,
                instance: Instance::new(1, Transform::IDENTITY),
            })
            .unwrap();

        let jobs = project_scene(&scene, 1.0);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(jobs[0].color, Rgba8::new(1, 2, 3, 4));
        assert!(jobs[0].vertices.iter().all(|point| point[2] > 0.0));

        let stored_camera = scene.camera();
        let fly_camera = ViewCamera {
            position: Vec3::new(2.0, 0.0, 5.0),
            view_direction: Vec3::new(-2.0, 0.0, -5.0),
            ..stored_camera
        };
        let fly_jobs = project_scene_with_camera(&scene, 1.0, fly_camera);
        assert_eq!(scene.camera(), stored_camera);
        assert_eq!(fly_jobs.len(), 1);
        assert_ne!(fly_jobs[0].vertices, jobs[0].vertices);
    }

    #[test]
    fn xyz_euler_rotation_is_applied() {
        let transform = Transform {
            rotation: Vec3::new(0.0, 0.0, core::f32::consts::FRAC_PI_2),
            ..Transform::IDENTITY
        };
        let point = transform.transform_point(Vec3::new(1.0, 0.0, 0.0));
        assert!(point.x.abs() < 1.0e-5);
        assert!((point.y - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn orders_opaque_front_to_back_then_blended_back_to_front() {
        let draw = |instance_id, depth, alpha| ProjectedMesh {
            instance_id,
            mesh_id: instance_id,
            vertices: Vec::new(),
            indices: Vec::new(),
            color: Rgba8::new(1, 2, 3, alpha),
            depth,
        };
        let mut draws = vec![
            draw(1, 2.0, 128),
            draw(2, 8.0, u8::MAX),
            draw(3, 9.0, 128),
            draw(4, 1.0, u8::MAX),
        ];
        draws.sort_by(compare_projected_draw_order);
        assert_eq!(
            draws
                .iter()
                .map(|draw| draw.instance_id)
                .collect::<Vec<_>>(),
            vec![4, 2, 3, 1]
        );
    }

    #[test]
    fn zero_alpha_mesh_is_not_projected() {
        let mut scene = Scene::default();
        scene
            .apply(Command::PutMesh {
                mesh_id: 1,
                mesh: Mesh::new(
                    vec![
                        Vec3::new(-1.0, -1.0, 0.0),
                        Vec3::new(1.0, -1.0, 0.0),
                        Vec3::new(0.0, 1.0, 0.0),
                    ],
                    Vec::new(),
                    vec![Face::new(vec![0, 1, 2])],
                    Rgba8::new(255, 255, 255, 0),
                ),
            })
            .unwrap();
        scene
            .apply(Command::PutInstance {
                instance_id: 1,
                instance: Instance::new(1, Transform::IDENTITY),
            })
            .unwrap();

        assert!(project_scene(&scene, 1.0).is_empty());
    }
}
