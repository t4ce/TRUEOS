//! Permanent kernel UI4 compositor service.
//!
//! Producers own frames and windows. This service owns the broker snapshot,
//! per-plane damage history, software-cursor composition, and the atomic plane
//! surface-flip batch. It intentionally creates no application windows.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::{
    DamageRect, FrameContent, FrameHandle, FramePoolError, FrameReadLease, FrameRgbaView, OutputId,
    WindowId, WindowPlacement, WindowSnapshot, acknowledge_window_frame, acquire_published_frame,
    frame_snapshot, published_rgba_view, release_published_frame, retain_published_frame,
    visible_windows_for_output,
};

const COMPOSITION_PERIOD_MS: u64 = 16;
const PENDING_POLL_PERIOD_MS: u64 = 1;
const UI4_ISOLATED_ASYNC_GUC_COMPOSITOR_ENABLED: bool = true;
const MAX_COMPOSITION_WINDOWS: usize = super::window_broker::MAX_ACTIVE_WINDOWS;
const PRESENT_FAILURE_LOG_INTERVAL: u32 = 600;
static DRAW3D_TRIPLE_DIRECT_SCANOUT_LOGGED: AtomicBool = AtomicBool::new(false);

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
    pending: Option<PendingFrame>,
    immediate_rescan: bool,
    retired_frames: u64,
    /// Exact producer buffers currently owned by overlay scanout. A lease is
    /// replaced only after the corresponding new SURFLIVE value is observed.
    live_direct: [Option<FrameReadLease>; 4],
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

#[derive(Copy, Clone)]
struct PlanePlan {
    target: CompositionTarget,
    changed: bool,
    next_windows: [Option<CompositionWindowStamp>; MAX_COMPOSITION_WINDOWS],
    damage: crate::intel::CompositionDamageRegion,
}

impl PlanePlan {
    const fn empty(target: CompositionTarget) -> Self {
        Self {
            target,
            changed: false,
            next_windows: [None; MAX_COMPOSITION_WINDOWS],
            damage: crate::intel::CompositionDamageRegion::EMPTY,
        }
    }
}

struct PendingFrame {
    windows: Vec<WindowSnapshot>,
    leases: Vec<FrameReadLease>,
    plans: [PlanePlan; 4],
    next_plane: usize,
    active: Option<crate::intel::Ui4AsyncComposition>,
    completed: Vec<crate::intel::Ui4AsyncComposition>,
    direct_leases: [Option<FrameReadLease>; 4],
    flip_submitted: bool,
    started_ns: u64,
}

enum DriveResult {
    Pending,
    Complete,
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
        "ui4 compositor rewire live consumer=draw3d-kernel-service-only composition_ms={} active_plane=slot3 present=triple-direct-import per_frame_guc_composition=off per_frame_display_flip=on slot0=disabled slot1=disabled slot2=disabled slot4=disabled input=disabled screenshots=disabled previews=disabled video=disabled\n",
        COMPOSITION_PERIOD_MS,
    );

    let mut consecutive_failures = 0u32;
    loop {
        let result = if UI4_ISOLATED_ASYNC_GUC_COMPOSITOR_ENABLED {
            advance_async_composition(&mut runtime)
        } else {
            present_composition(&mut runtime)
        };
        match result {
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

        let immediate_rescan = runtime.immediate_rescan;
        runtime.immediate_rescan = false;
        Timer::after(EmbassyDuration::from_millis(
            if runtime.pending.is_some() || immediate_rescan {
                PENDING_POLL_PERIOD_MS
            } else {
                COMPOSITION_PERIOD_MS
            },
        ))
        .await;
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
        pending: None,
        immediate_rescan: false,
        retired_frames: 0,
        live_direct: [None; 4],
    }
}

fn advance_async_composition(runtime: &mut Runtime) -> Result<(), Ui4CompositorError> {
    if runtime.pending.is_none() {
        runtime.pending = prepare_async_frame(runtime)?;
    }
    let Some(mut pending) = runtime.pending.take() else {
        return Ok(());
    };
    match drive_async_frame(runtime, &mut pending) {
        Ok(DriveResult::Pending) => {
            runtime.pending = Some(pending);
            Ok(())
        }
        Ok(DriveResult::Complete) => {
            // A streaming producer may already have published while this
            // frame waited for GPU completion and SURFLIVE. Re-snapshot on
            // the next executor turn instead of inserting another 16 ms
            // composition interval after every successful retirement.
            runtime.immediate_rescan = true;
            Ok(())
        }
        Err(error) => {
            crate::intel::cancel_ui4_plane_surface_flip_batch();
            settle_failed_direct_leases(runtime, &mut pending);
            release_leases(&pending.leases);
            Err(error)
        }
    }
}

