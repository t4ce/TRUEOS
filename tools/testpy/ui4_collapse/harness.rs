#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_vm;
pub mod vmcall {
    pub const OP_BP_UI4_SCENE_ENABLE_COLLAPSE_V1: u32 = 0x233;
}
mod hv {
    pub fn vm_run_generation(_: u8) -> Option<u64> {
        Some(42)
    }
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        None
    }
}
mod shell3 {
    pub mod tui {
        use alloc::vec::Vec;
        #[derive(Clone, Copy, PartialEq, Eq)]
        struct Owner {
            vm: u8,
            run: u64,
        }
        struct Route {
            selected: bool,
            owner: Option<Owner>,
            frontend: Option<u64>,
        }
        struct Registry {
            routes: Vec<Route>,
            ui4_windows: Vec<(u64, crate::ui4::WindowId)>,
            remote_frontends: Vec<(u64, ())>,
        }
        static ROUTES: spin::Mutex<Registry> = spin::Mutex::new(Registry {
            routes: Vec::new(),
            ui4_windows: Vec::new(),
            remote_frontends: Vec::new(),
        });
        include!("shell_binding.rs");
        pub fn set_window(window: u32) {
            let mut routes = ROUTES.lock();
            if routes.routes.is_empty() {
                routes.routes.push(Route {
                    selected: false,
                    owner: Some(Owner { vm: 1, run: 42 }),
                    frontend: Some(1),
                });
            }
            routes.routes[0].selected = window != 0;
            drop(routes);
            if window != 0 {
                bind_ui4_window(1, crate::ui4::WindowId(window));
            }
        }
    }
}

mod ui4 {
    pub const MAX_COLLAPSE_IMAGE_SIZE: u32 = 256;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct WindowId(pub u32);
    impl WindowId {
        fn raw(self) -> u32 {
            self.0
        }
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum WindowOwner {
        Vm(u8),
        Shell,
    }
    impl WindowOwner {
        pub const SHELL3_SERVICE: Self = Self::Shell;
    }
    pub fn enable_window_collapse(
        _: WindowOwner,
        _window: WindowId,
        _: u32,
        _: u32,
        _: usize,
        _: &[u8],
    ) -> Result<(), ()> {
        window_broker::STATE.lock().insert(_window, false);
        Ok(())
    }
    pub mod window_broker {
        use super::*;
        pub static STATE: spin::Mutex<std::collections::BTreeMap<WindowId, bool>> =
            spin::Mutex::new(std::collections::BTreeMap::new());
        pub fn clear_collapse(_: WindowOwner, window: WindowId) {
            STATE.lock().remove(&window);
        }
        pub fn collapse_state(_: WindowOwner, window: WindowId) -> Option<bool> {
            STATE.lock().get(&window).copied()
        }
        pub fn toggle_collapse(_: WindowOwner, window: WindowId) -> Result<(), ()> {
            let mut state = STATE.lock();
            let value = state.get_mut(&window).unwrap();
            *value = !*value;
            Ok(())
        }
    }
    pub mod blueprint_ui4 {
        use super::*;
        const ERROR_INVALID: i32 = -1;
        const ERROR_CONTEXT: i32 = -2;
        const ERROR_NOT_FOUND: i32 = -3;
        const ERROR_STATE: i32 = -4;
        const ERROR_UI4: i32 = -7;
        struct Surface {
            render_target: u32,
            window: WindowId,
        }
        static SURFACES: spin::Mutex<()> = spin::Mutex::new(());
        fn surface_mut(_: &mut (), _: WindowOwner, _: u32) -> Option<&mut Surface> {
            None
        }
        fn blueprint_owner() -> Option<WindowOwner> {
            Some(WindowOwner::Vm(1))
        }
        fn guest_status(_: u32, _: u64, _: u64, _: &[u8]) -> i32 {
            unreachable!()
        }
        pub(crate) mod collapse_api {
            use super::*;
            include!("terminal.rs");
            #[test]
            fn terminal_upload_activation_and_lease_lifecycle() {
                crate::shell3::tui::set_window(7);
                let owner = WindowOwner::Vm(1);
                assert_eq!(begin_terminal(owner, 0, 1), ERROR_INVALID);
                assert_eq!(begin_terminal(owner, 257, 1), ERROR_INVALID);
                assert_eq!(begin_terminal(WindowOwner::Vm(2), 1, 1), ERROR_NOT_FOUND);
                assert_eq!(begin_terminal(owner, 2, 1), 0);
                assert_eq!(finish_terminal(owner), ERROR_INVALID);
                assert_eq!(write_terminal(owner, 4, &[1; 4]), ERROR_INVALID);
                assert_eq!(write_terminal(owner, 0, &[1; 3]), ERROR_INVALID);
                assert_eq!(write_terminal(owner, 0, &[1; 12]), ERROR_INVALID);
                assert_eq!(write_terminal(WindowOwner::Vm(2), 0, &[1; 4]), ERROR_STATE);
                assert_eq!(write_terminal(owner, 0, &[1; 4]), 0);
                assert_eq!(finish_terminal(owner), ERROR_INVALID);
                assert_eq!(write_terminal(owner, 4, &[1; 4]), 0);
                assert_eq!(finish_terminal(owner), 0);
                assert_eq!(collapse(owner, 0), 0);
                assert_eq!(collapse(owner, 0), 0);
                assert_eq!(
                    super::super::window_broker::STATE
                        .lock()
                        .get(&WindowId(7))
                        .copied(),
                    Some(true)
                );
                crate::shell3::tui::set_window(8);
                assert_eq!(collapse(owner, 0), 0);
                crate::shell3::tui::set_window(0);
                assert_eq!(collapse(owner, 0), ERROR_NOT_FOUND);
                release_terminal(owner);
                assert!(super::super::window_broker::STATE.lock().is_empty());
                assert!(TERMINAL_ICONS.lock().is_empty());
                crate::shell3::tui::set_window(7);
                assert_eq!(begin_terminal(owner, 1, 1), 0);
                crate::shell3::tui::set_window(8);
                assert_eq!(write_terminal(owner, 0, &[1; 4]), 0);
                assert_eq!(finish_terminal(owner), 0);
                assert_eq!(collapse(owner, 0), 0);
                release_terminal(owner);
                // The real launch path claims its lease before Shell3 selects
                // the app. Registration must survive that initial missing frame.
                crate::shell3::tui::set_window(0);
                assert_eq!(begin_terminal(owner, 1, 1), 0);
                assert_eq!(write_terminal(owner, 0, &[255; 4]), 0);
                assert_eq!(finish_terminal(owner), 0);
                assert!(super::super::window_broker::STATE.lock().is_empty());
                crate::shell3::tui::set_window(9);
                assert_eq!(collapse(owner, 0), 0);
                // Shell3 replaces its broker window without changing the lease.
                crate::shell3::tui::set_window(10);
                assert_eq!(collapse(owner, 0), 0);
                assert!(
                    !super::super::window_broker::STATE
                        .lock()
                        .contains_key(&WindowId(9))
                );
                release_terminal(owner);
                assert!(super::super::window_broker::STATE.lock().is_empty());
            }
        }
    }
}
