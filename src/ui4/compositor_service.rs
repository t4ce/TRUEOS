//! Permanent kernel UI4 compositor service.
//!
//! Producers own frames and windows. This service owns the broker snapshot,
//! per-plane damage history, software-cursor composition, and the atomic plane
//! surface-flip batch. It intentionally creates no application windows.

use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::{
    DamageRect, FrameHandle, FramePoolError, FrameReadLease, FrameRgbaView, OutputId, WindowId,
    WindowPlacement, WindowSnapshot, acknowledge_window_frame, acquire_published_frame,
    published_rgba_view, release_published_frame, visible_windows_for_output,
};

const COMPOSITION_PERIOD_MS: u64 = 16;
const MAX_COMPOSITION_WINDOWS: usize = super::window_broker::MAX_ACTIVE_WINDOWS;
const PRESENT_FAILURE_LOG_INTERVAL: u32 = 600;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui4CompositorError {
    Frame(FramePoolError),
    PresentFailed,
}

impl From<FramePoolError> for Ui4CompositorError {
    fn from(error: FramePoolError) -> Self {
        Self::Frame(error)
    }
}

struct Runtime {
    composition: CompositionState,
}

#[derive(Copy, Clone)]
struct CompositionState {
    primary: PlaneCompositionState,
    alpha: PlaneCompositionState,
    solara: PlaneCompositionState,
    draw3d: PlaneCompositionState,
}

#[derive(Copy, Clone)]
struct PlaneCompositionState {
    initialized: bool,
    windows: [Option<CompositionWindowStamp>; MAX_COMPOSITION_WINDOWS],
}

#[derive(Copy, Clone)]
enum CompositionTarget {
    Primary,
    Overlay(usize),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CompositionWindowStamp {
    id: WindowId,
    frame: FrameHandle,
    publish_serial: u64,
    placement: WindowPlacement,
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_compositor_service_task() {
    crate::log_info!(
        target: "ui4";
        "ui4 compositor carrier online placement=ap1-ui-core expected_slot={} current_slot={}\n",
        crate::workers::AP1_UI_SERVICE_SLOT,
        crate::percpu::current_slot()
    );
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut runtime = initialize();

    crate::log_info!(
        target: "ui4";
        "ui4 compositor live application_windows=broker-owned composition_ms={} planes=slot0+slot1+slot2+slot3 interaction=independent-slot4-service input=ui4-owner-queues plane_contract=bootstrap-immutable-rgba8\n",
        COMPOSITION_PERIOD_MS,
    );

    let mut consecutive_failures = 0u32;
    loop {
        match present_composition(&mut runtime) {
            Ok(()) => {
                if consecutive_failures != 0 {
                    crate::log_info!(
                        target: "ui4";
                        "ui4 compositor recovered failures={} action=continue\n",
                        consecutive_failures
                    );
                    consecutive_failures = 0;
                }
            }
            Err(error) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if consecutive_failures <= 3
                    || consecutive_failures.is_multiple_of(PRESENT_FAILURE_LOG_INTERVAL)
                {
                    crate::log_error!(
                        target: "ui4";
                        "ui4 compositor present failed error={:?} consecutive={} action=retry\n",
                        error,
                        consecutive_failures
                    );
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(COMPOSITION_PERIOD_MS)).await;
    }
}

fn initialize() -> Runtime {
    Runtime {
        composition: CompositionState {
            primary: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            alpha: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            solara: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            draw3d: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
        },
    }
}

fn present_composition(runtime: &mut Runtime) -> Result<(), Ui4CompositorError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    super::advance_window_close_transitions();
    let windows = visible_windows_for_output(output);
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        // `ui4` deliberately has no narrower LogArea, so log-os routes this
        // admission failure to [global] [warn].
        crate::log_warn!(
            target: "ui4";
            "ui4 compositor visible-window soft-cap exceeded output={} requested={} cap={} action=reject-composition\n",
            output.name(),
            windows.len(),
            MAX_COMPOSITION_WINDOWS,
        );
        return Err(Ui4CompositorError::PresentFailed);
    }
    if windows.iter().any(|window| {
        let slot = window.plane.slot();
        slot != super::PRIMARY_PLANE_SLOT
            && slot != super::ALPHA_OVERLAY_PLANE_SLOT
            && slot != super::RGB_OVERLAY_PLANE_SLOT_2
            && slot != super::RGB_OVERLAY_PLANE_SLOT_3
    }) {
        return Err(Ui4CompositorError::PresentFailed);
    }

    if !crate::intel::begin_ui4_plane_surface_flip_batch() {
        return Err(Ui4CompositorError::PresentFailed);
    }
    let present_result = (|| {
        present_plane_composition(
            &mut runtime.composition.primary,
            &windows,
            CompositionTarget::Primary,
        )?;
        present_plane_composition(
            &mut runtime.composition.alpha,
            &windows,
            CompositionTarget::Overlay(super::ALPHA_OVERLAY_PLANE_SLOT),
        )?;
        present_plane_composition(
            &mut runtime.composition.solara,
            &windows,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2),
        )?;
        present_plane_composition(
            &mut runtime.composition.draw3d,
            &windows,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3),
        )
    })();
    let flips_committed = crate::intel::finish_ui4_plane_surface_flip_batch();
    present_result?;
    if !flips_committed {
        return Err(Ui4CompositorError::PresentFailed);
    }
    super::screenshot::capture_compositor_frame(&windows);
    Ok(())
}

