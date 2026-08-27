//! Texture-free runtime lowering of Helio's `portal_rooms.rs` scene.
//!
//! Each room is authored once in entrance-local coordinates. At frame time
//! it is rigidly mapped behind its full cube face and clipped in homogeneous
//! space against that projected portal window. The retained Intel renderer
//! therefore receives ordinary indexed triangles, while the impossible-room
//! topology and portal edge confinement remain explicit and deterministic.

use alloc::vec;
use alloc::vec::Vec;

use crate::churn::Batch;
use crate::{Camera, Error, linear_rgba_to_srgba8};
use trueos_helio_artifact::SectionKind;

pub const SECTION_NAME: &str = "scene/portal-rooms-v1.bin";
const MAGIC: &[u8; 8] = b"HPORTAL\0";
const VERSION: u16 = 1;
const HEADER_BYTES: usize = 64;
const PORTAL_BYTES: usize = 32;
const MATERIAL_BYTES: usize = 32;
const OBJECT_BYTES: usize = 32;
const SHADE_COUNT: usize = 3;
const MAX_CLIPPED_TRIANGLES: usize = 12;
const HIDDEN: [f32; 3] = [2.0, 2.0, 0.999];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Portal {
    normal: [f32; 3],
    up_hint: [f32; 3],
    base_material: usize,
    accent_material: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Material {
    linear_rgba: [f32; 4],
    emissive: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shape {
    Box,
    Sphere,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Object {
    portal: usize,
    material: usize,
    shape: Shape,
    center: [f32; 3],
    half_extent: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    hub_half_size: f32,
    wall_thickness: f32,
    room_half_size: f32,
    move_speed: f32,
    mouse_sensitivity: f32,
    vertical_fov: f32,
    near: f32,
    far: f32,
    portals: Vec<Portal>,
    materials: Vec<Material>,
    objects: Vec<Object>,
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let section = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingPortalRoomsScene)?;
        if section.kind != SectionKind::Unknown(u16::MAX) {
            return Err(Error::InvalidPortalRoomsScene);
        }
        let bytes = section.data;
        if bytes.len() < HEADER_BYTES
            || bytes.get(..8) != Some(MAGIC.as_slice())
            || read_u16(bytes, 8)? != VERSION
            || usize::from(read_u16(bytes, 10)?) != HEADER_BYTES
            || usize::try_from(read_u32(bytes, 12)?).ok() != Some(bytes.len())
        {
            return Err(Error::InvalidPortalRoomsScene);
        }
        let portal_count = usize::from(read_u16(bytes, 16)?);
        let material_count = usize::from(read_u16(bytes, 18)?);
        let object_count = usize::from(read_u16(bytes, 20)?);
        let object_stride = usize::from(read_u16(bytes, 22)?);
        let portal_offset = HEADER_BYTES;
        let material_offset = portal_offset
            .checked_add(
                portal_count
                    .checked_mul(PORTAL_BYTES)
                    .ok_or(Error::InvalidPortalRoomsScene)?,
            )
            .ok_or(Error::InvalidPortalRoomsScene)?;
        let object_offset = material_offset
            .checked_add(
                material_count
                    .checked_mul(MATERIAL_BYTES)
                    .ok_or(Error::InvalidPortalRoomsScene)?,
            )
            .ok_or(Error::InvalidPortalRoomsScene)?;
        let expected_len = object_offset
            .checked_add(
                object_count
                    .checked_mul(object_stride)
                    .ok_or(Error::InvalidPortalRoomsScene)?,
            )
            .ok_or(Error::InvalidPortalRoomsScene)?;
        if portal_count != 6
            || material_count != 14
            || object_count == 0
            || object_count > 256
            || object_stride != OBJECT_BYTES
            || expected_len != bytes.len()
            || bytes[56..64].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidPortalRoomsScene);
        }
        let scalars = read_f32s::<8>(bytes, 24)?;
        let mut portals = Vec::with_capacity(portal_count);
        for index in 0..portal_count {
            let offset = portal_offset + index * PORTAL_BYTES;
            let portal = Portal {
                normal: read_f32s(bytes, offset)?,
                up_hint: read_f32s(bytes, offset + 12)?,
                base_material: usize::from(read_u16(bytes, offset + 24)?),
                accent_material: usize::from(read_u16(bytes, offset + 26)?),
            };
            if read_u32(bytes, offset + 28)? != 0
                || portal.base_material >= material_count
                || portal.accent_material >= material_count
                || (length(portal.normal) - 1.0).abs() > 0.001
                || (length(portal.up_hint) - 1.0).abs() > 0.001
                || dot(portal.normal, portal.up_hint).abs() > 0.001
            {
                return Err(Error::InvalidPortalRoomsScene);
            }
            portals.push(portal);
        }
        let mut materials = Vec::with_capacity(material_count);
        for index in 0..material_count {
            let offset = material_offset + index * MATERIAL_BYTES;
            let material = Material {
                linear_rgba: read_f32s(bytes, offset)?,
                emissive: read_f32(bytes, offset + 16)?,
            };
            if material
                .linear_rgba
                .iter()
                .any(|value| !(0.0..=1.0).contains(value))
                || !(0.0..=4.0).contains(&material.emissive)
                || bytes[offset + 20..offset + MATERIAL_BYTES]
                    .iter()
                    .any(|byte| *byte != 0)
            {
                return Err(Error::InvalidPortalRoomsScene);
            }
            materials.push(material);
        }
        let mut objects = Vec::with_capacity(object_count);
        for index in 0..object_count {
            let offset = object_offset + index * object_stride;
            let portal = usize::from(read_u16(bytes, offset)?);
            let material = usize::from(read_u16(bytes, offset + 2)?);
            let shape = match *bytes
                .get(offset + 4)
                .ok_or(Error::InvalidPortalRoomsScene)?
            {
                0 => Shape::Box,
                1 => Shape::Sphere,
                _ => return Err(Error::InvalidPortalRoomsScene),
            };
            let object = Object {
                portal,
                material,
                shape,
                center: read_f32s(bytes, offset + 8)?,
                half_extent: read_f32s(bytes, offset + 20)?,
            };
            if portal >= portal_count
                || material >= material_count
                || bytes[offset + 5..offset + 8].iter().any(|byte| *byte != 0)
                || object.half_extent.iter().any(|value| *value <= 0.0)
            {
                return Err(Error::InvalidPortalRoomsScene);
            }
            objects.push(object);
        }
        let spec = Self {
            hub_half_size: scalars[0],
            wall_thickness: scalars[1],
            room_half_size: scalars[2],
            move_speed: scalars[3],
            mouse_sensitivity: scalars[4],
            vertical_fov: scalars[5],
            near: scalars[6],
            far: scalars[7],
            portals,
            materials,
            objects,
        };
        if spec.hub_half_size <= 0.0
            || spec.wall_thickness <= 0.0
            || spec.room_half_size != spec.hub_half_size
            || spec.move_speed <= 0.0
            || spec.mouse_sensitivity <= 0.0
            || !(0.1..core::f32::consts::PI).contains(&spec.vertical_fov)
            || spec.near <= 0.0
            || spec.far <= spec.near
        {
            return Err(Error::InvalidPortalRoomsScene);
        }
        Ok(spec)
    }
}

