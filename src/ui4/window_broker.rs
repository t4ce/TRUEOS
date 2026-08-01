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
use embassy_time::{Duration, Timer};
use heapless::Deque;
use spin::Mutex;

use super::{DamageRect, DamageRegion, FrameBuffering, FrameHandle, OutputId};

pub(super) const MAX_WINDOWS: usize = 256;
// Temporary static30 composition probe: one trusted app session owns all 30
// test windows. Plane assignment is independent of session ownership, while
// MAX_WINDOWS remains only the broker registry's hard storage bound.
const MAX_WINDOWS_PER_SESSION: usize = 32;
const MAX_SESSIONS: usize = 64;
/// Temporary direct-scanout admission boundary. Non-shareable double- and
/// triple-buffered windows each own one of the four application planes.
/// Single-buffered windows, dirty/double FontScene members, and
/// streaming/triple RenderScene members remain unrestricted by this soft cap
/// and may share a requested plane through UI4 composition.
pub(super) const MAX_EXPENSIVE_WINDOWS: usize = super::INTERACTION_OVERLAY_PLANE_SLOT;
pub(crate) const WINDOW_BROKER_SNAPSHOT_PERIOD_MS: u64 = 3_000;
const WINDOW_BROKER_SNAPSHOT_RECEIVERS: usize = 8;
const WINDOW_FIRST_PRESENTATION_QUEUE_CAP: usize = 32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowOwner {
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

/// Fixed hardware-composition target selected when a broker window is
/// created. Runtime migration is deliberately not part of this contract yet.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowPlane {
    Primary,
    Universal(u8),
}

impl WindowPlane {
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::Primary => super::PRIMARY_PLANE_SLOT,
            Self::Universal(slot) => slot as usize,
        }
    }

    const fn valid(self) -> bool {
        match self {
            Self::Primary => true,
            // Slot 4 is deliberately not a broker-window target: UI4 owns it
            // as the topmost per-vCursor interaction plane.
            Self::Universal(slot) => {
                slot > 0 && (slot as usize) < super::INTERACTION_OVERLAY_PLANE_SLOT
            }
        }
    }

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
    pub(crate) maximizable: bool,
    pub(crate) receives_input: bool,
    /// The producer can replace its complete frame allocation after a
    /// maximize/restore extent notification. Fixed-size producers still use
    /// maximize as a broker-owned 1:1 centering operation.
    pub(crate) resize_on_maximize: bool,
}

impl WindowInteraction {
    /// UI4 may translate the frame, while its producer remains independent of
    /// pointer/keyboard delivery and keeps a fixed pixel extent.
    pub(crate) const MOVABLE_FRAME: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: false,
        resize_on_maximize: false,
    };

    /// The producer consumes application input and can be centered/restored,
    /// but its native frame extent remains fixed throughout that transition.
    pub(crate) const APPLICATION_FIXED_FRAME: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: true,
        resize_on_maximize: false,
    };

    /// Full broker interaction for producers which drain owner events and can
    /// replace their frame allocation after a resize notification.
    pub(crate) const APPLICATION: Self = Self {
        movable: true,
        maximizable: true,
        receives_input: true,
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
    shrink: bool,
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
    pub(crate) const fn persist_final_frame_as(mut self, name: &'a str) -> Self {
        self.persist_final_frame = true;
        self.final_frame_name = Some(name);
        self
    }

    /// Keep each published window alive as a compositor-owned exit visual.
    /// The producer retains ownership of the underlying frames.
    pub(crate) const fn animate(mut self) -> Self {
        self.animate = true;
        self.shrink = true;
        self
    }

    /// Animate the close and transfer the detached frame lifetime to UI4.
    pub(crate) const fn animate_and_retire_frames(mut self) -> Self {
        self.animate = true;
        self.shrink = true;
        self.retire_frames = true;
        self
    }

    /// Shrink and fade direct planes using only pipe-scaler geometry and
    /// constant alpha. The exact published allocation and source geometry
    /// remain unchanged until the final SURFLIVE-backed retirement.
    pub(crate) const fn direct_plane_animate_and_retire_frames(mut self) -> Self {
        self.animate = true;
        self.shrink = true;
        self.direct_plane_scaling = true;
        self.retire_frames = true;
        self
    }

    /// Shrink and fade a direct plane while the producer keeps ownership of
    /// the underlying frame ring for a later presentation session.
    pub(crate) const fn direct_plane_animate(mut self) -> Self {
        self.animate = true;
        self.shrink = true;
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
    pub(crate) placement: WindowPlacement,
    pub(crate) interaction: WindowInteraction,
    pub(crate) state: WindowState,
    pub(crate) revision: u64,
    pub(crate) publish_serial: u64,
    pub(crate) damage: Option<DamageRegion>,
    pub(crate) maximized: bool,
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

    pub(crate) const fn has_data(&self) -> bool {
        self.update_count != 0
    }
}

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
    pub(crate) maximized: bool,
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
    state: WindowState,
    revision: u64,
    publish_serial: u64,
    first_presentation_emitted: bool,
    first_presentation_taken: bool,
    damage: Option<DamageRegion>,
    restore_placement: Option<WindowPlacement>,
    close_transition: Option<WindowCloseTransition>,
}