fn present_plane_composition(
    state: &mut PlaneCompositionState,
    all_windows: &[WindowSnapshot],
    target: CompositionTarget,
) -> Result<(), Ui4CompositorError> {
    let plane_slot = match target {
        CompositionTarget::Primary => super::PRIMARY_PLANE_SLOT,
        CompositionTarget::Overlay(slot) => slot,
    };
    let windows: Vec<WindowSnapshot> = all_windows
        .iter()
        .copied()
        .filter(|window| window.plane.slot() == plane_slot)
        .collect();
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        return Err(Ui4CompositorError::PresentFailed);
    }
    if windows.is_empty() && !state.initialized {
        return Ok(());
    }

    let mut next_windows = [None; MAX_COMPOSITION_WINDOWS];
    for (slot, window) in windows.iter().enumerate() {
        next_windows[slot] = Some(CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement: window.placement,
        });
    }

    let mut changed = [false; MAX_COMPOSITION_WINDOWS];
    let mut content_damage = [false; MAX_COMPOSITION_WINDOWS];
    let mut composition_changed = !state.initialized;
    let (output_width, output_height) = crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
    let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
    for (slot, current) in next_windows.iter().flatten().enumerate() {
        let previous = state
            .windows
            .iter()
            .flatten()
            .find(|previous| previous.id == current.id);
        changed[slot] = !state.initialized || previous != Some(current);
        composition_changed |= changed[slot];
        match previous {
            None => {
                add_placement_damage(&mut damage, current.placement, output_width, output_height)
            }
            Some(previous) if previous.placement != current.placement => {
                add_placement_damage(&mut damage, previous.placement, output_width, output_height);
                add_placement_damage(&mut damage, current.placement, output_width, output_height);
            }
            Some(previous) if previous.frame != current.frame => {
                add_placement_damage(&mut damage, current.placement, output_width, output_height)
            }
            Some(previous) if previous.publish_serial != current.publish_serial => {
                content_damage[slot] = true;
            }
            Some(_) => {}
        }
    }
    for previous in state.windows.iter().flatten() {
        if !next_windows
            .iter()
            .flatten()
            .any(|current| current.id == previous.id)
        {
            composition_changed = true;
            add_placement_damage(&mut damage, previous.placement, output_width, output_height);
        }
    }
    if !state.initialized {
        damage = crate::intel::CompositionDamageRegion::from_rect(
            crate::intel::CompositionDamageRect::new(0, 0, output_width, output_height),
        );
    }

    if composition_changed {
        let mut leases = Vec::with_capacity(windows.len());
        for window in &windows {
            match acquire_published_frame(window.frame) {
                Ok(lease) => leases.push(lease),
                Err(error) => {
                    release_leases(&leases);
                    return Err(error.into());
                }
            }
        }

        let result = (|| {
            let views: Vec<FrameRgbaView> = leases
                .iter()
                .copied()
                .map(published_rgba_view)
                .collect::<Result<_, _>>()?;
            for (slot, (window, view)) in windows.iter().zip(views.iter()).enumerate() {
                if !content_damage[slot] {
                    continue;
                }
                let local = window
                    .damage
                    .unwrap_or_else(|| super::DamageRegion::from_rect(DamageRect::FULL));
                add_mapped_window_damage(
                    &mut damage,
                    local,
                    *view,
                    window.placement,
                    output_width,
                    output_height,
                );
            }
            for (slot, view) in views.iter().enumerate() {
                if changed[slot] {
                    crate::intel::dma_flush(view.virt, view.byte_len);
                }
            }
            let pixels: Vec<&[u8]> = views
                .iter()
                .map(|view| unsafe {
                    core::slice::from_raw_parts(
                        view.virt.cast_const(),
                        (view.pitch as usize).saturating_mul(view.height as usize),
                    )
                })
                .collect();
            let tiles: Vec<_> = windows
                .iter()
                .zip(views.iter())
                .zip(pixels.iter())
                .map(|((window, view), pixels)| crate::intel::RgbaOverlayTile {
                    x: window.placement.x.max(0) as u32,
                    y: window.placement.y.max(0) as u32,
                    width: window.placement.width,
                    height: window.placement.height,
                    source_width: view.width,
                    source_height: view.height,
                    pitch_bytes: view.pitch as usize,
                    pixels,
                    opacity: window.placement.opacity,
                    expected_rgba: None,
                })
                .collect();
            if !damage.is_empty() {
                let presented = match target {
                    CompositionTarget::Primary => {
                        crate::intel::present_premultiplied_rgba_primary_tiles_damage(
                            &tiles,
                            damage,
                            "ui4-compositor-primary",
                        )
                    }
                    CompositionTarget::Overlay(slot) => {
                        let reason = match slot {
                            super::ALPHA_OVERLAY_PLANE_SLOT => "ui4-alpha-slot1",
                            super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-solara-slot2",
                            super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-draw3d-slot3",
                            _ => "ui4-overlay",
                        };
                        crate::intel::present_premultiplied_rgba_overlay_tiles_on_slot_damage(
                            slot, &tiles, damage, reason,
                        )
                    }
                };
                if !presented {
                    return Err(Ui4CompositorError::PresentFailed);
                }
            }
            Ok(())
        })();
        release_leases(&leases);
        result?;
        for window in &windows {
            let _ = acknowledge_window_frame(window.id, window.publish_serial);
        }
        state.initialized = true;
        state.windows = next_windows;
    }
    Ok(())
}