fn prepare_async_frame(runtime: &Runtime) -> Result<Option<PendingFrame>, Ui4CompositorError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    super::advance_window_close_transitions();
    let windows = visible_windows_for_output(output);
    validate_window_snapshot(output, &windows)?;

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
    let views: Vec<FrameRgbaView> = match leases
        .iter()
        .copied()
        .map(published_rgba_view)
        .collect::<Result<_, _>>()
    {
        Ok(views) => views,
        Err(error) => {
            release_leases(&leases);
            return Err(error.into());
        }
    };
    let plans = [
        build_plane_plan(
            &runtime.composition.primary,
            &windows,
            &views,
            CompositionTarget::Primary,
        ),
        build_plane_plan(
            &runtime.composition.alpha,
            &windows,
            &views,
            CompositionTarget::Overlay(super::ALPHA_OVERLAY_PLANE_SLOT),
        ),
        build_plane_plan(
            &runtime.composition.solara,
            &windows,
            &views,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2),
        ),
        build_plane_plan(
            &runtime.composition.draw3d,
            &windows,
            &views,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3),
        ),
    ];
    if !plans.iter().any(|plan| plan.changed) {
        release_leases(&leases);
        return Ok(None);
    }

    Ok(Some(PendingFrame {
        windows,
        leases,
        plans,
        next_plane: 0,
        active: None,
        completed: Vec::new(),
        direct_leases: [None; 4],
        flip_submitted: false,
        started_ns: crate::chronos::monotonic_nanos(),
    }))
}

fn validate_window_snapshot(
    output: OutputId,
    windows: &[WindowSnapshot],
) -> Result<(), Ui4CompositorError> {
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        crate::log_warn!(target: "ui4";
            "ui4 compositor visible-window soft-cap exceeded output={} requested={} cap={} action=reject-composition\n",
            output.name(), windows.len(), MAX_COMPOSITION_WINDOWS,
        );
        return Err(Ui4CompositorError::PresentFailed);
    }
    if windows.iter().any(|window| {
        !matches!(
            window.plane.slot(),
            super::PRIMARY_PLANE_SLOT
                | super::ALPHA_OVERLAY_PLANE_SLOT
                | super::RGB_OVERLAY_PLANE_SLOT_2
                | super::RGB_OVERLAY_PLANE_SLOT_3
        )
    }) {
        return Err(Ui4CompositorError::PresentFailed);
    }
    Ok(())
}

fn build_plane_plan(
    state: &PlaneCompositionState,
    all_windows: &[WindowSnapshot],
    views: &[FrameRgbaView],
    target: CompositionTarget,
) -> PlanePlan {
    let plane_slot = target_plane_slot(target);
    let mut plan = PlanePlan::empty(target);
    let plane_windows: Vec<(usize, WindowSnapshot)> = all_windows
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, window)| window.plane.slot() == plane_slot)
        .collect();
    if plane_windows.is_empty() && !state.initialized {
        return plan;
    }

    let (output_width, output_height) = crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
    let mut composition_changed = !state.initialized;
    for (local_slot, (global_slot, window)) in plane_windows.iter().copied().enumerate() {
        let current = CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement: window.placement,
        };
        plan.next_windows[local_slot] = Some(current);
        let previous = state
            .windows
            .iter()
            .flatten()
            .find(|previous| previous.id == current.id);
        let changed = !state.initialized || previous != Some(&current);
        composition_changed |= changed;
        match previous {
            None => {
                add_placement_damage(
                    &mut plan.damage,
                    current.placement,
                    output_width,
                    output_height,
                );
            }
            Some(previous) if previous.placement != current.placement => {
                add_placement_damage(
                    &mut plan.damage,
                    previous.placement,
                    output_width,
                    output_height,
                );
                add_placement_damage(
                    &mut plan.damage,
                    current.placement,
                    output_width,
                    output_height,
                );
            }
            Some(previous) if previous.frame != current.frame => {
                add_placement_damage(
                    &mut plan.damage,
                    current.placement,
                    output_width,
                    output_height,
                );
            }
            Some(previous) if previous.publish_serial != current.publish_serial => {
                let local = window
                    .damage
                    .unwrap_or_else(|| super::DamageRegion::from_rect(DamageRect::FULL));
                add_mapped_window_damage(
                    &mut plan.damage,
                    local,
                    views[global_slot],
                    window.placement,
                    output_width,
                    output_height,
                );
            }
            Some(_) => {}
        }
    }
    for previous in state.windows.iter().flatten() {
        if !plan
            .next_windows
            .iter()
            .flatten()
            .any(|current| current.id == previous.id)
        {
            composition_changed = true;
            add_placement_damage(&mut plan.damage, previous.placement, output_width, output_height);
        }
    }
    if !state.initialized {
        plan.damage = crate::intel::CompositionDamageRegion::from_rect(
            crate::intel::CompositionDamageRect::new(0, 0, output_width, output_height),
        );
    }
    plan.changed = composition_changed && !plan.damage.is_empty();
    plan
}

