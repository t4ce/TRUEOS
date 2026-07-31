//! Permanent kernel UI4 compositor service.
//!
//! Producers own frames and windows. This service owns the broker snapshot,
//! per-plane static-frame history and the atomic plane surface-flip batch. It
//! intentionally creates no application windows; slot 4 remains independent.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::{
    DamageRect, FrameContent, FrameGpuRelease, FrameHandle, FramePoolError, FrameReadLease,
    FrameRgbaView, OutputId, WindowId, WindowPlacement, WindowSnapshot, acknowledge_window_frame,
    acquire_published_frame, frame_snapshot, published_rgba_view, release_published_frame,
    retain_published_frame, visible_windows_for_output_with_revision, window_composition_revision,
};

const CLOSE_TRANSITION_PERIOD_MS: u64 = 16;
const PENDING_POLL_PERIOD_MS: u64 = 1;
const STATIC_SINGLE_CPU_PAINTER_BASELINE_ENABLED: bool = true;
const MAX_COMPOSITION_WINDOWS: usize = super::window_broker::MAX_WINDOWS;
const PRESENT_FAILURE_LOG_INTERVAL: u32 = 600;
const PROFILE_INITIAL_REPORT_TURNS: u64 = 16;
const PROFILE_REPORT_INTERVAL_TURNS: u64 = 1_024;
static RESIDENT_SCENE_TRIPLE_DIRECT_SCANOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static COMPUTE_DIRECT_SCANOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_OVERLAP_WARNED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_CPU_BASELINE_LOGGED: AtomicBool = AtomicBool::new(false);
static STATIC_SINGLE_BCS0_BASELINE_LOGGED: AtomicBool = AtomicBool::new(false);
static DIRTY_FONT_SHARED_COMPOSITION_LOGGED: AtomicBool = AtomicBool::new(false);
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
    observed_broker_revision: Option<u64>,
    next_wake_deadline_ns: Option<u64>,
    profile: CompositorCpuProfile,
    /// Exact producer buffers currently owned by overlay scanout. A lease is
    /// replaced only after the corresponding new SURFLIVE value is observed.
    live_direct: [Option<FrameReadLease>; 4],
}

#[derive(Default)]
struct CompositorCpuProfile {
    total_turns: u64,
    reported_turns: u64,
    window_turns: u64,
    prepare_turns: u64,
    no_change_turns: u64,
    fast_unchanged_turns: u64,
    prepared_frames: u64,
    pending_entry_turns: u64,
    failed_turns: u64,
    guc_polls: u64,
    surflive_polls: u64,
    submit_steps: u64,
    wake_samples: u64,
    total_ns: u64,
    max_turn_ns: u64,
    wake_late_ns: u64,
    max_wake_late_ns: u64,
    close_ns: u64,
    snapshot_ns: u64,
    lease_ns: u64,
    plan_ns: u64,
    guc_poll_ns: u64,
    surflive_poll_ns: u64,
    submit_ns: u64,
}

impl CompositorCpuProfile {
    fn record_wake(&mut self, started_ns: u64, deadline_ns: Option<u64>) {
        let Some(deadline_ns) = deadline_ns else {
            return;
        };
        let late_ns = started_ns.saturating_sub(deadline_ns);
        self.wake_samples = self.wake_samples.saturating_add(1);
        self.wake_late_ns = self.wake_late_ns.saturating_add(late_ns);
        self.max_wake_late_ns = self.max_wake_late_ns.max(late_ns);
    }

    fn finish_turn(&mut self, elapsed_ns: u64, pending_at_entry: bool, failed: bool) {
        self.total_turns = self.total_turns.saturating_add(1);
        self.window_turns = self.window_turns.saturating_add(1);
        self.total_ns = self.total_ns.saturating_add(elapsed_ns);
        self.max_turn_ns = self.max_turn_ns.max(elapsed_ns);
        self.pending_entry_turns = self
            .pending_entry_turns
            .saturating_add(u64::from(pending_at_entry));
        self.failed_turns = self.failed_turns.saturating_add(u64::from(failed));
    }