#[derive(Clone, Copy)]
struct LocalTriangle {
    portal: usize,
    points: [[f32; 3]; 3],
    group: usize,
}

pub struct Engine {
    spec: Spec,
    camera: Camera,
    editor_mode: bool,
    triangles: Vec<LocalTriangle>,
    group_batches: Vec<Option<usize>>,
    editor_batches: [usize; 2],
    batches: Vec<Batch>,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let group_count = spec.materials.len() * SHADE_COUNT;
        let mut triangles = Vec::new();
        let mut group_triangles = vec![0usize; group_count];
        for object in &spec.objects {
            append_object_triangles(*object, &mut triangles, &mut group_triangles);
        }
        let mut batches = Vec::new();
        let mut group_batches = vec![None; group_count];
        for (group, triangle_count) in group_triangles.iter().copied().enumerate() {
            if triangle_count == 0 {
                continue;
            }
            let material = spec.materials[group / SHADE_COUNT];
            let rgba = shaded_color(material, group % SHADE_COUNT)?;
            group_batches[group] = Some(batches.len());
            batches.push(triangle_batch(
                triangle_count
                    .checked_mul(MAX_CLIPPED_TRIANGLES)
                    .ok_or(Error::InvalidPortalRoomsScene)?,
                rgba,
            )?);
        }
        let editor_start = batches.len();
        let editor_triangles = spec.portals.len() * 4 * 4 * MAX_CLIPPED_TRIANGLES;
        batches.push(triangle_batch(editor_triangles, [70, 74, 82, 72])?);
        batches.push(triangle_batch(editor_triangles, [205, 210, 220, 72])?);
        let camera = Camera {
            position: [16.0, 12.0, 16.0],
            target: [15.375, 11.531, 15.375],
            up: [0.0, 1.0, 0.0],
            vertical_fov_radians: spec.vertical_fov,
            near: spec.near,
            far: spec.far,
        };
        Ok(Self {
            spec,
            camera,
            editor_mode: true,
            triangles,
            group_batches,
            editor_batches: [editor_start, editor_start + 1],
            batches,
        })
    }

    pub fn name(&self) -> &'static str {
        "portal-rooms"
    }

    pub fn controls(&self) -> &'static str {
        "WASD+Space+Shift,left-drag-look,Tab-portal-overlay"
    }

    pub fn object_count(&self) -> usize {
        self.spec.objects.len()
    }

    pub fn portal_count(&self) -> usize {
        self.spec.portals.len()
    }

    pub fn editor_mode(&self) -> bool {
        self.editor_mode
    }

    pub fn toggle_editor_mode(&mut self) {
        self.editor_mode = !self.editor_mode;
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), Error> {
        if camera
            .position
            .iter()
            .chain(camera.target.iter())
            .any(|value| !value.is_finite())
            || camera.near <= 0.0
            || camera.far <= camera.near
        {
            return Err(Error::InvalidPortalRoomsScene);
        }
        self.camera = camera;
        Ok(())
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn step(&mut self, aspect: f32) -> Result<&[Batch], Error> {
        if !aspect.is_finite() || aspect <= 0.0 {
            return Err(Error::InvalidPortalRoomsScene);
        }
        for batch in &mut self.batches {
            batch.vertices.fill(HIDDEN);
        }
        let projection = Projection::new(self.camera, aspect)?;
        let mut used = vec![0usize; self.batches.len()];
        for index in 0..self.triangles.len() {
            let triangle = self.triangles[index];
            let portal = self.spec.portals[triangle.portal];
            let Some(planes) = portal_clip_planes(portal, self.spec.hub_half_size, projection)
            else {
                continue;
            };
            let mut polygon = Vec::with_capacity(12);
            for point in triangle.points {
                polygon.push(projection.project(portal_point(
                    portal,
                    self.spec.hub_half_size,
                    point,
                )));
            }
            clip_polygon(&mut polygon, &planes);
            let Some(batch_index) = self.group_batches[triangle.group] else {
                return Err(Error::InvalidPortalRoomsScene);
            };
            emit_polygon(&polygon, &mut self.batches[batch_index], &mut used[batch_index])?;
        }
        if self.editor_mode {
            self.emit_editor_overlay(projection, &mut used)?;
        }
        Ok(&self.batches)
    }

    fn emit_editor_overlay(
        &mut self,
        projection: Projection,
        used: &mut [usize],
    ) -> Result<(), Error> {
        const GRID: usize = 4;
        for portal in self.spec.portals.iter().copied() {
            let Some(planes) = portal_clip_planes(portal, self.spec.hub_half_size, projection)
            else {
                continue;
            };
            let step = self.spec.hub_half_size * 2.0 / GRID as f32;
            for y in 0..GRID {
                for x in 0..GRID {
                    let left = -self.spec.hub_half_size + x as f32 * step;
                    let bottom = -self.spec.hub_half_size + y as f32 * step;
                    let local = [
                        [left, bottom, -0.002],
                        [left + step, bottom, -0.002],
                        [left + step, bottom + step, -0.002],
                        [left, bottom + step, -0.002],
                    ];
                    let batch_index = self.editor_batches[(x + y) & 1];
                    for indices in [[0usize, 1, 2], [0usize, 2, 3]] {
                        let mut polygon = Vec::with_capacity(8);
                        for index in indices {
                            polygon.push(projection.project(portal_point(
                                portal,
                                self.spec.hub_half_size,
                                local[index],
                            )));
                        }
                        clip_polygon(&mut polygon, &planes);
                        emit_polygon(
                            &polygon,
                            &mut self.batches[batch_index],
                            &mut used[batch_index],
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn shaded_color(material: Material, shade: usize) -> Result<[u8; 4], Error> {
    let illumination = [0.42, 0.68, 0.94][shade] + material.emissive;
    linear_rgba_to_srgba8([
        (material.linear_rgba[0] * illumination).min(1.0),
        (material.linear_rgba[1] * illumination).min(1.0),
        (material.linear_rgba[2] * illumination).min(1.0),
        material.linear_rgba[3],
    ])
}

fn triangle_batch(triangles: usize, rgba: [u8; 4]) -> Result<Batch, Error> {
    let vertex_count = triangles
        .checked_mul(3)
        .ok_or(Error::InvalidPortalRoomsScene)?;
    let vertices = vec![HIDDEN; vertex_count];
    let mut indices = Vec::with_capacity(vertex_count);
    for index in 0..vertex_count {
        indices.push(u32::try_from(index).map_err(|_| Error::InvalidPortalRoomsScene)?);
    }
    Ok(Batch {
        vertices,
        indices,
        rgba,
    })
}

fn append_object_triangles(
    object: Object,
    triangles: &mut Vec<LocalTriangle>,
    counts: &mut [usize],
) {
    let [cx, cy, cz] = object.center;
    let [hx, hy, hz] = object.half_extent;
    let mut push = |points: [[f32; 3]; 3], normal: [f32; 3]| {
        let light = normalize([-0.35, 0.8, -0.48]);
        let response = dot(normalize(normal), light);
        let shade = if response > 0.45 {
            2
        } else if response > -0.2 {
            1
        } else {
            0
        };
        let group = object.material * SHADE_COUNT + shade;
        counts[group] += 1;
        triangles.push(LocalTriangle {
            portal: object.portal,
            points,
            group,
        });
    };
    match object.shape {
        Shape::Box => {
            let v = [
                [cx - hx, cy - hy, cz - hz],
                [cx + hx, cy - hy, cz - hz],
                [cx + hx, cy + hy, cz - hz],
                [cx - hx, cy + hy, cz - hz],
                [cx - hx, cy - hy, cz + hz],
                [cx + hx, cy - hy, cz + hz],
                [cx + hx, cy + hy, cz + hz],
                [cx - hx, cy + hy, cz + hz],
            ];
            for (quad, normal) in [
                ([0, 3, 2, 1], [0.0, 0.0, -1.0]),
                ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
                ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
                ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
                ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
                ([3, 7, 6, 2], [0.0, 1.0, 0.0]),
            ] {
                push([v[quad[0]], v[quad[1]], v[quad[2]]], normal);
                push([v[quad[0]], v[quad[2]], v[quad[3]]], normal);
            }
        }
        Shape::Sphere => {
            let v = [
                [cx, cy + hy, cz],
                [cx, cy - hy, cz],
                [cx - hx, cy, cz],
                [cx + hx, cy, cz],
                [cx, cy, cz - hz],
                [cx, cy, cz + hz],
            ];
            for face in [
                [0, 4, 3],
                [0, 3, 5],
                [0, 5, 2],
                [0, 2, 4],
                [1, 3, 4],
                [1, 5, 3],
                [1, 2, 5],
                [1, 4, 2],
            ] {
                let points = [v[face[0]], v[face[1]], v[face[2]]];
                let normal = normalize([
                    (points[0][0] + points[1][0] + points[2][0]) / 3.0 - cx,
                    (points[0][1] + points[1][1] + points[2][1]) / 3.0 - cy,
                    (points[0][2] + points[1][2] + points[2][2]) / 3.0 - cz,
                ]);
                push(points, normal);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Projection {
    position: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    x_scale: f32,
    y_scale: f32,
    depth_a: f32,
    depth_b: f32,
}

impl Projection {
    fn new(camera: Camera, aspect: f32) -> Result<Self, Error> {
        let forward = normalize(sub(camera.target, camera.position));
        let right = normalize(cross(forward, camera.up));
        let up = normalize(cross(right, forward));
        let tangent = libm::tanf(camera.vertical_fov_radians * 0.5);
        if length(forward) <= f32::EPSILON || length(right) <= f32::EPSILON || tangent <= 0.0 {
            return Err(Error::InvalidPortalRoomsScene);
        }
        Ok(Self {
            position: camera.position,
            right,
            up,
            forward,
            x_scale: 1.0 / (tangent * aspect),
            y_scale: 1.0 / tangent,
            depth_a: camera.far / (camera.far - camera.near),
            depth_b: camera.near * camera.far / (camera.far - camera.near),
        })
    }

    fn project(self, point: [f32; 3]) -> [f32; 4] {
        let delta = sub(point, self.position);
        let z = dot(delta, self.forward);
        [
            dot(delta, self.right) * self.x_scale,
            dot(delta, self.up) * self.y_scale,
            self.depth_a * z - self.depth_b,
            z,
        ]
    }
}

#[derive(Clone, Copy)]
struct ClipPlane([f32; 4]);

fn portal_clip_planes(portal: Portal, half: f32, projection: Projection) -> Option<Vec<ClipPlane>> {
    let center = scale(portal.normal, half);
    if dot(portal.normal, sub(projection.position, center)) <= 0.001 {
        return None;
    }
    let right = normalize(cross(portal.up_hint, portal.normal));
    let up = normalize(cross(portal.normal, right));
    let local = [[-half, -half], [half, -half], [half, half], [-half, half]];
    let mut clip = [[0.0; 4]; 4];
    let mut ndc = [[0.0; 2]; 4];
    for (index, point) in local.iter().enumerate() {
        clip[index] =
            projection.project(add(center, add(scale(right, point[0]), scale(up, point[1]))));
        if clip[index][3] <= 0.001 {
            return None;
        }
        ndc[index] = [
            clip[index][0] / clip[index][3],
            clip[index][1] / clip[index][3],
        ];
    }
    let mut area = 0.0;
    for index in 0..4 {
        let next = (index + 1) & 3;
        area += ndc[index][0] * ndc[next][1] - ndc[next][0] * ndc[index][1];
    }
    if area.abs() < 0.00001 {
        return None;
    }
    let orientation = if area > 0.0 { 1.0 } else { -1.0 };
    let mut planes = Vec::with_capacity(6);
    planes.push(ClipPlane([0.0, 0.0, 1.0, 0.0]));
    planes.push(ClipPlane([0.0, 0.0, -1.0, 1.0]));
    for index in 0..4 {
        let next = (index + 1) & 3;
        let dx = ndc[next][0] - ndc[index][0];
        let dy = ndc[next][1] - ndc[index][1];
        planes.push(ClipPlane([
            -dy * orientation,
            dx * orientation,
            0.0,
            (dy * ndc[index][0] - dx * ndc[index][1]) * orientation,
        ]));
    }
    Some(planes)
}

fn clip_polygon(polygon: &mut Vec<[f32; 4]>, planes: &[ClipPlane]) {
    let mut output = Vec::with_capacity(16);
    for plane in planes {
        if polygon.is_empty() {
            return;
        }
        output.clear();
        let mut previous = *polygon.last().unwrap();
        let mut previous_distance = plane_distance(*plane, previous);
        for current in polygon.iter().copied() {
            let current_distance = plane_distance(*plane, current);
            let previous_inside = previous_distance >= 0.0;
            let current_inside = current_distance >= 0.0;
            if previous_inside != current_inside {
                let denominator = previous_distance - current_distance;
                if denominator.abs() > f32::EPSILON {
                    let t = previous_distance / denominator;
                    output.push(lerp4(previous, current, t));
                }
            }
            if current_inside {
                output.push(current);
            }
            previous = current;
            previous_distance = current_distance;
        }
        core::mem::swap(polygon, &mut output);
    }
}

fn emit_polygon(polygon: &[[f32; 4]], batch: &mut Batch, used: &mut usize) -> Result<(), Error> {
    if polygon.len() < 3 {
        return Ok(());
    }
    for index in 1..polygon.len() - 1 {
        let output = [polygon[0], polygon[index], polygon[index + 1]];
        let offset = used.checked_mul(3).ok_or(Error::InvalidPortalRoomsScene)?;
        let destination = batch
            .vertices
            .get_mut(offset..offset + 3)
            .ok_or(Error::InvalidPortalRoomsScene)?;
        for (dst, clip) in destination.iter_mut().zip(output) {
            if clip[3] <= 0.0 {
                return Err(Error::InvalidPortalRoomsScene);
            }
            *dst = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
        }
        *used += 1;
    }
    Ok(())
}

fn portal_point(portal: Portal, half: f32, local: [f32; 3]) -> [f32; 3] {
    let right = normalize(cross(portal.up_hint, portal.normal));
    let up = normalize(cross(portal.normal, right));
    add(
        scale(portal.normal, half),
        add(scale(right, local[0]), add(scale(up, local[1]), scale(portal.normal, -local[2]))),
    )
}

fn plane_distance(plane: ClipPlane, point: [f32; 4]) -> f32 {
    plane.0[0] * point[0] + plane.0[1] * point[1] + plane.0[2] * point[2] + plane.0[3] * point[3]
}

fn lerp4(left: [f32; 4], right: [f32; 4], t: f32) -> [f32; 4] {
    let mut out = [0.0; 4];
    for index in 0..4 {
        out[index] = left[index] + (right[index] - left[index]) * t;
    }
    out
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(value: [f32; 3], scalar: f32) -> [f32; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn length(value: [f32; 3]) -> f32 {
    libm::sqrtf(dot(value, value))
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let divisor = length(value).max(f32::EPSILON);
    scale(value, 1.0 / divisor)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(Error::InvalidPortalRoomsScene)?;
    Ok(u16::from_le_bytes(raw.try_into().unwrap()))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(Error::InvalidPortalRoomsScene)?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, Error> {
    let value = f32::from_bits(read_u32(bytes, offset)?);
    value
        .is_finite()
        .then_some(value)
        .ok_or(Error::InvalidPortalRoomsScene)
}

fn read_f32s<const N: usize>(bytes: &[u8], offset: usize) -> Result<[f32; N], Error> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_f32(bytes, offset + index * 4)?;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{Engine, Spec};

    const ARTIFACT: &[u8] = include_bytes!("../../../picasso/simple-cube.trueos.intel.helio");

    #[test]
    fn artifact_preserves_six_portals_and_furnished_rooms() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        assert_eq!(engine.portal_count(), 6);
        assert_eq!(engine.object_count(), 74);
        let batches = engine.step(16.0 / 9.0).unwrap();
        assert!(batches.len() >= 20);
        assert!(
            batches
                .iter()
                .any(|batch| batch.vertices.iter().any(|vertex| vertex[0] <= 1.0))
        );
    }

    #[test]
    fn editor_overlay_is_toggleable_without_changing_retained_topology() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine.step(16.0 / 9.0).unwrap();
        let topology = engine
            .batches()
            .iter()
            .map(|batch| batch.indices.len())
            .collect::<Vec<_>>();
        assert!(engine.editor_mode());
        engine.toggle_editor_mode();
        engine.step(16.0 / 9.0).unwrap();
        assert!(!engine.editor_mode());
        assert_eq!(
            topology,
            engine
                .batches()
                .iter()
                .map(|batch| batch.indices.len())
                .collect::<Vec<_>>()
        );
    }
}
