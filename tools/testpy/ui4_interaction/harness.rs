#![allow(dead_code)]
extern crate self as trueos_time;
extern crate self as trueos_vm;
extern crate self as v;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
pub mod bp_abi { include!("bp_abi_constants.rs"); }
pub mod vmcall {
    include!("vmcall_constants.rs");
    pub const STATUS_OK: u32 = 0;
    pub fn call(_: u32, _: u64, _: u64) -> (u32, u64) { (1, 0) }
    pub fn call_with_payload(_: u32, _: u64, _: u64, _: &[u8], _: &mut [u8]) -> (u32, u64) { (1, 0) }
}
pub struct Duration;
impl Duration { pub fn from_millis(_: u64) -> Self { Self } }
pub struct Timer;
impl Timer { pub async fn after(_: Duration) {} }
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 4; } }

mod hv {
    #[derive(Copy, Clone)]
    pub struct BlueprintInstanceIdentity { pub instance: [u8; 16], pub generation: u64 }
    #[derive(Copy, Clone)]
    pub struct HvVmState { pub supported: bool, pub running: bool, pub stop_requested: bool }
    #[derive(Copy, Clone)]
    pub struct Vm { pub state: HvVmState, pub run: Option<u64>, pub identity: Option<BlueprintInstanceIdentity> }
    const OFFLINE: Vm = Vm { state: HvVmState { supported: true, running: false, stop_requested: false }, run: Some(0), identity: None };
    pub static VMS: spin::Mutex<[Vm; 4]> = spin::Mutex::new([OFFLINE; 4]);
    pub fn vm_run_generation(owner: u8) -> Option<u64> { VMS.lock().get(owner as usize)?.run }
    pub fn vm_state(owner: u8) -> HvVmState { VMS.lock().get(owner as usize).map(|v| v.state).unwrap_or(HvVmState { supported: false, running: false, stop_requested: false }) }
    pub fn blueprint_instance_identity(owner: u8) -> Option<BlueprintInstanceIdentity> { VMS.lock().get(owner as usize)?.identity }
    pub fn app_vm_display_label(owner: u8) -> Option<String> { Some(format!("app-{owner}")) }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { None }
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    pub fn live(owner: u8) { VMS.lock()[owner as usize] = Vm { state: HvVmState { supported: true, running: true, stop_requested: false }, run: Some(1), identity: Some(BlueprintInstanceIdentity { instance: [owner; 16], generation: 1 }) }; }
}

static CURSOR_IDLE: AtomicBool = AtomicBool::new(true);
static KEYBOARD_IDLE: AtomicBool = AtomicBool::new(true);
static INPUT_COUNT: AtomicUsize = AtomicUsize::new(0);
static CAPTURE_HOOK: AtomicUsize = AtomicUsize::new(0);
static REENTRANT_RESULT: AtomicI32 = AtomicI32::new(0);
static BACKING_WIDTH: AtomicUsize = AtomicUsize::new(100);
mod r { pub mod services { pub mod keyboard_control_service {
    use crate::*;
    pub const KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1;
    pub const KEYBOARD_CONTROL_OPCODE_STROKE: u8 = 1;
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub enum KeyboardControlPrincipal { Kernel }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct KeyboardControlDevice { pub handle: u64, pub slot_id: u32 }
    #[derive(Default)]
    pub struct KeyboardControlCommand { pub opcode: u8, pub flags: u8, pub duration_ms: u32, pub key_code: u16 }
    pub fn keyboard_is_idle(_: KeyboardControlPrincipal, _: u64) -> Result<bool, ()> { Ok(KEYBOARD_IDLE.load(Ordering::Relaxed)) }
    pub fn submit_command(_: KeyboardControlPrincipal, _: u64, _: KeyboardControlCommand) -> Result<(), ()> { INPUT_COUNT.fetch_add(1, Ordering::Relaxed); KEYBOARD_IDLE.store(false, Ordering::Relaxed); Ok(()) }
    pub fn submit_text(_: KeyboardControlPrincipal, _: u64, text: &str, _: u32, _: bool) -> Result<usize, ()> { INPUT_COUNT.fetch_add(1, Ordering::Relaxed); KEYBOARD_IDLE.store(false, Ordering::Relaxed); Ok(text.chars().count()) }
} } }

