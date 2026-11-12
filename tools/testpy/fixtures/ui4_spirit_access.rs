#![allow(dead_code)]
use std::cell::RefCell;

#[derive(Default)]
struct World {
    devices: Vec<usb2::hid::hut::Device>,
    mouse: Vec<(
        u32,
        r::services::mouse_motion_service::MouseControlPrincipal,
    )>,
    keyboard: Vec<(
        u32,
        r::services::keyboard_control_service::KeyboardControlPrincipal,
    )>,
    windows: Vec<ui4::WindowSnapshot>,
    vms: Vec<(u8, hv::VmState, u64)>,
    notifications: usize,
    cursor_frontier: u64,
    keyboard_frontiers: (u32, u32),
    remote: Vec<shell3::tui::RemoteInputAuthority>,
    combo_owners: Vec<(u32, u8, u64)>,
    protected: Vec<(u8, u64)>,
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|w| f(&mut w.borrow_mut()))
}

mod usb2 {
    pub mod hid {
        pub fn cursor_event_frontier() -> u64 {
            crate::world(|w| w.cursor_frontier)
        }
        pub mod hut {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum HidSourceKind {
                Human,
                Ai,
                Remote,
                Unknown,
            }
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum HidDeviceKind {
                Mouse,
                Tablet,
                Keyboard,
            }
            #[derive(Clone, Copy)]
            pub struct HidKeyboardState {
                pub controller_id: u8,
                pub slot_id: u32,
                pub ep_target: u8,
            }
            #[derive(Clone, Copy)]
            pub struct DeviceId {
                pub kind: HidDeviceKind,
                pub controller_id: u8,
                pub slot_id: u32,
                pub ep_target: u8,
            }
            #[derive(Clone, Copy)]
            pub struct Device {
                pub id: DeviceId,
                pub combo_id: u32,
                pub source_kind: HidSourceKind,
                pub virtual_device: bool,
            }
            pub fn devices_snapshot() -> Vec<Device> {
                crate::world(|w| w.devices.clone())
            }
        }
    }
}
mod r {
    pub mod io {
        pub mod input_cabi {
            pub fn remote_input_combo_owned(vm: u8, run: u64, combo: u32) -> bool {
                crate::world(|w| w.combo_owners.contains(&(combo, vm, run)))
            }
        }
    }
    pub mod cursor {
        pub const HID_KIND_TABLET: u8 = 3;
    }
    pub mod keyboard {
        pub const KEYBOARD_OUTPUT_KIND_PHYSICAL: u8 = 2;
        pub fn output_event_frontiers() -> (u32, u32) {
            crate::world(|w| w.keyboard_frontiers)
        }
        #[derive(Clone, Copy, Default)]
        pub struct TrueosKeyboardOutputEvent {
            pub seq: u32,
            pub kind: u8,
            pub controller_id: u8,
            pub slot_id: u32,
            pub ep_target: u8,
        }
    }
    pub mod services {
        pub mod mouse_motion_service {
            #[derive(Clone, Copy)]
            pub enum MouseControlPrincipal {
                Kernel,
                Vm(u8),
            }
            pub fn cursor_principal(slot: u32) -> Option<MouseControlPrincipal> {
                crate::world(|w| {
                    w.mouse
                        .iter()
                        .find(|entry| entry.0 == slot)
                        .map(|entry| entry.1)
                })
            }
        }
        pub mod keyboard_control_service {
            #[derive(Clone, Copy)]
            pub enum KeyboardControlPrincipal {
                Kernel,
                Vm(u8),
            }
            pub fn keyboard_principal(slot: u32) -> Option<KeyboardControlPrincipal> {
                crate::world(|w| {
                    w.keyboard
                        .iter()
                        .find(|entry| entry.0 == slot)
                        .map(|entry| entry.1)
                })
            }
        }
    }
}
mod shell3 {
    pub mod tui {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct RemoteInputAuthority {
            pub account: u64,
            pub vm: u8,
            pub run: u64,
            pub frontend: u64,
            pub epoch: u64,
            pub keyboard_frontiers: (u32, u32),
            pub cursor_frontier: u64,
        }
        pub fn remote_input_authority(vm: u8, run: u64) -> Option<RemoteInputAuthority> {
            crate::world(|w| w.remote.iter().find(|a| a.vm == vm && a.run == run).copied())
        }
        pub fn remote_input_authority_valid(authority: RemoteInputAuthority) -> bool {
            remote_input_authority(authority.vm, authority.run) == Some(authority)
        }
    }
}
mod access_key_db {
    pub fn authorize_vm_control(vm: u8, actor: Option<u64>) -> Result<(), ()> {
        crate::world(|w| match w.protected.iter().find(|p| p.0 == vm) {
            Some((_, account)) if actor != Some(*account) => Err(()),
            _ => Ok(()),
        })
    }
}
mod hv {
    #[derive(Clone, Copy, Default)]
    pub struct VmState {
        pub running: bool,
        pub stop_requested: bool,
    }
    pub fn vm_state(vm: u8) -> VmState {
        crate::world(|w| {
            w.vms
                .iter()
                .find(|entry| entry.0 == vm)
                .map(|entry| entry.1)
                .unwrap_or_default()
        })
    }
    pub fn vm_run_generation(vm: u8) -> Option<u64> {
        crate::world(|w| {
            w.vms
                .iter()
                .find(|entry| entry.0 == vm)
                .map(|entry| entry.2)
        })
    }
}
mod ui4 {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum WindowOwner {
        Vm(u8),
        Kernel,
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct WindowId(u32);
    impl WindowId {
        pub fn from_raw(raw: u32) -> Option<Self> {
            (raw != 0).then_some(Self(raw))
        }
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct CursorFrameKey {
        pub owner: WindowOwner,
        pub window: WindowId,
    }
    impl CursorFrameKey {
        pub fn new(owner: WindowOwner, window: WindowId) -> Self {
            Self { owner, window }
        }
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Ui4CursorSource {
        pub controller_id: u8,
        pub slot_id: u32,
        pub ep_target: u8,
        pub hid_kind: u8,
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum WindowState {
        Ready,
        Closing,
        Closed,
    }
    #[derive(Clone, Copy)]
    pub struct WindowSessionId(pub u32);
    impl WindowSessionId {
        pub fn raw(self) -> u32 {
            self.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct WindowSnapshot {
        pub id: WindowId,
        pub owner: WindowOwner,
        pub session: WindowSessionId,
        pub state: WindowState,
    }
    pub mod window_broker {
        pub fn window_snapshot(
            owner: super::WindowOwner,
            window: super::WindowId,
        ) -> Option<super::WindowSnapshot> {
            crate::world(|w| {
                w.windows
                    .iter()
                    .find(|entry| entry.owner == owner && entry.id == window)
                    .copied()
            })
        }
    }
    pub mod input_broker {
        pub fn notify_slot4_visual_change() {
            crate::world(|w| w.notifications += 1);
        }
    }
    #[path = "@PRODUCTION@"]
    pub mod spirit_access;
}

#[cfg(test)]
mod tests {
    use super::*;
    use r::keyboard::TrueosKeyboardOutputEvent;
    use r::services::{
        keyboard_control_service::KeyboardControlPrincipal,
        mouse_motion_service::MouseControlPrincipal,
    };
    use ui4::{
        CursorFrameKey, Ui4CursorSource, WindowId, WindowOwner, WindowSessionId, WindowSnapshot,
        WindowState, spirit_access as access,
    };
    use usb2::hid::hut::{Device, DeviceId, HidDeviceKind, HidSourceKind};

    fn frame(id: u32) -> CursorFrameKey {
        CursorFrameKey::new(WindowOwner::Vm(1), WindowId::from_raw(id).unwrap())
    }
    fn window(key: CursorFrameKey, session: u32) -> WindowSnapshot {
        WindowSnapshot {
            owner: key.owner,
            id: key.window,
            session: WindowSessionId(session),
            state: WindowState::Ready,
        }
    }
    fn mouse(slot: u32, controller: u8) -> Ui4CursorSource {
        Ui4CursorSource {
            controller_id: controller,
            slot_id: slot,
            ep_target: 0,
            hid_kind: 1,
        }
    }
    fn keyboard(slot: u32, controller: u8) -> TrueosKeyboardOutputEvent {
        TrueosKeyboardOutputEvent {
            seq: 1,
            kind: 0,
            controller_id: controller,
            slot_id: slot,
            ep_target: 0,
        }
    }
    fn device(
        kind: HidDeviceKind,
        slot: u32,
        controller: u8,
        source: HidSourceKind,
        virtual_device: bool,
    ) -> Device {
        Device {
            id: DeviceId {
                kind,
                slot_id: slot,
                controller_id: controller,
                ep_target: 0,
            },
            source_kind: source,
            combo_id: 0,
            virtual_device,
        }
    }
    fn setup(id: u32) -> (CursorFrameKey, Ui4CursorSource, TrueosKeyboardOutputEvent) {
        let key = frame(id);
        world(|w| {
            *w = World::default();
            w.windows.push(window(key, 3));
            w.devices.push(device(
                HidDeviceKind::Mouse,
                1,
                4,
                HidSourceKind::Human,
                false,
            ));
            w.devices.push(device(
                HidDeviceKind::Keyboard,
                2,
                4,
                HidSourceKind::Human,
                false,
            ));
            w.vms.push((
                1,
                hv::VmState {
                    running: true,
                    stop_requested: false,
                },
                7,
            ));
        });
        (key, mouse(1, 4), keyboard(2, 4))
    }
    fn ai_pair(slot: u32, kind: HidSourceKind) -> (Ui4CursorSource, TrueosKeyboardOutputEvent) {
        world(|w| {
            w.devices
                .push(device(HidDeviceKind::Mouse, slot, 0, kind, true));
            w.devices
                .push(device(HidDeviceKind::Keyboard, slot + 1, 0, kind, true));
            w.mouse.push((slot, MouseControlPrincipal::Vm(1)));
            w.keyboard.push((slot + 1, KeyboardControlPrincipal::Vm(1)));
        });
        (mouse(slot, 0), keyboard(slot + 1, 0))
    }

    fn remote_pair(slot: u32, account: u64) -> (Ui4CursorSource, TrueosKeyboardOutputEvent) {
        let (cursor, mut keys) = ai_pair(slot, HidSourceKind::Remote);
        keys.seq = 11;
        world(|w| {
            for device in &mut w.devices {
                if device.id.controller_id == 0 && [slot, slot + 1].contains(&device.id.slot_id) {
                    device.combo_id = slot;
                }
            }
            w.combo_owners.push((slot, 1, 7));
            w.protected.push((1, account));
            w.remote.push(shell3::tui::RemoteInputAuthority {
                account, vm: 1, run: 7, frontend: 77, epoch: 1,
                keyboard_frontiers: (10, 10), cursor_frontier: 10,
            });
        });
        (cursor, keys)
    }

    #[test]
    fn authenticated_remote_human_controls_own_frame_without_spirit_grant() {
        let (key, _, _) = setup(1301);
        let (cursor, keys) = remote_pair(1301, 0);
        assert!(!access::is_granted(key));
        assert!(access::cursor_is_human(cursor));
        assert!(access::keyboard_is_human(&keys));
        assert!(access::cursor_target_allowed(cursor, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        assert!(access::cursor_event_allowed(cursor, 11));
        assert!(access::keyboard_event_allowed(&keys));
        assert!(matches!(access::stamp_cursor(cursor, key), Some(access::InputStamp::Remote { .. })));
        assert_eq!(access::toggle_from_human(cursor, key), Ok(true));
        access::frame_closed(key);
    }

    #[test]
    fn remote_labels_and_logged_in_ai_do_not_grant_human_input() {
        let (key, human, _) = setup(1401);
        let (cursor, keys) = remote_pair(1401, 7);
        access::toggle_from_human(human, key).unwrap();
        // An anonymous/disconnected session cannot even borrow a Spirit grant.
        world(|w| w.remote.clear());
        assert!(!access::cursor_is_human(cursor));
        assert!(!access::keyboard_is_human(&keys));
        assert!(!access::cursor_target_allowed(cursor, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        // A session alone does not upgrade AI devices or foreign combos.
        let (other, other_keys) = remote_pair(1411, 7);
        world(|w| {
            w.combo_owners.retain(|entry| entry.0 != 1411);
        });
        assert!(!access::cursor_is_human(other));
        assert!(!access::keyboard_is_human(&other_keys));
        let (ai, ai_keys) = ai_pair(1421, HidSourceKind::Ai);
        assert!(!access::cursor_is_human(ai));
        assert!(!access::keyboard_is_human(&ai_keys));
        access::frame_closed(key);
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&ai_keys, key));
    }

    #[test]
    fn remote_input_cannot_control_or_share_another_accounts_protected_frame() {
        let (key, _, _) = setup(1501);
        let (cursor, keys) = remote_pair(1501, 7);
        world(|w| w.protected = vec![(1, 8)]);
        assert!(!access::cursor_target_allowed(cursor, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        assert!(!access::cursor_sequence_allowed(cursor, key, 11));
        assert_eq!(access::toggle_from_human(cursor, key), Err(()));
        assert!(!access::is_granted(key));
    }

    #[test]
    fn queued_remote_stamps_expire_on_session_account_run_or_frame_reuse() {
        let (key, _, _) = setup(1601);
        let (cursor, keys) = remote_pair(1601, 7);
        let pointer_stamp = access::stamp_cursor(cursor, key).unwrap();
        let key_stamp = access::stamp_keyboard(&keys, key).unwrap();
        let original = world(|w| w.remote[0]);
        assert_eq!(access::cursor_remote_authority(cursor), Some(original));
        for replacement in [
            None,
            Some(shell3::tui::RemoteInputAuthority { epoch: 2, ..original }),
            Some(shell3::tui::RemoteInputAuthority { frontend: 78, ..original }),
            Some(shell3::tui::RemoteInputAuthority { account: 8, ..original }),
        ] {
            world(|w| w.remote = replacement.into_iter().collect());
            assert_ne!(access::cursor_remote_authority(cursor), Some(original));
            assert!(!access::stamp_valid(pointer_stamp, key));
            assert!(!access::stamp_valid(key_stamp, key));
        }
        world(|w| w.remote = vec![original]);
        assert_eq!(access::cursor_remote_authority(cursor), Some(original));
        assert!(access::stamp_valid(pointer_stamp, key));
        world(|w| w.vms[0].2 = 8);
        assert!(!access::stamp_valid(pointer_stamp, key));
        world(|w| { w.vms[0].2 = 7; w.windows[0].session = WindowSessionId(4); });
        assert!(!access::stamp_valid(pointer_stamp, key));
        assert!(!access::stamp_valid(key_stamp, key));
    }

    #[test]
    fn remote_station_release_and_reassignment_invalidate_queued_input() {
        let (key, _, _) = setup(1701);
        let (cursor, keys) = remote_pair(1701, 7);
        let pointer_stamp = access::stamp_cursor(cursor, key).unwrap();
        let key_stamp = access::stamp_keyboard(&keys, key).unwrap();
        world(|w| w.mouse.clear());
        assert!(!access::stamp_valid(pointer_stamp, key));
        world(|w| w.devices.iter_mut().for_each(|d| {
            if d.id.slot_id == keys.slot_id { d.combo_id = 0; }
        }));
        assert!(!access::stamp_valid(key_stamp, key));
    }

    #[test]
    fn raw_remote_reports_before_new_session_frontiers_cannot_borrow_its_authority() {
        let (key, _, _) = setup(1801);
        let (cursor, mut keys) = remote_pair(1801, 7);
        assert!(!access::cursor_event_allowed(cursor, 10));
        assert!(!access::cursor_sequence_allowed(cursor, key, 10));
        keys.seq = 10;
        assert!(!access::keyboard_event_allowed(&keys));
        assert!(access::stamp_keyboard(&keys, key).is_none());
        world(|w| {
            w.remote[0].cursor_frontier = 20;
            w.remote[0].keyboard_frontiers = (20, 20);
            w.remote[0].epoch = 2;
        });
        assert!(!access::cursor_event_allowed(cursor, 11));
        keys.seq = 11;
        assert!(!access::keyboard_event_allowed(&keys));
        assert!(access::stamp_keyboard(&keys, key).is_none());
        keys.seq = 21;
        assert!(access::keyboard_event_allowed(&keys));
        assert!(access::stamp_keyboard(&keys, key).is_some());
        assert!(access::cursor_event_allowed(cursor, 21));
    }

    #[test]
    fn physical_human_keeps_desktop_and_only_human_can_toggle() {
        let (key, human, keys) = setup(101);
        let (ai, ai_keys) = ai_pair(101, HidSourceKind::Ai);
        assert!(access::cursor_is_human(human));
        assert!(access::keyboard_is_human(&keys));
        assert!(access::cursor_target_allowed(human, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&ai_keys, key));
        assert_eq!(access::toggle_from_human(ai, key), Err(()));
        assert!(!access::is_granted(key));
        assert_eq!(access::toggle_from_human(human, key), Ok(true));
        assert!(access::cursor_target_allowed(ai, key));
        assert!(access::keyboard_target_allowed(&ai_keys, key));
        assert_eq!(world(|w| w.notifications), 1);
        access::frame_closed(key);
    }

    #[test]
    fn grants_are_exact_frames_and_shared_by_independent_ai_cursors() {
        let (key, human, _) = setup(201);
        let other = frame(202);
        world(|w| w.windows.push(window(other, 3)));
        let (ai, keys) = ai_pair(201, HidSourceKind::Ai);
        let (subagent, subkeys) = ai_pair(211, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        for (cursor, keyboard) in [(ai, keys), (subagent, subkeys)] {
            assert!(access::cursor_target_allowed(cursor, key));
            assert!(access::keyboard_target_allowed(&keyboard, key));
            assert!(!access::cursor_target_allowed(cursor, other));
            assert!(!access::keyboard_target_allowed(&keyboard, other));
        }
        assert!(access::authorize(other).is_none());
        access::frame_closed(key);
    }

    #[test]
    fn vm_station_ownership_overrides_human_label_and_retired_devices_fail_closed() {
        let (key, human, _) = setup(301);
        let (ai, keys) = ai_pair(301, HidSourceKind::Human);
        assert!(!access::cursor_is_human(ai));
        assert!(!access::keyboard_is_human(&keys));
        assert_eq!(access::toggle_from_human(ai, key), Err(()));
        access::toggle_from_human(human, key).unwrap();
        assert!(access::cursor_target_allowed(ai, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        world(|w| {
            w.mouse.clear();
            w.keyboard.clear();
        });
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        access::frame_closed(key);
    }

    #[test]
    fn remote_unknown_and_unowned_virtual_sources_do_not_become_local_humans() {
        let (key, human, _) = setup(401);
        access::toggle_from_human(human, key).unwrap();
        for (offset, kind, virtual_device) in [
            (0, HidSourceKind::Remote, false),
            (10, HidSourceKind::Unknown, false),
            (20, HidSourceKind::Human, true),
        ] {
            let slot = 401 + offset;
            world(|w| {
                w.devices
                    .push(device(HidDeviceKind::Mouse, slot, 7, kind, virtual_device));
                w.devices.push(device(
                    HidDeviceKind::Keyboard,
                    slot + 1,
                    7,
                    kind,
                    virtual_device,
                ));
            });
            let cursor = mouse(slot, 7);
            let keyboard = keyboard(slot + 1, 7);
            assert!(!access::cursor_is_human(cursor));
            assert!(!access::keyboard_is_human(&keyboard));
            assert!(!access::cursor_target_allowed(cursor, key));
            assert!(!access::keyboard_target_allowed(&keyboard, key));
            assert_eq!(access::toggle_from_human(cursor, key), Err(()));
        }
        assert!(!access::cursor_target_allowed(mouse(999, 9), key));
        assert!(!access::keyboard_target_allowed(&keyboard(999, 9), key));
        access::frame_closed(key);
    }

    #[test]
    fn revoke_regrant_does_not_revive_cached_observation_or_pending_input() {
        let (key, human, _) = setup(501);
        let (ai, keys) = ai_pair(501, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        let old = access::authorize(key).unwrap();
        assert!(access::bind_input(ai, keys.slot_id, old, 1, 7));
        assert!(access::cursor_target_allowed(ai, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        assert_eq!(access::toggle_from_human(human, key), Ok(false));
        assert!(!access::valid(old));
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        assert_eq!(access::toggle_from_human(human, key), Ok(true));
        assert!(!access::valid(old));
        assert!(!access::bind_input(ai, keys.slot_id, old, 1, 7));
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        let current = access::authorize(key).unwrap();
        assert!(access::bind_input(ai, keys.slot_id, current, 1, 7));
        assert!(access::cursor_target_allowed(ai, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        access::frame_closed(key);
    }

    #[test]
    fn bound_input_is_specific_to_frame_and_vm_run_and_stops_with_vm() {
        let (key, human, _) = setup(601);
        let other = frame(602);
        world(|w| w.windows.push(window(other, 3)));
        let (ai, keys) = ai_pair(601, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        access::toggle_from_human(human, other).unwrap();
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        assert!(!access::cursor_target_allowed(ai, other));
        assert!(!access::keyboard_target_allowed(&keys, other));
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(other).unwrap(),
            1,
            7
        ));
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        assert!(access::cursor_target_allowed(ai, other));
        assert!(access::keyboard_target_allowed(&keys, other));
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        world(|w| w.vms[0].2 = 8);
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        world(|w| {
            w.vms[0].2 = 7;
            w.vms[0].1.stop_requested = true;
        });
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        world(|w| {
            w.vms[0].1.stop_requested = false;
            w.vms[0].1.running = false;
        });
        assert!(!access::cursor_target_allowed(ai, key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        access::frame_closed(key);
        access::frame_closed(other);
    }

    #[test]
    fn closing_and_session_reuse_invalidate_old_frame_permits() {
        let (key, human, _) = setup(701);
        let (ai, keys) = ai_pair(701, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        let old = access::authorize(key).unwrap();
        assert!(access::bind_input(ai, keys.slot_id, old, 1, 7));
        world(|w| w.windows[0].state = WindowState::Closing);
        assert!(!access::valid(old));
        assert!(!access::is_granted(key));
        assert!(!access::cursor_target_allowed(ai, key));
        world(|w| {
            w.windows[0].state = WindowState::Ready;
            w.windows[0].session = WindowSessionId(4);
        });
        assert!(!access::valid(old));
        assert!(!access::is_granted(key));
        assert!(!access::keyboard_target_allowed(&keys, key));
        // Broker reclamation can replace a session without the explicit close
        // hook; admission must prune the retired grant before toggling.
        assert_eq!(access::toggle_from_human(human, key), Ok(true));
        assert!(!access::valid(old));
        assert!(!access::cursor_target_allowed(ai, key));
        access::frame_closed(key);
    }

    #[test]
    fn queued_stamps_keep_the_original_grant_across_regrant_and_rebind() {
        let (key, human, _) = setup(801);
        let (ai, keys) = ai_pair(801, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        let old_cursor = access::stamp_cursor(ai, key).unwrap();
        let old_keyboard = access::stamp_keyboard(&keys, key).unwrap();
        assert!(access::stamp_valid(old_cursor, key));
        assert!(access::stamp_valid(old_keyboard, key));
        access::toggle_from_human(human, key).unwrap();
        access::toggle_from_human(human, key).unwrap();
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        assert!(access::cursor_target_allowed(ai, key));
        assert!(access::keyboard_target_allowed(&keys, key));
        assert!(!access::stamp_valid(old_cursor, key));
        assert!(!access::stamp_valid(old_keyboard, key));
        let fresh_cursor = access::stamp_cursor(ai, key).unwrap();
        let fresh_keyboard = access::stamp_keyboard(&keys, key).unwrap();
        assert!(access::stamp_valid(fresh_cursor, key));
        assert!(access::stamp_valid(fresh_keyboard, key));
        world(|w| w.vms[0].2 = 8);
        assert!(!access::stamp_valid(fresh_cursor, key));
        assert!(!access::stamp_valid(fresh_keyboard, key));
        access::frame_closed(key);
    }

    #[test]
    fn unbound_vm_stamps_expire_when_the_station_owner_run_stops_or_restarts() {
        let (key, human, _) = setup(901);
        let (ai, keys) = ai_pair(901, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        let cursor = access::stamp_cursor(ai, key).unwrap();
        let keyboard = access::stamp_keyboard(&keys, key).unwrap();
        world(|w| w.vms[0].1.stop_requested = true);
        assert!(!access::stamp_valid(cursor, key));
        assert!(!access::stamp_valid(keyboard, key));
        assert!(access::stamp_cursor(ai, key).is_none());
        assert!(access::stamp_keyboard(&keys, key).is_none());
        world(|w| {
            w.vms[0].1.stop_requested = false;
            w.vms[0].2 = 8;
        });
        assert!(!access::stamp_valid(cursor, key));
        assert!(!access::stamp_valid(keyboard, key));
        assert!(access::stamp_cursor(ai, key).is_some());
        assert!(access::stamp_keyboard(&keys, key).is_some());
        access::frame_closed(key);
    }

    #[test]
    fn rebinding_does_not_lend_new_authority_to_already_emitted_raw_events() {
        let (key, human, _) = setup(1001);
        let (ai, mut logical) = ai_pair(1001, HidSourceKind::Ai);
        access::toggle_from_human(human, key).unwrap();
        world(|w| {
            w.keyboard_frontiers = (100, 200);
            w.cursor_frontier = 300;
        });
        assert!(access::bind_input(
            ai,
            logical.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        logical.seq = 100;
        assert!(access::stamp_keyboard(&logical, key).is_none());
        logical.seq = 101;
        assert!(access::stamp_keyboard(&logical, key).is_some());
        let mut physical = logical;
        physical.kind = r::keyboard::KEYBOARD_OUTPUT_KIND_PHYSICAL;
        physical.seq = 200;
        assert!(access::stamp_keyboard(&physical, key).is_none());
        physical.seq = 201;
        assert!(access::stamp_keyboard(&physical, key).is_some());
        assert!(!access::cursor_sequence_allowed(ai, key, 300));
        assert!(access::cursor_sequence_allowed(ai, key, 301));
        world(|w| {
            w.keyboard_frontiers = (101, 201);
            w.cursor_frontier = 301;
        });
        assert!(access::bind_input(
            ai,
            logical.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        assert!(access::stamp_keyboard(&logical, key).is_none());
        assert!(access::stamp_keyboard(&physical, key).is_none());
        assert!(!access::cursor_sequence_allowed(ai, key, 301));
        logical.seq = 102;
        physical.seq = 202;
        assert!(access::stamp_keyboard(&logical, key).is_some());
        assert!(access::stamp_keyboard(&physical, key).is_some());
        assert!(access::cursor_sequence_allowed(ai, key, 302));
        access::frame_closed(key);
    }

    #[test]
    fn keyboard_frontier_comparison_accepts_the_next_sequence_across_wrap() {
        let (key, human, _) = setup(1101);
        let (ai, mut keys) = ai_pair(1101, HidSourceKind::Ai);
        world(|w| w.keyboard_frontiers = (u32::MAX, u32::MAX));
        access::toggle_from_human(human, key).unwrap();
        assert!(access::bind_input(
            ai,
            keys.slot_id,
            access::authorize(key).unwrap(),
            1,
            7
        ));
        keys.seq = u32::MAX;
        assert!(access::stamp_keyboard(&keys, key).is_none());
        keys.seq = 0;
        assert!(access::stamp_keyboard(&keys, key).is_some());
        keys.kind = r::keyboard::KEYBOARD_OUTPUT_KIND_PHYSICAL;
        keys.seq = u32::MAX;
        assert!(access::stamp_keyboard(&keys, key).is_none());
        keys.seq = 0;
        assert!(access::stamp_keyboard(&keys, key).is_some());
        access::frame_closed(key);
    }

    #[test]
    fn generic_ai_raw_reports_emitted_before_admission_cannot_borrow_a_new_grant() {
        let (key, human, human_keys) = setup(1201);
        let (ai, mut logical) = ai_pair(1201, HidSourceKind::Ai);
        logical.seq = 40;
        let mut physical = logical;
        physical.kind = r::keyboard::KEYBOARD_OUTPUT_KIND_PHYSICAL;
        physical.seq = 80;
        assert!(access::stamp_keyboard(&logical, key).is_none());
        assert!(access::stamp_keyboard(&physical, key).is_none());
        assert!(!access::cursor_sequence_allowed(ai, key, 100));
        world(|w| {
            w.keyboard_frontiers = (40, 80);
            w.cursor_frontier = 100;
        });
        access::toggle_from_human(human, key).unwrap();
        assert!(access::stamp_keyboard(&logical, key).is_none());
        assert!(access::stamp_keyboard(&physical, key).is_none());
        assert!(!access::cursor_sequence_allowed(ai, key, 100));
        logical.seq = 41;
        physical.seq = 81;
        let old_logical = access::stamp_keyboard(&logical, key).unwrap();
        let old_physical = access::stamp_keyboard(&physical, key).unwrap();
        assert!(access::cursor_sequence_allowed(ai, key, 101));
        access::toggle_from_human(human, key).unwrap();
        world(|w| {
            w.keyboard_frontiers = (41, 81);
            w.cursor_frontier = 101;
        });
        access::toggle_from_human(human, key).unwrap();
        assert!(access::stamp_keyboard(&logical, key).is_none());
        assert!(access::stamp_keyboard(&physical, key).is_none());
        assert!(!access::cursor_sequence_allowed(ai, key, 101));
        assert!(!access::stamp_valid(old_logical, key));
        assert!(!access::stamp_valid(old_physical, key));
        logical.seq = 42;
        physical.seq = 82;
        assert!(access::stamp_keyboard(&logical, key).is_some());
        assert!(access::stamp_keyboard(&physical, key).is_some());
        assert!(access::cursor_sequence_allowed(ai, key, 102));
        // Local physical input retains the desktop despite AI sequence floors.
        assert!(access::stamp_keyboard(&human_keys, key).is_some());
        assert!(access::cursor_sequence_allowed(human, key, 0));
        access::frame_closed(key);
    }
}