fn drive_async_frame(
    runtime: &mut Runtime,
    pending: &mut PendingFrame,
) -> Result<DriveResult, Ui4CompositorError> {
    if pending.flip_submitted {
        return match crate::intel::poll_ui4_plane_surface_flip_batch() {
            crate::intel::Ui4PlaneSurfaceFlipPoll::Pending => Ok(DriveResult::Pending),
            crate::intel::Ui4PlaneSurfaceFlipPoll::Complete => {
                for composition in pending.completed.iter().copied() {
                    crate::intel::commit_ui4_composition_flip(composition);
                }
                commit_async_frame(runtime, pending);
                Ok(DriveResult::Complete)
            }
            crate::intel::Ui4PlaneSurfaceFlipPoll::Failed => Err(Ui4CompositorError::PresentFailed),
        };
    }

    if let Some(active) = pending.active {
        match crate::intel::poll_ui4_composition(active) {
            crate::intel::Ui4AsyncCompositionPoll::Pending => return Ok(DriveResult::Pending),
            crate::intel::Ui4AsyncCompositionPoll::Ready => {
                pending.completed.push(active);
                pending.active = None;
            }
            crate::intel::Ui4AsyncCompositionPoll::Failed => {
                return Err(Ui4CompositorError::PresentFailed);
            }
        }
    }

    while pending.next_plane < pending.plans.len() {
        let plan = pending.plans[pending.next_plane];
        pending.next_plane += 1;
        if !plan.changed {
            continue;
        }
        pending.active = Some(queue_async_plane(pending, plan)?);
        return Ok(DriveResult::Pending);
    }

    if pending.completed.is_empty() || !crate::intel::begin_ui4_plane_surface_flip_batch() {
        return Err(Ui4CompositorError::PresentFailed);
    }
    for composition in pending.completed.iter().copied() {
        if !crate::intel::stage_ui4_composition_flip(composition) {
            return Err(Ui4CompositorError::PresentFailed);
        }
    }
    if !crate::intel::submit_ui4_plane_surface_flip_batch() {
        return Err(Ui4CompositorError::PresentFailed);
    }
    pending.flip_submitted = true;
    Ok(DriveResult::Pending)
}