    fn maybe_report(&mut self) {
        let initial = self.reported_turns == 0 && self.total_turns >= PROFILE_INITIAL_REPORT_TURNS;
        let periodic = self.reported_turns != 0
            && self.total_turns.saturating_sub(self.reported_turns)
                >= PROFILE_REPORT_INTERVAL_TURNS;
        if !initial && !periodic {
            return;
        }

        crate::log_info!(target: "ui4";
            "ui4/compositor-profile turns={} samples={} prepare={} unchanged={} fast_unchanged={} prepared={} pending_entry={} failed={} sync_avg_us={} sync_max_us={} wake_late_avg_us={} wake_late_max_us={} phase_avg_us=close:{}/snapshot:{}/lease:{}/plan:{} drive_avg_us=submit:{}/guc_poll:{}/surflive_poll:{} drive_calls=submit:{}/guc_poll:{}/surflive_poll:{} idle_wake=broker-signal poll_model=one-shot-no-spin close_animation_ms={} pending_ms={}\n",
            self.total_turns,
            self.window_turns,
            self.prepare_turns,
            self.no_change_turns,
            self.fast_unchanged_turns,
            self.prepared_frames,
            self.pending_entry_turns,
            self.failed_turns,
            average_us(self.total_ns, self.window_turns),
            self.max_turn_ns / 1_000,
            average_us(self.wake_late_ns, self.wake_samples),
            self.max_wake_late_ns / 1_000,
            average_us(self.close_ns, self.prepare_turns),
            average_us(self.snapshot_ns, self.prepare_turns),
            average_us(self.lease_ns, self.prepare_turns),
            average_us(self.plan_ns, self.prepare_turns),
            average_us(self.submit_ns, self.submit_steps),
            average_us(self.guc_poll_ns, self.guc_polls),
            average_us(self.surflive_poll_ns, self.surflive_polls),
            self.submit_steps,
            self.guc_polls,
            self.surflive_polls,
            CLOSE_TRANSITION_PERIOD_MS,
            PENDING_POLL_PERIOD_MS,
        );
        self.reported_turns = self.total_turns;
        self.window_turns = 0;
        self.prepare_turns = 0;
        self.no_change_turns = 0;
        self.fast_unchanged_turns = 0;
        self.prepared_frames = 0;
        self.pending_entry_turns = 0;
        self.failed_turns = 0;
        self.guc_polls = 0;
        self.surflive_polls = 0;
        self.submit_steps = 0;
        self.wake_samples = 0;
        self.total_ns = 0;
        self.max_turn_ns = 0;
        self.wake_late_ns = 0;
        self.max_wake_late_ns = 0;
        self.close_ns = 0;
        self.snapshot_ns = 0;
        self.lease_ns = 0;
        self.plan_ns = 0;
        self.guc_poll_ns = 0;
        self.surflive_poll_ns = 0;
        self.submit_ns = 0;
    }
}

const fn average_us(total_ns: u64, samples: u64) -> u64 {
    if samples == 0 {
        0
    } else {
        total_ns / samples / 1_000
    }
}

struct CompositionState {
    primary: PlaneCompositionState,
    alpha: PlaneCompositionState,
    solara: PlaneCompositionState,
    draw3d: PlaneCompositionState,
}

struct PlaneCompositionState {
    initialized: bool,
    windows: Vec<CompositionWindowStamp>,
}

#[derive(Copy, Clone)]
enum CompositionTarget {
    Primary,
    Overlay(usize),
}

struct PlanePlan {
    target: CompositionTarget,
    changed: bool,
    /// Most recently changed window in broker/z order. Mixed buffering uses
    /// this one local winner without reconstructing other same-slot pixels.
    selected: Option<WindowId>,
    next_windows: Vec<CompositionWindowStamp>,
    damage: crate::intel::CompositionDamageRegion,
}

impl PlanePlan {
    fn empty(target: CompositionTarget, window_capacity: usize) -> Self {
        Self {
            target,
            changed: false,
            selected: None,
            next_windows: Vec::with_capacity(window_capacity),
            damage: crate::intel::CompositionDamageRegion::EMPTY,
        }
    }

