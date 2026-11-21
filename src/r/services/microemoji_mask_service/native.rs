//! Draw closed contours as native AA lines through Microfont's R8 shader.

use alloc::vec::Vec;
use microemoji::raster_stroke::{Outline, WIDTH_PX};

use crate::intel::{
    self, DmaFlushRows,
    render::{
        self, ResidentSceneDraw, ResidentSceneFragmentContract, ResidentScenePrimitiveTopology,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Error {
    Unavailable,
    Retry,
    Failed(&'static str),
}

// Serialize cold native requests. An unretired submission pins its backing;
// stop issuing work through this provider so failed retries cannot pin more.
static NATIVE: spin::Mutex<bool> = spin::Mutex::new(false);

const MAX_TARGET_EXTENT: u32 = 4096;
const TOPOLOGY: ResidentScenePrimitiveTopology = ResidentScenePrimitiveTopology::LineStripAdj;

pub(super) fn render(
    outline: &Outline<'_>,
    width: usize,
    height: usize,
    pixels: &mut [u8],
) -> Result<(), Error> {
    let Some(device) = intel::claimed_device() else {
        return Err(Error::Unavailable);
    };
    if !matches!(device.device_id, 0x4680 | 0xa780)
        || outline
            .contours()
            .any(|contour| !matches!(contour.alpha(), 0 | 255))
    {
        return Err(Error::Unavailable);
    }
    // Intel boot publishes the GuC and Render0 state before the scheduler
    // starts the blocking lanes. Missing state here has no later retry path.
    if !intel::guc_submission_ready()
        || render::warm_state().is_none_or(|warm| warm.vertex_len == 0)
    {
        return Err(Error::Unavailable);
    }

    let target_width = target_extent(width)?;
    let target_height = target_extent(height)?;
    if width.checked_mul(height) != Some(pixels.len()) {
        return Err(Error::Failed("microemoji-native-mask-shape"));
    }
    let mut quarantined = NATIVE.try_lock().ok_or(Error::Retry)?;
    if *quarantined {
        return Err(Error::Unavailable);
    }

    let mut meshes = Vec::new();
    let mut constants = None;
    let mut mask = None;
    let mut inputs_retired = true;
    let result = (|| {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for contour in outline.contours().filter(|contour| contour.alpha() == 255) {
            let mut edges = contour.edges();
            let Some(first) = edges.next() else { continue };
            vertices.clear();
            vertices.push(vertex(first.from));
            let mut end = first.to;
            for edge in edges {
                debug_assert_eq!(end, edge.from);
                vertices.push(vertex(edge.from));
                end = edge.to;
            }
            if vertices.len() < 2 {
                continue;
            }
            debug_assert_eq!(end, first.from);
            // The first and last vertices are adjacency-only. The second
            // appearance of the start index closes the visible strip.
            indices.clear();
            let count = u32::try_from(vertices.len())
                .map_err(|_| Error::Failed("microemoji-native-outline-size"))?;
            indices.push(count - 1);
            indices.extend(0..count);
            indices.extend([0, 1]);
            meshes.push(
                render::create_resident_indexed_mesh(&vertices, &indices, TOPOLOGY)
                    .map_err(classify)?,
            );
        }
        if meshes.is_empty() {
            pixels.fill(0);
            return Ok(());
        }
        drop(vertices);
        drop(indices);

        let constants =
            constants.insert(render::allocate_resident_render_buffer(64).map_err(classify)?);
        let mask = mask.insert(
            render::create_resident_coverage_mask(target_width, target_height).map_err(classify)?,
        );
        // A positive half-width selects native AA lines. Closed contours
        // have no end caps. Coordinates are already in target pixels.
        let mut words = [0.0f32; 16];
        words[8] = WIDTH_PX * 0.5;
        words[12..].copy_from_slice(&[
            1.0,
            WIDTH_PX,
            1.0 / target_width as f32,
            1.0 / target_height as f32,
        ]);
        for (index, word) in words.into_iter().enumerate() {
            if !constants.write(index * 4, &word.to_le_bytes()) {
                return Err(Error::Failed("microemoji-native-constants"));
            }
        }
        constants.flush();

        let draws: Vec<_> = meshes
            .iter()
            .map(|mesh| ResidentSceneDraw {
                mesh,
                depth_flags: None,
                rgba: [255; 4],
                sampled_texture: None,
                fragment_contract: ResidentSceneFragmentContract::MicrofontMask(
                    constants.gpu_base(),
                ),
                viewport_translation_px: [0.0; 2],
                topology: TOPOLOGY,
                point_width_px: 0,
                line_width_px: WIDTH_PX,
                clip_adjacency: true,
            })
            .collect();
        // Encoder Err exits occur before acceptance or after earlier batches
        // retire. An accepted draw without retirement instead returns a frame.
        let frame = render::render_resident_coverage_mask(&draws, mask).map_err(classify)?;
        inputs_retired = frame.frame_complete;
        if let Some(reason) = frame.completion_error() {
            return Err(Error::Failed(reason));
        }
        let surface = mask.surface();
        if !frame
            .release_fence
            .is_some_and(|fence| fence.matches(surface.phys, surface.bytes))
        {
            return Err(Error::Failed("microemoji-native-fence"));
        }

        let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
        let pitch = surface.pitch_bytes as usize;
        if !intel::dma_flush_strided_row_spans(&[DmaFlushRows::new(
            source,
            target_width as usize,
            pitch,
            target_height as usize,
        )]) {
            return Err(Error::Failed("microemoji-native-readback"));
        }
        for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
            for (x, value) in row.iter_mut().enumerate() {
                // SAFETY: Render0's matching release fence retired the
                // R8 writer. All pixels lie inside the live target.
                *value = unsafe { core::ptr::read_volatile(source.add(y * pitch + x)) };
            }
        }
        Ok(())
    })();

    // The mask owns its quarantine. Meshes and constants have explicit release,
    // so leave their GPU mappings pinned whenever the frame did not retire.
    let mask_released = mask.is_none_or(render::release_resident_coverage_mask);
    let constants_released = inputs_retired
        && constants
            .as_ref()
            .is_none_or(render::release_resident_render_buffer);
    let mut meshes_released = inputs_retired;
    if inputs_retired {
        for mesh in &meshes {
            meshes_released &= render::release_resident_font_mesh(mesh);
        }
    }
    if !(mask_released && constants_released && meshes_released) {
        *quarantined = true;
        crate::log_error!(target: "microemoji-mask";
            "microemoji-mask: native cleanup mask={} constants={} meshes={} result={:?}\n",
            mask_released, constants_released, meshes_released, result);
        return Err(Error::Unavailable);
    }
    result
}

fn vertex([x, y]: [f64; 2]) -> [f32; 3] {
    [x as f32, y as f32, 0.5]
}

fn target_extent(extent: usize) -> Result<u32, Error> {
    if extent == 0 {
        return Err(Error::Failed("microemoji-native-mask-shape"));
    }
    u32::try_from(extent)
        .ok()
        .filter(|&extent| extent <= MAX_TARGET_EXTENT)
        .ok_or(Error::Unavailable)
}

fn classify(reason: &'static str) -> Error {
    match reason {
        "no-device"
        | "render-boot-not-ready"
        | "render-map"
        | "render-ppgtt"
        | "microfont-coverage-device" => Error::Unavailable,
        "render-busy" | "render-storage-busy" | "in-flight" => Error::Retry,
        _ => Error::Failed(reason),
    }
}