fn queue_async_plane(
    pending: &mut PendingFrame,
    plan: PlanePlan,
) -> Result<crate::intel::Ui4AsyncComposition, Ui4CompositorError> {
    if matches!(plan.target, CompositionTarget::Primary) {
        if let Some(result) = queue_native_video_primary(pending) {
            return result;
        }
    }
    let views: Vec<FrameRgbaView> = pending
        .leases
        .iter()
        .copied()
        .map(published_rgba_view)
        .collect::<Result<_, _>>()?;
    let selected: Vec<(WindowSnapshot, FrameRgbaView)> = pending
        .windows
        .iter()
        .copied()
        .zip(views.iter().copied())
        .filter(|(window, _)| window.plane.slot() == target_plane_slot(plan.target))
        .collect();
    if let CompositionTarget::Overlay(slot) = plan.target {
        let released_draw3d = selected
            .iter()
            .any(|(_, view)| view.gpu_release.is_some());
        if released_draw3d
            && (slot != super::RGB_OVERLAY_PLANE_SLOT_3
                || selected.len() != 1
                || !direct_overlay_eligible(selected[0].0, selected[0].1))
        {
            crate::log_warn!(target: "ui4";
                "ui4/direct-present: released Draw3D frame rejected slot={} windows={} action=retain-front-and-retry no-copy-fallback=1\n",
                slot,
                selected.len(),
            );
            return Err(Ui4CompositorError::PresentFailed);
        }
        if let Some((window, view)) = selected.as_slice().first().copied()
            && selected.len() == 1
            && direct_overlay_eligible(window, view)
        {
            if view.gpu_release.is_some()
                && !DRAW3D_TRIPLE_DIRECT_SCANOUT_LOGGED.swap(true, Ordering::AcqRel)
            {
                crate::log_info!(target: "ui4";
                    "ui4/direct-present: compositor-rewire frame={} slot={} producer=draw3d-gpu-authored buffering=triple action=import-complete-buffer-and-flip per_frame_guc_jobs=0 render_release_sequence={} display_release=surflive cpu_frame_copy=0\n",
                    window.frame.raw(), slot, view.gpu_release.map_or(0, |release| release.sequence()),
                );
            }
            let lease_index = pending
                .windows
                .iter()
                .position(|candidate| candidate.id == window.id)
                .ok_or(Ui4CompositorError::PresentFailed)?;
            let display_lease = retain_published_frame(pending.leases[lease_index])?;
            let reason = overlay_async_reason(slot);
            let queued = crate::intel::queue_ui4_direct_overlay_frame(
                slot,
                crate::intel::Ui4DirectRgbaFrame {
                    phys: view.phys,
                    byte_len: view.byte_len,
                    width: view.width,
                    height: view.height,
                    pitch_bytes: view.pitch,
                    producer_frame: window.frame.raw(),
                    producer_buffer_index: display_lease.buffer_index,
                    producer_publish_serial: window.publish_serial,
                    producer_release_sequence: view
                        .gpu_release
                        .map_or(0, |release| release.sequence()),
                },
                window.placement.x as u32,
                window.placement.y as u32,
                reason,
            );
            match queued {
                Ok(composition) => {
                    if pending.direct_leases[slot].is_some() {
                        let _ = release_published_frame(display_lease);
                        return Err(Ui4CompositorError::PresentFailed);
                    }
                    pending.direct_leases[slot] = Some(display_lease);
                    return Ok(composition);
                }
                Err(_) => {
                    let _ = release_published_frame(display_lease);
                    crate::log_warn!(target: "ui4";
                        "ui4/direct-present: display import unavailable slot={} frame={} buffer={} action=retain-front-and-retry no-copy-fallback=1\n",
                        slot,
                        display_lease.frame.raw(),
                        display_lease.buffer_index,
                    );
                    if view.gpu_release.is_some() {
                        return Err(Ui4CompositorError::PresentFailed);
                    }
                }
            }
        }
    }
    let pixels: Vec<&[u8]> = selected
        .iter()
        .map(|(_, view)| unsafe {
            core::slice::from_raw_parts(
                view.virt.cast_const(),
                (view.pitch as usize).saturating_mul(view.height as usize),
            )
        })
        .collect();
    let tiles: Vec<_> = selected
        .iter()
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
            gpgpu_surface: crate::intel::gpgpu::GpgpuRgba8Surface::new(
                view.phys,
                view.gpu,
                view.byte_len,
                view.width,
                view.height,
                view.pitch,
            ),
            opacity: window.placement.opacity,
            known_opaque: frame_snapshot(window.frame)
                .map(|snapshot| snapshot.plan.content == FrameContent::Video)
                .unwrap_or(false),
            expected_rgba: None,
        })
        .collect();
    let queued = match plan.target {
        CompositionTarget::Primary => crate::intel::queue_ui4_primary_composition(
            &tiles,
            plan.damage,
            "ui4-compositor-primary-async",
        ),
        CompositionTarget::Overlay(slot) => {
            let reason = overlay_async_reason(slot);
            crate::intel::queue_ui4_overlay_composition(slot, &tiles, plan.damage, reason)
        }
    };
    queued.map_err(|_| Ui4CompositorError::PresentFailed)
}

fn direct_overlay_eligible(window: WindowSnapshot, view: FrameRgbaView) -> bool {
    if !direct_overlay_geometry_eligible(window, view) {
        return false;
    }
    if !view.gpu_authored {
        return true;
    }
    // The renderer's final PIPE_CONTROL release covers the exact allocation.
    // The compositor read lease then prevents reuse through the SURFLIVE
    // latch; no copy or CPU cache sweep is part of the transition.
    frame_snapshot(window.frame).is_ok_and(|snapshot| {
        snapshot.plan.content == FrameContent::RenderScene3d
            && snapshot.plan.buffering == super::FrameBuffering::Triple
            && view.gpu_release.is_some()
    })
}

