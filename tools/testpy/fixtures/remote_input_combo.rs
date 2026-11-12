#![allow(dead_code)]
extern crate alloc;
use std::cell::RefCell;
#[path = "@POLICY@"] mod input_combo_policy;
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
    }
}
static INPUT_COMBO_OWNERS: spin::Mutex<input_combo_policy::Ownership> = spin::Mutex::new(input_combo_policy::Ownership::new());
#[derive(Clone, Copy, PartialEq)] enum MouseControlPrincipal { Vm(u8) }
#[derive(Clone, Copy, PartialEq)] enum KeyboardControlPrincipal { Vm(u8) }
#[derive(Clone, Copy, PartialEq)] enum GamepadControlPrincipal { Vm(u8) }
#[derive(Default)] struct World {
    vm: Option<u8>, run: Option<u64>, hull: bool, authority: bool,
    combos: Vec<usb2::hid::hut::InputCombo>,
    devices: Vec<(u8, u32, u8)>, next_id: u32,
    calls: Vec<(u32, u64, Vec<u8>)>,
    transport_failure: bool,
    corrupt_response: bool,
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<T>(f: impl FnOnce(&mut World) -> T) -> T { WORLD.with(|state| f(&mut state.borrow_mut())) }
mod hv {
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { super::world(|w| if w.hull { None } else { w.vm }) }
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { super::world(|w| if w.hull { w.vm } else { None }) }
    pub fn vm_run_generation(_: u8) -> Option<u64> { super::world(|w| w.run) }
    pub fn with_guest_broker_context<T>(vm: u8, f: impl FnOnce() -> T) -> T {
        let (old_vm, old_hull) = super::world(|w| {
            let old = (w.vm, w.hull); w.vm = Some(vm); w.hull = false; old
        });
        let result = f();
        super::world(|w| { w.vm = old_vm; w.hull = old_hull; });
        result
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_INPUT_COMBO_CONTROL_V1: u32 = 0x258;
    pub const PAYLOAD_CAP: usize = 4096;
    pub const STATUS_OK: u32 = 0;
    pub fn call_with_payload(op: u32, arg0: u64, arg1: u64, request: &[u8], response: &mut [u8]) -> (u32, u64) {
        assert_eq!(op, OP_BP_INPUT_COMBO_CONTROL_V1);
        let vm = crate::world(|w| {
            assert!(w.hull, "only a Hull may use the guest transport");
            w.calls.push((arg0 as u32, arg1, request.to_vec()));
            w.vm.unwrap()
        });
        if crate::world(|w| w.transport_failure) { return (1, (-9i32) as u32 as u64); }
        let (rc, wrote) = crate::host_input_combo_control_v1(vm, arg0 as u32, arg1, request, response);
        if arg0 == 0 && rc == 0 { assert_eq!(wrote, crate::input_wire::INPUT_COMBO_LEN); }
        if crate::world(|w| w.corrupt_response) { response.fill(0); }
        (STATUS_OK, (rc as i64) as u64)
    }
} }
mod input_wire {
    pub const INPUT_COMBO_LEN: usize = 88;
    pub fn input_combo(out: &mut [u8], combo: &crate::v::vinput::TrueosInputCombo) {
        assert_eq!(out.len(), INPUT_COMBO_LEN);
        out.fill(0); out[..4].copy_from_slice(&combo.combo_id.to_le_bytes());
    }
    pub fn decode_input_combo(bytes: &[u8]) -> Option<crate::v::vinput::TrueosInputCombo> {
        if bytes.len() != INPUT_COMBO_LEN { return None; }
        Some(crate::v::vinput::TrueosInputCombo { combo_id: u32::from_le_bytes(bytes[..4].try_into().ok()?) })
    }
}
mod allocators { pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() } }
mod shell3 { pub mod tui {
    pub fn remote_input_authority(vm: u8, run: u64) -> Option<()> {
        crate::world(|w| (!w.hull && w.authority && w.vm == Some(vm) && w.run == Some(run)).then_some(()))
    }
} }
mod r { pub mod services {
    pub mod mouse_motion_service {
        pub fn cursor_principal(slot: u32) -> Option<crate::MouseControlPrincipal> {
            crate::world(|w| w.devices.iter().find(|d| d.0 == 1 && d.1 == slot).map(|d| crate::MouseControlPrincipal::Vm(d.2)))
        }
    }
    pub mod keyboard_control_service {
        pub fn keyboard_principal(slot: u32) -> Option<crate::KeyboardControlPrincipal> {
            crate::world(|w| w.devices.iter().find(|d| d.0 == 2 && d.1 == slot).map(|d| crate::KeyboardControlPrincipal::Vm(d.2)))
        }
    }
    pub mod gamepad_control_service { pub fn gamepad_principal(_: u32) -> Option<crate::GamepadControlPrincipal> { None } }
} }
mod v { pub mod vinput {
    pub const INPUT_COMBO_COLOR_AUTO: i32 = -1;
    pub struct InputComboColor;
    impl InputComboColor { pub const COUNT: u8 = 8; }
    #[derive(Default)] pub struct TrueosInputCombo { pub combo_id: u32 }
} }
mod usb2 { pub mod hid { pub mod hut {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Default)] pub enum HidSourceKind { #[default] Unknown, Human, Ai, Remote }
    impl HidSourceKind {
        pub fn from_code(code: u8) -> Self { match code { 1 => Self::Human, 2 => Self::Ai, 3 => Self::Remote, _ => Self::Unknown } }
    }
    #[derive(Clone, Default)] pub struct InputCombo {
        pub combo_id: u32, pub source_kind: HidSourceKind,
        pub mouse_controller_id: u32, pub mouse_slot_id: u32, pub mouse_ep_target: u32,
        pub keyboard_controller_id: u32, pub keyboard_slot_id: u32, pub keyboard_ep_target: u32,
        pub tablet_controller_id: u32, pub tablet_slot_id: u32, pub tablet_ep_target: u32,
        pub gamepad_controller_id: u32, pub gamepad_slot_id: u32, pub gamepad_ep_target: u32,
    }
    pub fn request_combo(source_kind: HidSourceKind, _: &str, _: Option<u8>) -> Option<InputCombo> {
        crate::world(|w| {
            assert!(!w.hull, "guest-private HUT must never service a request");
            w.next_id += 1;
            let combo = InputCombo { combo_id: w.next_id, source_kind, ..Default::default() };
            w.combos.push(combo.clone()); Some(combo)
        })
    }
    pub fn combos_snapshot() -> Vec<InputCombo> { crate::world(|w| {
        assert!(!w.hull, "guest-private HUT must never answer ownership checks");
        w.combos.clone()
    }) }
    pub fn remove_empty_combo(id: u32) -> bool { remove_combo(id) }
    pub fn remove_combo(id: u32) -> bool {
        crate::world(|w| {
            assert!(!w.hull, "guest-private HUT must never service remove");
            let count = w.combos.len(); w.combos.retain(|c| c.combo_id != id); count != w.combos.len()
        })
    }
    pub fn set_combo_color(id: u32, _: u8) -> bool { crate::world(|w| {
        assert!(!w.hull, "guest-private HUT must never service set color");
        w.combos.iter().any(|c| c.combo_id == id)
    }) }
    pub fn bind_combo_keyboard(id: u32, controller: u32, slot: u32, ep: u32) -> bool {
        crate::world(|w| {
            assert!(!w.hull, "guest-private HUT must never service keyboard bind");
            let Some(c) = w.combos.iter_mut().find(|c| c.combo_id == id) else { return false; };
            c.keyboard_controller_id = controller; c.keyboard_slot_id = slot; c.keyboard_ep_target = ep; true
        })
    }
    pub fn bind_combo_mouse(id: u32, controller: u32, slot: u32, ep: u32) -> bool {
        crate::world(|w| {
            assert!(!w.hull, "guest-private HUT must never service mouse bind");
            let Some(c) = w.combos.iter_mut().find(|c| c.combo_id == id) else { return false; };
            c.mouse_controller_id = controller; c.mouse_slot_id = slot; c.mouse_ep_target = ep; true
        })
    }
    pub fn bind_combo_tablet(_: u32, _: u32, _: u32, _: u32) -> bool { false }
    pub fn bind_combo_gamepad(_: u32, _: u32, _: u32, _: u32) -> bool { false }
} } }
unsafe fn checked_utf8<'a>(ptr: *const u8, len: usize) -> Result<&'a str, i32> {
    if ptr.is_null() || len == 0 || len > 16 * 1024 { return Err(-1); }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr, len) }).map_err(|_| -1)
}
fn input_combo_info(combo: &usb2::hid::hut::InputCombo) -> v::vinput::TrueosInputCombo { v::vinput::TrueosInputCombo { combo_id: combo.combo_id } }
@PRODUCTION@

