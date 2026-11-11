//! LINK bucket authority at UI4's frame boundary. Human input retains the desktop;
//! AI input and Spirit observations require an explicit, revocable frame permit.
//! Remote and unknown sources are not implicitly the local human bucket.
use super::{CursorFrameKey, Ui4CursorSource, WindowSnapshot, WindowState};
use crate::r::keyboard::TrueosKeyboardOutputEvent;
use crate::usb2::hid::hut::{self, HidSourceKind};
use heapless::Vec;
use spin::Mutex;

const LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Permit {
    frame: CursorFrameKey,
    session: u32,
    serial: u64,
}

#[derive(Clone, Copy)]
struct Binding {
    cursor: Ui4CursorSource,
    keyboard_slot: u32,
    permit: Permit,
    vm: u8,
    run: u64,
}

struct Registry {
    grants: Vec<Permit, LIMIT>,
    bindings: Vec<Binding, LIMIT>,
    serial: u64,
}

impl Registry {
    const fn new() -> Self {
        Self { grants: Vec::new(), bindings: Vec::new(), serial: 0 }
    }

    fn toggle(&mut self, frame: CursorFrameKey, session: u32) -> Result<bool, ()> {
        if let Some(index) = self.grants.iter().position(|grant| grant.frame == frame) {
            self.grants.swap_remove(index);
            return Ok(false);
        }
        self.serial = self.serial.checked_add(1).ok_or(())?;
        self.grants.push(Permit { frame, session, serial: self.serial }).map_err(|_| ())?;
        Ok(true)
    }