#[derive(Copy, Clone)]
struct WindowCloseTransition {
    lease: super::FrameReadLease,
    initial: WindowPlacement,
    started_ms: u64,
    delay_ms: u64,
    duration_ms: u64,
    shrink_per_mille: u64,
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
const CLOSE_TRANSITION_SHRINK_PER_MILLE: u64 = 900;
const DIRECT_PLANE_CLOSE_WAVE_DURATION_MS: u64 = 200;
// Gen12/13 pipe scalers top out just below 3x downscale. Ending at 35%
// stays comfortably inside that contract while alpha reaches zero.
const DIRECT_PLANE_CLOSE_SHRINK_PER_MILLE: u64 = 650;

#[derive(Copy, Clone)]
struct SessionRecord {
    generation: u16,
    owner: WindowOwner,
    active: bool,
}

struct WindowBroker {
    windows: Vec<WindowRecord>,
    sessions: Vec<SessionRecord>,
    /// Monotonic epoch for state which can change the compositor's plane plan.
    /// Damage acknowledgement deliberately does not advance this value.
    composition_revision: u64,
}

impl WindowBroker {
    const fn new() -> Self {
        Self {
            windows: Vec::new(),
            sessions: Vec::new(),
            composition_revision: 0,
        }
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

    fn select_plane(
        &self,
        requested: WindowPlane,
        buffering: FrameBuffering,
        share_requested_plane: bool,
        replacing_slot: Option<usize>,
        owner: WindowOwner,
        session: WindowSessionId,
    ) -> Result<WindowPlane, WindowBrokerError> {
        if share_requested_plane {
            return Ok(requested);
        }

        let mut occupied = [false; MAX_EXPENSIVE_WINDOWS];
        let mut active = 0usize;
        for (slot, window) in self.windows.iter().enumerate() {
            let shares_compositor_plane = super::frame_snapshot(window.frame)
                .is_ok_and(|snapshot| super::frame_plan_shares_compositor_plane(snapshot.plan));
            if Some(slot) != replacing_slot
                && window.state != WindowState::Closed
                && !shares_compositor_plane
            {
                active = active.saturating_add(1);
                occupied[window.plane.slot()] = true;
            }
        }
        if active >= MAX_EXPENSIVE_WINDOWS {
            crate::log_error!(
                target: "ui4";
                "ui4 expensive-window soft-cap reached requested={} cap={} buffering={:?} owner={:?} session={} policy=temporary-soft-cap action=reject-expensive-admission\n",
                active.saturating_add(1),
                MAX_EXPENSIVE_WINDOWS,
                buffering,
                owner,
                session.raw(),
            );
            return Err(WindowBrokerError::Capacity);
        }

        let requested_slot = requested.slot();
        for offset in 0..MAX_EXPENSIVE_WINDOWS {
            let slot = (requested_slot + offset) % MAX_EXPENSIVE_WINDOWS;
            if !occupied[slot] {
                return WindowPlane::from_slot(slot).ok_or(WindowBrokerError::InvalidPlane);
            }
        }
        Err(WindowBrokerError::Capacity)
    }

    fn create(
        &mut self,
        request: WindowCreate,
        buffering: FrameBuffering,
    ) -> Result<WindowId, WindowBrokerError> {
        self.create_with_plane_policy(request, buffering, buffering == FrameBuffering::Single)
    }

    fn create_with_plane_policy(
        &mut self,
        mut request: WindowCreate,
        buffering: FrameBuffering,
        share_requested_plane: bool,
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
        let requested_plane = request.plane;
        request.plane = self.select_plane(
            request.plane,
            buffering,
            share_requested_plane,
            None,
            request.owner,
            request.session,
        )?;
        if request.plane != requested_plane {
            crate::log_info!(
                target: "ui4";
                "ui4 expensive-window plane isolated requested_slot={} assigned_slot={} buffering={:?} owner={:?} session={}\n",
                requested_plane.slot(),
                request.plane.slot(),
                buffering,
                request.owner,
                request.session.raw(),
            );
        }
        if let Some((slot, window)) = self
            .windows
            .iter_mut()
            .enumerate()
            .find(|(_, window)| window.state == WindowState::Closed)
        {
            let generation = next_generation(window.generation);
            *window = WindowRecord::new(generation, request, buffering);
            let id = WindowId(pack_handle(slot, generation)?);
            self.mark_composition_changed();
            return Ok(id);
        }
        if self.windows.len() >= MAX_WINDOWS {
            return Err(WindowBrokerError::Capacity);
        }
        let slot = self.windows.len();
        self.windows.push(WindowRecord::new(1, request, buffering));
        let id = WindowId(pack_handle(slot, 1)?);
        self.mark_composition_changed();
        Ok(id)
    }

    fn replace_frame(
        &mut self,
        owner: WindowOwner,
        id: WindowId,
        frame: FrameHandle,
        buffering: FrameBuffering,
        share_requested_plane: bool,
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
        let previous_plane = current.plane;
        let plane = self.select_plane(
            current.plane,
            buffering,
            share_requested_plane,
            Some(slot),
            current.owner,
            current.session,
        )?;
        if plane != previous_plane {
            crate::log_info!(
                target: "ui4";
                "ui4 replacement-frame plane isolated previous_slot={} assigned_slot={} buffering={:?} owner={:?} window={}\n",
                previous_plane.slot(),
                plane.slot(),
                buffering,
                owner,
                id.raw(),
            );
        }
        let window = &mut self.windows[slot];
        window.frame = frame;
        window.buffering = buffering;
        window.plane = plane;
        window.state = WindowState::Pending;
        window.publish_serial = 0;
        window.damage = None;
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
        shrink: bool,
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
                let (delay_ms, duration_ms, shrink_per_mille) = if direct_plane_scaling {
                    let delay_ms = if window.plane.slot() == 3 && direct_first_wave_present {
                        DIRECT_PLANE_CLOSE_WAVE_DURATION_MS
                    } else {
                        0
                    };
                    (
                        delay_ms,
                        DIRECT_PLANE_CLOSE_WAVE_DURATION_MS,
                        DIRECT_PLANE_CLOSE_SHRINK_PER_MILLE,
                    )
                } else {
                    (
                        0,
                        CLOSE_TRANSITION_DURATION_MS,
                        if shrink {
                            CLOSE_TRANSITION_SHRINK_PER_MILLE
                        } else {
                            0
                        },
                    )
                };
                let transition = if animate && window.state == WindowState::Ready {
                    super::acquire_published_frame(window.frame)
                        .ok()
                        .map(|lease| WindowCloseTransition {
                            lease,
                            initial: window.placement,
                            started_ms,
                            delay_ms,
                            duration_ms,
                            shrink_per_mille,
                            retire_frame: retire_frames,
                        })
                } else {
                    None
                };
                if let Some(transition) = transition {
                    window.state = WindowState::Closing;
                    window.close_transition = Some(transition);
                    window.damage = Some(DamageRegion::FULL);
                    animation_duration_ms = animation_duration_ms
                        .max(transition.delay_ms.saturating_add(transition.duration_ms));
                    final_scale_percent = final_scale_percent
                        .min((1_000u64.saturating_sub(transition.shrink_per_mille)) / 10);
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

    fn advance_close_transitions(&mut self, now_ms: u64) -> Vec<WindowTransitionRetirement> {
        let mut retirements = Vec::new();
        let mut composition_changed = false;
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
                let terminal = close_transition_placement(
                    transition.initial,
                    transition.duration_ms,
                    transition.duration_ms,
                    transition.shrink_per_mille,
                );
                if window.placement.opacity != 0 || window.damage.is_some() {
                    if window.placement != terminal {
                        window.placement = terminal;
                        window.damage = Some(DamageRegion::FULL);
                        window.revision = next_serial(window.revision);
                        composition_changed = true;
                    }
                    continue;
                }
                window.close_transition = None;
                window.state = WindowState::Closed;
                window.damage = None;
                window.revision = next_serial(window.revision);
                composition_changed = true;
                retirements.push(WindowTransitionRetirement {
                    lease: transition.lease,
                    frame: window.frame,
                    retire_frame: transition.retire_frame,
                    elapsed_total_ms: elapsed_ms,
                });
                continue;
            }
            let active_elapsed_ms = elapsed_ms.saturating_sub(transition.delay_ms);
            let placement = close_transition_placement(
                transition.initial,
                active_elapsed_ms,
                transition.duration_ms,
                transition.shrink_per_mille,
            );
            if placement != window.placement {
                window.placement = placement;
                window.damage = Some(DamageRegion::FULL);
                window.revision = next_serial(window.revision);
                composition_changed = true;
            }
        }
        if composition_changed {
            self.mark_composition_changed();
        }
        retirements
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
            state: WindowState::Pending,
            revision: 1,
            publish_serial: 0,
            first_presentation_emitted: false,
            first_presentation_taken: false,
            damage: None,
            restore_placement: None,
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
            interaction: self.interaction,
            state: self.state,
            revision: self.revision,
            publish_serial: self.publish_serial,
            damage: self.damage,
            maximized: self.restore_placement.is_some(),
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

/// Optionally subscribe to future diagnostic publications.
pub(crate) fn subscribe_window_broker_snapshots() -> Option<WindowBrokerSnapshotReceiver<'static>> {
    WINDOW_BROKER_SNAPSHOT.receiver()
}

fn publish_window_broker_snapshot_once() -> WindowBrokerSnapshot {
    let update_count = next_serial(latest_window_broker_snapshot().update_count);
    let published_at_ms = embassy_time::Instant::now().as_millis();
    let snapshot = WINDOW_BROKER
        .lock()
        .published_snapshot(update_count, published_at_ms);
    WINDOW_BROKER_SNAPSHOT.sender().send(snapshot.clone());
    snapshot
}

#[embassy_executor::task(pool_size = 1)]
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
    let started_ms = embassy_time::Instant::now().as_millis();
    let finish = WINDOW_BROKER.lock().finish_session(
        owner,
        session,
        request.persist_final_frame,
        request.animate,
        request.shrink,
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
                "direct-plane-shrink+fade"
            } else if request.shrink {
                "shrink+fade"
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
    let now_ms = embassy_time::Instant::now().as_millis();
    let retirements = WINDOW_BROKER.lock().advance_close_transitions(now_ms);
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
    let id = if super::frame_plan_shares_compositor_plane(plan) {
        WINDOW_BROKER
            .lock()
            .create_with_plane_policy(request, plan.buffering, true)?
    } else {
        WINDOW_BROKER.lock().create(request, plan.buffering)?
    };
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
    WINDOW_BROKER.lock().replace_frame(
        owner,
        id,
        frame,
        plan.buffering,
        super::frame_plan_shares_compositor_plane(plan),
    )?;
    super::cursor_frame_inout::frame_visual_changed(owner, id);
    Ok(())
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
    let notify_resize = window.interaction.receives_input
        && (previous.width != placement.width || previous.height != placement.height);
    let changed = window.placement != placement;
    if changed {
        window.placement = placement;
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
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    Ok(())
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
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    Ok(())
}

/// Return the broker geometry represented by a maximize preview or commit.
///
/// Fixed-size producers preserve their exact pixel extent and are centered on
/// the output. Resize-capable producers receive full-output geometry; until
/// their replacement frame arrives, the compositor centers the previous
/// allocation at 1:1 inside that geometry.
pub(super) fn maximized_window_placement(
    interaction: WindowInteraction,
    previous: WindowPlacement,
    output_width: u32,
    output_height: u32,
) -> WindowPlacement {
    if interaction.resize_on_maximize {
        WindowPlacement {
            x: 0,
            y: 0,
            width: output_width,
            height: output_height,
            ..previous
        }
    } else {
        WindowPlacement {
            x: output_width.saturating_sub(previous.width) as i32 / 2,
            y: output_height.saturating_sub(previous.height) as i32 / 2,
            ..previous
        }
    }
}

/// Toggle one broker window between its saved geometry and its generic
/// maximize geometry. Gesture recognition stays in the input broker; this
/// function owns the atomic placement/restore transition and emits resize
/// callbacks only for producers which explicitly support frame replacement.
pub(crate) fn toggle_window_maximized(
    owner: WindowOwner,
    id: WindowId,
    output_width: u32,
    output_height: u32,
    restore_placement: Option<WindowPlacement>,
) -> Result<WindowPlacementTransition, WindowBrokerError> {
    if output_width == 0 || output_height == 0 {
        return Err(WindowBrokerError::EmptyExtent);
    }
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    if !window.interaction.maximizable {
        return Err(WindowBrokerError::InteractionDenied);
    }
    let previous = window.placement;
    let (placement, maximized) = if let Some(restore) = window.restore_placement.take() {
        (restore, false)
    } else {
        window.restore_placement = Some(restore_placement.unwrap_or(previous));
        (
            maximized_window_placement(window.interaction, previous, output_width, output_height),
            true,
        )
    };
    let changed = previous != placement;
    if changed {
        window.placement = placement;
        window.revision = next_serial(window.revision);
    }
    let notify_resize = window.interaction.receives_input
        && window.interaction.resize_on_maximize
        && (previous.width != placement.width || previous.height != placement.height);
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
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    Ok(WindowPlacementTransition {
        previous,
        placement,
        maximized,
    })
}

pub(crate) fn publish_window_frame(
    owner: WindowOwner,
    id: WindowId,
    damage: DamageRect,
) -> Result<u64, WindowBrokerError> {
    if !damage.valid() {
        return Err(WindowBrokerError::EmptyDamage);
    }
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    window.state = WindowState::Ready;
    window.publish_serial = next_serial(window.publish_serial);
    window.revision = next_serial(window.revision);
    let pending = window.damage.get_or_insert(DamageRegion::EMPTY);
    pending.add(damage);
    let publish_serial = window.publish_serial;
    broker.mark_composition_changed();
    drop(broker);
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
    let mut broker = WINDOW_BROKER.lock();
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
            WindowState::Pending | WindowState::Ready => {}
            WindowState::Closing => return Err(WindowBrokerError::SessionClosed),
            WindowState::Closed => return Err(WindowBrokerError::Closed),
        }
    }
    for (id, damage) in publications.iter().copied() {
        let (slot, _) = unpack_handle(id.0)?;
        let window = &mut broker.windows[slot];
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
    for (id, _) in publications.iter().copied() {
        super::cursor_frame_inout::frame_visual_changed(owner, id);
    }
    Ok(())
}

pub(crate) fn close_window(owner: WindowOwner, id: WindowId) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    window.state = WindowState::Closed;
    window.damage = None;
    window.revision = next_serial(window.revision);
    broker.mark_composition_changed();
    drop(broker);
    super::cursor_frame_inout::frame_closed(owner, id);
    super::context_menu::dismiss_window(owner, id);
    Ok(())
}

/// Cheap change detector for the compositor's idle path. The subsequent
/// snapshot API returns its epoch under the same broker lock, closing the race
/// between this optimistic check and a producer publication.
pub(crate) fn window_composition_revision() -> u64 {
    WINDOW_BROKER.lock().composition_revision
}

pub(crate) fn window_close_transitions_active() -> bool {
    if !TRANSITION_RETIRED_FRAMES.lock().is_empty() {
        return true;
    }
    WINDOW_BROKER
        .lock()
        .windows
        .iter()
        .any(|window| window.close_transition.is_some())
}

pub(crate) async fn wait_for_window_composition_change() {
    WINDOW_COMPOSITION_CHANGED.wait().await;
}

pub(crate) fn visible_windows_for_output_with_revision(
    output: OutputId,
) -> (u64, Vec<WindowSnapshot>) {
    let broker = WINDOW_BROKER.lock();
    (broker.composition_revision, broker.snapshots(output))
}

pub(crate) fn visible_windows_for_output(output: OutputId) -> Vec<WindowSnapshot> {
    WINDOW_BROKER.lock().snapshots(output)
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

/// Clear only the damage represented by a successfully composed snapshot.
/// If the producer published again meanwhile, the serial differs and its new
/// damage remains pending.
pub(crate) fn acknowledge_window_frame(id: WindowId, publish_serial: u64) -> bool {
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
                || window.first_presentation_emitted
            {
                None
            } else {
                // This operation is called only after the compositor's plane
                // batch reports SURFLIVE. Mark the window even when a faster
                // producer has already advanced `publish_serial`: the older
                // frame was still physically presented and its newer damage
                // must simply remain pending.
                window.first_presentation_emitted = true;
                window.snapshot(slot)
            }
        };
        let acknowledged = broker.acknowledge(id, publish_serial);
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
    shrink_per_mille: u64,
) -> WindowPlacement {
    let linear = elapsed_ms
        .saturating_mul(1_000)
        .checked_div(duration_ms.max(1))
        .unwrap_or(1_000)
        .min(1_000);
    // Fade across the full duration, while an ease-out curve gets most of the
    // much stronger scale-down done early enough to remain visually legible.
    let fade_eased = linear
        .saturating_mul(linear)
        .saturating_mul(3_000u64.saturating_sub(linear.saturating_mul(2)))
        / 1_000_000;
    let scale = if shrink_per_mille != 0 {
        let scale_remaining = 1_000u64.saturating_sub(linear);
        let scale_eased = 1_000u64.saturating_sub(
            scale_remaining
                .saturating_mul(scale_remaining)
                .saturating_mul(scale_remaining)
                / 1_000_000,
        );
        1_000u64.saturating_sub(shrink_per_mille.saturating_mul(scale_eased) / 1_000)
    } else {
        1_000
    };
    let width = ((u64::from(initial.width).saturating_mul(scale) + 500) / 1_000)
        .max(1)
        .min(u64::from(u32::MAX)) as u32;
    let height = ((u64::from(initial.height).saturating_mul(scale) + 500) / 1_000)
        .max(1)
        .min(u64::from(u32::MAX)) as u32;
    let x = centered_shrink_coordinate(initial.x, initial.width, width);
    let y = centered_shrink_coordinate(initial.y, initial.height, height);
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

fn centered_shrink_coordinate(origin: i32, initial: u32, current: u32) -> i32 {
    let centered = i64::from(origin) + i64::from(initial.saturating_sub(current) / 2);
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
            .create(test_window(owner, session, 1, 0, 4, true), FrameBuffering::Single)
            .unwrap();
        let closing = broker
            .create(test_window(owner, session, 2, 1, 2, false), FrameBuffering::Single)
            .unwrap();
        let closed = broker
            .create(test_window(owner, session, 3, 0, 1, true), FrameBuffering::Single)
            .unwrap();

        let (ready_slot, _) = unpack_handle(ready.raw()).unwrap();
        broker.windows[ready_slot].state = WindowState::Ready;
        broker.windows[ready_slot].damage = Some(DamageRegion::FULL);
        broker.windows[ready_slot].restore_placement = Some(broker.windows[ready_slot].placement);
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
    fn close_waits_for_an_acknowledged_zero_alpha_sample() {
        let owner = WindowOwner::GPGPU_PREVIEW;
        let mut broker = WindowBroker::new();
        let session = broker.begin_additional_session(owner).unwrap();
        let window = broker
            .create(test_window(owner, session, 7, 0, 0, true), FrameBuffering::Triple)
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
            shrink_per_mille: DIRECT_PLANE_CLOSE_SHRINK_PER_MILLE,
            retire_frame: true,
        });

        let retirements = broker.advance_close_transitions(1_200);
        assert!(retirements.is_empty());
        assert_eq!(broker.windows[slot].state, WindowState::Closing);
        assert_eq!(broker.windows[slot].placement.opacity, 0);
        assert_eq!(broker.windows[slot].damage, Some(DamageRegion::FULL));

        assert!(broker.acknowledge(window, 0));
        let retirements = broker.advance_close_transitions(1_216);
        assert_eq!(retirements.len(), 1);
        assert_eq!(broker.windows[slot].state, WindowState::Closed);
        assert!(broker.windows[slot].close_transition.is_none());
    }

