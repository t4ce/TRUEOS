//! Window ownership and placement over UI4 frames.
//!
//! The broker contains no rendering, decorations, input policy, guest
//! transport, or Blueprint ABI. A later transport must derive `WindowOwner`
//! from its trusted execution context rather than accepting it from a client.

use alloc::vec::Vec;
use embassy_sync::{
    signal::Signal,
    watch::{Receiver as WatchReceiver, Watch},
};
use heapless::Deque;
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::{DamageRect, DamageRegion, FrameBuffering, FrameHandle, OutputId};

pub(super) const MAX_WINDOWS: usize = 256;
// Temporary static30 composition probe: one trusted app session owns all 30
// test windows. Plane assignment is independent of session ownership, while
// MAX_WINDOWS remains only the broker registry's hard storage bound.
const MAX_WINDOWS_PER_SESSION: usize = 32;
const MAX_SESSIONS: usize = 64;
/// UI4 presents four hardware-blended application layers per output. Planes
/// 1-3 are lease planes: one frame each, presented by the display engine and
/// eligible for direct scanout. Plane 0 is the stack, where every frame which
/// holds no lease presents together through one ordered painter submission.
///
/// Create/close rebalancing gives the highest three frames in broker-z order
/// the lease planes, preserving that base order in the display engine; every
/// lower frame shares Slot0. Focus/hot interaction may then promote a stacked
/// frame and demote a lease holder. No admission ever fails: a frame which wins
/// no lease is still presented, composed, and movable. This policy does not
/// consult content, cadence or buffering.
pub(super) const STACK_PLANE_SLOT: usize = super::PRIMARY_PLANE_SLOT;
const FIRST_LEASE_PLANE_SLOT: usize = STACK_PLANE_SLOT + 1;
pub(super) const LEASE_PLANE_COUNT: usize =
    super::INTERACTION_OVERLAY_PLANE_SLOT - FIRST_LEASE_PLANE_SLOT;
/// Idle grace after the last interaction before a lease becomes revocable.
/// Revocation stays lazy: an expired lease is only taken when another frame
/// actually challenges for it, so an idle winner keeps its plane for free and
/// a continuously dragged frame is never evicted mid-gesture.
const LEASE_IDLE_GRACE_MS: u64 = 500;
pub(crate) const WINDOW_BROKER_SNAPSHOT_PERIOD_MS: u64 = 3_000;
const WINDOW_BROKER_SNAPSHOT_RECEIVERS: usize = 8;
const WINDOW_FIRST_PRESENTATION_QUEUE_CAP: usize = 32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowOwner {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Kernel,
    /// Temporary trusted-app identity used before Blueprint transport exists.
    KernelApp(u8),
    Vm(u8),
}

impl WindowOwner {
    /// Stable trusted identities for the kernel producers currently mapped
    /// into the UI4 frame/window model. Keep these assignments in one place so
    /// newly reactivated producers cannot silently collide.
    pub(crate) const VIDEO_PLAYER: Self = Self::KernelApp(2);
    pub(crate) const GRIDPAPER_SERVICE: Self = Self::KernelApp(4);
    pub(crate) const GPGPU_PREVIEW: Self = Self::KernelApp(5);
    pub(crate) const COLOR_PICKER_SERVICE: Self = Self::KernelApp(6);
    pub(crate) const SVG_OUTLINE_PROBE: Self = Self::KernelApp(7);
    pub(crate) const START_BUTTON_SERVICE: Self = Self::KernelApp(8);

    /// Stable, allocation-free producer name for diagnostics. The enum still
    /// carries the application or VM instance where the name is shared.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Kernel => "kernel",
            Self::VIDEO_PLAYER => "video-player",
            Self::GRIDPAPER_SERVICE => "gridpaper-service",
            Self::GPGPU_PREVIEW => "gpgpu-preview",
            Self::COLOR_PICKER_SERVICE => "color-picker-service",
            Self::SVG_OUTLINE_PROBE => "svg-outline-probe",
            Self::START_BUTTON_SERVICE => "start-button-service",
            Self::KernelApp(_) => "kernel-app",
            Self::Vm(_) => "blueprint-vm",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct WindowId(u32);

impl WindowId {
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct WindowSessionId(u32);

impl WindowSessionId {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowState {
    Pending,
    Ready,
    Closing,
    Closed,
}

/// Fixed presentation target selected when a broker window is created.
/// Runtime migration is deliberately not part of this contract.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowPlane {
    Primary,
    Universal(u8),
    /// UI4's topmost software-cursor/interaction plane. The slot-4 service,
    /// rather than the application compositor, paints this window.
    Interaction,
}

impl WindowPlane {
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::Primary => super::PRIMARY_PLANE_SLOT,
            Self::Universal(slot) => slot as usize,
            Self::Interaction => super::INTERACTION_OVERLAY_PLANE_SLOT,
        }
    }

    pub(crate) const fn is_application(self) -> bool {
        !matches!(self, Self::Interaction)
    }

    const fn valid(self) -> bool {
        match self {
            Self::Primary => true,
            Self::Universal(slot) => {
                slot > 0 && (slot as usize) < super::INTERACTION_OVERLAY_PLANE_SLOT
            }
            Self::Interaction => true,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    const fn from_slot(slot: usize) -> Option<Self> {
        match slot {
            super::PRIMARY_PLANE_SLOT => Some(Self::Primary),
            1..super::INTERACTION_OVERLAY_PLANE_SLOT => Some(Self::Universal(slot as u8)),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowPlacement {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) z: i32,
    pub(crate) opacity: u8,
    pub(crate) visible: bool,
}

impl WindowPlacement {
    const fn valid(self) -> bool {
        self.width != 0 && self.height != 0
    }
}

/// Broker-owned interaction policy for one window.
///
/// Moving frame geometry is a UI4 operation and does not imply that the
/// producer implements an input callback queue or dynamic resize. This keeps
/// simple GPU producers movable without feeding them events
/// they cannot consume or changing the render-target extent behind them.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowInteraction {
    pub(crate) movable: bool,
    /// Enables top-center maximize plus half-screen and quadrant docking.
    pub(crate) maximizable: bool,
    pub(crate) receives_input: bool,
    /// Whether the window participates in cursor selection and pointer hit tests.
    pub(crate) hit_testable: bool,
    /// The producer can replace its complete frame allocation after a
    /// dock/restore extent notification. Fixed-size producers still dock as a
    /// broker-owned 1:1 centering operation inside the selected region.
    pub(crate) resize_on_maximize: bool,
}

/// What UI4 does with Escape for one selected frame.
///
/// Closing is deliberately the default: frames behave like ordinary app
/// windows unless that particular frame explicitly reserves Escape for its
/// own interaction.  This is a frame property rather than an owner property
/// so a dialog and its parent can choose independently.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Ui4FrameEscapeKeyAction {
    #[default]
    Close = 0,
    DeliverToApplication = 1,
}

impl WindowInteraction {
    /// UI4 may translate the frame, while its producer remains independent of
    /// pointer/keyboard delivery and keeps a fixed pixel extent.
    pub(crate) const MOVABLE_FRAME: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: false,
        hit_testable: true,
        resize_on_maximize: false,
    };

    /// The producer consumes application input and can be centered/restored,
    /// but its native frame extent remains fixed throughout that transition.
    pub(crate) const APPLICATION_FIXED_FRAME: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: true,
        hit_testable: true,
        resize_on_maximize: false,
    };

    /// Full broker interaction for producers which drain owner events and can
    /// replace their frame allocation after a resize notification.
    pub(crate) const APPLICATION: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: true,
        hit_testable: true,
        resize_on_maximize: true,
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowCreate {
    pub(crate) owner: WindowOwner,
    pub(crate) session: WindowSessionId,
    pub(crate) frame: FrameHandle,
    pub(crate) output: OutputId,
    pub(crate) plane: WindowPlane,
    pub(crate) placement: WindowPlacement,
    pub(crate) interaction: WindowInteraction,
}

/// Optional work performed while a session still owns coherent published
/// frames. Ordinary teardown remains allocation-free and captures nothing.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowSessionCloseRequest<'a> {
    persist_final_frame: bool,
    final_frame_name: Option<&'a str>,
    animate: bool,
    puff: bool,
    direct_plane_scaling: bool,
    retire_frames: bool,
}

impl<'a> WindowSessionCloseRequest<'a> {
    /// Persist the last published frame under UI4's owner-derived identity.
    pub(crate) const fn persist_final_frame(mut self) -> Self {
        self.persist_final_frame = true;
        self
    }

    /// Persist the last published frame under an explicit stable identity.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn persist_final_frame_as(mut self, name: &'a str) -> Self {
        self.persist_final_frame = true;
        self.final_frame_name = Some(name);
        self
    }

    /// Keep each published window alive as a compositor-owned exit visual.
    /// The producer retains ownership of the underlying frames.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn animate(mut self) -> Self {
        self.animate = true;
        self.puff = true;
        self
    }

    /// Animate the close and transfer the detached frame lifetime to UI4.
    pub(crate) const fn animate_and_retire_frames(mut self) -> Self {
        self.animate = true;
        self.puff = true;
        self.retire_frames = true;
        self
    }

    /// Slightly enlarge and fade direct planes using only pipe-scaler geometry and
    /// constant alpha. The exact published allocation and source geometry
    /// remain unchanged until the final SURFLIVE-backed retirement.
    pub(crate) const fn direct_plane_animate_and_retire_frames(mut self) -> Self {
        self.animate = true;
        self.puff = true;
        self.direct_plane_scaling = true;
        self.retire_frames = true;
        self
    }

    /// Slightly enlarge and fade a direct plane while the producer keeps ownership of
    /// the underlying frame ring for a later presentation session.
    pub(crate) const fn direct_plane_animate(mut self) -> Self {
        self.animate = true;
        self.puff = true;
        self.direct_plane_scaling = true;
        self
    }

    pub(crate) const fn transfers_frame_ownership(self) -> bool {
        self.retire_frames
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowSnapshot {
    pub(crate) id: WindowId,
    pub(crate) owner: WindowOwner,
    pub(crate) producer_name: &'static str,
    pub(crate) session: WindowSessionId,
    pub(crate) frame: FrameHandle,
    pub(crate) buffering: FrameBuffering,
    pub(crate) output: OutputId,
    pub(crate) plane: WindowPlane,
    /// Producer/input authority. Extent changes here are delivered to the
    /// application exactly once and are never stepped by a UI4 animation.
    pub(crate) placement: WindowPlacement,
    /// Display-only geometry for the currently published surface. UI4 may
    /// advance this independently while `placement` already names the final
    /// logical target.
    pub(crate) presentation_placement: WindowPlacement,
    pub(crate) interaction: WindowInteraction,
    pub(crate) state: WindowState,
    pub(crate) revision: u64,
    pub(crate) publish_serial: u64,
    pub(crate) damage: Option<DamageRegion>,
    pub(crate) maximized: bool,
    pub(crate) dock_target: Option<WindowDockTarget>,
}

/// Small aggregate view accompanying a published broker snapshot.
///
/// `composable_windows` matches the broker-side conditions for a window to be
/// considered by a compositor: Ready/Closing, visible, and not yet closed.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowBrokerSnapshotStats {
    pub(crate) registry_window_slots: usize,
    pub(crate) registry_session_slots: usize,
    pub(crate) active_sessions: usize,
    pub(crate) live_windows: usize,
    pub(crate) composable_windows: usize,
    pub(crate) pending_windows: usize,
    pub(crate) ready_windows: usize,
    pub(crate) closing_windows: usize,
    pub(crate) damaged_windows: usize,
    pub(crate) maximized_windows: usize,
    pub(crate) windows_per_output: [usize; super::OUTPUT_COUNT],
}

/// Low-frequency, informational copy of the complete live window registry.
///
/// This is deliberately separate from the compositor's live snapshots.
/// Reading it never locks or otherwise influences the broker, and stale data
/// is expected between periodic publications.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowBrokerSnapshot {
    pub(crate) update_count: u64,
    pub(crate) published_at_ms: u64,
    pub(crate) composition_revision: u64,
    pub(crate) stats: WindowBrokerSnapshotStats,
    pub(crate) windows: Vec<WindowSnapshot>,
}