    const fn queue_plan(&self) -> PlaneQueuePlan {
        PlaneQueuePlan {
            target: self.target,
            selected: self.selected,
            damage: self.damage,
        }
    }
}

#[derive(Copy, Clone)]
struct PlaneQueuePlan {
    target: CompositionTarget,
    selected: Option<WindowId>,
    damage: crate::intel::CompositionDamageRegion,
}

struct PendingFrame {
    windows: Vec<WindowSnapshot>,
    leases: Vec<FrameReadLease>,
    plans: [PlanePlan; 4],
    next_plane: usize,
    active: Option<crate::intel::Ui4AsyncComposition>,
    completed: Vec<crate::intel::Ui4AsyncComposition>,
    direct_leases: [Option<FrameReadLease>; 4],
    direct_windows: [Option<WindowId>; 4],
    flip_submitted: bool,
    broker_revision: u64,
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
    if !crate::intel::activate_ui4_application_rgba_planes() {
        crate::log_error!(target: "ui4";
            "ui4 compositor startup rejected cause=slot0-rgba-handoff-failed action=return\n"
        );
        return;
    }
    let mut runtime = initialize();

    crate::log_info!(
        target: "ui4";
        "ui4 compositor frame/window reintegration live idle_wake=broker-signal close_animation_ms={} pending_poll_ms={} broker_planes=slot0+slot1+slot2+slot3/on-demand expensive=nonshared-double+triple/one-per-slot/soft-cap-4 shared=single+dirty-font-double/slot-local-composition mixed_slot=local-winner/placement-old+new-repair static_single=shared-slot-composition slot4=independent-interaction+software-cursor hardware-cursor=preferred-physical-source/concurrent input=enabled screenshots=parked linked_nv12_planes=off\n",
        CLOSE_TRANSITION_PERIOD_MS,
        PENDING_POLL_PERIOD_MS,
    );