fn direct_overlay_geometry_eligible(window: WindowSnapshot, view: FrameRgbaView) -> bool {
    let placement = window.placement;
    let Some((output_width, output_height)) = crate::intel::active_scanout_dimensions() else {
        return false;
    };
    placement.opacity == u8::MAX
        && placement.x >= 0
        && placement.y >= 0
        && placement.width == view.width
        && placement.height == view.height
        && (placement.x as u32)
            .checked_add(view.width)
            .is_some_and(|right| right <= output_width)
        && (placement.y as u32)
            .checked_add(view.height)
            .is_some_and(|bottom| bottom <= output_height)
}

const fn overlay_async_reason(slot: usize) -> &'static str {
    match slot {
        super::ALPHA_OVERLAY_PLANE_SLOT => "ui4-alpha-slot1-async",
        super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-solara-slot2-async",
        super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-draw3d-slot3-async",
        _ => "ui4-overlay-async",
    }
}

/// Select the exact native sidecar that belongs to the sole primary video
/// window.  Ordinary RGBA windows and multi-window primary compositions keep
/// using the general compositor; there is no silent format reinterpretation.
fn queue_native_video_primary(
    pending: &PendingFrame,
) -> Option<Result<crate::intel::Ui4AsyncComposition, Ui4CompositorError>> {
    let mut primary = pending
        .windows
        .iter()
        .filter(|window| window.plane.slot() == super::PRIMARY_PLANE_SLOT);
    let window = *primary.next()?;
    if primary.next().is_some() || window.placement.opacity != u8::MAX {
        return None;
    }
    let snapshot = frame_snapshot(window.frame).ok()?;
    if snapshot.plan.content != FrameContent::Video
        || snapshot.plan.width != window.placement.width
        || snapshot.plan.height != window.placement.height
    {
        return None;
    }
    let publication = super::native_video_publication(
        window.frame,
        snapshot.publish_serial,
    )?;
    Some((|| {
        let (output_width, output_height) = crate::intel::active_scanout_dimensions()
            .ok_or(Ui4CompositorError::PresentFailed)?;
        let intended_x =
            i64::from(window.placement.x) + i64::from(publication.layout.destination_x);
        let intended_y =
            i64::from(window.placement.y) + i64::from(publication.layout.destination_y);
        let intended_right = intended_x + i64::from(publication.layout.width);
        let intended_bottom = intended_y + i64::from(publication.layout.height);
        let destination_x = intended_x.clamp(0, i64::from(output_width));
        let destination_y = intended_y.clamp(0, i64::from(output_height));
        let destination_right = intended_right.clamp(0, i64::from(output_width));
        let destination_bottom = intended_bottom.clamp(0, i64::from(output_height));
        let content_width = u32::try_from(destination_right.saturating_sub(destination_x))
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        let content_height = u32::try_from(destination_bottom.saturating_sub(destination_y))
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        if content_width == 0 || content_height == 0 {
            return Err(Ui4CompositorError::PresentFailed);
        }
        let clipped_left = u32::try_from(destination_x.saturating_sub(intended_x))
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        let clipped_top = u32::try_from(destination_y.saturating_sub(intended_y))
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        let source_x = publication
            .layout
            .source_x
            .checked_add(clipped_left)
            .ok_or(Ui4CompositorError::PresentFailed)?;
        let source_y = publication
            .layout
            .source_y
            .checked_add(clipped_top)
            .ok_or(Ui4CompositorError::PresentFailed)?;
        let pitch_bytes = u32::try_from(publication.source.pitch_bytes)
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        let uv_offset = u32::try_from(publication.source.uv_offset)
            .map_err(|_| Ui4CompositorError::PresentFailed)?;
        let source = crate::intel::gpgpu::GpgpuNv12Tile64Surface::new(
            publication.source.phys,
            publication.source.gpu,
            publication.source.byte_len,
            publication.source.width,
            publication.source.height,
            pitch_bytes,
            uv_offset,
        )
        .ok_or(Ui4CompositorError::PresentFailed)?;
        crate::intel::queue_ui4_primary_native_nv12_composition(
            source,
            destination_x as u32,
            destination_y as u32,
            content_width,
            content_height,
            source_x,
            source_y,
            "ui4-video-native-nv12-primary-async",
        )
        .map_err(|_| Ui4CompositorError::PresentFailed)
    })())
}