impl WindowBrokerSnapshot {
    pub(crate) const fn empty() -> Self {
        Self {
            update_count: 0,
            published_at_ms: 0,
            composition_revision: 0,
            stats: WindowBrokerSnapshotStats {
                registry_window_slots: 0,
                registry_session_slots: 0,
                active_sessions: 0,
                live_windows: 0,
                composable_windows: 0,
                pending_windows: 0,
                ready_windows: 0,
                closing_windows: 0,
                damaged_windows: 0,
                maximized_windows: 0,
                windows_per_output: [0; super::OUTPUT_COUNT],
            },
            windows: Vec::new(),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn has_data(&self) -> bool {
        self.update_count != 0
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) type WindowBrokerSnapshotReceiver<'a> = WatchReceiver<
    'a,
    crate::wait::EmbassySpinRawMutex,
    WindowBrokerSnapshot,
    WINDOW_BROKER_SNAPSHOT_RECEIVERS,
>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowPlacementTransition {
    pub(crate) previous: WindowPlacement,
    pub(crate) placement: WindowPlacement,
    pub(crate) dock_target: Option<WindowDockTarget>,
}

/// Screen-owned placement selected by a secondary-button drag/drop gesture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowDockTarget {
    Maximize,
    LeftHalf,
    RightHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowBrokerError {
    InvalidHandle,
    OwnerMismatch,
    SessionClosed,
    EmptyExtent,
    EmptyDamage,
    InvalidPlane,
    InteractionDenied,
    Capacity,
    Closed,
}

#[derive(Copy, Clone)]
struct WindowRecord {
    generation: u16,
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    buffering: FrameBuffering,
    output: OutputId,
    plane: WindowPlane,
    placement: WindowPlacement,
    interaction: WindowInteraction,
    escape_key_action: Ui4FrameEscapeKeyAction,
    state: WindowState,
    revision: u64,
    publish_serial: u64,
    /// Recent exact publish serials observed only after the compositor's
    /// batched SURFLIVE boundary. This stays separate from damage ACK because
    /// a producer may publish again while an older transaction is pending.
    presented_serials: [u64; 8],
    presented_serial_cursor: u8,
    first_presentation_emitted: bool,
    first_presentation_taken: bool,
    damage: Option<DamageRegion>,
    restore_placement: Option<WindowPlacement>,
    dock_target: Option<WindowDockTarget>,
    replacement_presentation: Option<WindowPlacement>,
    open_transition: Option<WindowOpenTransition>,
    close_transition: Option<WindowCloseTransition>,
}

#[derive(Copy, Clone)]
struct WindowOpenTransition {
    initial: WindowPlacement,
    current: WindowPlacement,
    started_ms: u64,
    duration_ms: u64,
    shrink_per_mille: u64,
}

#[derive(Copy, Clone)]
struct WindowCloseTransition {
    lease: super::FrameReadLease,
    initial: WindowPlacement,
    started_ms: u64,
    delay_ms: u64,
    duration_ms: u64,
    puff_per_mille: u64,
    retire_frame: bool,
}

#[derive(Copy, Clone)]
struct WindowTransitionRetirement {
    lease: super::FrameReadLease,
    frame: FrameHandle,
    retire_frame: bool,
    elapsed_total_ms: u64,
}

struct WindowSessionFinish {
    closed: usize,
    animated: usize,
    animation_skipped: usize,
    animation_duration_ms: u64,
    final_scale_percent: u64,
    final_frames: Vec<WindowSnapshot>,
    immediate_retire_frames: Vec<FrameHandle>,
}

const CLOSE_TRANSITION_DURATION_MS: u64 = 300;
/// Five percent is visible without making the exit feel like a zoom.
const CLOSE_TRANSITION_PUFF_PER_MILLE: u64 = 50;
const OPEN_TRANSITION_SHRINK_PER_MILLE: u64 = 900;
const DIRECT_PLANE_CLOSE_WAVE_DURATION_MS: u64 = 200;
const DIRECT_PLANE_CLOSE_PUFF_PER_MILLE: u64 = 50;
// Opening retains the established 35%-to-100% reveal on direct planes.
const DIRECT_PLANE_OPEN_SHRINK_PER_MILLE: u64 = 650;

#[derive(Copy, Clone)]
struct SessionRecord {
    generation: u16,
    owner: WindowOwner,
    active: bool,
}

/// One hardware lease plane's current holder.
///
/// The lease is owned by a frame, not by a cursor: N cursors may be dragging
/// N frames, and each frame's claim is independent of which cursor moved it.
#[derive(Copy, Clone)]
struct PlaneLease {
    window: WindowId,
    last_hot_ms: u64,
}

struct WindowBroker {
    windows: Vec<WindowRecord>,
    sessions: Vec<SessionRecord>,
    /// Indexed by lease plane, i.e. `FIRST_LEASE_PLANE_SLOT + index`.
    leases: [Option<PlaneLease>; LEASE_PLANE_COUNT],
    /// Monotonic epoch for state which can change the compositor's plane plan.
    /// Damage acknowledgement deliberately does not advance this value.
    composition_revision: u64,
}

impl WindowBroker {
    const fn new() -> Self {
        Self {
            windows: Vec::new(),
            sessions: Vec::new(),
            leases: [None; LEASE_PLANE_COUNT],
            composition_revision: 0,
        }
    }

    const fn lease_index(slot: usize) -> Option<usize> {
        if slot >= FIRST_LEASE_PLANE_SLOT && slot < super::INTERACTION_OVERLAY_PLANE_SLOT {
            Some(slot - FIRST_LEASE_PLANE_SLOT)
        } else {
            None
        }
    }

    const fn lease_plane(index: usize) -> WindowPlane {
        WindowPlane::Universal((FIRST_LEASE_PLANE_SLOT + index) as u8)
    }

    /// Drop this window's lease if it holds one, freeing the plane for the
    /// next claimant. Safe to call for a window which never held a lease.
    fn release_lease_of(&mut self, id: WindowId) {
        for lease in &mut self.leases {
            if lease.is_some_and(|held| held.window == id) {
                *lease = None;
            }
        }
    }

    /// Claim a lease plane for `id`, preferring a free one and otherwise
    /// revoking the plane whose holder has been idle longest past the grace
    /// period. Returns `None` when every lease is still live, in which case
    /// the caller stays on the stack - never an error.
    fn claim_lease(&mut self, id: WindowId, now_ms: u64) -> Option<WindowPlane> {
        let mut chosen = None;
        for index in 0..LEASE_PLANE_COUNT {
            let vacant = match self.leases[index] {
                None => true,
                Some(held) => self.window_is_closed(held.window),
            };
            if vacant {
                chosen = Some(index);
                break;
            }
        }
        let index = match chosen {
            Some(index) => index,
            None => {
                // Every plane is still held. Take the one whose holder has
                // been idle longest, and only once it is past the grace
                // period; otherwise this frame stays on the stack.
                let mut oldest: Option<(usize, u64)> = None;
                for index in 0..LEASE_PLANE_COUNT {
                    let Some(held) = self.leases[index] else {
                        continue;
                    };
                    if now_ms.saturating_sub(held.last_hot_ms) < LEASE_IDLE_GRACE_MS {
                        continue;
                    }
                    if oldest.is_none_or(|(_, idle_ms)| held.last_hot_ms < idle_ms) {
                        oldest = Some((index, held.last_hot_ms));
                    }
                }
                oldest?.0
            }
        };
        // Demote the outgoing holder before the plane changes hands, or its
        // record would keep naming a plane it no longer owns and two windows
        // would present on the same hardware layer.
        if let Some(previous) = self.leases[index] {
            self.demote_to_stack(previous.window);
        }
        self.leases[index] = Some(PlaneLease {
            window: id,
            last_hot_ms: now_ms,
        });
        Some(Self::lease_plane(index))
    }

    fn demote_to_stack(&mut self, id: WindowId) {
        self.set_window_plane(id, WindowPlane::Primary);
    }

    fn set_window_plane(&mut self, id: WindowId, plane: WindowPlane) {
        let Ok((slot, generation)) = unpack_handle(id.0) else {
            return;
        };
        let Some(window) = self.windows.get_mut(slot) else {
            return;
        };
        if window.generation != generation
            || window.state == WindowState::Closed
            || window.plane == WindowPlane::Interaction
            || window.plane == plane
        {
            return;
        }
        window.plane = plane;
        window.damage = Some(DamageRegion::FULL);
        window.revision = next_serial(window.revision);
    }

    /// Give the visible application windows their deterministic base layout.
    /// Up to three windows occupy Slots1-3. Once that capacity is exceeded,
    /// the bottom windows share Slot0 in broker-z order and the highest three
    /// retain the hardware planes. Since hardware planes blend in ascending
    /// slot order, this keeps physical and broker z-order consistent.
    fn rebalance_application_planes(&mut self, output: OutputId, now_ms: u64) {
        let mut ordered: Vec<(WindowId, i32, usize)> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| {
                window.state != WindowState::Closed
                    && window.output == output
                    && window.plane != WindowPlane::Interaction
            })
            .filter_map(|(slot, window)| {
                Some((
                    WindowId(pack_handle(slot, window.generation).ok()?),
                    window.placement.z,
                    slot,
                ))
            })
            .collect();
        ordered.sort_unstable_by_key(|(id, z, _)| (*z, *id));
        let stack_count = ordered.len().saturating_sub(LEASE_PLANE_COUNT);
        self.leases = [None; LEASE_PLANE_COUNT];
        for (rank, (id, _, slot)) in ordered.into_iter().enumerate() {
            let plane = if rank < stack_count {
                WindowPlane::Primary
            } else {
                Self::lease_plane(rank - stack_count)
            };
            let window = &mut self.windows[slot];
            if window.plane != plane {
                window.plane = plane;
                window.damage = Some(DamageRegion::FULL);
                window.revision = next_serial(window.revision);
            }
            if let Some(index) = Self::lease_index(plane.slot()) {
                self.leases[index] = Some(PlaneLease {
                    window: id,
                    last_hot_ms: now_ms,
                });
            }
        }
    }

    /// Raise the lease at `index` to the topmost lease plane, exchanging
    /// places with whoever holds it.
    ///
    /// Planes blend in ascending hardware order, so this is what stops a
    /// focused frame from staying visually behind another lease holder. It is
    /// a swap rather than a compaction: exactly two windows change plane, both
    /// commit in the same flip batch, and no other holder is disturbed.
    fn raise_lease(&mut self, index: usize, now_ms: u64) -> WindowPlane {
        let top = LEASE_PLANE_COUNT - 1;
        let raised = Self::lease_plane(top);
        if index == top {
            return raised;
        }
        // Lease grace governs whether a stacked window may reclaim a busy
        // hardware plane. It must not defer a focused lease holder: slot
        // order is global visual order, so focus must reach the top plane in
        // this same broker transaction.
        let hot = self.leases[index];
        let displaced = self.leases[top];
        self.leases[top] = hot.map(|held| PlaneLease {
            last_hot_ms: now_ms,
            ..held
        });
        self.leases[index] = displaced;
        if let Some(held) = hot {
            self.set_window_plane(held.window, raised);
        }
        if let Some(held) = displaced {
            self.set_window_plane(held.window, Self::lease_plane(index));
        }
        raised
    }

    fn window_is_closed(&self, id: WindowId) -> bool {
        let Ok((slot, generation)) = unpack_handle(id.0) else {
            return true;
        };
        self.windows.get(slot).is_none_or(|window| {
            window.generation != generation || window.state == WindowState::Closed
        })
    }

    fn mark_composition_changed(&mut self) {
        self.composition_revision = next_serial(self.composition_revision);
        WINDOW_COMPOSITION_CHANGED.signal(());
    }

    fn begin_session(
        &mut self,
        owner: WindowOwner,
    ) -> (Result<WindowSessionId, WindowBrokerError>, Vec<WindowTransitionRetirement>) {
        self.mark_composition_changed();
        let mut retirements = Vec::new();
        for session in &mut self.sessions {
            if session.active && session.owner == owner {
                session.active = false;
            }
        }
        for window in &mut self.windows {
            if window.state != WindowState::Closed && window.owner == owner {
                if let Some(transition) = window.close_transition.take() {
                    retirements.push(WindowTransitionRetirement {
                        lease: transition.lease,
                        frame: window.frame,
                        retire_frame: transition.retire_frame,
                        elapsed_total_ms: 0,
                    });
                }
                window.state = WindowState::Closed;
                window.damage = None;
                window.revision = next_serial(window.revision);
            }
        }

        if let Some((slot, session)) = self
            .sessions
            .iter_mut()
            .enumerate()
            .find(|(_, session)| !session.active)
        {
            session.generation = next_generation(session.generation);
            session.owner = owner;
            session.active = true;
            return (pack_handle(slot, session.generation).map(WindowSessionId), retirements);
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return (Err(WindowBrokerError::Capacity), retirements);
        }
        let slot = self.sessions.len();
        self.sessions.push(SessionRecord {
            generation: 1,
            owner,
            active: true,
        });
        (pack_handle(slot, 1).map(WindowSessionId), retirements)
    }

    /// Add an independent session for an owner which already has live
    /// windows. This is reserved for multi-frame producers: closing one
    /// session affects only its own windows, while ordinary `begin_session`
    /// retains the replace-all behavior used by single-scene producers.
    fn begin_additional_session(
        &mut self,
        owner: WindowOwner,
    ) -> Result<WindowSessionId, WindowBrokerError> {
        self.mark_composition_changed();
        if let Some((slot, session)) = self
            .sessions
            .iter_mut()
            .enumerate()
            .find(|(_, session)| !session.active)
        {
            session.generation = next_generation(session.generation);
            session.owner = owner;
            session.active = true;
            return pack_handle(slot, session.generation).map(WindowSessionId);
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(WindowBrokerError::Capacity);
        }
        let slot = self.sessions.len();
        self.sessions.push(SessionRecord {
            generation: 1,
            owner,
            active: true,
        });
        pack_handle(slot, 1).map(WindowSessionId)
    }

    fn checked_session(
        &self,
        owner: WindowOwner,
        id: WindowSessionId,
    ) -> Result<(), WindowBrokerError> {
        let (slot, generation) = unpack_handle(id.0)?;
        let session = self
            .sessions
            .get(slot)
            .ok_or(WindowBrokerError::InvalidHandle)?;
        if session.generation != generation {
            return Err(WindowBrokerError::InvalidHandle);
        }
        if session.owner != owner {
            return Err(WindowBrokerError::OwnerMismatch);
        }
        if !session.active {
            return Err(WindowBrokerError::SessionClosed);
        }
        Ok(())
    }

    /// Choose the presentation plane for one window.
    ///
    /// Slot 4 keeps its trusted-small-control contract. Only named kernel UI
    /// owners may enter it, and each may own one live interaction window. A
    /// Blueprint can therefore never promote a full application frame onto
    /// the software-cursor plane. Every other window joins the ordinary slot-0
    /// application stack. Content, cadence and buffering are deliberately not
    /// consulted for that stack.
    fn assign_plane(
        &mut self,
        requested: WindowPlane,
        id: WindowId,
        owner: WindowOwner,
        now_ms: u64,
    ) -> Result<WindowPlane, WindowBrokerError> {
        let (id_slot, _) = unpack_handle(id.0)?;
        if requested == WindowPlane::Interaction {
            if !matches!(
                owner,
                WindowOwner::COLOR_PICKER_SERVICE | WindowOwner::START_BUTTON_SERVICE
            ) {
                return Err(WindowBrokerError::InvalidPlane);
            }
            if self.windows.iter().enumerate().any(|(slot, window)| {
                slot != id_slot
                    && window.state != WindowState::Closed
                    && window.plane == WindowPlane::Interaction
                    && window.owner == owner
            }) {
                return Err(WindowBrokerError::Capacity);
            }
            return Ok(requested);
        }
        let _ = (id, now_ms);
        Ok(WindowPlane::Primary)
    }

    fn create(
        &mut self,
        mut request: WindowCreate,
        buffering: FrameBuffering,
        now_ms: u64,
    ) -> Result<WindowId, WindowBrokerError> {
        self.checked_session(request.owner, request.session)?;
        if !request.placement.valid() {
            return Err(WindowBrokerError::EmptyExtent);
        }
        if !request.plane.valid() {
            return Err(WindowBrokerError::InvalidPlane);
        }
        let count = self
            .windows
            .iter()
            .filter(|window| {
                window.state != WindowState::Closed && window.session == request.session
            })
            .count();
        if count >= MAX_WINDOWS_PER_SESSION {
            return Err(WindowBrokerError::Capacity);
        }
        // Reserve the registry slot before deciding the plane: a lease is
        // keyed by window identity, which does not exist until the slot and
        // its generation are known.
        let (slot, generation) = match self
            .windows
            .iter()
            .position(|window| window.state == WindowState::Closed)
        {
            Some(slot) => (slot, next_generation(self.windows[slot].generation)),
            None => {
                if self.windows.len() >= MAX_WINDOWS {
                    return Err(WindowBrokerError::Capacity);
                }
                (self.windows.len(), 1)
            }
        };
        let id = WindowId(pack_handle(slot, generation)?);
        let requested_plane = request.plane;
        request.plane = self.assign_plane(requested_plane, id, request.owner, now_ms)?;
        let record = WindowRecord::new(generation, request, buffering);
        if slot < self.windows.len() {
            self.windows[slot] = record;
        } else {
            self.windows.push(record);
        }
        self.rebalance_application_planes(request.output, now_ms);
        let assigned_plane = self.windows[slot].plane;
        if assigned_plane != requested_plane {
            crate::log_info!(
                target: "ui4";
                "ui4 window plane assigned requested_slot={} assigned_slot={} presentation={} buffering={:?} owner={:?} session={} window={} phase=post-spawn-rebalance\n",
                requested_plane.slot(),
                assigned_plane.slot(),
                if assigned_plane.slot() == STACK_PLANE_SLOT { "stack" } else { "lease" },
                buffering,
                request.owner,
                request.session.raw(),
                id.raw(),
            );
        }
        self.mark_composition_changed();
        Ok(id)
    }

    fn replace_frame(
        &mut self,
        owner: WindowOwner,
        id: WindowId,
        frame: FrameHandle,
        buffering: FrameBuffering,
    ) -> Result<(), WindowBrokerError> {
        let (slot, generation) = unpack_handle(id.0)?;
        let current = self
            .windows
            .get(slot)
            .ok_or(WindowBrokerError::InvalidHandle)?;
        if current.generation != generation {
            return Err(WindowBrokerError::InvalidHandle);
        }
        if current.owner != owner {
            return Err(WindowBrokerError::OwnerMismatch);
        }
        match current.state {
            WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
            WindowState::Closed => return Err(WindowBrokerError::Closed),
            WindowState::Pending | WindowState::Ready => {}
        }
        // The plane follows this window's lease, not its frame plan, so a
        // replacement never migrates the window between planes.
        let window = &mut self.windows[slot];
        window.frame = frame;
        window.buffering = buffering;
        window.state = WindowState::Pending;
        window.publish_serial = 0;
        window.damage = None;
        window.open_transition = None;
        window.revision = next_serial(window.revision);
        self.mark_composition_changed();
        Ok(())
    }

    fn checked_window_mut(
        &mut self,
        owner: WindowOwner,
        id: WindowId,
    ) -> Result<&mut WindowRecord, WindowBrokerError> {
        let (slot, generation) = unpack_handle(id.0)?;
        let window = self
            .windows
            .get_mut(slot)
            .ok_or(WindowBrokerError::InvalidHandle)?;
        if window.generation != generation {
            return Err(WindowBrokerError::InvalidHandle);
        }
        if window.owner != owner {
            return Err(WindowBrokerError::OwnerMismatch);
        }
        match window.state {
            WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
            WindowState::Closed => return Err(WindowBrokerError::Closed),
            WindowState::Pending | WindowState::Ready => {}
        }
        Ok(window)
    }

    fn finish_session(
        &mut self,
        owner: WindowOwner,
        id: WindowSessionId,
        capture_final_frames: bool,
        animate: bool,
        puff: bool,
        direct_plane_scaling: bool,
        retire_frames: bool,
        started_ms: u64,
    ) -> Result<WindowSessionFinish, WindowBrokerError> {
        self.checked_session(owner, id)?;
        let (slot, _) = unpack_handle(id.0)?;
        self.sessions[slot].active = false;
        let mut closed = 0;
        let mut animated = 0;
        let mut animation_skipped = 0;
        let mut animation_duration_ms = 0;
        let mut final_scale_percent = 100;
        let mut final_frames = Vec::new();
        let mut immediate_retire_frames = Vec::new();
        let direct_first_wave_present = direct_plane_scaling
            && self.windows.iter().any(|window| {
                window.state == WindowState::Ready
                    && window.session == id
                    && matches!(window.plane.slot(), 1 | 2)
            });
        for (window_slot, window) in self.windows.iter_mut().enumerate() {
            if window.state != WindowState::Closed && window.session == id {
                if capture_final_frames
                    && window.state == WindowState::Ready
                    && let Some(snapshot) = (*window).snapshot(window_slot)
                {
                    final_frames.push(snapshot);
                }
                let (delay_ms, duration_ms, puff_per_mille) = if direct_plane_scaling {
                    let delay_ms = if window.plane.slot() == 3 && direct_first_wave_present {
                        DIRECT_PLANE_CLOSE_WAVE_DURATION_MS
                    } else {
                        0
                    };
                    (
                        delay_ms,
                        DIRECT_PLANE_CLOSE_WAVE_DURATION_MS,
                        DIRECT_PLANE_CLOSE_PUFF_PER_MILLE,
                    )
                } else {
                    (
                        0,
                        CLOSE_TRANSITION_DURATION_MS,
                        if puff {
                            CLOSE_TRANSITION_PUFF_PER_MILLE
                        } else {
                            0
                        },
                    )
                };
                let presentation = (*window).presentation_placement();
                window.replacement_presentation = None;
                window.open_transition = None;
                let transition = if animate && window.state == WindowState::Ready {
                    super::acquire_published_frame(window.frame)
                        .ok()
                        .map(|lease| WindowCloseTransition {
                            lease,
                            initial: presentation,
                            started_ms,
                            delay_ms,
                            duration_ms,
                            puff_per_mille,
                            retire_frame: retire_frames,
                        })
                } else {
                    None
                };
                if let Some(transition) = transition {
                    window.state = WindowState::Closing;
                    window.placement = presentation;
                    window.close_transition = Some(transition);
                    window.damage = Some(DamageRegion::FULL);
                    animation_duration_ms = animation_duration_ms
                        .max(transition.delay_ms.saturating_add(transition.duration_ms));
                    final_scale_percent = final_scale_percent
                        .max((1_000u64.saturating_add(transition.puff_per_mille)) / 10);
                    animated += 1;
                } else {
                    if animate {
                        animation_skipped += 1;
                    }
                    window.state = WindowState::Closed;
                    window.damage = None;
                    if retire_frames {
                        immediate_retire_frames.push(window.frame);
                    }
                }
                window.revision = next_serial(window.revision);
                closed += 1;
            }
        }
        final_frames
            .sort_unstable_by_key(|window| (window.plane.slot(), window.placement.z, window.id));
        if closed != 0 {
            self.mark_composition_changed();
        }
        Ok(WindowSessionFinish {
            closed,
            animated,
            animation_skipped,
            animation_duration_ms,
            final_scale_percent,
            final_frames,
            immediate_retire_frames,
        })
    }

    fn advance_close_transitions(
        &mut self,
        now_ms: u64,
        output_extent: Option<(u32, u32)>,
    ) -> Vec<WindowTransitionRetirement> {
        let mut retirements = Vec::new();
        let mut composition_changed = false;
        let mut interaction_changed = false;
        for window in &mut self.windows {
            let Some(transition) = window.close_transition else {
                continue;
            };
            let elapsed_ms = now_ms.saturating_sub(transition.started_ms);
            let completed_ms = transition.delay_ms.saturating_add(transition.duration_ms);
            if elapsed_ms >= completed_ms {
                // Do not remove a closing window until an explicit zero-alpha
                // sample has crossed the compositor's SURFLIVE boundary.
                // Otherwise a delayed close tick can jump directly from the
                // last non-zero direct-plane sample to the transparent parking
                // surface, leaving the old producer surface vulnerable to an
                // opacity re-arm during that final handoff.
                let terminal = clamp_transition_to_output(
                    close_transition_placement(
                        transition.initial,
                        transition.duration_ms,
                        transition.duration_ms,
                        transition.puff_per_mille,
                    ),
                    output_extent,
                );
                if window.placement.opacity != 0 || window.damage.is_some() {
                    if window.placement != terminal {
                        window.placement = terminal;
                        window.damage = Some(DamageRegion::FULL);
                        window.revision = next_serial(window.revision);
                        composition_changed = true;
                        interaction_changed |= window.plane == WindowPlane::Interaction;
                    }
                    continue;
                }
                window.close_transition = None;
                window.state = WindowState::Closed;
                window.damage = None;
                window.revision = next_serial(window.revision);
                composition_changed = true;
                interaction_changed |= window.plane == WindowPlane::Interaction;
                retirements.push(WindowTransitionRetirement {
                    lease: transition.lease,
                    frame: window.frame,
                    retire_frame: transition.retire_frame,
                    elapsed_total_ms: elapsed_ms,
                });
                continue;
            }
            let active_elapsed_ms = elapsed_ms.saturating_sub(transition.delay_ms);
            let placement = clamp_transition_to_output(
                close_transition_placement(
                    transition.initial,
                    active_elapsed_ms,
                    transition.duration_ms,
                    transition.puff_per_mille,
                ),
                output_extent,
            );
            if placement != window.placement {
                window.placement = placement;
                window.damage = Some(DamageRegion::FULL);
                window.revision = next_serial(window.revision);
                composition_changed = true;
                interaction_changed |= window.plane == WindowPlane::Interaction;
            }
        }
        if composition_changed {
            self.mark_composition_changed();
            super::cursor_frame_inout::selection_strip_stack_changed();
        }
        if interaction_changed {
            super::input_broker::notify_slot4_visual_change();
        }
        retirements
    }

    fn advance_open_transitions(&mut self, now_ms: u64) -> usize {
        let mut advanced = 0usize;
        for window in &mut self.windows {
            let Some(mut transition) = window.open_transition else {
                continue;
            };
            let elapsed_ms = now_ms.saturating_sub(transition.started_ms);
            let placement = open_transition_placement(
                transition.initial,
                elapsed_ms,
                transition.duration_ms,
                transition.shrink_per_mille,
            );
            let complete = elapsed_ms >= transition.duration_ms;
            if placement != transition.current || complete {
                transition.current = placement;
                window.damage = Some(DamageRegion::FULL);
                window.revision = next_serial(window.revision);
                advanced = advanced.saturating_add(1);
            }
            window.open_transition = (!complete).then_some(transition);
        }
        if advanced != 0 {
            self.mark_composition_changed();
            super::cursor_frame_inout::selection_strip_stack_changed();
        }
        advanced
    }

    fn snapshots(&self, output: OutputId) -> Vec<WindowSnapshot> {
        let mut snapshots = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| {
                matches!(window.state, WindowState::Ready | WindowState::Closing)
                    && window.placement.visible
                    && window.output == output
            })
            .filter_map(|(slot, window)| window.snapshot(slot))
            .collect::<Vec<_>>();
        snapshots.sort_unstable_by_key(|window| (window.placement.z, window.id));
        snapshots
    }

    fn live_resource_counts(&self) -> (usize, usize) {
        let active_sessions = self
            .sessions
            .iter()
            .filter(|session| session.active)
            .count();
        let live_windows = self
            .windows
            .iter()
            .filter(|window| window.state != WindowState::Closed)
            .count();
        (active_sessions, live_windows)
    }

    fn published_snapshot(&self, update_count: u64, published_at_ms: u64) -> WindowBrokerSnapshot {
        let mut stats = WindowBrokerSnapshotStats {
            registry_window_slots: self.windows.len(),
            registry_session_slots: self.sessions.len(),
            active_sessions: self
                .sessions
                .iter()
                .filter(|session| session.active)
                .count(),
            ..WindowBrokerSnapshotStats::default()
        };
        let mut windows = Vec::new();
        for (slot, window) in self.windows.iter().copied().enumerate() {
            if window.state == WindowState::Closed {
                continue;
            }
            let Some(snapshot) = window.snapshot(slot) else {
                continue;
            };
            stats.live_windows = stats.live_windows.saturating_add(1);
            stats.windows_per_output[snapshot.output.slot()] =
                stats.windows_per_output[snapshot.output.slot()].saturating_add(1);
            match snapshot.state {
                WindowState::Pending => {
                    stats.pending_windows = stats.pending_windows.saturating_add(1);
                }
                WindowState::Ready => {
                    stats.ready_windows = stats.ready_windows.saturating_add(1);
                }
                WindowState::Closing => {
                    stats.closing_windows = stats.closing_windows.saturating_add(1);
                }
                WindowState::Closed => {}
            }
            if matches!(snapshot.state, WindowState::Ready | WindowState::Closing)
                && snapshot.placement.visible
            {
                stats.composable_windows = stats.composable_windows.saturating_add(1);
            }
            stats.damaged_windows = stats
                .damaged_windows
                .saturating_add(usize::from(snapshot.damage.is_some()));
            stats.maximized_windows = stats
                .maximized_windows
                .saturating_add(usize::from(snapshot.maximized));
            windows.push(snapshot);
        }
        windows.sort_unstable_by_key(|window| (window.output, window.placement.z, window.id));
        WindowBrokerSnapshot {
            update_count,
            published_at_ms,
            composition_revision: self.composition_revision,
            stats,
            windows,
        }
    }

    fn acknowledge(&mut self, id: WindowId, publish_serial: u64) -> bool {
        let Ok((slot, generation)) = unpack_handle(id.0) else {
            return false;
        };
        let Some(window) = self.windows.get_mut(slot) else {
            return false;
        };
        if window.generation != generation
            || !matches!(window.state, WindowState::Ready | WindowState::Closing)
            || window.publish_serial != publish_serial
        {
            return false;
        }
        window.damage = None;
        true
    }

    fn acknowledge_revision(&mut self, id: WindowId, publish_serial: u64, revision: u64) -> bool {
        let Ok((slot, generation)) = unpack_handle(id.0) else {
            return false;
        };
        if !self
            .windows
            .get(slot)
            .is_some_and(|window| window.generation == generation && window.revision == revision)
        {
            return false;
        }
        self.acknowledge(id, publish_serial)
    }
}

impl WindowRecord {
    fn new(generation: u16, request: WindowCreate, buffering: FrameBuffering) -> Self {
        Self {
            generation,
            owner: request.owner,
            session: request.session,
            frame: request.frame,
            buffering,
            output: request.output,
            plane: request.plane,
            placement: request.placement,
            interaction: request.interaction,
            escape_key_action: Ui4FrameEscapeKeyAction::Close,
            state: WindowState::Pending,
            revision: 1,
            publish_serial: 0,
            presented_serials: [0; 8],
            presented_serial_cursor: 0,
            first_presentation_emitted: false,
            first_presentation_taken: false,
            damage: None,
            restore_placement: None,
            dock_target: None,
            replacement_presentation: None,
            open_transition: None,
            close_transition: None,
        }
    }