    let mut consecutive_failures = 0u32;
    let mut readiness_published = false;
    loop {
        let result = advance_async_composition(&mut runtime);
        match result {
            Ok(()) => {
                if !readiness_published {
                    crate::r::readiness::set(crate::r::readiness::UI4_COMPOSITOR_READY);
                    readiness_published = true;
                }
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
        let timed_wait_ms = if runtime.pending.is_some() || immediate_rescan {
            Some(PENDING_POLL_PERIOD_MS)
        } else if consecutive_failures != 0 || super::window_close_transitions_active() {
            Some(CLOSE_TRANSITION_PERIOD_MS)
        } else {
            None
        };
        if let Some(sleep_ms) = timed_wait_ms {
            runtime.next_wake_deadline_ns = Some(
                crate::chronos::monotonic_nanos()
                    .saturating_add(sleep_ms.saturating_mul(1_000_000)),
            );
            Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        } else {
            runtime.next_wake_deadline_ns = None;
            super::wait_for_window_composition_change().await;
        }
    }
}

fn initialize() -> Runtime {
    Runtime {
        composition: CompositionState {
            primary: PlaneCompositionState {
                initialized: false,
                windows: Vec::new(),
            },
            alpha: PlaneCompositionState {
                initialized: false,
                windows: Vec::new(),
            },
            solara: PlaneCompositionState {
                initialized: false,
                windows: Vec::new(),
            },
            draw3d: PlaneCompositionState {
                initialized: false,
                windows: Vec::new(),
            },
        },
        pending: None,
        immediate_rescan: false,
        retired_frames: 0,
        observed_broker_revision: None,
        next_wake_deadline_ns: None,
        profile: CompositorCpuProfile::default(),
        live_direct: [None; 4],
    }
}

fn advance_async_composition(runtime: &mut Runtime) -> Result<(), Ui4CompositorError> {
    let started_ns = crate::chronos::monotonic_nanos();
    runtime
        .profile
        .record_wake(started_ns, runtime.next_wake_deadline_ns.take());
    let pending_at_entry = runtime.pending.is_some();
    let result = advance_async_composition_inner(runtime);
    let elapsed_ns = crate::chronos::monotonic_nanos().saturating_sub(started_ns);
    runtime
        .profile
        .finish_turn(elapsed_ns, pending_at_entry, result.is_err());
    runtime.profile.maybe_report();
    result
}

fn advance_async_composition_inner(runtime: &mut Runtime) -> Result<(), Ui4CompositorError> {
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
            // the next executor turn before returning to broker-signal sleep.
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

fn prepare_async_frame(runtime: &mut Runtime) -> Result<Option<PendingFrame>, Ui4CompositorError> {
    runtime.profile.prepare_turns = runtime.profile.prepare_turns.saturating_add(1);
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let phase_started_ns = crate::chronos::monotonic_nanos();
    super::advance_window_close_transitions();
    runtime.profile.close_ns = runtime
        .profile
        .close_ns
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));

    // This epoch is both a safety gate for coalesced broker signals and the
    // cheap path after a close-animation timer tick which changed no pixels.
    // It avoids allocating a snapshot, acquiring every frame lease, rereading
    // display topology, or constructing four plane plans.
    let broker_revision = window_composition_revision();
    if runtime.observed_broker_revision == Some(broker_revision) {
        runtime.profile.no_change_turns = runtime.profile.no_change_turns.saturating_add(1);
        runtime.profile.fast_unchanged_turns =
            runtime.profile.fast_unchanged_turns.saturating_add(1);
        return Ok(None);
    }

    let phase_started_ns = crate::chronos::monotonic_nanos();
    let (broker_revision, windows) = visible_windows_for_output_with_revision(output);
    validate_window_snapshot(output, &windows)?;
    runtime.profile.snapshot_ns = runtime
        .profile
        .snapshot_ns
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = crate::chronos::monotonic_nanos();
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
    runtime.profile.lease_ns = runtime
        .profile
        .lease_ns
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = crate::chronos::monotonic_nanos();
    // Resolve the display topology once for this broker snapshot. Calling the
    // compatibility resolver independently for all four plane plans rereads
    // every pipe's MMIO state four times on every unchanged compositor scan.
    let (output_width, output_height) = crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
    let plans = [
        build_plane_plan(
            &runtime.composition.primary,
            &windows,
            &views,
            CompositionTarget::Primary,
            output_width,
            output_height,
        ),
        build_plane_plan(
            &runtime.composition.alpha,
            &windows,
            &views,
            CompositionTarget::Overlay(super::ALPHA_OVERLAY_PLANE_SLOT),
            output_width,
            output_height,
        ),
        build_plane_plan(
            &runtime.composition.solara,
            &windows,
            &views,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2),
            output_width,
            output_height,
        ),
        build_plane_plan(
            &runtime.composition.draw3d,
            &windows,
            &views,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3),
            output_width,
            output_height,
        ),
    ];
    runtime.profile.plan_ns = runtime
        .profile
        .plan_ns
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));
    if !plans.iter().any(|plan| plan.changed) {
        runtime.profile.no_change_turns = runtime.profile.no_change_turns.saturating_add(1);
        runtime.observed_broker_revision = Some(broker_revision);
        release_leases(&leases);
        return Ok(None);
    }
    runtime.profile.prepared_frames = runtime.profile.prepared_frames.saturating_add(1);

    Ok(Some(PendingFrame {
        windows,
        leases,
        plans,
        next_plane: 0,
        active: None,
        completed: Vec::new(),
        direct_leases: [None; 4],
        direct_windows: [None; 4],
        flip_submitted: false,
        broker_revision,
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
    output_width: u32,
    output_height: u32,
) -> PlanePlan {
    let plane_slot = target_plane_slot(target);
    let window_capacity = all_windows
        .iter()
        .filter(|window| window.plane.slot() == plane_slot)
        .count();
    let mut plan = PlanePlan::empty(target, window_capacity);
    if window_capacity == 0 && !state.initialized {
        return plan;
    }

    let mut composition_changed = !state.initialized;
    for (global_slot, window) in all_windows
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, window)| window.plane.slot() == plane_slot)
    {
        let placement = presentation_placement(window, views[global_slot]);
        let current = CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement,
        };
        plan.next_windows.push(current);
        let previous = state
            .windows
            .iter()
            .find(|previous| previous.id == current.id);
        let changed = !state.initialized || previous != Some(&current);
        composition_changed |= changed;
        if changed {
            plan.selected = Some(current.id);
        }
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
                // Every swap surface can still contain the frame at its old
                // placement. Clear the exposed old bounds as well as drawing
                // the new bounds; close animations do not publish another
                // producer frame which could repair those pixels later.
                add_placement_change_damage(
                    &mut plan.damage,
                    previous.placement,
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
    for previous in &state.windows {
        if !plan
            .next_windows
            .iter()
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
        let phase_started_ns = crate::chronos::monotonic_nanos();
        let poll = crate::intel::poll_ui4_plane_surface_flip_batch();
        runtime.profile.surflive_polls = runtime.profile.surflive_polls.saturating_add(1);
        runtime.profile.surflive_poll_ns = runtime
            .profile
            .surflive_poll_ns
            .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));
        return match poll {
            crate::intel::Ui4PlaneSurfaceFlipPoll::Pending => Ok(DriveResult::Pending),
            crate::intel::Ui4PlaneSurfaceFlipPoll::Complete { wait_ns, polls } => {
                trace_video_surflive_transitions(runtime, pending, wait_ns, polls);
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
        let phase_started_ns = crate::chronos::monotonic_nanos();
        let poll = crate::intel::poll_ui4_composition(active);
        runtime.profile.guc_polls = runtime.profile.guc_polls.saturating_add(1);
        runtime.profile.guc_poll_ns = runtime
            .profile
            .guc_poll_ns
            .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));
        match poll {
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

    let phase_started_ns = crate::chronos::monotonic_nanos();
    while pending.next_plane < pending.plans.len() {
        let plan_index = pending.next_plane;
        pending.next_plane += 1;
        if !pending.plans[plan_index].changed {
            continue;
        }
        let plan = pending.plans[plan_index].queue_plan();
        let queued = queue_async_plane(pending, plan);
        runtime.profile.submit_steps = runtime.profile.submit_steps.saturating_add(1);
        runtime.profile.submit_ns = runtime
            .profile
            .submit_ns
            .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));
        pending.active = Some(queued?);
        return Ok(DriveResult::Pending);
    }

    let submit_result =
        if pending.completed.is_empty() || !crate::intel::begin_ui4_plane_surface_flip_batch() {
            Err(Ui4CompositorError::PresentFailed)
        } else {
            let mut staged = true;
            for composition in pending.completed.iter().copied() {
                staged &= crate::intel::stage_ui4_composition_flip(composition);
                if !staged {
                    break;
                }
            }
            if staged && crate::intel::submit_ui4_plane_surface_flip_batch() {
                Ok(())
            } else {
                Err(Ui4CompositorError::PresentFailed)
            }
        };
    runtime.profile.submit_steps = runtime.profile.submit_steps.saturating_add(1);
    runtime.profile.submit_ns = runtime
        .profile
        .submit_ns
        .saturating_add(crate::chronos::monotonic_nanos().saturating_sub(phase_started_ns));
    submit_result?;
    pending.flip_submitted = true;
    Ok(DriveResult::Pending)
}

