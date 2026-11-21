//! Draw resolved outline spans through Microfont's stored-quad R8 shader.

use alloc::vec::Vec;
use microemoji::{
    Glyph,
    raster_mask::{self, SUBPIXELS, Scratch},
};

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

pub(super) fn render(
    glyph: Glyph<'_>,
    width: usize,
    height: usize,
    scratch: &mut Scratch<'_>,
    pixels: &mut [u8],
) -> Result<(), Error> {
    let Some(device) = intel::claimed_device() else {
        return Err(Error::Unavailable);
    };
    if !matches!(device.device_id, 0x4680 | 0xa780)
        || glyph
            .layers()
            .any(|layer| !matches!(layer.color()[3], 0 | 255))
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

    let target_width = sample_extent(width)?;
    let target_height = sample_extent(height)?;
    if width.checked_mul(height) != Some(pixels.len()) {
        return Err(Error::Failed("microemoji-native-mask-shape"));
    }
    let mut quarantined = NATIVE.try_lock().ok_or(Error::Retry)?;
    if *quarantined {
        return Err(Error::Unavailable);
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    raster_mask::scan_layers(glyph, width, height, scratch, |alpha, span| {
        if alpha == 255 {
            let base = vertices.len() as u32;
            let left = span.start as f32;
            let right = span.end as f32;
            let top = span.y as f32;
            let bottom = top + 1.0;
            vertices.extend_from_slice(&[
                [left, top, -1.0],
                [right, top, -1.0],
                [right, bottom, -1.0],
                [left, bottom, -1.0],
            ]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    })
    .map_err(|_| Error::Failed("microemoji-native-outline"))?;

    if indices.is_empty() {
        pixels.fill(0);
        return Ok(());
    }

    let mesh = render::create_resident_triangle_mesh(&vertices, &indices).map_err(classify)?;
    drop(vertices);
    drop(indices);
    let mut constants = None;
    let mut mask = None;
    let mut inputs_retired = true;
    let result = (|| {
        let constants =
            constants.insert(render::allocate_resident_render_buffer(64).map_err(classify)?);
        let mask = mask.insert(
            render::create_resident_coverage_mask(target_width, target_height).map_err(classify)?,
        );
        // Zero style selects the solid quad path. Coordinates are already in
        // target pixels; the only transform is the viewport's pixel-to-clip scale.
        let mut words = [0.0f32; 16];
        words[12..].copy_from_slice(&[
            1.0,
            1.0,
            1.0 / target_width as f32,
            1.0 / target_height as f32,
        ]);
        for (index, word) in words.into_iter().enumerate() {
            if !constants.write(index * 4, &word.to_le_bytes()) {
                return Err(Error::Failed("microemoji-native-constants"));
            }
        }
        constants.flush();

        let draw = ResidentSceneDraw {
            mesh: &mesh,
            depth_flags: None,
            rgba: [255; 4],
            sampled_texture: None,
            fragment_contract: ResidentSceneFragmentContract::MicrofontMask(constants.gpu_base()),
            viewport_translation_px: [0.0; 2],
            topology: ResidentScenePrimitiveTopology::TriangleList,
            point_width_px: 0,
            line_width_px: 0.0,
            clip_adjacency: false,
        };
        // Encoder Err exits occur before acceptance or after earlier batches
        // retire. An accepted draw without retirement instead returns a frame.
        let frame = render::render_resident_coverage_mask(&[draw], mask).map_err(classify)?;
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
        let samples = (SUBPIXELS * SUBPIXELS) as u32;
        for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
            for (x, value) in row.iter_mut().enumerate() {
                let mut sum = 0u32;
                for sy in 0..SUBPIXELS {
                    let offset = (y * SUBPIXELS + sy) * pitch + x * SUBPIXELS;
                    for sx in 0..SUBPIXELS {
                        // SAFETY: Render0's matching release fence retired the
                        // R8 writer. All samples lie inside the live target.
                        sum +=
                            u32::from(unsafe { core::ptr::read_volatile(source.add(offset + sx)) });
                    }
                }
                *value = ((sum + samples / 2) / samples) as u8;
            }
        }
        Ok(())
    })();

    // The mask owns its quarantine. Mesh and constants have explicit release,
    // so leave their GPU mappings pinned whenever the frame did not retire.
    let mask_released = mask.is_none_or(render::release_resident_coverage_mask);
    let constants_released = inputs_retired
        && constants
            .as_ref()
            .is_none_or(render::release_resident_render_buffer);
    let mesh_released = inputs_retired && render::release_resident_triangle_mesh(&mesh);
    if !(mask_released && constants_released && mesh_released) {
        *quarantined = true;
        crate::log_error!(target: "microemoji-mask";
            "microemoji-mask: native cleanup mask={} constants={} mesh={} result={:?}\n",
            mask_released, constants_released, mesh_released, result);
        return Err(Error::Unavailable);
    }
    result
}

fn sample_extent(extent: usize) -> Result<u32, Error> {
    if extent == 0 {
        return Err(Error::Failed("microemoji-native-mask-shape"));
    }
    extent
        .checked_mul(SUBPIXELS)
        .and_then(|samples| u32::try_from(samples).ok())
        .filter(|&samples| samples <= MAX_TARGET_EXTENT)
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
