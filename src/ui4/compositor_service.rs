//! Permanent kernel UI4 compositor service.
//!
//! Producers own frames and windows. This service owns the broker snapshot,
//! per-plane damage history, software-cursor composition, and the atomic plane
//! surface-flip batch. It intentionally creates no application windows.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::{
    DamageRect, FrameContent, FrameGpuRelease, FrameHandle, FramePoolError, FrameReadLease,
    FrameRgbaView, OutputId, WindowId, WindowPlacement, WindowSnapshot, acknowledge_window_frame,
    acquire_published_frame, frame_snapshot, published_rgba_view, release_published_frame,
    retain_published_frame, visible_windows_for_output,
};

// AP1 wake-rate diagnostic: normal broker rescans are intentionally slow;
// in-flight work still uses the 1 ms pending cadence below.
const COMPOSITION_PERIOD_MS: u64 = 160;
const PENDING_POLL_PERIOD_MS: u64 = 1;
const STATIC_SINGLE_CPU_PAINTER_BASELINE_ENABLED: bool = true;
const MAX_COMPOSITION_WINDOWS: usize = super::window_broker::MAX_ACTIVE_WINDOWS;
const PRESENT_FAILURE_LOG_INTERVAL: u32 = 600;
static RESIDENT_SCENE_TRIPLE_DIRECT_SCANOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static COMPUTE_DIRECT_SCANOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_OVERLAP_WARNED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_CPU_BASELINE_LOGGED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_BCS0_BASELINE_LOGGED: AtomicBool = AtomicBool::new(false);
static VIDEO_SURFLIVE_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        "ui4 compositor frame/window reintegration live composition_ms={} broker_planes=slot0+slot1+slot2+slot3/on-demand compute=slots1+2+3-direct-import resident_scenes=gridpaper:slot2+draw3d:slot3-triple-direct-import per_frame_scene_guc_composition=off per_frame_display_flip=on slot4=independent-interaction+software-cursor hardware-cursor=preferred-physical-source/concurrent input=enabled screenshots=parked previews=Shell2/on-demand-trio video=slot1-double-rgba8-direct-or-guc-compose linked_nv12_planes=off\n",
        COMPOSITION_PERIOD_MS,
    );

    let mut consecutive_failures = 0u32;
    loop {
        let result = advance_async_composition(&mut runtime);
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
    let mut plans = [
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
    // During restore, a resize-capable direct producer replaces its complete
    // Frame ring after receiving the broker callback. Keep the old exact
    // scanout live during that short handoff. Maximize takes the other path:
    // the old allocation is immediately centered at 1:1 until its larger
    // replacement arrives.
    for plan in &mut plans {
        if plan.changed && direct_resize_handoff_pending(*plan, &windows, &views) {
            plan.changed = false;
        }
    }
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
        let placement = presentation_placement(window, views[global_slot]);
        let current = CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement,
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
                    placement,
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
        .map(|(mut window, view)| {
            window.placement = presentation_placement(window, view);
            (window, view)
        })
        .collect();
    // Immutable single-buffer images are already complete rectangles. They do
    // not need a per-output-pixel layer search: the first pristine backbuffer
    // is painted by one ordered BCS batch, while the CPU fallback still owns
    // non-pristine damage until a BCS clear is activated. Plane slots are
    // independent scanout inputs, so overlap is considered only inside this
    // already slot-filtered selection.
    let sparse_static_painter = matches!(plan.target, CompositionTarget::Overlay(_))
        && !selected.is_empty()
        && selected.iter().all(|(window, _)| {
            frame_snapshot(window.frame).is_ok_and(|snapshot| {
                snapshot.plan.content == FrameContent::Image
                    && snapshot.plan.buffering == super::FrameBuffering::Single
            })
        });
    if sparse_static_painter
        && same_slot_windows_overlap(&selected)
        && !STATIC_SINGLE_OVERLAP_WARNED.swap(true, Ordering::AcqRel)
    {
        crate::log_warn!(target: "ui4";
            "ui4/static-painter: same-slot overlap detected slot={} windows={} needs_threadment=1 action=painter-order-baseline zstack-specialization=deferred log=once\n",
            target_plane_slot(plan.target), selected.len(),
        );
    }
    if let CompositionTarget::Overlay(slot) = plan.target {
        if let Some((window, view)) = selected.as_slice().first().copied()
            && selected.len() == 1
            && direct_overlay_eligible(window, view)
        {
            if let Some(release) = view.gpu_release {
                let (buffering, first_for_producer) = match release {
                    FrameGpuRelease::ResidentScene(_) => (
                        "triple",
                        !RESIDENT_SCENE_TRIPLE_DIRECT_SCANOUT_LOGGED.swap(true, Ordering::AcqRel),
                    ),
                    FrameGpuRelease::Compute(_) => (
                        match frame_snapshot(window.frame)?.plan.buffering {
                            super::FrameBuffering::Single => "single",
                            super::FrameBuffering::Double => "double",
                            super::FrameBuffering::Triple => "triple",
                        },
                        !COMPUTE_DIRECT_SCANOUT_LOGGED.swap(true, Ordering::AcqRel),
                    ),
                };
                if first_for_producer {
                    crate::log_info!(target: "ui4";
                        "ui4/direct-present: compositor-rewire frame={} slot={} producer={} gpu_authored=1 buffering={} action=import-complete-buffer-and-flip per_frame_guc_jobs=0 producer_release_sequence={} display_release=surflive cpu_frame_copy=0\n",
                        window.frame.raw(), slot, release.producer_label(), buffering, release.sequence(),
                    );
                }
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
                window.placement.width,
                window.placement.height,
                window.placement.opacity,
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
                        "ui4/direct-present: display import unavailable slot={} frame={} buffer={} action=guc-compose-released-rgba8 cpu-copy-fallback=0\n",
                        slot,
                        display_lease.frame.raw(),
                        display_lease.buffer_index,
                    );
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
            gpgpu_scanout_cache: view.gpu_release.is_some(),
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
            if sparse_static_painter && STATIC_SINGLE_CPU_PAINTER_BASELINE_ENABLED {
                let bcs = crate::intel::queue_ui4_static_overlay_composition_bcs0(
                    slot,
                    &tiles,
                    plan.damage,
                    reason,
                );
                match bcs {
                    Ok(composition) => {
                        if !STATIC_SINGLE_BCS0_BASELINE_LOGGED.swap(true, Ordering::AcqRel) {
                            crate::log_info!(target: "ui4";
                                "ui4/static-painter: backend=guc-bcs0-fast-copy buffering=single content=image plane_isolation=slot-local batch=one-per-changed-plane completion=mi-flush-dw-post-sync flip=after-retire cpu_pixel_copy=0 shader_dispatches=0 clear=fresh-transparent-only log=once\n"
                            );
                        }
                        Ok(composition)
                    }
                    Err(crate::intel::Ui4AsyncCompositionError::Unavailable) => {
                        if !STATIC_SINGLE_CPU_BASELINE_LOGGED.swap(true, Ordering::AcqRel) {
                            crate::log_warn!(target: "ui4";
                                "ui4/static-painter-fallback: backend=cpu-sparse-copy reason=non-pristine-or-bcs-unavailable buffering=single content=image plane_isolation=slot-local guc_jobs=0 shader_dispatches=0 damage=old+new flip=ui4-batched log=once\n"
                            );
                        }
                        crate::intel::queue_ui4_static_overlay_composition_cpu(
                            slot,
                            &tiles,
                            plan.damage,
                            reason,
                        )
                    }
                    Err(error) => Err(error),
                }
            } else {
                crate::intel::queue_ui4_overlay_composition(
                    slot,
                    &tiles,
                    plan.damage,
                    sparse_static_painter,
                    reason,
                )
            }
        }
    };
    queued.map_err(|_| Ui4CompositorError::PresentFailed)
}

fn same_slot_windows_overlap(selected: &[(WindowSnapshot, FrameRgbaView)]) -> bool {
    selected.iter().enumerate().any(|(left_index, (left, _))| {
        selected
            .iter()
            .skip(left_index.saturating_add(1))
            .any(|(right, _)| placements_overlap(left.placement, right.placement))
    })
}

fn placements_overlap(left: WindowPlacement, right: WindowPlacement) -> bool {
    let left_x = i64::from(left.x);
    let left_y = i64::from(left.y);
    let right_x = i64::from(right.x);
    let right_y = i64::from(right.y);
    left_x < right_x.saturating_add(i64::from(right.width))
        && right_x < left_x.saturating_add(i64::from(left.width))
        && left_y < right_y.saturating_add(i64::from(right.height))
        && right_y < left_y.saturating_add(i64::from(left.height))
}

fn direct_overlay_eligible(window: WindowSnapshot, view: FrameRgbaView) -> bool {
    if !direct_overlay_geometry_eligible(window, view) {
        return false;
    }
    if !view.gpu_authored {
        return true;
    }
    let Some(release) = view.gpu_release else {
        return false;
    };
    if !release.matches(view.phys, view.byte_len) {
        return false;
    }
    // Each producer's final PIPE_CONTROL release covers this exact allocation.
    // The compositor read lease then prevents reuse through the SURFLIVE latch;
    // no copy or CPU cache sweep is part of either transition.
    frame_snapshot(window.frame).is_ok_and(|snapshot| match release {
        FrameGpuRelease::ResidentScene(_) => {
            matches!(
                window.plane.slot(),
                super::RGB_OVERLAY_PLANE_SLOT_2 | super::RGB_OVERLAY_PLANE_SLOT_3
            ) && snapshot.plan.content == FrameContent::RenderScene3d
                && snapshot.plan.buffering == super::FrameBuffering::Triple
        }
        FrameGpuRelease::Compute(_) => {
            matches!(
                window.plane.slot(),
                super::ALPHA_OVERLAY_PLANE_SLOT
                    | super::RGB_OVERLAY_PLANE_SLOT_2
                    | super::RGB_OVERLAY_PLANE_SLOT_3
            ) && matches!(
                (snapshot.plan.content, snapshot.plan.buffering),
                (FrameContent::Image, super::FrameBuffering::Double)
                    | (FrameContent::BlueprintScene, super::FrameBuffering::Triple)
                    | (FrameContent::Video, super::FrameBuffering::Double)
            )
        }
    })
}

fn direct_overlay_geometry_eligible(window: WindowSnapshot, view: FrameRgbaView) -> bool {
    let placement = window.placement;
    let Some((output_width, output_height)) = crate::intel::active_scanout_dimensions() else {
        return false;
    };
    let exact_size = placement.width == view.width && placement.height == view.height;
    let scaler_safe_close = window.state == super::WindowState::Closing
        && placement.width >= 8
        && placement.height >= 8
        && placement.width <= view.width
        && placement.height <= view.height
        && u64::from(view.width) < u64::from(placement.width).saturating_mul(3)
        && u64::from(view.height) < u64::from(placement.height).saturating_mul(3);
    placement.x >= 0
        && placement.y >= 0
        && (exact_size || scaler_safe_close)
        && (placement.x as u32)
            .checked_add(placement.width)
            .is_some_and(|right| right <= output_width)
        && (placement.y as u32)
            .checked_add(placement.height)
            .is_some_and(|bottom| bottom <= output_height)
}

/// A resize-capable producer may need several service turns to replace its
/// complete Frame ring after maximize. Preserve the old allocation exactly:
/// center it inside the new logical window without scaling or copying. Once
/// the replacement extent matches, the ordinary full-output placement wins.
fn presentation_placement(window: WindowSnapshot, view: FrameRgbaView) -> WindowPlacement {
    let placement = window.placement;
    if !window.maximized
        || !window.interaction.resize_on_maximize
        || (placement.width == view.width && placement.height == view.height)
        || view.width > placement.width
        || view.height > placement.height
    {
        return placement;
    }
    WindowPlacement {
        x: placement
            .x
            .saturating_add(placement.width.saturating_sub(view.width) as i32 / 2),
        y: placement
            .y
            .saturating_add(placement.height.saturating_sub(view.height) as i32 / 2),
        width: view.width,
        height: view.height,
        ..placement
    }
}

fn direct_resize_handoff_pending(
    plan: PlanePlan,
    windows: &[WindowSnapshot],
    views: &[FrameRgbaView],
) -> bool {
    let CompositionTarget::Overlay(slot) = plan.target else {
        return false;
    };
    let mut selected = windows
        .iter()
        .copied()
        .zip(views.iter().copied())
        .filter(|(window, _)| window.plane.slot() == slot);
    let Some((window, view)) = selected.next() else {
        return false;
    };
    selected.next().is_none()
        && window.state == super::WindowState::Ready
        && window.interaction.maximizable
        && window.interaction.receives_input
        && window.interaction.resize_on_maximize
        && !window.maximized
        && view
            .gpu_release
            .is_some_and(|release| release.matches(view.phys, view.byte_len))
        && (window.placement.width != view.width || window.placement.height != view.height)
}

const fn overlay_async_reason(slot: usize) -> &'static str {
    match slot {
        super::ALPHA_OVERLAY_PLANE_SLOT => "ui4-alpha-slot1-async",
        super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-rgb-slot2-async",
        super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-rgb-slot3-async",
        _ => "ui4-overlay-async",
    }
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
    let guc_jobs = pending
        .completed
        .iter()
        .copied()
        .filter(|composition| crate::intel::ui4_composition_has_guc_work(*composition))
        .count();
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
            release_replaced_direct_lease(slot, previous, "flip-batch-complete");
        }
    }
}

/// Drop the old scanout owner's exact buffer only after the caller proved that
/// the replacement surface is live. Video logs this boundary explicitly so a
/// hardware run can distinguish producer completion from display retirement.
fn release_replaced_direct_lease(slot: usize, lease: FrameReadLease, boundary: &'static str) {
    let video = frame_snapshot(lease.frame)
        .is_ok_and(|snapshot| snapshot.plan.content == FrameContent::Video);
    let released = release_published_frame(lease).is_ok();
    if video {
        let sequence = VIDEO_SURFLIVE_RELEASE_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if sequence <= 8 || sequence.is_multiple_of(120) || !released {
            crate::log_info!(target: "ui4";
                "ui4 video-frame display-retired seq={} frame={} buffer={} slot={} boundary=surflive-{} display_lease_released={} display_ownership_released={} cpu_pixel_copy=0\n",
                sequence,
                lease.frame.raw(),
                lease.buffer_index,
                slot,
                boundary,
                released as u8,
                released as u8,
            );
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
                release_replaced_direct_lease(slot, previous, "failed-batch-partial-latch");
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