    fn snapshot(self, slot: usize) -> Option<WindowSnapshot> {
        Some(WindowSnapshot {
            id: WindowId(pack_handle(slot, self.generation).ok()?),
            owner: self.owner,
            producer_name: self.owner.name(),
            session: self.session,
            frame: self.frame,
            buffering: self.buffering,
            output: self.output,
            plane: self.plane,
            placement: self.placement,
            presentation_placement: self.presentation_placement(),
            interaction: self.interaction,
            state: self.state,
            revision: self.revision,
            publish_serial: self.publish_serial,
            damage: self.damage,
            maximized: self.dock_target == Some(WindowDockTarget::Maximize),
            dock_target: self.dock_target,
        })
    }

    fn presentation_placement(self) -> WindowPlacement {
        self.replacement_presentation.unwrap_or_else(|| {
            self.open_transition
                .map(|transition| transition.current)
                .unwrap_or(self.placement)
        })
    }
}

static WINDOW_BROKER: Mutex<WindowBroker> = Mutex::new(WindowBroker::new());
static TRANSITION_RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
static WINDOW_COMPOSITION_CHANGED: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static WINDOW_FIRST_PRESENTATIONS: Mutex<
    Deque<WindowSnapshot, WINDOW_FIRST_PRESENTATION_QUEUE_CAP>,
> = Mutex::new(Deque::new());
static WINDOW_FIRST_PRESENTATION_READY: Signal<crate::wait::EmbassySpinRawMutex, ()> =
    Signal::new();
static WINDOW_BROKER_SNAPSHOT: Watch<
    crate::wait::EmbassySpinRawMutex,
    WindowBrokerSnapshot,
    WINDOW_BROKER_SNAPSHOT_RECEIVERS,
> = Watch::new_with(WindowBrokerSnapshot::empty());

/// Return the last periodically published diagnostic copy. This operation
/// never takes the live broker lock.
pub(crate) fn latest_window_broker_snapshot() -> WindowBrokerSnapshot {
    WINDOW_BROKER_SNAPSHOT
        .try_get()
        .unwrap_or_else(WindowBrokerSnapshot::empty)
}

/// Return fresh broker ownership counts without waiting for the diagnostic
/// snapshot publisher. Pending and hidden windows remain live resources; only
/// generation-safe Closed registry slots are excluded.
pub(super) fn live_resource_counts() -> (usize, usize) {
    WINDOW_BROKER.lock().live_resource_counts()
}

/// Count every non-closed window which can consume application compositor
/// slots 0..3 on one output. Pending and hidden windows still count; slot-4
/// interaction-service windows deliberately do not.
pub(crate) fn live_application_window_count(output: OutputId) -> usize {
    WINDOW_BROKER
        .lock()
        .windows
        .iter()
        .filter(|window| {
            window.state != WindowState::Closed
                && window.output == output
                && window.plane.is_application()
        })
        .count()
}

/// Optionally subscribe to future diagnostic publications.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn subscribe_window_broker_snapshots() -> Option<WindowBrokerSnapshotReceiver<'static>> {
    WINDOW_BROKER_SNAPSHOT.receiver()
}

