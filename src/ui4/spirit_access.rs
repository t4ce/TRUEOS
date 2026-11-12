//! LINK bucket authority at UI4's frame boundary. Human input retains the desktop;
//! AI input and Spirit observations require an explicit, revocable frame permit.
//! Remote human input carries its live terminal authority through queued events.
use super::{CursorFrameKey, Ui4CursorSource, WindowSnapshot, WindowState};
use crate::r::keyboard::TrueosKeyboardOutputEvent;
use crate::shell3::tui::RemoteInputAuthority;
use crate::usb2::hid::hut::{self, HidSourceKind};
use heapless::Vec;
use spin::Mutex;

const LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Permit {
    frame: CursorFrameKey,
    session: u32,
    serial: u64,
    keyboard_frontiers: (u32, u32),
    cursor_frontier: u64,
}

#[derive(Clone, Copy)]
struct Binding {
    cursor: Ui4CursorSource,
    keyboard_slot: u32,
    permit: Permit,
    vm: u8,
    run: u64,
    keyboard_frontiers: (u32, u32),
    cursor_frontier: u64,
}

struct Registry {
    grants: Vec<Permit, LIMIT>,
    bindings: Vec<Binding, LIMIT>,
    serial: u64,
}

impl Registry {
    const fn new() -> Self {
        Self {
            grants: Vec::new(),
            bindings: Vec::new(),
            serial: 0,
        }
    }

    fn toggle(&mut self, frame: CursorFrameKey, session: u32) -> Result<bool, ()> {
        if let Some(index) = self.grants.iter().position(|grant| grant.frame == frame) {
            self.grants.swap_remove(index);
            return Ok(false);
        }
        self.serial = self.serial.checked_add(1).ok_or(())?;
        let keyboard_frontiers = crate::r::keyboard::output_event_frontiers();
        let cursor_frontier = crate::usb2::hid::cursor_event_frontier();
        self.grants
            .push(Permit {
                frame,
                session,
                serial: self.serial,
                keyboard_frontiers,
                cursor_frontier,
            })
            .map_err(|_| ())?;
        Ok(true)
    }