fn commit_async_frame(runtime: &mut Runtime, pending: &mut PendingFrame) {
    for plan in pending.plans.iter().copied().filter(|plan| plan.changed) {
        let state = match plan.target {
            CompositionTarget::Primary => &mut runtime.composition.primary,
            CompositionTarget::Overlay(super::ALPHA_OVERLAY_PLANE_SLOT) => {
                &mut runtime.composition.alpha
            }
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2) => {
                &mut runtime.composition.solara
            }
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3) => {
                &mut runtime.composition.draw3d
            }
            CompositionTarget::Overlay(_) => continue,
        };
        state.initialized = true;
        state.windows = plan.next_windows;
        let slot = target_plane_slot(plan.target);
        for window in pending
            .windows
            .iter()
            .filter(|window| window.plane.slot() == slot)
        {
            if let Ok(snapshot) = frame_snapshot(window.frame) {
                if let Some(publication) = super::native_video_publication(
                    window.frame,
                    snapshot.publish_serial,
                ) {
                    super::acknowledge_native_video_publication(publication.sequence);
                }
            }
            let _ = acknowledge_window_frame(window.id, window.publish_serial);
        }
    }
    // Compositor-rewire has no screenshot consumer. Keeping capture entirely
    // out of the only supported present path also makes the no-CPU-pixel-read
    // contract structural rather than dependent on an empty request queue.
    commit_direct_leases(runtime, pending);
    release_leases(&pending.leases);
    let elapsed_us = crate::chronos::monotonic_nanos().saturating_sub(pending.started_ns) / 1_000;
    let direct_planes = pending
        .completed
        .iter()
        .copied()
        .filter_map(crate::intel::ui4_direct_composition_plane_slot)
        .count();
    let guc_jobs = pending.completed.len().saturating_sub(direct_planes);
    runtime.retired_frames = runtime.retired_frames.saturating_add(1);
    if runtime.retired_frames <= 8 || runtime.retired_frames.is_multiple_of(120) {
        crate::log_trace!(target: "ui4";
            "ui4/compositor: frame-retired seq={} planes={} direct_planes={} guc_jobs={} windows={} elapsed_us={} ap1_wait_loops=0 telemetry=first8+every120\n",
            runtime.retired_frames, pending.completed.len(), direct_planes, guc_jobs,
            pending.windows.len(), elapsed_us,
        );
    }
}

fn commit_direct_leases(runtime: &mut Runtime, pending: &mut PendingFrame) {
    for plan in pending.plans.iter().copied().filter(|plan| plan.changed) {
        let CompositionTarget::Overlay(slot) = plan.target else {
            continue;
        };
        let replacement = pending.direct_leases[slot].take();
        let previous = core::mem::replace(&mut runtime.live_direct[slot], replacement);
        if let Some(previous) = previous {
            let _ = release_published_frame(previous);
        }
    }
}

/// A timed-out multi-plane batch may have latched only some planes. Preserve
/// the exact producer buffer for every direct plane whose SURFLIVE already
/// reached the new alias; otherwise its pending retain can be released.
fn settle_failed_direct_leases(runtime: &mut Runtime, pending: &mut PendingFrame) {
    for slot in 1..pending.direct_leases.len() {
        let Some(lease) = pending.direct_leases[slot].take() else {
            continue;
        };
        let latched = pending.flip_submitted
            && pending.completed.iter().copied().any(|composition| {
                crate::intel::ui4_direct_composition_plane_slot(composition) == Some(slot)
                    && crate::intel::ui4_composition_flip_is_live(composition)
            });
        if latched {
            let previous = runtime.live_direct[slot].replace(lease);
            if let Some(previous) = previous {
                let _ = release_published_frame(previous);
            }
        } else {
            let _ = release_published_frame(lease);
        }
    }
}

const fn target_plane_slot(target: CompositionTarget) -> usize {
    match target {
        CompositionTarget::Primary => super::PRIMARY_PLANE_SLOT,
        CompositionTarget::Overlay(slot) => slot,
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
                    gpgpu_surface: crate::intel::gpgpu::GpgpuRgba8Surface::new(
                        view.phys,
                        view.gpu,
                        view.byte_len,
                        view.width,
                        view.height,
                        view.pitch,
                    ),
                    opacity: window.placement.opacity,
                    known_opaque: frame_snapshot(window.frame)
                        .map(|snapshot| snapshot.plan.content == FrameContent::Video)
                        .unwrap_or(false),
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