fn reset(authority: bool) {
    *INPUT_COMBO_OWNERS.lock() = input_combo_policy::Ownership::new();
    world(|w| *w = World { vm: Some(1), run: Some(7), authority, devices: vec![(1, 10, 1), (2, 20, 1), (2, 30, 2)], ..Default::default() });
}
fn request(kind: u8) -> Result<u32, i32> {
    let mut out = v::vinput::TrueosInputCombo::default();
    let status = unsafe { trueos_cabi_input_combo_request(kind, -1, b"control".as_ptr(), 7, &mut out) };
    if status == 0 { Ok(out.combo_id) } else { Err(status) }
}
#[test] fn approved_remote_controller_can_create_bind_color_and_release() {
    reset(true);
    let id = request(3).unwrap();
    assert!(remote_input_combo_owned(1, 7, id));
    assert_eq!(trueos_cabi_input_combo_bind_mouse(id, 0, 10, 0), 0);
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), 0);
    assert_eq!(trueos_cabi_input_combo_set_color(id, 2), 0);
    assert_eq!(trueos_cabi_input_combo_remove(id), 0);
    assert!(!remote_input_combo_owned(1, 7, id));
}
#[test] fn account_or_remote_label_alone_cannot_promote_an_ai_app() {
    reset(false);
    assert_eq!(request(3), Err(-2));
    assert_eq!(request(1), Err(-2));
    assert_eq!(request(0), Err(-2));
    let id = request(2).unwrap();
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), 0);
}
#[test] fn remote_authority_cannot_capture_foreign_or_physical_devices() {
    reset(true); let id = request(3).unwrap();
    for (controller, slot, ep) in [(0, 30, 0), (1, 20, 0), (0, 20, 1)] {
        assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, controller, slot, ep), -2);
    }
}
#[test] fn expired_session_denies_further_binding_but_allows_owned_cleanup() {
    reset(true); let id = request(3).unwrap();
    assert_eq!(trueos_cabi_input_combo_bind_mouse(id, 0, 10, 0), 0);
    world(|w| w.authority = false);
    assert_eq!(request(3), Err(-2));
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), -2);
    assert_eq!(trueos_cabi_input_combo_set_color(id, 1), -2);
    assert_eq!(trueos_cabi_input_combo_remove(id), 0);
}
#[test] fn reused_vm_and_foreign_vm_cannot_reuse_old_combo() {
    reset(true); let id = request(3).unwrap();
    for (vm, run) in [(1, 8), (2, 7)] {
        world(|w| { w.vm = Some(vm); w.run = Some(run); });
        assert!(!remote_input_combo_owned(vm, run, id));
        assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), -2);
        assert_eq!(trueos_cabi_input_combo_remove(id), -2);
    }
}
#[test] fn trusted_link_edit_cannot_lend_foreign_devices_to_combo_owner() {
    reset(true); let id = request(3).unwrap();
    world(|w| w.combos[0].keyboard_slot_id = 30);
    assert_eq!(trueos_cabi_input_combo_set_color(id, 1), -2);
    assert_eq!(trueos_cabi_input_combo_remove(id), -2);
}
#[test] fn hull_requests_use_same_remote_authority_and_ownership() {
    reset(true); world(|w| w.hull = true);
    let id = request(3).unwrap();
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), 0);
    world(|w| w.run = Some(0));
    assert_eq!(trueos_cabi_input_combo_remove(id), -2);
}