    fn contains(&self, permit: Permit) -> bool {
        self.grants.contains(&permit)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bucket {
    Human,
    Remote(RemoteDevice),
    Ai,
    Restricted,
}

/// Keep station and combo provenance alongside the session, so moving or
/// releasing a device also invalidates input already queued for that source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteDevice {
    authority: RemoteInputAuthority,
    vm: u8,
    run: u64,
    kind: hut::HidDeviceKind,
    slot: u32,
    combo: u32,
}

fn remote_device(vm: u8, kind: hut::HidDeviceKind, slot: u32) -> Option<RemoteDevice> {
    if !vm_live(vm, None) {
        return None;
    }
    let run = crate::hv::vm_run_generation(vm)?;
    let authority = crate::shell3::tui::remote_input_authority(vm, run)?;
    let device = hut::devices_snapshot().into_iter().find(|device| {
        device.id.kind == kind
            && device.id.controller_id == 0
            && device.id.slot_id == slot
            && device.id.ep_target == 0
            && device.virtual_device
            && device.source_kind == HidSourceKind::Remote
            && device.combo_id != 0
    })?;
    if !crate::r::io::input_cabi::remote_input_combo_owned(vm, run, device.combo_id) {
        return None;
    }
    Some(RemoteDevice {
        authority,
        vm,
        run,
        kind,
        slot,
        combo: device.combo_id,
    })
}

fn guest_bucket(vm: u8, kind: hut::HidDeviceKind, slot: u32) -> Bucket {
    if !vm_live(vm, None) {
        return Bucket::Restricted;
    }
    let remote = hut::devices_snapshot().into_iter().any(|device| {
        device.id.kind == kind
            && device.id.controller_id == 0
            && device.id.slot_id == slot
            && device.id.ep_target == 0
            && device.source_kind == HidSourceKind::Remote
    });
    if remote {
        remote_device(vm, kind, slot).map_or(Bucket::Restricted, Bucket::Remote)
    } else {
        Bucket::Ai
    }
}

fn remote_device_valid(device: RemoteDevice) -> bool {
    use crate::r::services::{keyboard_control_service as keyboard, mouse_motion_service as mouse};
    let owned = match device.kind {
        hut::HidDeviceKind::Mouse => matches!(mouse::cursor_principal(device.slot),
            Some(mouse::MouseControlPrincipal::Vm(vm)) if vm == device.vm),
        hut::HidDeviceKind::Keyboard => matches!(keyboard::keyboard_principal(device.slot),
            Some(keyboard::KeyboardControlPrincipal::Vm(vm)) if vm == device.vm),
        _ => false,
    };
    owned
        && crate::shell3::tui::remote_input_authority_valid(device.authority)
        && remote_device(device.vm, device.kind, device.slot) == Some(device)
}

fn remote_target_allowed(device: RemoteDevice, frame: CursorFrameKey) -> bool {
    remote_device_valid(device)
        && match frame.owner {
            super::WindowOwner::Vm(vm) => {
                crate::access_key_db::authorize_vm_control(vm, Some(device.authority.account))
                    .is_ok()
            }
            _ => true,
        }
}

fn bucket(kind: HidSourceKind, virtual_device: bool, owned_virtual: bool) -> Bucket {
    // Trusted station provenance takes precedence over guest-selected labels.
    if owned_virtual {
        return Bucket::Ai;
    }
    match (kind, virtual_device) {
        (HidSourceKind::Human, false) => Bucket::Human,
        (HidSourceKind::Ai, _) => Bucket::Ai,
        _ => Bucket::Restricted,
    }
}

fn cursor_bucket(source: Ui4CursorSource) -> Bucket {
    use crate::r::services::mouse_motion_service::{self as mouse, MouseControlPrincipal};
    let principal = (source.controller_id == 0 && source.ep_target == 0)
        .then(|| mouse::cursor_principal(source.slot_id))
        .flatten();
    if let Some(MouseControlPrincipal::Vm(vm)) = principal {
        return guest_bucket(vm, hut::HidDeviceKind::Mouse, source.slot_id);
    }
    let kind = if source.hid_kind == crate::r::cursor::HID_KIND_TABLET {
        hut::HidDeviceKind::Tablet
    } else {
        hut::HidDeviceKind::Mouse
    };
    hut::devices_snapshot()
        .into_iter()
        .find(|device| {
            device.id.kind == kind
                && device.id.controller_id == source.controller_id
                && device.id.slot_id == source.slot_id
                && device.id.ep_target == source.ep_target
        })
        .map_or(Bucket::Restricted, |device| {
            // A retired virtual station cannot regain access through stale HUT data.
            if device.virtual_device && principal.is_none() {
                Bucket::Restricted
            } else {
                bucket(device.source_kind, device.virtual_device, false)
            }
        })
}

fn keyboard_bucket(event: &TrueosKeyboardOutputEvent) -> Bucket {
    use crate::r::services::keyboard_control_service::{
        self as keyboard, KeyboardControlPrincipal,
    };
    let principal = (event.controller_id == 0 && event.ep_target == 0)
        .then(|| keyboard::keyboard_principal(event.slot_id))
        .flatten();
    if let Some(KeyboardControlPrincipal::Vm(vm)) = principal {
        return guest_bucket(vm, hut::HidDeviceKind::Keyboard, event.slot_id);
    }
    hut::devices_snapshot()
        .into_iter()
        .find(|device| {
            device.id.kind == hut::HidDeviceKind::Keyboard
                && device.id.controller_id == event.controller_id
                && device.id.slot_id == event.slot_id
                && device.id.ep_target == event.ep_target
        })
        .map_or(Bucket::Restricted, |device| {
            if device.virtual_device && principal.is_none() {
                Bucket::Restricted
            } else {
                bucket(device.source_kind, device.virtual_device, false)
            }
        })
}

pub(crate) fn cursor_is_human(source: Ui4CursorSource) -> bool {
    matches!(cursor_bucket(source), Bucket::Human | Bucket::Remote(_))
}

/// Broker gestures must not survive a terminal session or lease change even
/// when the same account later resumes control of the same virtual cursor.
pub(crate) fn cursor_remote_authority(source: Ui4CursorSource) -> Option<RemoteInputAuthority> {
    match cursor_bucket(source) {
        Bucket::Remote(device) => Some(device.authority),
        _ => None,
    }
}

pub(crate) fn keyboard_is_human(event: &TrueosKeyboardOutputEvent) -> bool {
    matches!(keyboard_bucket(event), Bucket::Human | Bucket::Remote(_))
}

/// Raw reports can wait in a ring across terminal detach/reentry. An old
/// report must not acquire the newly established human session's authority.
pub(crate) fn cursor_event_allowed(source: Ui4CursorSource, sequence: u64) -> bool {
    match cursor_bucket(source) {
        Bucket::Remote(device) => sequence > device.authority.cursor_frontier,
        Bucket::Restricted => false,
        _ => true,
    }
}

pub(crate) fn keyboard_event_allowed(event: &TrueosKeyboardOutputEvent) -> bool {
    match keyboard_bucket(event) {
        Bucket::Remote(device) => {
            keyboard_sequence_allowed(event, device.authority.keyboard_frontiers)
        }
        Bucket::Restricted => false,
        _ => true,
    }
}

fn live(frame: CursorFrameKey) -> Option<WindowSnapshot> {
    super::window_broker::window_snapshot(frame.owner, frame.window)
        .filter(|window| window.state == WindowState::Ready)
}

pub(crate) fn authorize(frame: CursorFrameKey) -> Option<Permit> {
    let window = live(frame)?;
    REGISTRY
        .lock()
        .grants
        .iter()
        .copied()
        .find(|grant| grant.frame == frame && grant.session == window.session.raw())
}

pub(crate) fn valid(permit: Permit) -> bool {
    live(permit.frame).is_some_and(|window| window.session.raw() == permit.session)
        && REGISTRY.lock().contains(permit)
}

pub(crate) fn is_granted(frame: CursorFrameKey) -> bool {
    authorize(frame).is_some()
}

/// Dock sharing cannot take over an ungranted UI4 frame: the input broker
/// lets AI select or move only frames already admitted to AI, so it cannot drag
/// another frame into Spirit's share circle to acquire access. Grant changes
/// additionally require the trusted Human bucket; guest-selected device labels
/// do not confer that authority. This is the logged-in user's bucket today.
/// We plan to reuse owner-controlled sharing between multiple users later;
/// per-user grants are not implemented yet.
pub(crate) fn toggle_from_human(
    source: Ui4CursorSource,
    frame: CursorFrameKey,
) -> Result<bool, ()> {
    if !cursor_is_human(source) || !cursor_target_allowed(source, frame) {
        return Err(());
    }
    let window = live(frame).ok_or(())?;
    // Retired sessions/frames can close through broker reclamation as well as
    // close_window. Collect outside the registry lock to keep lock order flat.
    let previous = REGISTRY.lock().grants.clone();
    let stale: Vec<Permit, LIMIT> = previous
        .into_iter()
        .filter(|permit| {
            !live(permit.frame).is_some_and(|window| window.session.raw() == permit.session)
        })
        .collect();
    let mut registry = REGISTRY.lock();
    registry.grants.retain(|permit| !stale.contains(permit));
    let result = registry.toggle(frame, window.session.raw());
    drop(registry);
    super::input_broker::notify_slot4_visual_change();
    result
}

pub(crate) fn frame_closed(frame: CursorFrameKey) {
    REGISTRY.lock().grants.retain(|grant| grant.frame != frame);
}

/// Bind the shared Spirit devices before queuing input. This retained permit
/// makes late strokes fail after revocation/regrant or a focus change.
pub(crate) fn bind_input(
    cursor: Ui4CursorSource,
    keyboard_slot: u32,
    permit: Permit,
    vm: u8,
    run: u64,
) -> bool {
    if !valid(permit) {
        return false;
    }
    let keyboard_frontiers = crate::r::keyboard::output_event_frontiers();
    let cursor_frontier = crate::usb2::hid::cursor_event_frontier();
    let mut registry = REGISTRY.lock();
    if !registry.contains(permit) {
        return false;
    }
    registry
        .bindings
        .retain(|binding| binding.cursor != cursor && binding.keyboard_slot != keyboard_slot);
    registry
        .bindings
        .push(Binding {
            cursor,
            keyboard_slot,
            permit,
            vm,
            run,
            keyboard_frontiers,
            cursor_frontier,
        })
        .is_ok()
}

/// A queue keeps the exact grant which admitted its event. Rebinding devices
/// after revoke/regrant cannot lend a newer grant to an older queued action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputStamp {
    Human,
    Remote {
        device: RemoteDevice,
        frame: CursorFrameKey,
        session: u32,
    },
    Ai {
        permit: Permit,
        owner: Option<(u8, u64)>,
    },
}