fn publish_window_broker_snapshot_once() -> WindowBrokerSnapshot {
    let update_count = next_serial(latest_window_broker_snapshot().update_count);
    let published_at_ms = trueos_time::Instant::now().as_millis();
    let snapshot = WINDOW_BROKER
        .lock()
        .published_snapshot(update_count, published_at_ms);
    WINDOW_BROKER_SNAPSHOT.sender().send(snapshot.clone());
    snapshot
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_window_broker_snapshot_service_task() {
    crate::log_info!(
        target: "ui4";
        "ui4 window-broker snapshot publisher online period_ms={} scope=informational access=optional\n",
        WINDOW_BROKER_SNAPSHOT_PERIOD_MS,
    );
    loop {
        let _ = publish_window_broker_snapshot_once();
        Timer::after(Duration::from_millis(WINDOW_BROKER_SNAPSHOT_PERIOD_MS)).await;
    }
}

pub(crate) fn begin_window_session(
    owner: WindowOwner,
) -> Result<WindowSessionId, WindowBrokerError> {
    begin_window_session_admitted(owner)
}

fn begin_window_session_admitted(owner: WindowOwner) -> Result<WindowSessionId, WindowBrokerError> {
    let (result, retirements) = WINDOW_BROKER.lock().begin_session(owner);
    complete_transition_retirements(retirements);
    super::cursor_frame_inout::owner_closed(owner);
    result
}

pub(crate) fn begin_additional_window_session(
    owner: WindowOwner,
) -> Result<WindowSessionId, WindowBrokerError> {
    WINDOW_BROKER.lock().begin_additional_session(owner)
}

pub(crate) fn finish_window_session(
    owner: WindowOwner,
    session: WindowSessionId,
) -> Result<usize, WindowBrokerError> {
    let closed = WINDOW_BROKER
        .lock()
        .finish_session(owner, session, false, false, false, false, false, 0)
        .map(|finish| finish.closed)?;
    super::cursor_frame_inout::session_closed(owner, session);
    Ok(closed)
}

pub(crate) fn finish_window_session_with_request(
    owner: WindowOwner,
    session: WindowSessionId,
    request: WindowSessionCloseRequest<'_>,
) -> Result<usize, WindowBrokerError> {
    let started_ms = trueos_time::Instant::now().as_millis();
    let finish = WINDOW_BROKER.lock().finish_session(
        owner,
        session,
        request.persist_final_frame,
        request.animate,
        request.puff,
        request.direct_plane_scaling,
        request.retire_frames,
        started_ms,
    )?;
    super::cursor_frame_inout::session_closed(owner, session);
    if request.persist_final_frame {
        super::screenshot::capture_final_session_frames(
            owner,
            session,
            finish.final_frames.as_slice(),
            request.final_frame_name,
        );
    }
    retire_transferred_frames(finish.immediate_retire_frames);
    if request.animate {
        crate::log_info!(
            target: "ui4";
            "ui4 close-transition started owner={:?} session={} windows={} animated={} skipped={} duration_ms={} mode={} final_scale_percent={} retire_frames={} persist_final_frame={}\n",
            owner,
            session.raw(),
            finish.closed,
            finish.animated,
            finish.animation_skipped,
            finish.animation_duration_ms,
            if request.direct_plane_scaling {
                "direct-plane-puff+fade"
            } else if request.puff {
                "puff+fade"
            } else {
                "fade"
            },
            finish.final_scale_percent,
            request.retire_frames as u8,
            request.persist_final_frame as u8,
        );
    }
    Ok(finish.closed)
}

/// Advance compositor-owned close visuals. This runs at the UI4 composition
/// cadence so transition geometry and presentation stay in one clock domain.
pub(crate) fn advance_window_close_transitions() {
    reap_transition_retired_frames();
    let now_ms = trueos_time::Instant::now().as_millis();
    let output_extent = crate::intel::active_scanout_dimensions();
    let retirements = WINDOW_BROKER
        .lock()
        .advance_close_transitions(now_ms, output_extent);
    if retirements.is_empty() {
        return;
    }
    let completed = retirements.len();
    let elapsed_total_ms = retirements
        .iter()
        .map(|retirement| retirement.elapsed_total_ms)
        .max()
        .unwrap_or(0);
    complete_transition_retirements(retirements);
    crate::log_info!(
        target: "ui4";
        "ui4 close-transition completed windows={} duration_ms={}\n",
        completed,
        elapsed_total_ms,
    );
}

/// Advance first-presentation visuals. Replacement frames produced for a
/// resize do not enter this path.
pub(crate) fn advance_window_open_transitions() {
    let now_ms = trueos_time::Instant::now().as_millis();
    let _ = WINDOW_BROKER.lock().advance_open_transitions(now_ms);
}

fn complete_transition_retirements(retirements: Vec<WindowTransitionRetirement>) {
    let mut frames = Vec::new();
    for retirement in retirements {
        let _ = super::release_published_frame(retirement.lease);
        if retirement.retire_frame {
            frames.push(retirement.frame);
        }
    }
    retire_transferred_frames(frames);
}

fn retire_transferred_frames(frames: Vec<FrameHandle>) {
    for frame in frames {
        retire_frame_when_released(frame);
    }
}

/// Transfer a detached frame to UI4 for destruction after every compositor or
/// direct-scanout reader has released it. The caller must not use `frame` again.
/// An already-unreferenced frame is destroyed synchronously; a busy frame joins
/// the close-transition reaper and is retried on subsequent compositor ticks.
pub(crate) fn retire_frame_when_released(frame: FrameHandle) {
    match super::destroy_frame(frame) {
        Ok(()) | Err(super::FramePoolError::InvalidHandle) => {}
        Err(super::FramePoolError::Busy) => {
            if enqueue_busy_retirement(&mut TRANSITION_RETIRED_FRAMES.lock(), frame) {
                // A detached frame need not have an active close animation to
                // provide compositor timer ticks. Wake the compositor so the
                // reaper observes the display-reader handoff to completion.
                WINDOW_COMPOSITION_CHANGED.signal(());
            }
        }
        Err(error) => crate::log_warn!(
            target: "ui4";
            "ui4 deferred frame retire abandoned frame={} error={:?}\n",
            frame.raw(),
            error,
        ),
    }
}

fn enqueue_busy_retirement(frames: &mut Vec<FrameHandle>, frame: FrameHandle) -> bool {
    if !frames.contains(&frame) {
        frames.push(frame);
        true
    } else {
        false
    }
}

fn reap_transition_retired_frames() {
    TRANSITION_RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(super::destroy_frame(*frame), Err(super::FramePoolError::Busy)));
}

pub(crate) fn create_window(request: WindowCreate) -> Result<WindowId, WindowBrokerError> {
    let plan = super::frame_snapshot(request.frame)
        .map_err(|_| WindowBrokerError::InvalidHandle)?
        .plan;
    let now_ms = trueos_time::Instant::now().as_millis();
    let id = WINDOW_BROKER
        .lock()
        .create(request, plan.buffering, now_ms)?;
    if super::cursor_frame_inout::frame_opened(request.owner, request.session, id).is_err() {
        let _ = close_window(request.owner, id);
        return Err(WindowBrokerError::Capacity);
    }
    Ok(id)
}

pub(crate) fn replace_window_frame(
    owner: WindowOwner,
    id: WindowId,
    frame: FrameHandle,
) -> Result<(), WindowBrokerError> {
    let plan = super::frame_snapshot(frame)
        .map_err(|_| WindowBrokerError::InvalidHandle)?
        .plan;
    WINDOW_BROKER
        .lock()
        .replace_frame(owner, id, frame, plan.buffering)?;
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    Ok(())
}