    #[test]
    fn independent_expensive_sessions_fill_four_planes_and_reuse_the_released_slot() {
        let owner = WindowOwner::GRIDPAPER_SERVICE;
        let mut broker = WindowBroker::new();
        let mut sessions = Vec::new();
        let mut windows = Vec::new();

        for frame in 1..=MAX_EXPENSIVE_WINDOWS as u64 {
            let session = broker.begin_additional_session(owner).unwrap();
            let mut request = test_window(owner, session, frame, 0, frame as i32, true);
            request.plane = WindowPlane::Universal(super::super::RGB_OVERLAY_PLANE_SLOT_2 as u8);
            let window = broker
                .create(request, FrameBuffering::Triple)
                .expect("one independent window per application plane");
            sessions.push(session);
            windows.push(window);
        }

        let assigned = windows
            .iter()
            .map(|window| {
                let (slot, _) = unpack_handle(window.raw()).unwrap();
                broker.windows[slot].plane.slot()
            })
            .collect::<Vec<_>>();
        assert_eq!(assigned, Vec::from([2, 3, 0, 1]));

        let waiting_session = broker.begin_additional_session(owner).unwrap();
        let mut waiting = test_window(owner, waiting_session, 5, 0, 5, true);
        waiting.plane = WindowPlane::Universal(super::super::RGB_OVERLAY_PLANE_SLOT_2 as u8);
        assert_eq!(
            broker.create(waiting, FrameBuffering::Triple),
            Err(WindowBrokerError::Capacity)
        );

        broker
            .finish_session(owner, sessions[0], false, false, false, false, false, 0)
            .unwrap();
        for window in windows.iter().skip(1) {
            let (slot, _) = unpack_handle(window.raw()).unwrap();
            assert_ne!(broker.windows[slot].state, WindowState::Closed);
        }

        let admitted = broker
            .create(waiting, FrameBuffering::Triple)
            .expect("released application plane admits the waiting session");
        let (slot, _) = unpack_handle(admitted.raw()).unwrap();
        assert_eq!(broker.windows[slot].plane.slot(), 2);
    }
}