fn vm_live(vm: u8, run: Option<u64>) -> bool {
    let state = crate::hv::vm_state(vm);
    state.running
        && !state.stop_requested
        && crate::hv::vm_run_generation(vm)
            .is_some_and(|current| current != 0 && run.is_none_or(|run| current == run))
}

pub(crate) fn stamp_valid(stamp: InputStamp, frame: CursorFrameKey) -> bool {
    match stamp {
        InputStamp::Human => true,
        InputStamp::Remote {
            device,
            frame: admitted,
            session,
        } => {
            admitted == frame
                && live(frame).is_some_and(|window| window.session.raw() == session)
                && remote_target_allowed(device, frame)
        }
        InputStamp::Ai { permit, owner } => {
            permit.frame == frame
                && valid(permit)
                && owner.is_none_or(|(vm, run)| vm_live(vm, Some(run)))
        }
    }
}

fn ai_stamp(
    binding: Option<Binding>,
    frame: CursorFrameKey,
    owner: Option<(u8, u64)>,
) -> Option<InputStamp> {
    let stamp = match binding {
        Some(binding) => InputStamp::Ai {
            permit: binding.permit,
            owner: Some((binding.vm, binding.run)),
        },
        None => InputStamp::Ai {
            permit: authorize(frame)?,
            owner,
        },
    };
    stamp_valid(stamp, frame).then_some(stamp)
}