/// Commit an already-published replacement frame, its logical geometry, and
/// its first damage as one broker transaction. The compositor therefore sees
/// either the old ready front or the complete replacement; it cannot observe
/// the replacement in `Pending` state between separate API calls.
pub(crate) fn commit_window_frame_replacement(
    owner: WindowOwner,
    id: WindowId,
    frame: FrameHandle,
    placement: WindowPlacement,
    damage: DamageRect,
) -> Result<u64, WindowBrokerError> {
    if !placement.valid() {
        return Err(WindowBrokerError::EmptyExtent);
    }
    if !damage.valid() {
        return Err(WindowBrokerError::EmptyDamage);
    }
    let plan = super::frame_snapshot(frame)
        .map_err(|_| WindowBrokerError::InvalidHandle)?
        .plan;
    let mut broker = WINDOW_BROKER.lock();
    let (slot, generation) = unpack_handle(id.0)?;
    let current = broker
        .windows
        .get(slot)
        .ok_or(WindowBrokerError::InvalidHandle)?;
    if current.generation != generation {
        return Err(WindowBrokerError::InvalidHandle);
    }
    if current.owner != owner {
        return Err(WindowBrokerError::OwnerMismatch);
    }
    match current.state {
        WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
        WindowState::Closed => return Err(WindowBrokerError::Closed),
        WindowState::Pending | WindowState::Ready => {}
    }
    let previous_placement = current.placement;
    let stack_changed = current.state == WindowState::Pending || previous_placement != placement;
    // The plane follows this window's lease, not its frame plan.
    let window = &mut broker.windows[slot];
    window.frame = frame;
    window.buffering = plan.buffering;
    window.placement = placement;
    window.replacement_presentation = None;
    window.open_transition = None;
    window.state = WindowState::Ready;
    window.publish_serial = next_serial(window.publish_serial);
    window.revision = next_serial(window.revision);
    let pending = window.damage.get_or_insert(DamageRegion::EMPTY);
    pending.add(damage);
    let publish_serial = window.publish_serial;
    let notify_resize = producer_resize_required(window.interaction, previous_placement, placement);
    broker.mark_composition_changed();
    drop(broker);
    if stack_changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    if notify_resize {
        super::input_broker::enqueue_window_resize(
            owner,
            id,
            previous_placement.width,
            previous_placement.height,
            placement.width,
            placement.height,
        );
    }
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    Ok(publish_serial)
}

/// Read the broker's live geometry for a producer which is about to replace
/// its backing frame. This preserves maximize/restore position changes that
/// are newer than a producer's own cached scene placement.
pub(crate) fn window_placement(
    owner: WindowOwner,
    id: WindowId,
) -> Result<WindowPlacement, WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    Ok(broker.checked_window_mut(owner, id)?.placement)
}

pub(crate) fn set_window_placement(
    owner: WindowOwner,
    id: WindowId,
    placement: WindowPlacement,
) -> Result<(), WindowBrokerError> {
    if !placement.valid() {
        return Err(WindowBrokerError::EmptyExtent);
    }
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    let previous = window.placement;
    let notify_resize = producer_resize_required(window.interaction, previous, placement);
    let changed = window.placement != placement;
    if changed {
        window.placement = placement;
        window.dock_target = None;
        window.restore_placement = None;
        window.replacement_presentation = None;
        window.revision = next_serial(window.revision);
    }
    if changed {
        broker.mark_composition_changed();
    }
    drop(broker);
    if notify_resize {
        super::input_broker::enqueue_window_resize(
            owner,
            id,
            previous.width,
            previous.height,
            placement.width,
            placement.height,
        );
    }
    if changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    Ok(())
}

/// Include or exclude a window from cursor selection without changing its
/// placement, rendered frame, or producer-owned input policy.
pub(crate) fn set_window_hit_testable(
    owner: WindowOwner,
    id: WindowId,
    hit_testable: bool,
) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    if window.interaction.hit_testable == hit_testable {
        return Ok(());
    }
    window.interaction.hit_testable = hit_testable;
    window.revision = next_serial(window.revision);
    broker.mark_composition_changed();
    drop(broker);
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    Ok(())
}

/// Set the Escape behaviour for one frame.  This has no owner-wide effect.
pub(crate) fn set_window_escape_key_action(
    owner: WindowOwner,
    id: WindowId,
    action: Ui4FrameEscapeKeyAction,
) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    if window.escape_key_action == action {
        return Ok(());
    }
    window.escape_key_action = action;
    window.revision = next_serial(window.revision);
    Ok(())
}

/// Resolve the selected frame's Escape policy without exposing broker storage.
pub(crate) fn window_escape_key_action(
    owner: WindowOwner,
    id: WindowId,
) -> Result<Ui4FrameEscapeKeyAction, WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    Ok(broker.checked_window_mut(owner, id)?.escape_key_action)
}

/// Change several windows' visibility as one broker transaction. The complete
/// set is validated before any record changes, so compositor snapshots cannot
/// observe a prefix of a multi-window pass transition.
pub(crate) fn set_windows_visible(
    owner: WindowOwner,
    ids: &[WindowId],
    visible: bool,
) -> Result<u64, WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let mut changed_ids = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().copied().enumerate() {
        if ids[..index].contains(&id) {
            return Err(WindowBrokerError::InvalidHandle);
        }
        let (slot, generation) = unpack_handle(id.0)?;
        let window = broker
            .windows
            .get(slot)
            .ok_or(WindowBrokerError::InvalidHandle)?;
        if window.generation != generation {
            return Err(WindowBrokerError::InvalidHandle);
        }
        if window.owner != owner {
            return Err(WindowBrokerError::OwnerMismatch);
        }
        match window.state {
            WindowState::Pending | WindowState::Ready => {}
            WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
            WindowState::Closed => return Err(WindowBrokerError::Closed),
        }
        if window.placement.visible != visible {
            changed_ids.push(id);
        }
    }

    for id in changed_ids.iter().copied() {
        let (slot, _) = unpack_handle(id.0)?;
        let window = &mut broker.windows[slot];
        window.placement.visible = visible;
        window.revision = next_serial(window.revision);
    }
    if !changed_ids.is_empty() {
        broker.mark_composition_changed();
    }
    let stack_changed = !changed_ids.is_empty();
    let composition_revision = broker.composition_revision;
    drop(broker);

    for id in changed_ids {
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    if stack_changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    Ok(composition_revision)
}

/// Translate a window through UI4's frame interaction policy.
///
/// Unlike the owner-facing placement setter, this entry point cannot resize a
/// producer surface and rejects windows which did not opt into frame motion.
pub(crate) fn move_window(
    owner: WindowOwner,
    id: WindowId,
    placement: WindowPlacement,
) -> Result<(), WindowBrokerError> {
    if !placement.valid() {
        return Err(WindowBrokerError::EmptyExtent);
    }
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    if !window.interaction.movable
        || placement.width != window.placement.width
        || placement.height != window.placement.height
    {
        return Err(WindowBrokerError::InteractionDenied);
    }
    let changed = window.placement != placement;
    if changed {
        window.placement = placement;
        window.revision = next_serial(window.revision);
        broker.mark_composition_changed();
    }
    drop(broker);
    if changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    Ok(())
}

/// Return the broker geometry represented by a dock preview or commit.
///
/// Fixed-size producers preserve their exact pixel extent and are centered in
/// the selected region. Resize-capable producers receive that region's exact
/// geometry and one final resize event.
pub(super) fn docked_window_placement(
    interaction: WindowInteraction,
    previous: WindowPlacement,
    target: WindowDockTarget,
    output_width: u32,
    output_height: u32,
) -> WindowPlacement {
    let split_x = output_width / 2;
    let split_y = output_height / 2;
    let (x, y, width, height) = match target {
        WindowDockTarget::Maximize => (0, 0, output_width, output_height),
        WindowDockTarget::LeftHalf => (0, 0, split_x, output_height),
        WindowDockTarget::RightHalf => {
            (split_x, 0, output_width.saturating_sub(split_x), output_height)
        }
        WindowDockTarget::TopLeft => (0, 0, split_x, split_y),
        WindowDockTarget::TopRight => (split_x, 0, output_width.saturating_sub(split_x), split_y),
        WindowDockTarget::BottomLeft => {
            (0, split_y, split_x, output_height.saturating_sub(split_y))
        }
        WindowDockTarget::BottomRight => (
            split_x,
            split_y,
            output_width.saturating_sub(split_x),
            output_height.saturating_sub(split_y),
        ),
    };
    if interaction.resize_on_maximize {
        WindowPlacement {
            x: x as i32,
            y: y as i32,
            width,
            height,
            ..previous
        }
    } else {
        let center_x = i64::from(x).saturating_add(i64::from(width / 2));
        let center_y = i64::from(y).saturating_add(i64::from(height / 2));
        let max_x = i64::from(output_width.saturating_sub(previous.width));
        let max_y = i64::from(output_height.saturating_sub(previous.height));
        WindowPlacement {
            x: center_x
                .saturating_sub(i64::from(previous.width / 2))
                .clamp(0, max_x) as i32,
            y: center_y
                .saturating_sub(i64::from(previous.height / 2))
                .clamp(0, max_y) as i32,
            ..previous
        }
    }
}