mod ui4 {
    use crate::*;
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct WindowId(pub u32);
    impl WindowId { pub fn from_raw(raw: u32) -> Option<Self> { (raw != 0).then_some(Self(raw)) } pub fn raw(self) -> u32 { self.0 } }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub enum WindowOwner { Vm(u8), Kernel }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct OutputId;
    impl OutputId { pub fn from_slot(_: usize) -> Option<Self> { Some(Self) } }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct Plane(pub usize);
    impl Plane { pub fn slot(self) -> usize { self.0 } }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct WindowPlacement { pub x: i32, pub y: i32, pub width: u32, pub height: u32, pub z: i32 }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub enum WindowState { Ready, Closing }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct Interaction { pub receives_input: bool }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct WindowSnapshot { pub id: WindowId, pub frame: u32, pub owner: WindowOwner, pub producer_name: &'static str, pub plane: Plane, pub placement: WindowPlacement, pub presentation_placement: WindowPlacement, pub interaction: Interaction, pub state: WindowState, pub revision: u64, pub publish_serial: u64 }
    pub struct FrameSnapshot { pub plan: FramePlan }
    pub struct FramePlan { pub width: u32, pub height: u32 }
    pub fn frame_snapshot(_: u32) -> Result<FrameSnapshot, ()> { Ok(FrameSnapshot { plan: FramePlan { width: BACKING_WIDTH.load(Ordering::Relaxed) as u32, height: 80 } }) }
    #[derive(Copy, Clone, Debug, Eq, PartialEq)] pub struct CursorFrameKey { pub owner: WindowOwner, pub window: WindowId }
    impl CursorFrameKey { pub fn new(owner: WindowOwner, window: WindowId) -> Self { Self { owner, window } } }
    pub static WINDOWS: spin::Mutex<Vec<WindowSnapshot>> = spin::Mutex::new(Vec::new());
    pub static SELECTED: spin::Mutex<Option<CursorFrameKey>> = spin::Mutex::new(None);
    pub const COMPACT_WINDOW_GRID_EXTENT: u32 = 1000;
    pub const COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES: usize = 65536;
    pub fn visible_windows_for_output(_: OutputId) -> Vec<WindowSnapshot> { WINDOWS.lock().clone() }
    pub fn selected_frame_for_source(_: u32) -> Option<CursorFrameKey> { *SELECTED.lock() }
    pub fn select_window_for_cursor(_: u32, owner: WindowOwner, window: WindowId) -> Result<(), ()> { *SELECTED.lock() = Some(CursorFrameKey::new(owner, window)); Ok(()) }
    pub struct Observation { pub window_id: WindowId, pub native_width: u32, pub native_height: u32, pub capture_width: u32, pub capture_height: u32, pub placement: WindowPlacement, pub grid_extent: u16, pub grid_major_step: u16, pub revision: u64, pub publish_serial: u64, pub png: Vec<u8> }
    pub fn capture_compact_window_observation(window: WindowSnapshot) -> Result<Observation, ()> {
        match CAPTURE_HOOK.load(Ordering::Relaxed) {
            1 => { REENTRANT_RESULT.store(crate::spirit::ui4_interaction::focus(2, 12), Ordering::Relaxed); }
            2 => { crate::hv::VMS.lock()[1].run = Some(2); }
            _ => {}
        }
        Ok(Observation { window_id: window.id, native_width: BACKING_WIDTH.load(Ordering::Relaxed) as u32, native_height: 80, capture_width: 100, capture_height: 80, placement: window.presentation_placement, grid_extent: 1000, grid_major_step: 100, revision: window.revision, publish_serial: window.publish_serial, png: vec![1, 2, 3, 4] })
    }
    pub fn window(id: u32) -> WindowSnapshot { let placement = WindowPlacement { x: 100, y: 200, width: 100, height: 80, z: 0 }; WindowSnapshot { id: WindowId(id), frame: id, owner: WindowOwner::Vm(3), producer_name: "test", plane: Plane(4), placement, presentation_placement: placement, interaction: Interaction { receives_input: true }, state: WindowState::Ready, revision: 1, publish_serial: 1 } }
}
mod spirit {
    pub mod lilly_cursor {
        use crate::*;
        pub fn selection_source() -> Result<u32, ()> { Ok(4) }
        pub fn window_approach_complete() -> Result<bool, ()> { Ok(CURSOR_IDLE.load(Ordering::Relaxed)) }
        pub fn queue_pointer_action(_: i32, _: i32, _: u32, _: bool, _: u32, _: u32) -> Result<(), ()> { INPUT_COUNT.fetch_add(1, Ordering::Relaxed); CURSOR_IDLE.store(false, Ordering::Relaxed); Ok(()) }
    }
    #[path = "@TRUEOS_ROOT@/src/spirit/ui4_interaction.rs"]
    pub mod ui4_interaction;
    // Keep native-only visibility identical to the real module tree.
    pub fn register() { ui4_interaction::register_lilly_keyboard(crate::r::services::keyboard_control_service::KeyboardControlDevice { handle: 1, slot_id: 4 }); }
    pub fn native() -> bool { ui4_interaction::try_acquire_native_io().is_ok() }
    pub fn hold_native(f: impl FnOnce()) { let _guard = ui4_interaction::try_acquire_native_io().unwrap(); f(); }
    pub async fn hold_response() { ui4_interaction::acquire_response_io(crate::r::services::keyboard_control_service::KeyboardControlDevice { handle: 1, slot_id: 4 }).await; }
    pub fn release_response() { ui4_interaction::release_response_io(crate::r::services::keyboard_control_service::KeyboardControlDevice { handle: 1, slot_id: 4 }); }
}

#[cfg(test)] mod tests {
    use super::*;
    use spirit::ui4_interaction as api;
    fn setup() {
        hv::live(1); hv::live(2);
        spirit::register();
        *ui4::WINDOWS.lock() = vec![ui4::window(11), ui4::window(12)];
        *ui4::SELECTED.lock() = Some(ui4::CursorFrameKey::new(ui4::WindowOwner::Vm(3), ui4::WindowId(11)));
    }
    fn ready() { CURSOR_IDLE.store(true, Ordering::Relaxed); KEYBOARD_IDLE.store(true, Ordering::Relaxed); }