fn add_placement_damage(
    region: &mut crate::intel::CompositionDamageRegion,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) {
    if let Some(rect) = clipped_output_rect(
        i64::from(placement.x),
        i64::from(placement.y),
        i64::from(placement.x).saturating_add(i64::from(placement.width)),
        i64::from(placement.y).saturating_add(i64::from(placement.height)),
        output_width,
        output_height,
    ) {
        region.add(rect);
    }
}

fn add_mapped_window_damage(
    output: &mut crate::intel::CompositionDamageRegion,
    local: super::DamageRegion,
    view: FrameRgbaView,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) {
    for rect in local.rects() {
        if let Some(rect) = map_window_damage_rect(
            *rect,
            view.width,
            view.height,
            placement,
            output_width,
            output_height,
        ) {
            output.add(rect);
        }
    }
}

fn map_window_damage_rect(
    local: DamageRect,
    source_width: u32,
    source_height: u32,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) -> Option<crate::intel::CompositionDamageRect> {
    if source_width == 0 || source_height == 0 || placement.width == 0 || placement.height == 0 {
        return None;
    }
    let local = local.intersection(DamageRect::new(0, 0, source_width, source_height))?;
    let source_right = local.x.saturating_add(local.width);
    let source_bottom = local.y.saturating_add(local.height);
    let destination_left = scale_floor(local.x, placement.width, source_width);
    let destination_top = scale_floor(local.y, placement.height, source_height);
    let destination_right = scale_ceil(source_right, placement.width, source_width);
    let destination_bottom = scale_ceil(source_bottom, placement.height, source_height);
    clipped_output_rect(
        i64::from(placement.x).saturating_add(i64::from(destination_left)),
        i64::from(placement.y).saturating_add(i64::from(destination_top)),
        i64::from(placement.x).saturating_add(i64::from(destination_right)),
        i64::from(placement.y).saturating_add(i64::from(destination_bottom)),
        output_width,
        output_height,
    )
}

fn scale_floor(coordinate: u32, destination_extent: u32, source_extent: u32) -> u32 {
    (u64::from(coordinate).saturating_mul(u64::from(destination_extent)) / u64::from(source_extent))
        .min(u64::from(u32::MAX)) as u32
}

fn scale_ceil(coordinate: u32, destination_extent: u32, source_extent: u32) -> u32 {
    let numerator = u64::from(coordinate).saturating_mul(u64::from(destination_extent));
    numerator
        .saturating_add(u64::from(source_extent).saturating_sub(1))
        .checked_div(u64::from(source_extent))
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32
}

fn clipped_output_rect(
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    output_width: u32,
    output_height: u32,
) -> Option<crate::intel::CompositionDamageRect> {
    let left = left.clamp(0, i64::from(output_width));
    let top = top.clamp(0, i64::from(output_height));
    let right = right.clamp(0, i64::from(output_width));
    let bottom = bottom.clamp(0, i64::from(output_height));
    (right > left && bottom > top).then(|| {
        crate::intel::CompositionDamageRect::new(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

fn release_leases(leases: &[FrameReadLease]) {
    for lease in leases {
        let _ = release_published_frame(*lease);
    }
}

#[cfg(test)]
mod damage_tests {
    use super::*;

    #[test]
    fn producer_damage_scales_outward() {
        let placement = WindowPlacement {
            x: 10,
            y: 20,
            width: 33,
            height: 33,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        assert_eq!(
            map_window_damage_rect(DamageRect::new(1, 1, 1, 1), 100, 100, placement, 200, 200),
            Some(crate::intel::CompositionDamageRect::new(10, 20, 1, 1))
        );
    }

    #[test]
    fn producer_damage_clips_negative_placement() {
        let placement = WindowPlacement {
            x: -25,
            y: -10,
            width: 100,
            height: 80,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        assert_eq!(
            map_window_damage_rect(DamageRect::FULL, 100, 80, placement, 100, 100),
            Some(crate::intel::CompositionDamageRect::new(0, 0, 75, 70))
        );
    }
}
