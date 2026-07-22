//! Window ownership and placement over UI4 frames.
//!
//! The broker contains no rendering, decorations, input policy, guest
//! transport, or Blueprint ABI. A later transport must derive `WindowOwner`
//! from its trusted execution context rather than accepting it from a client.

use alloc::vec::Vec;
use embassy_sync::signal::Signal;
use spin::Mutex;

use super::{DamageRect, DamageRegion, FrameHandle, OutputId};

const MAX_WINDOWS: usize = 256;
// Temporary static30 composition probe: one trusted app session owns all 30
// test windows. Plane assignment is independent of session ownership, while
// the global active-window cap remains the hard system bound.
const MAX_WINDOWS_PER_SESSION: usize = 32;
const MAX_SESSIONS: usize = 64;
pub(super) const MAX_ACTIVE_WINDOWS: usize = 64;

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
    pub(crate) const DRAW3D_SERVICE: Self = Self::KernelApp(3);
    pub(crate) const GRIDPAPER_SERVICE: Self = Self::KernelApp(4);
    pub(crate) const GPGPU_PREVIEW: Self = Self::KernelApp(5);
    pub(crate) const FONT_STAMP: Self = Self::KernelApp(6);
    pub(crate) const SVG_OUTLINE_PROBE: Self = Self::KernelApp(7);
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
/// simple GPU producers such as Draw3D movable without feeding them events
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
    pub(crate) session: WindowSessionId,
    pub(crate) frame: FrameHandle,
    pub(crate) output: OutputId,
    pub(crate) plane: WindowPlane,
    pub(crate) placement: WindowPlacement,
    pub(crate) interaction: WindowInteraction,
    /// The window paints its own pointer at the routed cursor position. UI4
    /// suppresses the default slot-4 cursor only while that window is topmost
    /// below the cursor.
    pub(crate) custom_cursor: bool,
    pub(crate) state: WindowState,
    pub(crate) revision: u64,
    pub(crate) publish_serial: u64,
    pub(crate) damage: Option<DamageRegion>,
    pub(crate) maximized: bool,
}

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
    output: OutputId,
    plane: WindowPlane,
    placement: WindowPlacement,
    interaction: WindowInteraction,
    custom_cursor: bool,
    state: WindowState,
    revision: u64,
    publish_serial: u64,
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

    fn create(&mut self, request: WindowCreate) -> Result<WindowId, WindowBrokerError> {
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
        let active_count = self
            .windows
            .iter()
            .filter(|window| window.state != WindowState::Closed)
            .count();
        if active_count >= MAX_ACTIVE_WINDOWS {
            crate::log_warn!(
                target: "ui4";
                "ui4 window admission soft-cap exceeded requested={} cap={} owner={:?} session={} action=reject-create\n",
                active_count.saturating_add(1),
                MAX_ACTIVE_WINDOWS,
                request.owner,
                request.session.raw(),
            );
            return Err(WindowBrokerError::Capacity);
        }

        if let Some((slot, window)) = self
            .windows
            .iter_mut()
            .enumerate()
            .find(|(_, window)| window.state == WindowState::Closed)
        {
            let generation = next_generation(window.generation);
            *window = WindowRecord::new(generation, request);
            let id = WindowId(pack_handle(slot, generation)?);
            self.mark_composition_changed();
            return Ok(id);
        }
        if self.windows.len() >= MAX_WINDOWS {
            return Err(WindowBrokerError::Capacity);
        }
        let slot = self.windows.len();
        self.windows.push(WindowRecord::new(1, request));
        let id = WindowId(pack_handle(slot, 1)?);
        self.mark_composition_changed();
        Ok(id)
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
    fn new(generation: u16, request: WindowCreate) -> Self {
        Self {
            generation,
            owner: request.owner,
            session: request.session,
            frame: request.frame,
            output: request.output,
            plane: request.plane,
            placement: request.placement,
            interaction: request.interaction,
            custom_cursor: false,
            state: WindowState::Pending,
            revision: 1,
            publish_serial: 0,
            damage: None,
            restore_placement: None,
            close_transition: None,
        }
    }

    fn snapshot(self, slot: usize) -> Option<WindowSnapshot> {
        Some(WindowSnapshot {
            id: WindowId(pack_handle(slot, self.generation).ok()?),
            owner: self.owner,
            session: self.session,
            frame: self.frame,
            output: self.output,
            plane: self.plane,
            placement: self.placement,
            interaction: self.interaction,
            custom_cursor: self.custom_cursor,
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

pub(crate) fn begin_window_session(
    owner: WindowOwner,
) -> Result<WindowSessionId, WindowBrokerError> {
    let (result, retirements) = WINDOW_BROKER.lock().begin_session(owner);
    complete_transition_retirements(retirements);
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
    WINDOW_BROKER
        .lock()
        .finish_session(owner, session, false, false, false, false, false, 0)
        .map(|finish| finish.closed)
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
        match super::destroy_frame(frame) {
            Ok(()) | Err(super::FramePoolError::InvalidHandle) => {}
            Err(super::FramePoolError::Busy) => {
                let mut retired = TRANSITION_RETIRED_FRAMES.lock();
                if !retired.contains(&frame) {
                    retired.push(frame);
                }
            }
            Err(error) => crate::log_warn!(
                target: "ui4";
                "ui4 close-transition frame retire abandoned frame={} error={:?}\n",
                frame.raw(),
                error,
            ),
        }
    }
}

fn reap_transition_retired_frames() {
    TRANSITION_RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(super::destroy_frame(*frame), Err(super::FramePoolError::Busy)));
}

pub(crate) fn create_window(request: WindowCreate) -> Result<WindowId, WindowBrokerError> {
    WINDOW_BROKER.lock().create(request)
}

pub(crate) fn replace_window_frame(
    owner: WindowOwner,
    id: WindowId,
    frame: FrameHandle,
) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    window.frame = frame;
    window.state = WindowState::Pending;
    window.publish_serial = 0;
    window.damage = None;
    window.revision = next_serial(window.revision);
    broker.mark_composition_changed();
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

/// Declare whether a window replaces UI4's default slot-4 cursor with pixels
/// in its own frame. The declaration is window-scoped: the OS cursor returns
/// automatically when the pointer leaves this window or another window is
/// above it.
pub(crate) fn set_window_custom_cursor(
    owner: WindowOwner,
    id: WindowId,
    enabled: bool,
) -> Result<(), WindowBrokerError> {
    let changed = {
        let mut broker = WINDOW_BROKER.lock();
        let window = broker.checked_window_mut(owner, id)?;
        let changed = window.custom_cursor != enabled;
        window.custom_cursor = enabled;
        changed
    };
    if changed {
        super::input_broker::notify_slot4_visual_change();
    }
    Ok(())
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
    let notify_custom_cursor = changed && window.custom_cursor;
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
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
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
    let notify_custom_cursor = window.custom_cursor && window.placement != placement;
    if window.placement != placement {
        window.placement = placement;
        window.revision = next_serial(window.revision);
        broker.mark_composition_changed();
    }
    drop(broker);
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
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
    let notify_custom_cursor = changed && window.custom_cursor;
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
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
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
    let notify_custom_cursor = window.custom_cursor && window.state != WindowState::Ready;
    window.state = WindowState::Ready;
    window.publish_serial = next_serial(window.publish_serial);
    window.revision = next_serial(window.revision);
    let pending = window.damage.get_or_insert(DamageRegion::EMPTY);
    pending.add(damage);
    let publish_serial = window.publish_serial;
    broker.mark_composition_changed();
    drop(broker);
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
    }
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
    let mut notify_custom_cursor = false;
    for (id, damage) in publications.iter().copied() {
        let (slot, _) = unpack_handle(id.0)?;
        let window = &mut broker.windows[slot];
        notify_custom_cursor |= window.custom_cursor && window.state != WindowState::Ready;
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
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
    }
    Ok(())
}

pub(crate) fn close_window(owner: WindowOwner, id: WindowId) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    let notify_custom_cursor = window.custom_cursor && window.state == WindowState::Ready;
    window.state = WindowState::Closed;
    window.damage = None;
    window.revision = next_serial(window.revision);
    broker.mark_composition_changed();
    drop(broker);
    if notify_custom_cursor {
        super::input_broker::notify_slot4_visual_change();
    }
    Ok(())
}

/// Cheap change detector for the compositor's idle path. The subsequent
/// snapshot API returns its epoch under the same broker lock, closing the race
/// between this optimistic check and a producer publication.
pub(crate) fn window_composition_revision() -> u64 {
    WINDOW_BROKER.lock().composition_revision
}

pub(crate) fn window_close_transitions_active() -> bool {
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

/// Clear only the damage represented by a successfully composed snapshot.
/// If the producer published again meanwhile, the serial differs and its new
/// damage remains pending.
pub(crate) fn acknowledge_window_frame(id: WindowId, publish_serial: u64) -> bool {
    WINDOW_BROKER.lock().acknowledge(id, publish_serial)
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