#[test] fn hull_forwards_to_host_session_instead_of_using_private_authority_or_hut() {
    reset(true); world(|w| w.hull = true);
    assert!(shell3::tui::remote_input_authority(1, 7).is_none(), "Hull has no private terminal authority");
    let id = request(3).unwrap();
    assert_eq!(trueos_cabi_input_combo_bind_mouse(id, 0, 10, 0), 0);
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), 0);
    assert_eq!(trueos_cabi_input_combo_set_color(id, 3), 0);
    // These exports must also forward before host ownership rejects them.
    assert_eq!(trueos_cabi_input_combo_bind_tablet(id, 0, 40, 0), -2);
    assert_eq!(trueos_cabi_input_combo_bind_gamepad(id, 0, 50, 0), -2);
    world(|w| {
        let c = &w.combos[0];
        assert_eq!((c.mouse_slot_id, c.keyboard_slot_id), (10, 20));
        assert_eq!(w.calls.iter().map(|c| c.0).collect::<Vec<_>>(), [0, 2, 3, 1, 4, 5]);
        assert_eq!(w.calls[0], (0, 0, [vec![3, 0, 0, 0], (-1i32).to_le_bytes().to_vec(), b"control".to_vec()].concat()));
        assert_eq!(w.calls[1], (2, id as u64, [0u32.to_le_bytes(), 10u32.to_le_bytes(), 0u32.to_le_bytes()].concat()));
        assert_eq!(w.calls[2], (3, id as u64, [0u32.to_le_bytes(), 20u32.to_le_bytes(), 0u32.to_le_bytes()].concat()));
        assert_eq!(w.calls[3], (1, id as u64, vec![3]));
        w.authority = false;
    });
    assert_eq!(trueos_cabi_input_combo_bind_keyboard(id, 0, 20, 0), -2);
    assert_eq!(trueos_cabi_input_combo_remove(id), 0, "host still permits owned cleanup after detach");
    world(|w| {
        assert!(w.hull, "broker restores guest context after each operation");
        assert!(w.combos.is_empty());
        assert_eq!(w.calls.last().unwrap(), &(6, id as u64, vec![]));
    });
}