fn queue_async_plane(
    pending: &mut PendingFrame,
    plan: PlaneQueuePlan,
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
    // Explicitly shareable frames compose into one broker plane instead of
    // consuming one hardware plane per window. Immutable/single sources use
    // the sparse BCS/CPU painter. Dirty/double FontScene sources keep a stable
    // published front while their producer writes the back buffer, then use
    // the ordinary GPU compositor. Plane slots are independent scanout inputs,
    // so overlap is considered only inside this already slot-filtered set.
    let all_shared_composable = !selected.is_empty()
        && selected.iter().all(|(window, _)| {
            frame_snapshot(window.frame)
                .is_ok_and(|snapshot| super::frame_plan_shares_compositor_plane(snapshot.plan))
        });
    let all_static_single = all_shared_composable
        && selected.iter().all(|(window, _)| {
            frame_snapshot(window.frame).is_ok_and(|snapshot| {
                snapshot.plan.cadence == super::FrameCadence::Immutable
                    && snapshot.plan.buffering == super::FrameBuffering::Single
            })
        });
    let all_dirty_double_font = all_shared_composable
        && selected.iter().all(|(window, _)| {
            frame_snapshot(window.frame).is_ok_and(|snapshot| {
                matches!(
                    (snapshot.plan.content, snapshot.plan.cadence, snapshot.plan.buffering),
                    (
                        FrameContent::FontScene2d,
                        super::FrameCadence::Dirty,
                        super::FrameBuffering::Double
                    )
                )
            })
        });
    if all_dirty_double_font && !DIRTY_FONT_SHARED_COMPOSITION_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "ui4";
            "ui4/font-composition: mode=shared-slot plane={} windows={} buffering=dirty/double backend=guc-rcs-rgba8-batch source_ownership=stable-front+producer-back direct_scanout=0 cpu_frame_copy=0 log=once\n",
            target_plane_slot(plan.target),
            selected.len(),
        );
    }
    let sparse_static_painter = all_static_single;
    if sparse_static_painter
        && same_slot_windows_overlap(&selected)
        && !STATIC_SINGLE_OVERLAP_WARNED.swap(true, Ordering::AcqRel)
    {
        crate::log_warn!(target: "ui4";
            "ui4/static-painter: same-slot overlap detected slot={} windows={} needs_threadment=1 action=painter-order-baseline zstack-specialization=deferred log=once\n",
            target_plane_slot(plan.target), selected.len(),
        );
    }
    let local_direct = if selected.len() == 1 {
        selected.first().copied()
    } else if !all_shared_composable {
        plan.selected
            .and_then(|id| selected.iter().copied().find(|(window, _)| window.id == id))
            .or_else(|| selected.last().copied())
    } else {
        None
    };
    if let Some((window, view)) = local_direct {
        let slot = target_plane_slot(plan.target);
        if direct_overlay_eligible(window, view) {
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
                            super::FrameBuffering::Quad => "quad",
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
                    pending.direct_windows[slot] = Some(window.id);
                    return Ok(composition);
                }
                Err(_) => {
                    let _ = release_published_frame(display_lease);
                    crate::log_warn!(target: "ui4";
                        "ui4/direct-present: display import unavailable slot={} frame={} buffer={} action=leave-live-plane-and-retry cpu-copy-fallback=0 compositor-blend-fallback=0\n",
                        slot,
                        display_lease.frame.raw(),
                        display_lease.buffer_index,
                    );
                }
            }
        }
    }
    if !all_shared_composable && !selected.is_empty() {
        return Err(Ui4CompositorError::PresentFailed);
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
    let slot = target_plane_slot(plan.target);
    let reason = overlay_async_reason(slot);
    let queued = if sparse_static_painter && STATIC_SINGLE_CPU_PAINTER_BASELINE_ENABLED {
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
                        "ui4/static-painter: backend=guc-bcs0-fast-copy buffering=single plane_isolation=slot-local batch=one-per-changed-plane completion=mi-flush-dw-post-sync flip=after-retire cpu_pixel_copy=0 shader_dispatches=0 clear=fresh-transparent-only log=once\n"
                    );
                }
                Ok(composition)
            }
            Err(crate::intel::Ui4AsyncCompositionError::Unavailable) => {
                if !STATIC_SINGLE_CPU_BASELINE_LOGGED.swap(true, Ordering::AcqRel) {
                    crate::log_warn!(target: "ui4";
                        "ui4/static-painter-fallback: backend=cpu-sparse-copy reason=non-pristine-or-bcs-unavailable buffering=single plane_isolation=slot-local guc_jobs=0 shader_dispatches=0 damage=old+new-placement flip=ui4-batched log=once\n"
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
            window.plane.slot() < super::INTERACTION_OVERLAY_PLANE_SLOT
                && snapshot.plan.content == FrameContent::RenderScene3d
                && snapshot.plan.buffering == super::FrameBuffering::Triple
        }
        FrameGpuRelease::Compute(_) => {
            window.plane.slot() < super::INTERACTION_OVERLAY_PLANE_SLOT
                && matches!(
                    (snapshot.plan.content, snapshot.plan.buffering),
                    (FrameContent::Image, super::FrameBuffering::Double)
                        | (FrameContent::BlueprintScene, super::FrameBuffering::Triple)
                        | (FrameContent::Video, super::FrameBuffering::Quad)
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
    let scaler_safe_maximize = window.maximized
        && window.interaction.resize_on_maximize
        && half_scale_backing_matches(placement.width, placement.height, view.width, view.height);
    let scaler_safe_close = window.state == super::WindowState::Closing
        && placement.width >= 8
        && placement.height >= 8
        && placement.width <= view.width
        && placement.height <= view.height
        && u64::from(view.width) < u64::from(placement.width).saturating_mul(3)
        && u64::from(view.height) < u64::from(placement.height).saturating_mul(3);
    placement.x >= 0
        && placement.y >= 0
        && (exact_size || scaler_safe_maximize || scaler_safe_close)
        && (placement.x as u32)
            .checked_add(placement.width)
            .is_some_and(|right| right <= output_width)
        && (placement.y as u32)
            .checked_add(placement.height)
            .is_some_and(|bottom| bottom <= output_height)
}

/// A resize-capable producer may need several service turns to replace its
/// complete Frame ring after maximize. A deliberate half-resolution backing
/// is scaled to the logical extent by the direct display plane. Any other old
/// allocation remains centered at 1:1 until the replacement arrives.
fn presentation_placement(window: WindowSnapshot, view: FrameRgbaView) -> WindowPlacement {
    let placement = window.placement;
    if !window.maximized
        || !window.interaction.resize_on_maximize
        || (placement.width == view.width && placement.height == view.height)
        || half_scale_backing_matches(placement.width, placement.height, view.width, view.height)
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

const fn half_scale_backing_matches(
    logical_width: u32,
    logical_height: u32,
    backing_width: u32,
    backing_height: u32,
) -> bool {
    logical_width > backing_width
        && logical_height > backing_height
        && logical_width.div_ceil(2) == backing_width
        && logical_height.div_ceil(2) == backing_height
}

const fn overlay_async_reason(slot: usize) -> &'static str {
    match slot {
        super::PRIMARY_PLANE_SLOT => "ui4-rgba-slot0-direct",
        super::ALPHA_OVERLAY_PLANE_SLOT => "ui4-alpha-slot1-async",
        super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-rgb-slot2-async",
        super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-rgb-slot3-async",
        _ => "ui4-overlay-async",
    }
}

fn commit_async_frame(runtime: &mut Runtime, pending: &mut PendingFrame) {
    for plan_index in 0..pending.plans.len() {
        if !pending.plans[plan_index].changed {
            continue;
        }
        let target = pending.plans[plan_index].target;
        let next_windows = core::mem::take(&mut pending.plans[plan_index].next_windows);
        let state = match target {
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
        state.windows = next_windows;
        let slot = target_plane_slot(target);
        if let Some(id) = pending.direct_windows[slot] {
            if let Some(window) = pending.windows.iter().find(|window| window.id == id) {
                let _ = acknowledge_window_frame(window.id, window.publish_serial);
            }
        } else {
            for window in pending
                .windows
                .iter()
                .filter(|window| window.plane.slot() == slot)
            {
                let _ = acknowledge_window_frame(window.id, window.publish_serial);
            }
        }
    }
    // Compositor-rewire has no screenshot consumer. Keeping capture entirely
    // out of the only supported present path also makes the no-CPU-pixel-read
    // contract structural rather than dependent on an empty request queue.
    runtime.observed_broker_revision = Some(pending.broker_revision);
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
    for plan in pending.plans.iter().filter(|plan| plan.changed) {
        let slot = target_plane_slot(plan.target);
        let replacement = pending.direct_leases[slot].take();
        let previous = core::mem::replace(&mut runtime.live_direct[slot], replacement);
        if let Some(previous) = previous {
            release_replaced_direct_lease(slot, previous, "flip-batch-complete");
        }
    }
}

fn trace_video_surflive_transitions(
    runtime: &Runtime,
    pending: &PendingFrame,
    flip_wait_ns: u64,
    flip_polls: u32,
) {
    let observed_ns = crate::chronos::monotonic_nanos();
    let transaction_ns = observed_ns.saturating_sub(pending.started_ns);
    let pre_flip_ns = transaction_ns.saturating_sub(flip_wait_ns);
    let guc_jobs = pending
        .completed
        .iter()
        .copied()
        .filter(|composition| crate::intel::ui4_composition_has_guc_work(*composition))
        .count();
    for plan in pending.plans.iter().filter(|plan| plan.changed) {
        let slot = target_plane_slot(plan.target);
        let Some(replacement) = pending.direct_leases[slot] else {
            continue;
        };
        if !frame_snapshot(replacement.frame)
            .is_ok_and(|snapshot| snapshot.plan.content == FrameContent::Video)
        {
            continue;
        }
        let publish_serial = pending.direct_windows[slot]
            .and_then(|id| pending.windows.iter().find(|window| window.id == id))
            .map_or(0, |window| window.publish_serial);
        let previous = runtime.live_direct[slot];
        crate::log_trace!(target: "ui4";
            "ui4 video-surface-lifecycle event=surflive-observed frame={} buffer={} publish_serial={} slot={} previous_frame={} previous_buffer={} observed_ns={} transaction_us={} pre_flip_us={} flip_wait_us={} flip_polls={} batch_planes={} batch_guc_jobs={} boundary=surflive-before-old-display-release ownership_unchanged=1\n",
            replacement.frame.raw(),
            replacement.buffer_index,
            publish_serial,
            slot,
            previous.map_or(0, |lease| lease.frame.raw()),
            previous.map_or(u8::MAX, |lease| lease.buffer_index),
            observed_ns,
            transaction_ns / 1_000,
            pre_flip_ns / 1_000,
            flip_wait_ns / 1_000,
            flip_polls,
            pending.completed.len(),
            guc_jobs,
        );
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
        crate::log_trace!(target: "ui4";
            "ui4 video-surface-lifecycle event=display-release seq={} frame={} buffer={} slot={} released={} observed_ns={} boundary=surflive-{} ownership_unchanged=1\n",
            sequence,
            lease.frame.raw(),
            lease.buffer_index,
            slot,
            released as u8,
            crate::chronos::monotonic_nanos(),
            boundary,
        );
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
    for slot in 0..pending.direct_leases.len() {
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

fn add_placement_change_damage(
    region: &mut crate::intel::CompositionDamageRegion,
    previous: WindowPlacement,
    current: WindowPlacement,
    output_width: u32,
    output_height: u32,
) {
    add_placement_damage(region, previous, output_width, output_height);
    add_placement_damage(region, current, output_width, output_height);
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
    fn maximized_half_resolution_backing_is_an_explicit_two_x_scale() {
        assert!(half_scale_backing_matches(2560, 1440, 1280, 720));
        assert!(half_scale_backing_matches(2559, 1439, 1280, 720));
        assert!(!half_scale_backing_matches(2560, 1440, 640, 400));
        assert!(!half_scale_backing_matches(2560, 1440, 2560, 1440));
    }

    #[test]
    fn placement_change_damages_exposed_old_bounds() {
        let previous = WindowPlacement {
            x: 100,
            y: 80,
            width: 320,
            height: 200,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        let current = WindowPlacement {
            x: 180,
            y: 130,
            width: 160,
            height: 100,
            opacity: 96,
            ..previous
        };
        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_placement_change_damage(&mut damage, previous, current, 1920, 1080);
        assert_eq!(damage.rects(), &[crate::intel::CompositionDamageRect::new(100, 80, 320, 200)]);
    }

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
