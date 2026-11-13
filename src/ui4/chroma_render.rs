//! CHROMATIC's resident, native POINT_LIST color cube.
//!
//! Maxpix's dot groundwork is the native point rasterizer. Its app-owned VS
//! animates a tornado and its PS is fixed cyan, so neither binary expresses a
//! color cube. The kernel's embedded clip-position/UV shaders instead sample
//! a small immutable color atlas, keeping all 4,913 dots in one point draw.
use super::chroma_color::{LUT_EDGE, LUT_ENTRIES, identity_code, unpack_node};
use crate::intel::{gpgpu::GpgpuRgba8Surface, render};
use alloc::vec::Vec;

#[path = "chroma_geometry.rs"]
mod geometry;
use geometry::{ATLAS_HEIGHT, ATLAS_WIDTH, Projection, atlas_uv, ease_node};

const _: () = assert!(LUT_ENTRIES <= ATLAS_WIDTH * ATLAS_HEIGHT);
// apps/maxpix/scene.rs POINT_WIDTHS_RAW: the next step after 2px is 4px.
const POINT_WIDTH_PX: u32 = 4;

pub(super) struct ChromaPoints {
    mesh: render::ResidentTriangleMesh,
    atlas: render::ResidentSampledTexture,
    vertices: Vec<[f32; 5]>,
    positions: Vec<[f32; 3]>,
    targets: Vec<[f32; 3]>,
    unretired: bool,
}

impl ChromaPoints {
    pub(super) fn new() -> Result<Self, &'static str> {
        let pipeline = crate::intel::shader::clip_position3_uv_texture_pipeline();
        if pipeline.vs.code.is_empty() || pipeline.ps.code.is_empty() {
            return Err("chroma-point-shader-unavailable");
        }
        let mut vertices = Vec::with_capacity(LUT_ENTRIES);
        let mut positions = Vec::with_capacity(LUT_ENTRIES);
        let mut atlas_bytes = alloc::vec![0u8; ATLAS_WIDTH * ATLAS_HEIGHT * 4];
        for r in 0..LUT_EDGE {
            for g in 0..LUT_EDGE {
                for b in 0..LUT_EDGE {
                    let index = positions.len();
                    positions.push([r, g, b].map(|axis| identity_code(axis) as f32 / 1023.0));
                    let uv = atlas_uv(index);
                    vertices.push([0.0, 0.0, 0.5, uv[0], uv[1]]);
                    let encode = |axis: usize| ((identity_code(axis) * 255 + 511) / 1023) as u8;
                    atlas_bytes[index * 4..index * 4 + 4].copy_from_slice(&[
                        encode(r),
                        encode(g),
                        encode(b),
                        255,
                    ]);
                }
            }
        }
        // Identity/input colors are intentional: Pipe A grades the dot once.
        // Supplying already-graded RGB here would grade it a second time.
        let atlas = render::create_resident_sampled_rgba8_texture(
            ATLAS_WIDTH as u32,
            ATLAS_HEIGHT as u32,
            (ATLAS_WIDTH * 4) as u32,
            0,
            &atlas_bytes,
        )?;
        let indices = (0..LUT_ENTRIES as u32).collect::<Vec<_>>();
        let mesh = match render::create_resident_textured_indexed_mesh(
            &vertices,
            &indices,
            render::ResidentScenePrimitiveTopology::PointList,
        ) {
            Ok(mesh) => mesh,
            Err(reason) => {
                let _ = render::release_resident_sampled_texture(&atlas);
                return Err(reason);
            }
        };
        let targets = positions.clone();
        Ok(Self {
            mesh,
            atlas,
            vertices,
            positions,
            targets,
            unretired: false,
        })
    }

    /// Hardware order: red outermost, blue fastest, packed R10G10B10.
    pub(super) fn set_table(&mut self, entries: &[u32; LUT_ENTRIES]) {
        for (target, &entry) in self.targets.iter_mut().zip(entries) {
            *target = unpack_node(entry);
        }
    }

    /// Ease node positions toward their new RGB outputs; stationary cubes
    /// need no repeated submission. Elapsed time is bounded after stalls.
    pub(super) fn animate(&mut self, elapsed_seconds: f32) -> bool {
        let step = 1.0 - libm::expf(-elapsed_seconds.clamp(0.0, 0.1) * 15.0);
        let mut moving = false;
        for (position, &target) in self.positions.iter_mut().zip(&self.targets) {
            moving |= ease_node(position, target, step);
        }
        moving
    }

    pub(super) fn is_poisoned(&self) -> bool {
        self.unretired
    }

    /// Render directly into a leased UI4 destination. `None` preserves a
    /// BCS0-prepared transparent target and its cached controls. The caller
    /// publishes only the returned exact producer-release fence.
    pub(super) fn render(
        &mut self,
        destination: GpgpuRgba8Surface,
        cube_width: u32,
        yaw: f32,
        pitch: f32,
        clear_rgba: Option<[u8; 4]>,
    ) -> Result<render::ResidentSceneFrameResult, &'static str> {
        if self.unretired {
            return Err("chroma-point-retirement-unproven");
        }
        if !yaw.is_finite()
            || !pitch.is_finite()
            || destination.width == 0
            || destination.height == 0
            || cube_width == 0
            || cube_width > destination.width
        {
            return Err("chroma-point-projection");
        }
        let projection = Projection::new(
            destination.width,
            destination.height,
            cube_width,
            [
                libm::sinf(yaw),
                libm::cosf(yaw),
                libm::sinf(pitch),
                libm::cosf(pitch),
            ],
        );
        for (index, (vertex, &point)) in self.vertices.iter_mut().zip(&self.positions).enumerate() {
            *vertex = projection.vertex(index, point);
        }
        // Opaque points need only far-to-near ordering. This avoids a full
        // output-sized depth clear/allocation for fewer than 5,000 dots, and
        // preserves the BCS0-painted controls when loading target color.
        self.vertices.sort_unstable_by(|a, b| b[2].total_cmp(&a[2]));
        render::update_resident_textured_vertices(&self.mesh, &self.vertices)?;
        let draws = [render::ResidentSceneDraw {
            mesh: &self.mesh,
            depth_flags: None,
            rgba: [255; 4],
            sampled_texture: Some(&self.atlas),
            fragment_contract: render::ResidentSceneFragmentContract::ClipPosition3UvTextureCopy,
            viewport_translation_px: [0.0; 2],
            topology: render::ResidentScenePrimitiveTopology::PointList,
            point_width_px: POINT_WIDTH_PX,
            line_width_px: 0.0,
            clip_adjacency: false,
        }];
        self.unretired = true;
        let result =
            match render::render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
                &draws,
                clear_rgba,
                destination,
                false,
            ) {
                Ok(result) => result,
                // Render0's admission rejection proves this call did not
                // submit. A mode change can revoke the fullscreen extent
                // between the service's size check and this admission.
                Err(
                    reason @ ("render-busy"
                    | "render-storage-busy"
                    | "resident-scene-capture-shape"),
                ) => {
                    self.unretired = false;
                    return Err(reason);
                }
                Err(reason) => return Err(reason),
            };
        if result.frame_complete && result.release_fence.is_some() {
            self.unretired = false;
        }
        Ok(result)
    }
}

impl Drop for ChromaPoints {
    fn drop(&mut self) {
        if !self.unretired {
            let _ = render::release_resident_triangle_mesh(&self.mesh);
            let _ = render::release_resident_sampled_texture(&self.atlas);
        }
        // An uncertain submission retains its resident sources. The raw
        // resource handles have no implicit deallocation behind this guard.
    }
}