#[test] fn hull_denials_and_transport_failures_do_not_fall_back_to_private_state() {
    reset(false); world(|w| w.hull = true);
    assert_eq!(request(3), Err(-2));
    assert!(world(|w| w.combos.is_empty()));
    world(|w| { w.authority = true; w.transport_failure = true; });
    assert_eq!(request(3), Err(-1));
    assert!(world(|w| w.combos.is_empty()));
    assert_eq!(trueos_cabi_input_combo_remove(1), -1);
}

#[test] fn malformed_host_payloads_never_mutate_combo_registry() {
    reset(true);
    let id = request(3).unwrap();
    let create = [vec![3, 0, 0, 0], (-1i32).to_le_bytes().to_vec(), b"control".to_vec()].concat();
    for (op, combo, payload) in [
        (0, 0, vec![]), (0, 0, vec![3; 8]), (0, 1, create.clone()),
        (0, 0, [vec![3, 1, 0, 0], create[4..].to_vec()].concat()),
        (0, 0, [vec![3, 0, 0, 0], (-1i32).to_le_bytes().to_vec(), vec![255]].concat()),
        (1, id as u64, vec![]), (1, id as u64, vec![1, 2]),
        (2, id as u64, vec![0; 11]), (3, id as u64, vec![0; 13]),
        (4, id as u64, vec![]), (5, id as u64, vec![]),
        (6, id as u64, vec![1]), (7, id as u64, vec![]),
        (6, u32::MAX as u64 + 1, vec![]),
    ] {
        let mut response = [0u8; 88];
        assert_ne!(host_input_combo_control_v1(1, op, combo, &payload, &mut response).0, 0);
        world(|w| {
            assert_eq!(w.combos.len(), 1);
            assert_eq!(w.combos[0].combo_id, id);
            assert_eq!((w.combos[0].mouse_slot_id, w.combos[0].keyboard_slot_id), (0, 0));
        });
    }
    assert_ne!(host_input_combo_control_v1(1, 0, 0, &create, &mut [0u8; 87]).0, 0);
    assert_eq!(world(|w| w.combos.len()), 1);
}

#[test] fn malformed_guest_output_and_oversized_request_do_not_publish_a_handle() {
    reset(true); world(|w| { w.hull = true; w.corrupt_response = true; });
    let mut out = v::vinput::TrueosInputCombo { combo_id: 77 };
    assert_eq!(unsafe { trueos_cabi_input_combo_request(3, -1, b"control".as_ptr(), 7, &mut out) }, -1);
    assert_eq!(out.combo_id, 77, "invalid host response never reaches caller output");
    let calls = world(|w| w.calls.len());
    let label = vec![b'x'; trueos_vm::vmcall::PAYLOAD_CAP];
    assert_eq!(unsafe { trueos_cabi_input_combo_request(3, -1, label.as_ptr(), label.len(), &mut out) }, -1);
    assert_eq!(unsafe { trueos_cabi_input_combo_request(3, -1, b"control".as_ptr(), 7, std::ptr::null_mut()) }, -1);
    assert_eq!(world(|w| w.calls.len()), calls, "invalid caller buffers never enter the transport");
}