    #[test] fn admits_live_blueprints_without_any_archive_name_gate() {
        setup();
        assert!(api::windows(1, &mut []) > 0);
        assert!(api::windows(2, &mut []) > 0);
        assert_eq!(api::focus(1, 11), 0);
        assert_eq!(api::focus(2, 12), 0);
    }
    #[test] fn denies_missing_offline_stopping_and_out_of_range_owners() {
        setup();
        assert_eq!(api::windows(255, &mut []), api::ERROR_DENIED as isize);
        assert_eq!(api::windows(0, &mut []), api::ERROR_DENIED as isize);
        hv::VMS.lock()[1].identity = None;
        assert_eq!(api::focus(1, 11), api::ERROR_DENIED);
        hv::live(1); hv::VMS.lock()[1].state.running = false;
        assert_eq!(api::focus(1, 11), api::ERROR_DENIED);
        hv::live(1); hv::VMS.lock()[1].state.stop_requested = true;
        assert_eq!(api::focus(1, 11), api::ERROR_DENIED);
    }
    #[test] fn observation_is_private_to_instance_and_run_generation() {
        setup();
        assert_eq!(api::observe_prepare(1), 4);
        assert!(api::observe_metadata(1, &mut []) > 0);
        assert_eq!(api::observe_metadata(2, &mut []), api::ERROR_BAD_STATE as isize);
        hv::VMS.lock()[1].run = Some(2);
        assert_eq!(api::observe_read(1, 0, &mut [0; 4]), api::ERROR_BAD_STATE as isize);
        hv::VMS.lock()[1].run = Some(1);
        hv::VMS.lock()[1].identity.as_mut().unwrap().generation = 2;
        assert_eq!(api::observe_metadata(1, &mut []), api::ERROR_BAD_STATE as isize);
    }
    #[test] fn cannot_publish_capture_after_owner_retired() {
        setup(); CAPTURE_HOOK.store(2, Ordering::Relaxed);
        assert_eq!(api::observe_prepare(1), api::ERROR_DENIED as isize);
        assert_eq!(api::observe_metadata(1, &mut []), api::ERROR_BAD_STATE as isize);
    }
    #[test] fn in_flight_capture_does_not_reap_as_idle_hardware() {
        setup(); CAPTURE_HOOK.store(1, Ordering::Relaxed);
        assert_eq!(api::observe_prepare(1), 4);
        assert_eq!(REENTRANT_RESULT.load(Ordering::Relaxed), api::ERROR_BUSY);
        assert_eq!(api::focus(2, 12), 0);
    }
    #[test] fn pointer_requires_own_capture_and_current_geometry() {
        setup();
        assert_eq!(api::pointer(1, 500, 500, api::POINTER_ACTION_MOVE), api::ERROR_BAD_STATE);
        assert_eq!(api::observe_prepare(1), 4);
        ui4::WINDOWS.lock()[0].placement.x += 100;
        assert_eq!(api::pointer(1, 500, 500, api::POINTER_ACTION_MOVE), api::ERROR_BAD_STATE);
        assert_eq!(INPUT_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(api::observe_prepare(1), 4);
        assert_eq!(api::pointer(1, 500, 500, api::POINTER_ACTION_MOVE), 0);
    }
    #[test] fn type_and_key_require_own_capture_of_selected_target() {
        setup();
        assert_eq!(api::type_text(1, b"hi"), api::ERROR_BAD_STATE);
        assert_eq!(api::key(1, api::KEY_ENTER), api::ERROR_BAD_STATE);
        api::observe_prepare(1);
        assert_eq!(api::focus(2, 12), 0);
        assert_eq!(api::type_text(1, b"hi"), api::ERROR_BAD_STATE);
        assert_eq!(api::key(1, api::KEY_ENTER), api::ERROR_BAD_STATE);
        assert_eq!(INPUT_COUNT.load(Ordering::Relaxed), 0);
        api::observe_prepare(1);
        assert_eq!(api::type_text(1, b"hi"), 0);
        ready(); assert_eq!(api::key(1, api::KEY_ENTER), 0);
    }
    #[test] fn ordinary_repaints_do_not_invalidate_input_geometry() {
        setup(); api::observe_prepare(1);
        ui4::WINDOWS.lock()[0].revision += 1;
        ui4::WINDOWS.lock()[0].publish_serial += 1;
        assert_eq!(api::type_text(1, b"hi"), 0);
    }
    #[test] fn backing_extent_or_plane_change_requires_new_observation() {
        setup(); api::observe_prepare(1);
        BACKING_WIDTH.store(120, Ordering::Relaxed);
        assert_eq!(api::pointer(1, 500, 500, api::POINTER_ACTION_MOVE), api::ERROR_BAD_STATE);
        assert_eq!(api::type_text(1, b"hi"), api::ERROR_BAD_STATE);
        assert_eq!(api::observe_prepare(1), 4);
        ui4::WINDOWS.lock()[0].plane = ui4::Plane(1);
        assert_eq!(api::key(1, api::KEY_ENTER), api::ERROR_BAD_STATE);
        assert_eq!(INPUT_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(api::observe_prepare(1), 4);
        assert_eq!(api::pointer(1, 500, 500, api::POINTER_ACTION_MOVE), 0);
    }
    #[test] fn paired_serializer_waits_for_both_devices_and_respects_native_owner() {
        setup(); api::observe_prepare(1);
        assert_eq!(api::pointer(1, 100, 100, api::POINTER_ACTION_PRIMARY_CLICK), 0);
        assert_eq!(api::focus(2, 12), api::ERROR_BUSY);
        assert!(!spirit::native());
        KEYBOARD_IDLE.store(false, Ordering::Relaxed);
        CURSOR_IDLE.store(true, Ordering::Relaxed);
        assert_eq!(api::focus(2, 12), api::ERROR_BUSY);
        ready();
        spirit::hold_native(|| { assert_eq!(api::focus(2, 12), api::ERROR_BUSY); assert!(!spirit::native()); });
        assert_eq!(api::focus(2, 12), 0);
    }
    #[test] fn native_cancellation_keeps_pending_motion_draining() {
        setup();
        spirit::hold_native(|| CURSOR_IDLE.store(false, Ordering::Relaxed));
        assert_eq!(api::focus(1, 11), api::ERROR_BUSY);
        ready(); assert_eq!(api::focus(1, 11), 0);
    }
    #[test] fn boot_greeting_reservation_remains_exclusive() {
        setup();
        let mut future = Box::pin(spirit::hold_response());
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_ready());
        assert_eq!(api::focus(1, 11), api::ERROR_BUSY);
        assert!(!spirit::native());
        spirit::release_response();
        assert_eq!(api::focus(1, 11), 0);
    }
}