pub(crate) fn stamp_cursor(source: Ui4CursorSource, frame: CursorFrameKey) -> Option<InputStamp> {
    match cursor_bucket(source) {
        Bucket::Human => Some(InputStamp::Human),
        Bucket::Remote(device) => remote_stamp(device, frame),
        Bucket::Restricted => None,
        Bucket::Ai => {
            let binding = REGISTRY
                .lock()
                .bindings
                .iter()
                .find(|binding| binding.cursor == source)
                .copied();
            let owner = if source.controller_id == 0 && source.ep_target == 0 {
                match crate::r::services::mouse_motion_service::cursor_principal(source.slot_id) {
                    Some(crate::r::services::mouse_motion_service::MouseControlPrincipal::Vm(
                        vm,
                    )) => crate::hv::vm_run_generation(vm).map(|run| (vm, run)),
                    _ => None,
                }
            } else {
                None
            };
            ai_stamp(binding, frame, owner)
        }
    }
}

fn remote_stamp(device: RemoteDevice, frame: CursorFrameKey) -> Option<InputStamp> {
    let session = live(frame)?.session.raw();
    let stamp = InputStamp::Remote {
        device,
        frame,
        session,
    };
    stamp_valid(stamp, frame).then_some(stamp)
}

fn sequence_after(sequence: u32, frontier: u32) -> bool {
    (sequence.wrapping_sub(frontier) as i32) > 0
}

/// The raw cursor ring precedes broker event stamping. Bindings record its
/// frontier so a new focus cannot consume reports emitted by the old command.
pub(crate) fn cursor_sequence_allowed(
    source: Ui4CursorSource,
    frame: CursorFrameKey,
    sequence: u64,
) -> bool {
    let permit = match stamp_cursor(source, frame) {
        Some(InputStamp::Human) => return true,
        Some(InputStamp::Remote { device, .. }) => {
            return sequence > device.authority.cursor_frontier;
        }
        Some(InputStamp::Ai { permit, .. }) => permit,
        None => return false,
    };
    sequence > permit.cursor_frontier
        && REGISTRY
            .lock()
            .bindings
            .iter()
            .find(|binding| binding.cursor == source)
            .is_none_or(|binding| sequence > binding.cursor_frontier)
}

fn keyboard_sequence_allowed(event: &TrueosKeyboardOutputEvent, frontiers: (u32, u32)) -> bool {
    let frontier = if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_PHYSICAL {
        frontiers.1
    } else {
        frontiers.0
    };
    sequence_after(event.seq, frontier)
}