    fn contains(&self, permit: Permit) -> bool {
        self.grants.contains(&permit)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bucket { Human, Ai, Restricted }

fn bucket(kind: HidSourceKind, virtual_device: bool, owned_virtual: bool) -> Bucket {
    // Trusted station provenance takes precedence over guest-selected labels.
    if owned_virtual { return Bucket::Ai; }
    match (kind, virtual_device) {
        (HidSourceKind::Human, false) => Bucket::Human,
        (HidSourceKind::Ai, _) => Bucket::Ai,
        _ => Bucket::Restricted,
    }
}

fn cursor_bucket(source: Ui4CursorSource) -> Bucket {
    use crate::r::services::mouse_motion_service::{self as mouse, MouseControlPrincipal};
    let principal = (source.controller_id == 0 && source.ep_target == 0)
        .then(|| mouse::cursor_principal(source.slot_id)).flatten();
    if matches!(principal, Some(MouseControlPrincipal::Vm(_))) { return Bucket::Ai; }
    let kind = if source.hid_kind == crate::r::cursor::HID_KIND_TABLET {
        hut::HidDeviceKind::Tablet
    } else { hut::HidDeviceKind::Mouse };
    hut::devices_snapshot().into_iter().find(|device| {
        device.id.kind == kind && device.id.controller_id == source.controller_id
            && device.id.slot_id == source.slot_id && device.id.ep_target == source.ep_target
    }).map_or(Bucket::Restricted, |device| {
        // A retired virtual station cannot regain access through stale HUT data.
        if device.virtual_device && principal.is_none() { Bucket::Restricted }
        else { bucket(device.source_kind, device.virtual_device, false) }
    })
}

fn keyboard_bucket(event: &TrueosKeyboardOutputEvent) -> Bucket {
    use crate::r::services::keyboard_control_service::{self as keyboard, KeyboardControlPrincipal};
    let principal = (event.controller_id == 0 && event.ep_target == 0)
        .then(|| keyboard::keyboard_principal(event.slot_id)).flatten();
    if matches!(principal, Some(KeyboardControlPrincipal::Vm(_))) { return Bucket::Ai; }
    hut::devices_snapshot().into_iter().find(|device| {
        device.id.kind == hut::HidDeviceKind::Keyboard
            && device.id.controller_id == event.controller_id
            && device.id.slot_id == event.slot_id && device.id.ep_target == event.ep_target
    }).map_or(Bucket::Restricted, |device| {
        if device.virtual_device && principal.is_none() { Bucket::Restricted }
        else { bucket(device.source_kind, device.virtual_device, false) }
    })
}

pub(crate) fn cursor_is_human(source: Ui4CursorSource) -> bool {
    cursor_bucket(source) == Bucket::Human
}

pub(crate) fn keyboard_is_human(event: &TrueosKeyboardOutputEvent) -> bool {
    keyboard_bucket(event) == Bucket::Human
}

fn live(frame: CursorFrameKey) -> Option<WindowSnapshot> {
    super::window_broker::window_snapshot(frame.owner, frame.window)
        .filter(|window| window.state == WindowState::Ready)
}

pub(crate) fn authorize(frame: CursorFrameKey) -> Option<Permit> {
    let window = live(frame)?;
    REGISTRY.lock().grants.iter().copied()
        .find(|grant| grant.frame == frame && grant.session == window.session.raw())
}

pub(crate) fn valid(permit: Permit) -> bool {
    live(permit.frame).is_some_and(|window| window.session.raw() == permit.session)
        && REGISTRY.lock().contains(permit)
}

pub(crate) fn is_granted(frame: CursorFrameKey) -> bool { authorize(frame).is_some() }

/// No guest setter exists. Only the local Human LINK bucket can admit frames.
pub(crate) fn toggle_from_human(source: Ui4CursorSource, frame: CursorFrameKey) -> Result<bool, ()> {
    if !cursor_is_human(source) { return Err(()); }
    let window = live(frame).ok_or(())?;
    let result = REGISTRY.lock().toggle(frame, window.session.raw());
    super::input_broker::notify_slot4_visual_change();
    result
}

pub(crate) fn frame_closed(frame: CursorFrameKey) {
    REGISTRY.lock().grants.retain(|grant| grant.frame != frame);
}

/// Bind the shared Spirit devices before queuing input. This retained permit
/// makes late strokes fail after revocation/regrant or a focus change.
pub(crate) fn bind_input(cursor: Ui4CursorSource, keyboard_slot: u32, permit: Permit, vm: u8, run: u64) -> bool {
    if !valid(permit) { return false; }
    let mut registry = REGISTRY.lock();
    if !registry.contains(permit) { return false; }
    registry.bindings.retain(|binding| binding.cursor != cursor && binding.keyboard_slot != keyboard_slot);
    registry.bindings.push(Binding { cursor, keyboard_slot, permit, vm, run }).is_ok()
}

fn binding_allows(binding: Binding, frame: CursorFrameKey) -> bool {
    let state = crate::hv::vm_state(binding.vm);
    binding.permit.frame == frame && state.running && !state.stop_requested
        && crate::hv::vm_run_generation(binding.vm) == Some(binding.run) && valid(binding.permit)
}

pub(crate) fn cursor_target_allowed(source: Ui4CursorSource, frame: CursorFrameKey) -> bool {
    match cursor_bucket(source) {
        Bucket::Human => true,
        Bucket::Restricted => false,
        Bucket::Ai => {
            let binding = REGISTRY.lock().bindings.iter().find(|binding| binding.cursor == source).copied();
            binding.map_or_else(|| is_granted(frame), |binding| binding_allows(binding, frame))
        }
    }
}

pub(crate) fn keyboard_target_allowed(event: &TrueosKeyboardOutputEvent, frame: CursorFrameKey) -> bool {
    match keyboard_bucket(event) {
        Bucket::Human => true,
        Bucket::Restricted => false,
        Bucket::Ai => {
            let binding = REGISTRY.lock().bindings.iter().find(|binding| {
                event.controller_id == 0 && event.ep_target == 0 && binding.keyboard_slot == event.slot_id
            }).copied();
            binding.map_or_else(|| is_granted(frame), |binding| binding_allows(binding, frame))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provenance_overrides_human_labels_and_remote_is_not_local_human() {
        assert_eq!(bucket(HidSourceKind::Human, false, false), Bucket::Human);
        assert_eq!(bucket(HidSourceKind::Human, true, true), Bucket::Ai);
        assert_eq!(bucket(HidSourceKind::Human, true, false), Bucket::Restricted);
        assert_eq!(bucket(HidSourceKind::Ai, false, false), Bucket::Ai);
        assert_eq!(bucket(HidSourceKind::Remote, false, false), Bucket::Restricted);
        assert_eq!(bucket(HidSourceKind::Unknown, false, false), Bucket::Restricted);
    }
    #[test]
    fn revocation_regrant_and_window_session_reuse_never_revive_old_permits() {
        let mut registry = Registry::new();
        let frame = CursorFrameKey::new(super::super::WindowOwner::Vm(1), super::super::WindowId::from_raw(7).unwrap());
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
