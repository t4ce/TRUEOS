//! Window ownership and placement over UI4 frames.
//!
//! The broker contains no rendering, decorations, input policy, guest
//! transport, or Blueprint ABI. A later transport must derive `WindowOwner`
//! from its trusted execution context rather than accepting it from a client.

use alloc::vec::Vec;
use spin::Mutex;

use super::{FrameHandle, OutputId};

const MAX_WINDOWS: usize = 256;
const MAX_WINDOWS_PER_SESSION: usize = 16;
const MAX_SESSIONS: usize = 64;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowOwner {
    Kernel,
    /// Temporary trusted-app identity used before Blueprint transport exists.
    KernelApp(u8),
    Vm(u8),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct WindowId(u32);

impl WindowId {
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        if raw == 0 {
            None
        } else {
            Some(Self(raw))
        }
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
        if raw == 0 {
            None
        } else {
            Some(Self(raw))
        }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowState {
    Pending,
    Ready,
    Closed,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DamageRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DamageRect {
    pub(crate) const FULL: Self = Self {
        x: 0,
        y: 0,
        width: u32::MAX,
        height: u32::MAX,
    };

    const fn valid(self) -> bool {
        self.width != 0 && self.height != 0
    }

    fn union(self, other: Self) -> Self {
        if self == Self::FULL || other == Self::FULL {
            return Self::FULL;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .max(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .max(other.y.saturating_add(other.height));
        Self {
            x,
            y,
            width: right.saturating_sub(x),
            height: bottom.saturating_sub(y),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowCreate {
    pub(crate) owner: WindowOwner,
    pub(crate) session: WindowSessionId,
    pub(crate) frame: FrameHandle,
    pub(crate) output: OutputId,
    pub(crate) placement: WindowPlacement,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowSnapshot {
    pub(crate) id: WindowId,
    pub(crate) owner: WindowOwner,
    pub(crate) session: WindowSessionId,
    pub(crate) frame: FrameHandle,
    pub(crate) output: OutputId,
    pub(crate) placement: WindowPlacement,
    pub(crate) state: WindowState,
    pub(crate) revision: u64,
    pub(crate) publish_serial: u64,
    pub(crate) damage: Option<DamageRect>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowBrokerError {
    InvalidHandle,
    OwnerMismatch,
    SessionClosed,
    EmptyExtent,
    EmptyDamage,
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
    placement: WindowPlacement,
    state: WindowState,
    revision: u64,
    publish_serial: u64,
    damage: Option<DamageRect>,
}

#[derive(Copy, Clone)]
struct SessionRecord {
    generation: u16,
    owner: WindowOwner,
    active: bool,
}

struct WindowBroker {
    windows: Vec<WindowRecord>,
    sessions: Vec<SessionRecord>,
}

impl WindowBroker {
    const fn new() -> Self {
        Self {
            windows: Vec::new(),
            sessions: Vec::new(),
        }
    }

    fn begin_session(&mut self, owner: WindowOwner) -> Result<WindowSessionId, WindowBrokerError> {
        for session in &mut self.sessions {
            if session.active && session.owner == owner {
                session.active = false;
            }
        }
        for window in &mut self.windows {
            if window.state != WindowState::Closed && window.owner == owner {
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
            return Ok(WindowSessionId(pack_handle(slot, session.generation)?));
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
        Ok(WindowSessionId(pack_handle(slot, 1)?))
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

        if let Some((slot, window)) = self
            .windows
            .iter_mut()
            .enumerate()
            .find(|(_, window)| window.state == WindowState::Closed)
        {
            let generation = next_generation(window.generation);
            *window = WindowRecord::new(generation, request);
            return Ok(WindowId(pack_handle(slot, generation)?));
        }
        if self.windows.len() >= MAX_WINDOWS {
            return Err(WindowBrokerError::Capacity);
        }
        let slot = self.windows.len();
        self.windows.push(WindowRecord::new(1, request));
        Ok(WindowId(pack_handle(slot, 1)?))
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
        if window.state == WindowState::Closed {
            return Err(WindowBrokerError::Closed);
        }
        Ok(window)
    }

    fn finish_session(
        &mut self,
        owner: WindowOwner,
        id: WindowSessionId,
    ) -> Result<usize, WindowBrokerError> {
        self.checked_session(owner, id)?;
        let (slot, _) = unpack_handle(id.0)?;
        self.sessions[slot].active = false;
        let mut closed = 0;
        for window in &mut self.windows {
            if window.state != WindowState::Closed && window.session == id {
                window.state = WindowState::Closed;
                window.damage = None;
                window.revision = next_serial(window.revision);
                closed += 1;
            }
        }
        Ok(closed)
    }

    fn snapshots(&self, output: OutputId) -> Vec<WindowSnapshot> {
        let mut snapshots = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| {
                window.state == WindowState::Ready
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
            || window.state != WindowState::Ready
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
            placement: request.placement,
            state: WindowState::Pending,
            revision: 1,
            publish_serial: 0,
            damage: None,
        }
    }

    fn snapshot(self, slot: usize) -> Option<WindowSnapshot> {
        Some(WindowSnapshot {
            id: WindowId(pack_handle(slot, self.generation).ok()?),
            owner: self.owner,
            session: self.session,
            frame: self.frame,
            output: self.output,
            placement: self.placement,
            state: self.state,
            revision: self.revision,
            publish_serial: self.publish_serial,
            damage: self.damage,
        })
    }
}

static WINDOW_BROKER: Mutex<WindowBroker> = Mutex::new(WindowBroker::new());

pub(crate) fn begin_window_session(
    owner: WindowOwner,
) -> Result<WindowSessionId, WindowBrokerError> {
    WINDOW_BROKER.lock().begin_session(owner)
}

pub(crate) fn finish_window_session(
    owner: WindowOwner,
    session: WindowSessionId,
) -> Result<usize, WindowBrokerError> {
    WINDOW_BROKER.lock().finish_session(owner, session)
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
    if window.placement != placement {
        window.placement = placement;
        window.revision = next_serial(window.revision);
    }
    drop(broker);
    if previous.width != placement.width || previous.height != placement.height {
        super::input_broker::enqueue_window_resize(
            owner,
            id,
            previous.width,
            previous.height,
            placement.width,
            placement.height,
        );
    }
    Ok(())
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
    window.damage = Some(match window.damage {
        Some(previous) => previous.union(damage),
        None => damage,
    });
    Ok(window.publish_serial)
}

pub(crate) fn close_window(owner: WindowOwner, id: WindowId) -> Result<(), WindowBrokerError> {
    let mut broker = WINDOW_BROKER.lock();
    let window = broker.checked_window_mut(owner, id)?;
    window.state = WindowState::Closed;
    window.damage = None;
    window.revision = next_serial(window.revision);
    Ok(())
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