fn stamp_keyboard_source(
    event: &TrueosKeyboardOutputEvent,
    frame: CursorFrameKey,
) -> Option<InputStamp> {
    match keyboard_bucket(event) {
        Bucket::Human => Some(InputStamp::Human),
        Bucket::Remote(device) => remote_stamp(device, frame),
        Bucket::Restricted => None,
        Bucket::Ai => {
            let binding = REGISTRY
                .lock()
                .bindings
                .iter()
                .find(|binding| {
                    event.controller_id == 0
                        && event.ep_target == 0
                        && binding.keyboard_slot == event.slot_id
                })
                .copied();
            let owner = if event.controller_id == 0 && event.ep_target == 0 {
                match crate::r::services::keyboard_control_service::keyboard_principal(
                    event.slot_id,
                ) {
                    Some(
                        crate::r::services::keyboard_control_service::KeyboardControlPrincipal::Vm(
                            vm,
                        ),
                    ) => crate::hv::vm_run_generation(vm).map(|run| (vm, run)),
                    _ => None,
                }
            } else {
                None
            };
            ai_stamp(binding, frame, owner)
        }
    }
}

pub(crate) fn stamp_keyboard(
    event: &TrueosKeyboardOutputEvent,
    frame: CursorFrameKey,
) -> Option<InputStamp> {
    let stamp = stamp_keyboard_source(event, frame)?;
    if let InputStamp::Remote { device, .. } = stamp {
        if !keyboard_sequence_allowed(event, device.authority.keyboard_frontiers) {
            return None;
        }
    }
    if let InputStamp::Ai { permit, .. } = stamp {
        if !keyboard_sequence_allowed(event, permit.keyboard_frontiers) {
            return None;
        }
        let binding = REGISTRY
            .lock()
            .bindings
            .iter()
            .find(|binding| {
                event.controller_id == 0
                    && event.ep_target == 0
                    && binding.keyboard_slot == event.slot_id
            })
            .copied();
        if binding
            .is_some_and(|binding| !keyboard_sequence_allowed(event, binding.keyboard_frontiers))
        {
            return None;
        }
    }
    Some(stamp)
}

pub(crate) fn keyboard_state_target_allowed(
    state: &hut::HidKeyboardState,
    frame: CursorFrameKey,
) -> bool {
    let event = TrueosKeyboardOutputEvent {
        controller_id: state.controller_id,
        slot_id: state.slot_id,
        ep_target: state.ep_target,
        ..TrueosKeyboardOutputEvent::default()
    };
    stamp_keyboard_source(&event, frame).is_some()
}

pub(crate) fn cursor_target_allowed(source: Ui4CursorSource, frame: CursorFrameKey) -> bool {
    stamp_cursor(source, frame).is_some()
}

pub(crate) fn keyboard_target_allowed(
    event: &TrueosKeyboardOutputEvent,
    frame: CursorFrameKey,
) -> bool {
    stamp_keyboard(event, frame).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provenance_overrides_human_labels_and_remote_is_not_local_human() {
        assert_eq!(bucket(HidSourceKind::Human, false, false), Bucket::Human);
        assert_eq!(bucket(HidSourceKind::Human, true, true), Bucket::Ai);
        assert_eq!(
            bucket(HidSourceKind::Human, true, false),
            Bucket::Restricted
        );
        assert_eq!(bucket(HidSourceKind::Ai, false, false), Bucket::Ai);
        assert_eq!(
            bucket(HidSourceKind::Remote, false, false),
            Bucket::Restricted
        );
        assert_eq!(
            bucket(HidSourceKind::Unknown, false, false),
            Bucket::Restricted
        );
    }
    #[test]
    fn revocation_regrant_and_window_session_reuse_never_revive_old_permits() {
        let mut registry = Registry::new();
        let frame = CursorFrameKey::new(
            super::super::WindowOwner::Vm(1),
            super::super::WindowId::from_raw(7).unwrap(),
        );
        assert_eq!(registry.toggle(frame, 3), Ok(true));
        let old = registry.grants[0];
        assert_eq!(registry.toggle(frame, 3), Ok(false));
        assert!(!registry.contains(old));
        assert_eq!(registry.toggle(frame, 3), Ok(true));
        assert!(!registry.contains(old));
        assert_ne!(registry.grants[0].serial, old.serial);
        registry.grants.clear();
        registry.toggle(frame, 4).unwrap();
        assert_ne!(registry.grants[0].session, old.session);
    }
}
