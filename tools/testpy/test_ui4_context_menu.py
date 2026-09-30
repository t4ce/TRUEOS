#!/usr/bin/env python3
"""Run the production context-menu state machine on the host, without a rig."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = root / "src/ui4/context_menu.rs"
harness = r'''
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[macro_export] macro_rules! log_info { ($($tokens:tt)*) => {} }
mod graphics { pub mod primitives {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Rgba8;
} }
mod ui4 {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum WindowOwner { Vm(u8) }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowId(u32);
    impl WindowId { pub fn raw(self) -> u32 { self.0 } }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Ui4CursorSource { pub controller_id:u32, pub slot_id:u32, pub ep_target:u32, pub hid_kind:u8 }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Ui4VisualRect { pub x:u32, pub y:u32, pub width:u32, pub height:u32 }
    mod input_broker {
        pub const CONTEXT_MENU_WIDTH_PX:u32 = 180;
        pub const CONTEXT_MENU_ROW_HEIGHT_PX:u32 = 24;
        pub fn notify_slot4_visual_change() {}
    }
    mod window_broker {
        pub struct Placement { pub x:i32, pub y:i32 }
        pub struct Snapshot { pub presentation_placement:Placement }
        pub fn window_snapshot(_:super::WindowOwner, _:super::WindowId)->Option<Snapshot> {
            Some(Snapshot { presentation_placement:Placement {x:20,y:30} })
        }
    }
    #[path = "PRODUCTION_SOURCE"] mod context_menu;
    #[cfg(test)] mod lifetime_tests {
        use super::*;
        use context_menu::*;
        static EVENTS: crate::Mutex<Vec<ContextMenuResult>> = crate::Mutex::new(Vec::new());
        fn complete(result:ContextMenuResult) { EVENTS.lock().push(result); }
        fn take_prepare() -> u64 {
            dispatch_pending_callbacks();
            let event = EVENTS.lock().pop().unwrap();
            assert_eq!(event.reason, ContextMenuCloseReason::Prepare);
            assert_eq!(event.local, (100,100));
            event.serial
        }
        #[test]
        fn dynamic_invocations_are_owned_frozen_and_cannot_be_resurrected() {
            let owner = WindowOwner::Vm(1);
            let window = WindowId(1);
            let cursor = Ui4CursorSource { controller_id:1, slot_id:2, ep_target:3, hid_kind:0 };
            let color = crate::graphics::primitives::Rgba8;
            register_dynamic_window_menu(owner, window, complete).unwrap();
            let open_next = || open(cursor,owner,window,(120,130),color,registered_request(owner,window).unwrap()).unwrap();
            open_next();
            assert!(visual().is_none(), "preparation never paints placeholder rows");
            let serial = take_prepare();
            let entries = || vec![ContextMenuEntry::action("OPEN IMG",7), ContextMenuEntry::disabled("disabled")];
            assert_eq!(resolve_window_menu(WindowOwner::Vm(2),window,serial,entries()), Err(ContextMenuError::NotFocused));
            assert_eq!(resolve_window_menu(owner,WindowId(2),serial,entries()), Err(ContextMenuError::NotFocused));
            resolve_window_menu(owner,window,serial,entries()).unwrap();
            assert_eq!(resolve_window_menu(owner,window,serial,entries()), Err(ContextMenuError::NotFocused), "reply is one-shot");
            let menu = visual().unwrap();
            assert_eq!(menu.entries[0].label,"OPEN IMG");
            let row = entry_rect(menu_rect(menu.anchor,2,1000,1000),0).unwrap();
            let gesture = pointer_down(cursor,row.x+2,row.y+2,1000,1000).unwrap();
            pointer_up(cursor,gesture,row.x+2,row.y+2,1000,1000);
            dispatch_pending_callbacks();
            let selected = EVENTS.lock().pop().unwrap();
            assert_eq!(selected.serial,serial);
            assert_eq!(selected.local,(100,100));
            assert_eq!(selected.selected_action,Some(7));
            assert!(visual().is_none());
            assert!(registered_request(owner,window).is_some(), "standing claim survives selection");

            open_next();
            let dismissed = take_prepare();
            assert_eq!(pointer_down(cursor,0,0,1000,1000),None);
            assert_eq!(resolve_window_menu(owner,window,dismissed,entries()),Err(ContextMenuError::NotFocused));
            dispatch_pending_callbacks();
            assert_eq!(EVENTS.lock().pop().unwrap().reason,ContextMenuCloseReason::Dismissed);

            open_next();
            let replaced = take_prepare();
            open_next();
            let current = take_prepare();
            assert_ne!(replaced,current);
            assert_eq!(resolve_window_menu(owner,window,replaced,entries()),Err(ContextMenuError::NotFocused));
            assert!(dismiss_window(owner,window));
            assert!(registered_request(owner,window).is_none());
            assert_eq!(resolve_window_menu(owner,window,current,entries()),Err(ContextMenuError::NotFocused));

            register_dynamic_window_menu(owner,window,complete).unwrap();
            open_next();
            let released = take_prepare();
            release_owner(owner);
            assert!(registered_request(owner,window).is_none());
            assert_eq!(resolve_window_menu(owner,window,released,entries()),Err(ContextMenuError::NotFocused));

            register_window_menu(owner,window,42,entries(),complete).unwrap();
            open_next();
            assert!(visual().is_some(), "legacy standing menus need no preparation");
            assert!(cancel_window(owner,window));
            clear_window_menu(owner,window);
            assert!(registered_request(owner,window).is_none());
        }
    }
}
'''.replace("PRODUCTION_SOURCE", str(source))
with tempfile.TemporaryDirectory(prefix="trueos-context-menu-") as directory:
    directory = Path(directory)
    test = directory / "test.rs"
    test.write_text(harness)
    subprocess.run(["rustc", "--edition=2024", "-A", "warnings", "--test", str(test), "-o", str(directory / "test")], check=True)
    subprocess.run([str(directory / "test")], check=True)