const fn producer_resize_required(
    interaction: WindowInteraction,
    previous: WindowPlacement,
    placement: WindowPlacement,
) -> bool {
    interaction.receives_input
        && (previous.width != placement.width || previous.height != placement.height)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct WindowDockChange {
    transition: WindowPlacementTransition,
    changed: bool,
    notify_resize: bool,
}

fn change_window_dock_record(
    window: &mut WindowRecord,
    target: Option<WindowDockTarget>,
    output_width: u32,
    output_height: u32,
    restore_placement: Option<WindowPlacement>,
    restore_center: Option<(u32, u32)>,
) -> Result<WindowDockChange, WindowBrokerError> {
    if !window.interaction.maximizable {
        return Err(WindowBrokerError::InteractionDenied);
    }
    let previous = window.placement;
    let previous_target = window.dock_target;
    let placement = if let Some(target) = target {
        if window.restore_placement.is_none() {
            window.restore_placement = Some(restore_placement.unwrap_or(previous));
        }
        window.dock_target = Some(target);
        docked_window_placement(window.interaction, previous, target, output_width, output_height)
    } else {
        let restore = window
            .restore_placement
            .take()
            .ok_or(WindowBrokerError::InteractionDenied)?;
        window.dock_target = None;
        restore_center.map_or(restore, |(cursor_x, cursor_y)| {
            center_restored_window_on_cursor(
                restore,
                cursor_x,
                cursor_y,
                output_width,
                output_height,
            )
        })
    };
    let notify_resize = window.interaction.resize_on_maximize
        && producer_resize_required(window.interaction, previous, placement);
    let changed = previous != placement || previous_target != window.dock_target;
    if changed {
        window.placement = placement;
        // Keep the old producer-sized frame at 1:1 until the replacement is
        // published. UI4 never stretches an intermediate maximize/dock frame.
        window.replacement_presentation = notify_resize.then_some(previous);
        window.damage = Some(DamageRegion::FULL);
        window.revision = next_serial(window.revision);
    }
    Ok(WindowDockChange {
        transition: WindowPlacementTransition {
            previous,
            placement,
            dock_target: window.dock_target,
        },
        changed,
        notify_resize,
    })
}

fn change_window_dock(
    owner: WindowOwner,
    id: WindowId,
    target: Option<WindowDockTarget>,
    output_width: u32,
    output_height: u32,
    restore_placement: Option<WindowPlacement>,
    restore_center: Option<(u32, u32)>,
) -> Result<WindowPlacementTransition, WindowBrokerError> {
    if output_width == 0 || output_height == 0 {
        return Err(WindowBrokerError::EmptyExtent);
    }
    let mut broker = WINDOW_BROKER.lock();
    let change = {
        let window = broker.checked_window_mut(owner, id)?;
        change_window_dock_record(
            window,
            target,
            output_width,
            output_height,
            restore_placement,
            restore_center,
        )?
    };
    if change.changed {
        broker.mark_composition_changed();
    }
    drop(broker);
    if change.notify_resize {
        super::input_broker::enqueue_window_resize(
            owner,
            id,
            change.transition.previous.width,
            change.transition.previous.height,
            change.transition.placement.width,
            change.transition.placement.height,
        );
    }
    if change.changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    Ok(change.transition)
}

/// Dock one window while retaining the pre-drag geometry for a later restore.
pub(crate) fn dock_window(
    owner: WindowOwner,
    id: WindowId,
    target: WindowDockTarget,
    output_width: u32,
    output_height: u32,
    restore_placement: Option<WindowPlacement>,
) -> Result<WindowPlacementTransition, WindowBrokerError> {
    change_window_dock(
        owner,
        id,
        Some(target),
        output_width,
        output_height,
        restore_placement,
        None,
    )
}

/// Restore one docked window under the cursor at the beginning of a drag.
pub(crate) fn restore_docked_window(
    owner: WindowOwner,
    id: WindowId,
    output_width: u32,
    output_height: u32,
    cursor: (u32, u32),
) -> Result<WindowPlacementTransition, WindowBrokerError> {
    change_window_dock(owner, id, None, output_width, output_height, None, Some(cursor))
}

fn center_restored_window_on_cursor(
    restore: WindowPlacement,
    cursor_x: u32,
    cursor_y: u32,
    output_width: u32,
    output_height: u32,
) -> WindowPlacement {
    let max_x = i64::from(output_width.saturating_sub(restore.width));
    let max_y = i64::from(output_height.saturating_sub(restore.height));
    let x = (i64::from(cursor_x) - i64::from(restore.width / 2)).clamp(0, max_x) as i32;
    let y = (i64::from(cursor_y) - i64::from(restore.height / 2)).clamp(0, max_y) as i32;
    WindowPlacement { x, y, ..restore }
}

pub(crate) fn publish_window_frame(
    owner: WindowOwner,
    id: WindowId,
    damage: DamageRect,
) -> Result<u64, WindowBrokerError> {
    if !damage.valid() {
        return Err(WindowBrokerError::EmptyDamage);
    }
    let started_ms = trueos_time::Instant::now().as_millis();
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    let became_ready = window.state == WindowState::Pending;
    if became_ready {
        if !window.first_presentation_emitted && window.replacement_presentation.is_none() {
            window.open_transition =
                Some(open_transition(window.placement, window.plane, started_ms));
        }
        window.replacement_presentation = None;
    }
    window.state = WindowState::Ready;
    window.publish_serial = next_serial(window.publish_serial);
    window.revision = next_serial(window.revision);
    let pending = window.damage.get_or_insert(DamageRegion::EMPTY);
    pending.add(damage);
    let publish_serial = window.publish_serial;
    broker.mark_composition_changed();
    drop(broker);
    if became_ready {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    Ok(publish_serial)
}

/// Publish a coherent set of already-complete producer frames as one broker
/// transaction. The validation pass changes nothing; only after every handle
/// and damage rectangle is accepted do all records become Ready under the same
/// lock. A compositor snapshot therefore observes either the old set or the
/// complete new set, never a prefix of a multi-window scene.
pub(crate) fn publish_window_frames(
    owner: WindowOwner,
    publications: &[(WindowId, DamageRect)],
) -> Result<(), WindowBrokerError> {
    if publications.iter().any(|(_, damage)| !damage.valid()) {
        return Err(WindowBrokerError::EmptyDamage);
    }
    let started_ms = trueos_time::Instant::now().as_millis();
    let mut broker = WINDOW_BROKER.lock();
    let mut stack_changed = false;
    for (index, (id, _)) in publications.iter().copied().enumerate() {
        if publications[..index]
            .iter()
            .any(|(previous, _)| *previous == id)
        {
            return Err(WindowBrokerError::InvalidHandle);
        }
        let (slot, generation) = unpack_handle(id.0)?;
        let window = broker
            .windows
            .get(slot)
            .ok_or(WindowBrokerError::InvalidHandle)?;
        if window.generation != generation {
            return Err(WindowBrokerError::InvalidHandle);
        }
        if window.owner != owner {
            return Err(WindowBrokerError::OwnerMismatch);
        }
        match window.state {
            WindowState::Pending => stack_changed = true,
            WindowState::Ready => {}
            WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
            WindowState::Closed => return Err(WindowBrokerError::Closed),
        }
    }
    for (id, damage) in publications.iter().copied() {
        let (slot, _) = unpack_handle(id.0)?;
        let window = &mut broker.windows[slot];
        if window.state == WindowState::Pending {
            if !window.first_presentation_emitted && window.replacement_presentation.is_none() {
                window.open_transition =
                    Some(open_transition(window.placement, window.plane, started_ms));
            }
            window.replacement_presentation = None;
        }
        window.state = WindowState::Ready;
        window.publish_serial = next_serial(window.publish_serial);
        window.revision = next_serial(window.revision);
        let pending = window.damage.get_or_insert(DamageRegion::EMPTY);
        pending.add(damage);
    }
    if !publications.is_empty() {
        broker.mark_composition_changed();
    }
    drop(broker);
    if stack_changed {
        super::cursor_frame_inout::selection_strip_stack_changed();
    }
    for (id, _) in publications.iter().copied() {
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    Ok(())
}

pub(crate) fn close_window(owner: WindowOwner, id: WindowId) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let output = {
        let window = broker.checked_window_mut(owner, id)?;
        window.state = WindowState::Closed;
        window.damage = None;
        window.revision = next_serial(window.revision);
        window.output
    };
    broker.release_lease_of(id);
    broker.rebalance_application_planes(output, trueos_time::Instant::now().as_millis());
    broker.mark_composition_changed();
    drop(broker);
    super::cursor_frame_inout::frame_closed(owner, id);
    super::context_menu::dismiss_window(owner, id);
    Ok(())
}

/// Report that a frame became the target of user interaction.
///
/// A frame which already holds a lease refreshes it, so a continuous drag is
/// never evicted mid-gesture. A stacked frame claims a free or revocable lease
/// plane and migrates onto it; if every lease is still live it simply stays on
/// the stack and keeps moving there. This never fails and never blocks a
/// gesture: the plane is a presentation optimisation, not permission to
/// interact. Returns the plane the window presents on after the call.
pub(crate) fn note_window_hot(
    owner: WindowOwner,
    id: WindowId,
) -> Result<WindowPlane, WindowBrokerError> {
    note_window_interaction(owner, id, false)
}

/// Report that a frame took focus.
///
/// Identical to [`note_window_hot`], except that a frame which already holds a
/// lease is also raised to the topmost lease plane. Raising is bound to focus
/// rather than to motion on purpose: focus is a discrete user action, while a
/// drag delivers continuous events, and two cursors dragging two frames would
/// otherwise trade the top plane on every motion sample.
pub(crate) fn note_window_focused(
    owner: WindowOwner,
    id: WindowId,
) -> Result<WindowPlane, WindowBrokerError> {
    note_window_interaction(owner, id, true)
}

fn note_window_interaction(
    owner: WindowOwner,
    id: WindowId,
    raise: bool,
) -> Result<WindowPlane, WindowBrokerError> {
    let now_ms = trueos_time::Instant::now().as_millis();
    let mut broker = WINDOW_BROKER.lock();
    let (slot, _) = unpack_handle(id.0)?;
    let current = broker.checked_window_mut(owner, id)?.plane;
    if current == WindowPlane::Interaction {
        return Ok(current);
    }
    if let Some(index) = WindowBroker::lease_index(current.slot())
        && broker.leases[index].is_some_and(|held| held.window == id)
    {
        broker.leases[index] = Some(PlaneLease {
            window: id,
            last_hot_ms: now_ms,
        });
        if !raise {
            return Ok(current);
        }
        let raised = broker.raise_lease(index, now_ms);
        if raised == current {
            return Ok(current);
        }
        broker.mark_composition_changed();
        drop(broker);
        crate::log_info!(
            target: "ui4";
            "ui4 window raised previous_slot={} assigned_slot={} owner={:?} window={} trigger=focus policy=swap-with-top-lease\n",
            current.slot(),
            raised.slot(),
            owner,
            id.raw(),
        );
        super::cursor_frame_inout::frame_visual_changed(owner, id);
        super::cursor_frame_inout::selection_strip_stack_changed();
        return Ok(raised);
    }
    let Some(plane) = broker.claim_lease(id, now_ms) else {
        return Ok(current);
    };
    // A frame arriving from the stack lands wherever a lease was available.
    // Focus then raises it, so a click promotes and fronts in one transaction
    // rather than leaving it behind the other lease holders.
    let plane = if raise {
        WindowBroker::lease_index(plane.slot())
            .map_or(plane, |index| broker.raise_lease(index, now_ms))
    } else {
        plane
    };
    let window = &mut broker.windows[slot];
    window.plane = plane;
    window.damage = Some(DamageRegion::FULL);
    window.revision = next_serial(window.revision);
    broker.mark_composition_changed();
    drop(broker);
    crate::log_info!(
        target: "ui4";
        "ui4 window promoted previous_slot={} assigned_slot={} owner={:?} window={} trigger=hot-interaction grace_ms={}\n",
        current.slot(),
        plane.slot(),
        owner,
        id.raw(),
        LEASE_IDLE_GRACE_MS,
    );
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    super::cursor_frame_inout::selection_strip_stack_changed();
    Ok(plane)
}

/// Cheap change detector for the compositor's idle path. The subsequent
/// snapshot API returns its epoch under the same broker lock, closing the race
/// between this optimistic check and a producer publication.
pub(crate) fn window_composition_revision() -> u64 {
    WINDOW_BROKER.lock().composition_revision
}

pub(crate) fn window_transitions_active() -> bool {
    if !TRANSITION_RETIRED_FRAMES.lock().is_empty() {
        return true;
    }
    WINDOW_BROKER
        .lock()
        .windows
        .iter()
        .any(|window| window.close_transition.is_some() || window.open_transition.is_some())
}

pub(crate) async fn wait_for_window_composition_change() {
    WINDOW_COMPOSITION_CHANGED.wait().await;
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn visible_windows_for_output_with_revision(
    output: OutputId,
) -> (u64, Vec<WindowSnapshot>) {
    let broker = WINDOW_BROKER.lock();
    (broker.composition_revision, broker.snapshots(output))
}

/// Snapshot only broker windows consumed by the slot 0-3 application
/// compositor. Slot-4 interaction windows retain the same broker lifecycle
/// and input routing but are presented exclusively by the slot-4 service.
pub(crate) fn application_windows_for_output_with_revision(
    output: OutputId,
) -> (u64, Vec<WindowSnapshot>) {
    let broker = WINDOW_BROKER.lock();
    let revision = broker.composition_revision;
    let windows = broker
        .snapshots(output)
        .into_iter()
        .filter(|window| window.plane.is_application())
        .collect();
    (revision, windows)
}

pub(crate) fn visible_windows_for_output(output: OutputId) -> Vec<WindowSnapshot> {
    WINDOW_BROKER.lock().snapshots(output)
}

/// Whether this owner has a live window whose first frame reached SURFLIVE.
///
/// Unlike `take_window_first_presentation`, this is an observation rather than
/// a consuming event, so kernel lifecycle orchestration cannot steal the
/// application's own first-presentation notification.
pub(crate) fn owner_has_first_presentation(owner: WindowOwner) -> bool {
    WINDOW_BROKER.lock().windows.iter().any(|window| {
        window.owner == owner
            && window.state != WindowState::Closed
            && window.first_presentation_emitted
    })
}

pub(super) fn interaction_windows_for_output(output: OutputId) -> Vec<WindowSnapshot> {
    WINDOW_BROKER
        .lock()
        .snapshots(output)
        .into_iter()
        .filter(|window| window.plane == WindowPlane::Interaction)
        .collect()
}

pub(super) fn window_snapshot(owner: WindowOwner, id: WindowId) -> Option<WindowSnapshot> {
    let (slot, generation) = unpack_handle(id.0).ok()?;
    let broker = WINDOW_BROKER.lock();
    let window = broker.windows.get(slot)?;
    if window.generation != generation || window.owner != owner {
        return None;
    }
    window.snapshot(slot)
}

/// Whether this exact publication crossed the real display SURFLIVE boundary.
/// Eight entries comfortably cover the live history of a double-buffered
/// producer while keeping the broker record fixed-size.
pub(crate) fn window_frame_was_presented(
    owner: WindowOwner,
    id: WindowId,
    publish_serial: u64,
) -> bool {
    let Ok((slot, generation)) = unpack_handle(id.0) else {
        return false;
    };
    let broker = WINDOW_BROKER.lock();
    broker.windows.get(slot).is_some_and(|window| {
        window.generation == generation
            && window.owner == owner
            && publish_serial != 0
            && window.presented_serials.contains(&publish_serial)
    })
}

/// Clear only the damage represented by a successfully composed snapshot.
/// If the producer published again meanwhile, the serial differs and its new
/// damage remains pending.
pub(crate) fn acknowledge_window_frame(id: WindowId, publish_serial: u64) -> bool {
    acknowledge_window_frame_inner(id, publish_serial, None)
}

/// Slot 4 advances independently from the application compositor. Require the
/// exact broker revision copied into its pending scene so an older SURFLIVE
/// completion cannot clear damage for a newer placement or opacity sample.
pub(super) fn acknowledge_window_frame_revision(
    id: WindowId,
    publish_serial: u64,
    revision: u64,
) -> bool {
    acknowledge_window_frame_inner(id, publish_serial, Some(revision))
}

fn acknowledge_window_frame_inner(
    id: WindowId,
    publish_serial: u64,
    required_revision: Option<u64>,
) -> bool {
    let (acknowledged, first_presentation) = {
        let mut broker = WINDOW_BROKER.lock();
        let first_presentation = {
            let Ok((slot, generation)) = unpack_handle(id.0) else {
                return false;
            };
            let Some(window) = broker.windows.get_mut(slot) else {
                return false;
            };
            if window.generation != generation
                || !matches!(window.state, WindowState::Ready | WindowState::Closing)
            {
                None
            } else {
                let index =
                    usize::from(window.presented_serial_cursor) % window.presented_serials.len();
                window.presented_serials[index] = publish_serial;
                window.presented_serial_cursor = window.presented_serial_cursor.wrapping_add(1);
                // This operation is called only after the compositor's plane
                // batch reports SURFLIVE. Mark the window even when a faster
                // producer has already advanced `publish_serial`: the older
                // frame was still physically presented and its newer damage
                // must simply remain pending.
                if window.first_presentation_emitted {
                    None
                } else {
                    window.first_presentation_emitted = true;
                    window.snapshot(slot)
                }
            }
        };
        let acknowledged = if let Some(revision) = required_revision {
            broker.acknowledge_revision(id, publish_serial, revision)
        } else {
            broker.acknowledge(id, publish_serial)
        };
        (acknowledged, first_presentation)
    };
    if let Some(window) = first_presentation {
        publish_window_first_presentation(window);
    }
    acknowledged
}

fn publish_window_first_presentation(window: WindowSnapshot) {
    let mut queue = WINDOW_FIRST_PRESENTATIONS.lock();
    if queue.is_full() {
        let dropped = queue.pop_front();
        crate::log_warn!(
            target: "ui4";
            "ui4/window: first-presentation queue full capacity={} dropped_window={:?} policy=retain-newest\n",
            WINDOW_FIRST_PRESENTATION_QUEUE_CAP,
            dropped.map(|window| window.id.raw()),
        );
    }
    let _ = queue.push_back(window);
    drop(queue);
    WINDOW_FIRST_PRESENTATION_READY.signal(());
}

/// Wait for the next window whose first composed frame has crossed the actual
/// display SURFLIVE boundary. The bounded queue preserves launch order while
/// a consumer performs asynchronous cursor choreography.
pub(crate) async fn wait_for_window_first_presentation() -> WindowSnapshot {
    loop {
        if let Some(window) = WINDOW_FIRST_PRESENTATIONS.lock().pop_front() {
            return window;
        }
        WINDOW_FIRST_PRESENTATION_READY.wait().await;
    }
}

/// Take this window's owner-visible first-presentation event.
///
/// The compositor latches the event only after the first composed frame has
/// crossed SURFLIVE. This per-window latch is independent of the global Spirit
/// notification queue, so an application observing its own event cannot steal
/// another kernel consumer's notification.
pub(crate) fn take_window_first_presentation(
    owner: WindowOwner,
    id: WindowId,
) -> Result<bool, WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    if !window.first_presentation_emitted || window.first_presentation_taken {
        return Ok(false);
    }
    window.first_presentation_taken = true;
    Ok(true)
}

fn close_transition_placement(
    initial: WindowPlacement,
    elapsed_ms: u64,
    duration_ms: u64,
    puff_per_mille: u64,
) -> WindowPlacement {
    scaled_fade_transition_placement(
        initial,
        elapsed_ms,
        duration_ms,
        1_000u64.saturating_add(puff_per_mille),
    )
}

fn clamp_transition_to_output(
    mut placement: WindowPlacement,
    output_extent: Option<(u32, u32)>,
) -> WindowPlacement {
    let Some((output_width, output_height)) = output_extent else {
        return placement;
    };
    placement.width = placement.width.min(output_width).max(1);
    placement.height = placement.height.min(output_height).max(1);
    placement.x = i64::from(placement.x)
        .clamp(0, i64::from(output_width.saturating_sub(placement.width)))
        as i32;
    placement.y = i64::from(placement.y)
        .clamp(0, i64::from(output_height.saturating_sub(placement.height)))
        as i32;
    placement
}

fn scaled_fade_transition_placement(
    initial: WindowPlacement,
    elapsed_ms: u64,
    duration_ms: u64,
    final_scale_per_mille: u64,
) -> WindowPlacement {
    let linear = elapsed_ms
        .saturating_mul(1_000)
        .checked_div(duration_ms.max(1))
        .unwrap_or(1_000)
        .min(1_000);
    // Fade across the full duration while a cubic ease-out gives the scale
    // change an immediate but soft response.
    let fade_eased = linear
        .saturating_mul(linear)
        .saturating_mul(3_000u64.saturating_sub(linear.saturating_mul(2)))
        / 1_000_000;
    let scale_remaining = 1_000u64.saturating_sub(linear);
    let scale_eased = 1_000u64.saturating_sub(
        scale_remaining
            .saturating_mul(scale_remaining)
            .saturating_mul(scale_remaining)
            / 1_000_000,
    );
    let scale = if final_scale_per_mille >= 1_000 {
        1_000u64.saturating_add(
            final_scale_per_mille
                .saturating_sub(1_000)
                .saturating_mul(scale_eased)
                / 1_000,
        )
    } else {
        1_000u64.saturating_sub(
            1_000u64
                .saturating_sub(final_scale_per_mille)
                .saturating_mul(scale_eased)
                / 1_000,
        )
    };
    let width = ((u64::from(initial.width).saturating_mul(scale) + 500) / 1_000)
        .max(1)
        .min(u64::from(u32::MAX)) as u32;
    let height = ((u64::from(initial.height).saturating_mul(scale) + 500) / 1_000)
        .max(1)
        .min(u64::from(u32::MAX)) as u32;
    let x = centered_scale_coordinate(initial.x, initial.width, width);
    let y = centered_scale_coordinate(initial.y, initial.height, height);
    let opacity = (u64::from(initial.opacity).saturating_mul(1_000u64.saturating_sub(fade_eased))
        / 1_000) as u8;
    WindowPlacement {
        x,
        y,
        width,
        height,
        opacity,
        ..initial
    }
}

fn open_transition(
    initial: WindowPlacement,
    plane: WindowPlane,
    started_ms: u64,
) -> WindowOpenTransition {
    let (duration_ms, shrink_per_mille) = if matches!(plane.slot(), 1..=3) {
        (DIRECT_PLANE_CLOSE_WAVE_DURATION_MS, DIRECT_PLANE_OPEN_SHRINK_PER_MILLE)
    } else {
        (CLOSE_TRANSITION_DURATION_MS, OPEN_TRANSITION_SHRINK_PER_MILLE)
    };
    WindowOpenTransition {
        initial,
        current: open_transition_placement(initial, 0, duration_ms, shrink_per_mille),
        started_ms,
        duration_ms,
        shrink_per_mille,
    }
}

fn open_transition_placement(
    initial: WindowPlacement,
    elapsed_ms: u64,
    duration_ms: u64,
    shrink_per_mille: u64,
) -> WindowPlacement {
    scaled_fade_transition_placement(
        initial,
        duration_ms.saturating_sub(elapsed_ms),
        duration_ms,
        1_000u64.saturating_sub(shrink_per_mille),
    )
}

fn centered_scale_coordinate(origin: i32, initial: u32, current: u32) -> i32 {
    let centered =
        i64::from(origin).saturating_add(i64::from(initial).saturating_sub(i64::from(current)) / 2);
    centered.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn next_generation(generation: u16) -> u16 {
    generation.wrapping_add(1).max(1)
}

fn next_serial(serial: u64) -> u64 {
    serial.wrapping_add(1).max(1)
}

fn pack_handle(slot: usize, generation: u16) -> Result<u32, WindowBrokerError> {
    let slot = u16::try_from(slot).map_err(|_| WindowBrokerError::Capacity)?;
    let low = slot.checked_add(1).ok_or(WindowBrokerError::Capacity)?;
    Ok((u32::from(generation) << 16) | u32::from(low))
}

fn unpack_handle(raw: u32) -> Result<(usize, u16), WindowBrokerError> {
    let low = raw as u16;
    let generation = (raw >> 16) as u16;
    if low == 0 || generation == 0 {
        return Err(WindowBrokerError::InvalidHandle);
    }
    Ok((usize::from(low - 1), generation))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_window(
        owner: WindowOwner,
        session: WindowSessionId,
        frame: u64,
        output: usize,
        z: i32,
        visible: bool,
    ) -> WindowCreate {
        WindowCreate {
            owner,
            session,
            frame: FrameHandle::from_raw(frame).unwrap(),
            output: OutputId::from_slot(output).unwrap(),
            plane: WindowPlane::Primary,
            placement: WindowPlacement {
                x: 10 + z,
                y: 20 + z,
                width: 320,
                height: 200,
                z,
                opacity: u8::MAX,
                visible,
            },
            interaction: WindowInteraction::MOVABLE_FRAME,
        }
    }

    #[test]
    fn published_snapshot_reports_all_live_windows_without_closed_slots() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let ready = broker
            .create(test_window(owner, session, 1, 0, 4, true), FrameBuffering::Single, 0)
            .unwrap();
        let closing = broker
            .create(test_window(owner, session, 2, 1, 2, false), FrameBuffering::Single, 0)
            .unwrap();
        let closed = broker
            .create(test_window(owner, session, 3, 0, 1, true), FrameBuffering::Single, 0)
            .unwrap();

        let (ready_slot, _) = unpack_handle(ready.raw()).unwrap();
        broker.windows[ready_slot].state = WindowState::Ready;
        broker.windows[ready_slot].damage = Some(DamageRegion::FULL);
        broker.windows[ready_slot].restore_placement = Some(broker.windows[ready_slot].placement);
        broker.windows[ready_slot].dock_target = Some(WindowDockTarget::Maximize);
        let (closing_slot, _) = unpack_handle(closing.raw()).unwrap();
        broker.windows[closing_slot].state = WindowState::Closing;
        let (closed_slot, _) = unpack_handle(closed.raw()).unwrap();
        broker.windows[closed_slot].state = WindowState::Closed;

        let snapshot = broker.published_snapshot(7, 12_000);
        assert_eq!(snapshot.update_count, 7);
        assert_eq!(snapshot.published_at_ms, 12_000);
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].id, ready);
        assert_eq!(snapshot.windows[0].producer_name, "gpgpu-preview");
        assert_eq!(snapshot.windows[1].id, closing);
        assert_eq!(snapshot.stats.registry_window_slots, 3);
        assert_eq!(snapshot.stats.active_sessions, 1);
        assert_eq!(snapshot.stats.live_windows, 2);
        assert_eq!(snapshot.stats.composable_windows, 1);
        assert_eq!(snapshot.stats.ready_windows, 1);
        assert_eq!(snapshot.stats.closing_windows, 1);
        assert_eq!(snapshot.stats.damaged_windows, 1);
        assert_eq!(snapshot.stats.maximized_windows, 1);
        assert_eq!(snapshot.stats.windows_per_output, [1, 1, 0, 0]);
    }

    #[test]
    fn owner_names_keep_instance_identity_in_the_enum() {
        assert_eq!(WindowOwner::VIDEO_PLAYER.name(), "video-player");
        assert_eq!(WindowOwner::KernelApp(42).name(), "kernel-app");
        assert_eq!(WindowOwner::Vm(9).name(), "blueprint-vm");
        assert_ne!(WindowOwner::KernelApp(42), WindowOwner::KernelApp(43));
        assert_ne!(WindowOwner::Vm(9), WindowOwner::Vm(10));
    }

    #[test]
    fn drag_restore_centers_saved_extent_on_cursor_and_clamps_to_output() {
        let restore = WindowPlacement {
            x: 80,
            y: 60,
            width: 640,
            height: 360,
            z: 40,
            opacity: u8::MAX,
            visible: true,
        };
        let centered = center_restored_window_on_cursor(restore, 1_200, 700, 2_560, 1_440);
        assert_eq!((centered.x, centered.y), (880, 520));
        assert_eq!((centered.width, centered.height), (640, 360));

        let top_left = center_restored_window_on_cursor(restore, 10, 8, 2_560, 1_440);
        assert_eq!((top_left.x, top_left.y), (0, 0));

        let bottom_right = center_restored_window_on_cursor(restore, 2_550, 1_430, 2_560, 1_440);
        assert_eq!((bottom_right.x, bottom_right.y), (1_920, 1_080));
    }

    #[test]
    fn dock_geometry_partitions_odd_outputs_and_preserves_fixed_frame_size() {
        let previous = WindowPlacement {
            x: 100,
            y: 80,
            width: 320,
            height: 200,
            z: 2,
            opacity: u8::MAX,
            visible: true,
        };
        let bottom_right = docked_window_placement(
            WindowInteraction::APPLICATION,
            previous,
            WindowDockTarget::BottomRight,
            1_921,
            1_081,
        );
        assert_eq!(
            (
                bottom_right.x,
                bottom_right.y,
                bottom_right.width,
                bottom_right.height,
            ),
            (960, 540, 961, 541)
        );

        let fixed = docked_window_placement(
            WindowInteraction::APPLICATION_FIXED_FRAME,
            previous,
            WindowDockTarget::BottomRight,
            1_920,
            1_080,
        );
        assert_eq!((fixed.x, fixed.y, fixed.width, fixed.height), (1_280, 710, 320, 200));
    }

    #[test]
    fn dock_resize_holds_the_old_frame_at_one_to_one_without_a_tween() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let id = broker
            .create(test_window(owner, session, 1, 0, 0, true), FrameBuffering::Double, 0)
            .unwrap();
        let (slot, _) = unpack_handle(id.raw()).unwrap();
        let initial = broker.windows[slot].placement;
        broker.windows[slot].interaction = WindowInteraction::APPLICATION;
        let change = change_window_dock_record(
            &mut broker.windows[slot],
            Some(WindowDockTarget::Maximize),
            2_560,
            1_440,
            None,
            None,
        )
        .unwrap();
        let snapshot = broker.windows[slot].snapshot(slot).unwrap();
        assert!(change.notify_resize);
        assert_eq!((snapshot.placement.width, snapshot.placement.height), (2_560, 1_440));
        assert_eq!(snapshot.presentation_placement, initial);
        assert_eq!(snapshot.dock_target, Some(WindowDockTarget::Maximize));
    }

    #[test]
    fn maximize_resize_is_requested_once_before_replacement_commit() {
        let previous = WindowPlacement {
            x: 40,
            y: 30,
            width: 960,
            height: 720,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        let target = WindowPlacement {
            x: 0,
            y: 0,
            width: 2_560,
            height: 1_440,
            ..previous
        };
        assert!(producer_resize_required(WindowInteraction::APPLICATION, previous, target,));
        // The replacement commit observes that logical placement already is
        // the target, so it cannot emit a second resize for the same action.
        assert!(!producer_resize_required(WindowInteraction::APPLICATION, target, target,));
        assert!(!producer_resize_required(WindowInteraction::MOVABLE_FRAME, previous, target,));
    }

    #[test]
    fn restoring_a_docked_application_also_holds_its_old_frame_at_one_to_one() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let id = broker
            .create(test_window(owner, session, 1, 0, 0, true), FrameBuffering::Double, 0)
            .unwrap();
        let (slot, _) = unpack_handle(id.raw()).unwrap();
        let saved = broker.windows[slot].placement;
        broker.windows[slot].interaction = WindowInteraction::APPLICATION;

        let maximized = change_window_dock_record(
            &mut broker.windows[slot],
            Some(WindowDockTarget::Maximize),
            2_560,
            1_440,
            None,
            None,
        )
        .unwrap();
        assert_eq!(maximized.transition.dock_target, Some(WindowDockTarget::Maximize));
        assert_eq!(broker.windows[slot].placement.width, 2_560);
        let maximized_placement = broker.windows[slot].placement;
        broker.windows[slot].replacement_presentation = None;

        let restored = change_window_dock_record(
            &mut broker.windows[slot],
            None,
            2_560,
            1_440,
            None,
            None,
        )
        .unwrap();
        assert_eq!(restored.transition.dock_target, None);
        assert!(restored.notify_resize);
        assert_eq!(broker.windows[slot].placement, saved);
        assert_eq!(
            broker.windows[slot]
                .snapshot(slot)
                .unwrap()
                .presentation_placement,
            maximized_placement
        );
    }

    #[test]
    fn live_resource_counts_include_pending_windows_and_active_empty_sessions() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        assert_eq!(broker.live_resource_counts(), (0, 0));

        let session = broker.begin_additional_session(owner).unwrap();
        assert_eq!(broker.live_resource_counts(), (1, 0));

        let window = broker
            .create(test_window(owner, session, 1, 0, 0, false), FrameBuffering::Single, 0)
            .unwrap();
        assert_eq!(broker.live_resource_counts(), (1, 1));

        let (slot, _) = unpack_handle(window.raw()).unwrap();
        broker.windows[slot].state = WindowState::Closed;
        assert_eq!(broker.live_resource_counts(), (1, 0));

        broker
            .finish_session(owner, session, false, false, false, false, false, 0)
            .unwrap();
        assert_eq!(broker.live_resource_counts(), (0, 0));
    }

    #[test]
    fn busy_frame_retirement_queue_deduplicates_handles() {
        let first = FrameHandle::from_raw(11).unwrap();
        let second = FrameHandle::from_raw(12).unwrap();
        let mut frames = Vec::new();

        assert!(enqueue_busy_retirement(&mut frames, first));
        assert!(!enqueue_busy_retirement(&mut frames, first));
        assert!(enqueue_busy_retirement(&mut frames, second));

        assert_eq!(frames, Vec::from([first, second]));
    }

    #[test]
    fn close_puffs_from_the_center_while_opening_keeps_its_shrink_reveal() {
        let initial = WindowPlacement {
            x: 100,
            y: 80,
            width: 800,
            height: 600,
            z: 2,
            opacity: u8::MAX,
            visible: true,
        };
        let closed = close_transition_placement(initial, 300, 300, 50);
        assert_eq!((closed.x, closed.y, closed.width, closed.height), (80, 65, 840, 630));
        assert_eq!(closed.opacity, 0);

        let full_screen = WindowPlacement {
            x: 0,
            y: 0,
            width: 1_920,
            height: 1_080,
            ..initial
        };
        let clamped = clamp_transition_to_output(
            close_transition_placement(full_screen, 300, 300, 50),
            Some((1_920, 1_080)),
        );
        assert_eq!((clamped.x, clamped.y, clamped.width, clamped.height), (0, 0, 1_920, 1_080));
        assert_eq!(clamped.opacity, 0);

        let opening = open_transition_placement(initial, 0, 300, 900);
        assert_eq!((opening.x, opening.y, opening.width, opening.height), (460, 350, 80, 60));
        assert_eq!(opening.opacity, 0);
        assert_eq!(open_transition_placement(initial, 300, 300, 900), initial);
    }

    #[test]
    fn close_waits_for_an_acknowledged_zero_alpha_sample() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let window = broker
            .create(test_window(owner, session, 7, 0, 0, true), FrameBuffering::Triple, 0)
            .unwrap();
        let (slot, _) = unpack_handle(window.raw()).unwrap();
        let frame = broker.windows[slot].frame;
        broker.windows[slot].state = WindowState::Closing;
        broker.windows[slot].damage = None;
        broker.windows[slot].close_transition = Some(WindowCloseTransition {
            lease: FrameReadLease {
                frame,
                buffer_index: 0,
            },
            initial: broker.windows[slot].placement,
            started_ms: 1_000,
            delay_ms: 0,
            duration_ms: 200,
            puff_per_mille: DIRECT_PLANE_CLOSE_PUFF_PER_MILLE,
            retire_frame: true,
        });

        let retirements = broker.advance_close_transitions(1_200, None);
        assert!(retirements.is_empty());
        assert_eq!(broker.windows[slot].state, WindowState::Closing);
        assert_eq!(broker.windows[slot].placement.opacity, 0);
        assert_eq!(broker.windows[slot].damage, Some(DamageRegion::FULL));

        assert!(broker.acknowledge(window, 0));
        let retirements = broker.advance_close_transitions(1_216, None);
        assert_eq!(retirements.len(), 1);
        assert_eq!(broker.windows[slot].state, WindowState::Closed);
        assert!(broker.windows[slot].close_transition.is_none());
    }

    fn plane_slot_of(broker: &WindowBroker, window: WindowId) -> usize {
        let (slot, _) = unpack_handle(window.raw()).unwrap();
        broker.windows[slot].plane.slot()
    }

    #[test]
    fn lowest_windows_join_the_stack_while_highest_three_keep_z_order() {
        let owner = WindowOwner::GRIDPAPER_SERVICE;
        let mut broker = WindowBroker::new();
        let mut windows = Vec::new();

        for frame in 1..=(LEASE_PLANE_COUNT as u64 + 2) {
            let session = broker.begin_additional_session(owner).unwrap();
            let request = test_window(owner, session, frame, 0, 40, true);
            windows.push(
                broker
                    .create(request, FrameBuffering::Triple, 0)
                    .expect("admission never fails once the lease planes are full"),
            );
        }

        let assigned = windows
            .iter()
            .map(|window| plane_slot_of(&broker, *window))
            .collect::<Vec<_>>();
        // Slot0 is physically below Slots1-3, so the lowest frames share it
        // and the highest three retain monotonically ordered hardware planes.
        assert_eq!(assigned, Vec::from([STACK_PLANE_SLOT, STACK_PLANE_SLOT, 1, 2, 3]));
    }

    #[test]
    fn a_live_lease_is_never_revoked_but_an_idle_one_is() {
        let owner = WindowOwner::GRIDPAPER_SERVICE;
        let mut broker = WindowBroker::new();
        let mut held = Vec::new();
        for frame in 1..=LEASE_PLANE_COUNT as u64 {
            let session = broker.begin_additional_session(owner).unwrap();
            let request = test_window(owner, session, frame, 0, frame as i32, true);
            held.push(
                broker
                    .create(request, FrameBuffering::Triple, 1_000)
                    .unwrap(),
            );
        }

        // Every lease is still inside its grace period, so the stacked frame
        // stays on the stack rather than displacing a live winner.
        let session = broker.begin_additional_session(owner).unwrap();
        let newest = broker
            .create(test_window(owner, session, 9, 0, 9, true), FrameBuffering::Triple, 1_000)
            .unwrap();
        let stacked = held[0];
        assert_eq!(plane_slot_of(&broker, stacked), STACK_PLANE_SLOT);
        assert_eq!(plane_slot_of(&broker, newest), FIRST_LEASE_PLANE_SLOT + 2);
        assert!(
            broker
                .claim_lease(stacked, 1_000 + LEASE_IDLE_GRACE_MS - 1)
                .is_none()
        );

        // Past the grace period the longest-idle holder gives up its plane and
        // is demoted to the stack in the same transaction.
        let plane = broker
            .claim_lease(stacked, 1_000 + LEASE_IDLE_GRACE_MS)
            .expect("an expired lease is revocable");
        assert_eq!(plane.slot(), FIRST_LEASE_PLANE_SLOT);
        assert_eq!(plane_slot_of(&broker, held[1]), STACK_PLANE_SLOT);
    }

    #[test]
    fn focused_lease_reaches_the_top_plane_without_waiting_for_reclaim_grace() {
        let owner = WindowOwner::GRIDPAPER_SERVICE;
        let mut broker = WindowBroker::new();
        let mut windows = Vec::new();
        for frame in 1..=LEASE_PLANE_COUNT as u64 {
            let session = broker.begin_additional_session(owner).unwrap();
            windows.push(
                broker
                    .create(
                        test_window(owner, session, frame, 0, frame as i32, true),
                        FrameBuffering::Triple,
                        1_000,
                    )
                    .unwrap(),
            );
        }

        assert_eq!(plane_slot_of(&broker, windows[0]), FIRST_LEASE_PLANE_SLOT);
        assert_eq!(
            broker.raise_lease(0, 1_001).slot(),
            FIRST_LEASE_PLANE_SLOT + LEASE_PLANE_COUNT - 1
        );
        assert_eq!(
            plane_slot_of(&broker, windows[0]),
            FIRST_LEASE_PLANE_SLOT + LEASE_PLANE_COUNT - 1
        );
    }

    #[test]
    fn closing_a_window_frees_its_lease_plane_for_the_next_claimant() {
        let owner = WindowOwner::GRIDPAPER_SERVICE;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let first = broker
            .create(test_window(owner, session, 1, 0, 1, true), FrameBuffering::Triple, 0)
            .unwrap();
        assert_eq!(plane_slot_of(&broker, first), FIRST_LEASE_PLANE_SLOT);

        let (slot, _) = unpack_handle(first.raw()).unwrap();
        broker.windows[slot].state = WindowState::Closed;
        broker.release_lease_of(first);

        let next = broker
            .create(test_window(owner, session, 2, 0, 2, true), FrameBuffering::Triple, 0)
            .unwrap();
        assert_eq!(plane_slot_of(&broker, next), FIRST_LEASE_PLANE_SLOT);
    }

    #[test]
    fn trusted_small_controls_share_interaction_plane_without_blueprint_admission() {
        let mut broker = WindowBroker::new();
        for frame in 1..=LEASE_PLANE_COUNT as u64 {
            let owner = WindowOwner::GPGPU_PREVIEW;
            let session = broker.begin_additional_session(owner).unwrap();
            let request = test_window(owner, session, frame, 0, frame as i32, true);
            broker
                .create(request, FrameBuffering::Triple, 0)
                .expect("fill every lease plane");
        }

        let owner = WindowOwner::COLOR_PICKER_SERVICE;
        let session = broker.begin_additional_session(owner).unwrap();
        let mut request = test_window(owner, session, 10, 0, 100, true);
        request.plane = WindowPlane::Interaction;
        let picker = broker
            .create(request, FrameBuffering::Double, 0)
            .expect("slot 4 remains independent of the lease planes");
        let (slot, _) = unpack_handle(picker.raw()).unwrap();
        assert_eq!(broker.windows[slot].plane, WindowPlane::Interaction);
        assert_eq!(broker.windows[slot].plane.slot(), super::super::INTERACTION_OVERLAY_PLANE_SLOT);

        broker
            .replace_frame(
                owner,
                picker,
                FrameHandle::from_raw(11).unwrap(),
                FrameBuffering::Double,
            )
            .expect("replacement remains on the fixed interaction plane");
        assert_eq!(broker.windows[slot].plane, WindowPlane::Interaction);

        let start_owner = WindowOwner::START_BUTTON_SERVICE;
        let start_session = broker.begin_additional_session(start_owner).unwrap();
        let mut start = test_window(start_owner, start_session, 12, 0, i32::MAX, true);
        start.plane = WindowPlane::Interaction;
        let start = broker
            .create(start, FrameBuffering::Double, 0)
            .expect("trusted start button shares slot 4 with the color picker");
        let (start_slot, _) = unpack_handle(start.raw()).unwrap();
        assert_eq!(broker.windows[start_slot].plane, WindowPlane::Interaction);

        let second_session = broker.begin_additional_session(owner).unwrap();
        let mut second = test_window(owner, second_session, 13, 0, 101, true);
        second.plane = WindowPlane::Interaction;
        assert_eq!(
            broker.create(second, FrameBuffering::Double, 0),
            Err(WindowBrokerError::Capacity)
        );

        let blueprint_owner = WindowOwner::Vm(1);
        let blueprint_session = broker.begin_additional_session(blueprint_owner).unwrap();
        let mut blueprint = test_window(blueprint_owner, blueprint_session, 14, 0, 102, true);
        blueprint.plane = WindowPlane::Interaction;
        assert_eq!(
            broker.create(blueprint, FrameBuffering::Double, 0),
            Err(WindowBrokerError::InvalidPlane)
        );
    }

    #[test]
    fn revision_ack_cannot_clear_newer_interaction_damage() {
        let owner = WindowOwner::COLOR_PICKER_SERVICE;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let mut request = test_window(owner, session, 21, 0, 0, true);
        request.plane = WindowPlane::Interaction;
        let window = broker.create(request, FrameBuffering::Double, 0).unwrap();
        let (slot, _) = unpack_handle(window.raw()).unwrap();
        broker.windows[slot].state = WindowState::Closing;
        broker.windows[slot].publish_serial = 4;
        broker.windows[slot].revision = 9;
        broker.windows[slot].damage = Some(DamageRegion::FULL);

        assert!(!broker.acknowledge_revision(window, 4, 8));
        assert_eq!(broker.windows[slot].damage, Some(DamageRegion::FULL));
        assert!(broker.acknowledge_revision(window, 4, 9));
        assert_eq!(broker.windows[slot].damage, None);
    }
}
