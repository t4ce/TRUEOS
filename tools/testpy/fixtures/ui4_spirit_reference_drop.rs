#![allow(dead_code)]
extern crate alloc;
extern crate self as v;
use std::cell::RefCell;

pub mod vdrag { pub const PATH_LIST_V1: u32 = 1; }
#[derive(Default)]
struct World {
    current: Option<r::services::codex_input_service::Target>,
    live: Option<r::services::codex_input_service::Target>,
    marker: Option<(u32, u32, i32, i32, u32, u32)>,
    circle: Option<ui4::input_broker::Ui4SpiritDockCircle>,
    denied: bool,
    human: bool,
    epoch: u64,
    notifications: usize,
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<R>(f: impl FnOnce(&mut World) -> R) -> R { WORLD.with(|w| f(&mut w.borrow_mut())) }
mod r { pub mod services {
    pub mod codex_input_service {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Target(pub u64);
        pub fn current_target() -> Option<Target> { crate::world(|w| w.current) }
        pub fn is_target_live(target: &Target) -> bool { crate::world(|w| w.live == Some(*target)) }
    }
    pub mod microfont_mask_service {
        pub fn copy_text_mask(pixels: &mut [u8], _: usize, _: usize, _: usize, _: usize, _: &str, _: usize, _: usize) -> Result<(), ()> {
            pixels.fill(128); Ok(())
        }
    }
}}
mod spirit {
    pub fn marker_anchor() -> Option<(u32, u32, i32, i32, u32, u32)> { crate::world(|w| w.marker) }
}
mod shell3 { pub mod tui { pub fn vm_for_window(_: crate::ui4::WindowId) -> Option<u8> { None } } }
mod ui4 {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum WindowOwner { Vm(u8), Service(u8) }
    impl WindowOwner {
        pub const SHELL3_SERVICE: Self = Self::Service(1);
        pub const CODEX_PROMPT_SERVICE: Self = Self::Service(2);
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowId(u32);
    impl WindowId { pub fn from_raw(value: u32) -> Option<Self> { (value != 0).then_some(Self(value)) } }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct CursorFrameKey { pub owner: WindowOwner, pub window: WindowId }
    impl CursorFrameKey { pub fn new(owner: WindowOwner, window: WindowId) -> Self { Self { owner, window } } }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Ui4CursorSource { pub controller_id: u32, pub slot_id: u32, pub ep_target: u32, pub hid_kind: u8 }
    #[derive(Clone, Copy)] pub struct Placement { pub x: i32, pub y: i32 }
    #[derive(Clone, Copy)] pub struct WindowSnapshot { pub owner: WindowOwner, pub id: WindowId, pub presentation_placement: Placement }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum WindowDockTarget { SpiritAccess }
    pub mod spirit_access {
        #[derive(Clone, Copy)] pub enum InputStamp { Human, Ai(u64) }
        pub fn cursor_is_human(_: super::Ui4CursorSource) -> bool { crate::world(|w| w.human) }
        pub fn stamp_cursor(_: super::Ui4CursorSource, _: super::CursorFrameKey) -> Option<InputStamp> {
            crate::world(|w| (!w.denied).then_some(if w.human { InputStamp::Human } else { InputStamp::Ai(w.epoch) }))
        }
        pub fn stamp_valid(stamp: InputStamp, _: super::CursorFrameKey) -> bool {
            crate::world(|w| !w.denied && match stamp { InputStamp::Human => w.human, InputStamp::Ai(epoch) => epoch == w.epoch })
        }
        pub fn cursor_target_allowed(source: super::Ui4CursorSource, frame: super::CursorFrameKey) -> bool { stamp_cursor(source, frame).is_some() }
    }
    #[path="@DRAG_DROP@"] pub mod drag_drop;
    pub fn reference_drag(source: Ui4CursorSource) -> bool { drag_drop::codex_reference_drag(source) }
    pub mod context_menu {
        pub fn registered_request(_: super::WindowOwner, _: super::WindowId) -> Option<()> { None }
        pub fn open(_: super::Ui4CursorSource, _: super::WindowOwner, _: super::WindowId, _: (u32,u32), _: u32, _: ()) -> Result<(), ()> { Ok(()) }
    }
    pub mod codex_prompt {
        use alloc::{string::String, sync::Arc, vec::Vec};
        use core::sync::atomic::{AtomicBool, Ordering};
        use spin::Mutex;
        use super::Ui4CursorSource;
        use crate::r::services::codex_input_service as codex;
        #[path="@ATTACHMENTS@"] mod attachments;
        struct OpenRequest {
            source: Ui4CursorSource, anchor: (u32, u32), target: codex::Target,
            paths: Vec<String>, previews: Vec<(Arc<super::drag_drop::DragPreview>, (u32,u32))>,
        }
        static OPEN_REQUEST: Mutex<Option<OpenRequest>> = Mutex::new(None);
        static SUBMITTING: AtomicBool = AtomicBool::new(false);
        static ACTIVE_TARGET: Mutex<Option<codex::Target>> = Mutex::new(None);
        static RESERVED_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());
        struct Signal;
        impl Signal { fn signal(&self, _: ()) {} }
        static OPEN_SIGNAL: Signal = Signal;
        @REQUEST_ATTACHMENTS@
        pub fn reset() { *OPEN_REQUEST.lock() = None; *ACTIVE_TARGET.lock() = None; RESERVED_PATHS.lock().clear(); SUBMITTING.store(false, Ordering::Release); }
        pub fn pending() -> Option<(codex::Target, Vec<String>)> { OPEN_REQUEST.lock().as_ref().map(|r| (r.target, r.paths.clone())) }
        pub fn retain(target: codex::Target) { *ACTIVE_TARGET.lock() = Some(target); }
    }
    pub mod input_broker {
        use super::*;
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Ui4SpiritDockCircle { pub center_x: i32, pub center_y: i32, pub radius: u32 }
        struct WindowTarget(CursorFrameKey);
        impl From<WindowSnapshot> for WindowTarget { fn from(window: WindowSnapshot) -> Self { Self(CursorFrameKey::new(window.owner, window.id)) } }
        impl WindowTarget { fn cursor_frame_key(self) -> CursorFrameKey { self.0 } }
        @CIRCLE_FUNCTIONS@
        fn dock_zones(_: u32, _: u32) -> [();0] { [] }
        fn dock_target_at_in_zones(_: u32, _: u32, _: &[()]) -> Option<WindowDockTarget> { None }
        pub fn notify_slot4_visual_change() { crate::world(|w| w.notifications += 1); }
        pub fn claim_drag_gesture(_: Ui4CursorSource) { let circle = spirit_dock_circle(); crate::world(|w| w.circle = circle); }
        struct Route {
            secondary_anchor: Option<(u32,u32)>, secondary_dragged: bool,
            suppress_context_menu_open: bool, context_menu: Option<(u32,u32)>,
            spirit_dock_circle: Option<Ui4SpiritDockCircle>,
        }
        impl Route { fn color(&self) -> u32 { 0 } }
        struct Router { cursors: Vec<Route> }
        impl Router {
            fn secondary_release(&mut self, source: Ui4CursorSource, x: u32, y: u32) -> Option<WindowDockTarget> {
                let index = 0;
                let secondary_released = true;
                let secondary_drop = true;
                let hit: Option<WindowSnapshot> = None;
                @SECONDARY_RELEASE@
                dock_target_at(x, y, 1920, 1080, self.cursors[index].spirit_dock_circle)
            }
            fn primary_release(&mut self, source: Ui4CursorSource, x: u32, y: u32) {
                const PRIMARY_BUTTON_MASK: u32 = 1;
                let released = PRIMARY_BUTTON_MASK;
                let index = 0;
                let hit: Option<WindowSnapshot> = None;
                @PRIMARY_RELEASE@
            }
        }
        fn router() -> Router {
            Router { cursors: vec![Route {
                secondary_anchor: Some((10,10)), secondary_dragged: true,
                suppress_context_menu_open: false, context_menu: None,
                spirit_dock_circle: crate::world(|w| w.circle),
            }] }
        }
        pub fn release_secondary(source: Ui4CursorSource, x: u32, y: u32) -> Option<WindowDockTarget> { router().secondary_release(source,x,y) }
        pub fn release_primary(source: Ui4CursorSource, x: u32, y: u32) { router().primary_release(source,x,y); }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    use r::services::codex_input_service::Target;
    use ui4::{CursorFrameKey, Ui4CursorSource, WindowId, WindowOwner, codex_prompt, drag_drop, input_broker};
    fn source(id: u32) -> Ui4CursorSource { Ui4CursorSource { controller_id: id, slot_id: 1, ep_target: 1, hid_kind: 1 } }
    fn frame(id: u32) -> CursorFrameKey { CursorFrameKey::new(WindowOwner::Vm(id as u8), WindowId::from_raw(id).unwrap()) }
    fn reset() {
        drag_drop::release_owner(WindowOwner::Vm(20));
        codex_prompt::reset();
        world(|w| *w = World { current: Some(Target(7)), live: Some(Target(7)), marker: Some((0,0,400,400,64,64)), human: true, epoch: 1, ..World::default() });
    }
    fn paths(names: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for name in names { bytes.extend_from_slice(&(name.len() as u32).to_le_bytes()); bytes.extend_from_slice(name.as_bytes()); }
        bytes
    }
    fn begin() -> i32 {
        drag_drop::begin(WindowOwner::Vm(20),frame(20),source(20),vdrag::PATH_LIST_V1,"files",&paths(&["2:/apps/common/report.csv","2:/apps/common/data folder/"])).unwrap()
    }
    #[test]
    fn circle_requires_a_live_codex_and_uses_the_spirit_plane_center() {
        reset();
        let circle = input_broker::spirit_dock_circle().unwrap();
        assert_eq!((circle.center_x,circle.center_y,circle.radius),(432,432,150));
        world(|w| w.live = None);
        assert!(input_broker::spirit_dock_circle().is_none());
        world(|w| { w.live = Some(Target(7)); w.current = None; });
        assert!(input_broker::spirit_dock_circle().is_none());
    }
    #[test]
    fn actual_secondary_release_preserves_the_circle_until_hit_testing() {
        reset();
        input_broker::claim_drag_gesture(source(20));
        assert_eq!(input_broker::release_secondary(source(20),581,432),Some(ui4::WindowDockTarget::SpiritAccess));
    }
    #[test]
    fn termdir_paths_dropped_inside_circle_outside_sprite_reach_existing_prompt_once() {
        reset();
        let token = begin();
        assert!(ui4::reference_drag(source(20)));
        // 149px from center lies well outside the 64px Spirit sprite.
        input_broker::release_primary(source(20),581,432);
        assert_eq!(codex_prompt::pending(),Some((Target(7),vec!["2:/apps/common/report.csv".into(),"2:/apps/common/data folder/".into()])));
        assert!(drag_drop::cancel(WindowOwner::Vm(20),token),"external acceptance suppresses the producer's local action");
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
    }
    #[test]
    fn stopped_or_replaced_codex_cannot_receive_an_older_drag() {
        reset();
        let token = begin();
        world(|w| { w.live = None; w.current = None; });
        assert!(!ui4::reference_drag(source(20)));
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
        world(|w| { w.live = Some(Target(8)); w.current = Some(Target(8)); });
        assert!(!ui4::reference_drag(source(20)));
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
        assert!(codex_prompt::pending().is_none());
        assert!(!drag_drop::cancel(WindowOwner::Vm(20),token));
    }
    #[test]
    fn retained_other_conversation_and_nonpath_payloads_are_not_consumed() {
        reset();
        let token = begin();
        codex_prompt::retain(Target(99));
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
        assert!(codex_prompt::pending().is_none());
        assert!(!drag_drop::cancel(WindowOwner::Vm(20),token));
        let token = drag_drop::begin(WindowOwner::Vm(20),frame(20),source(20),99,"opaque",b"arbitrary").unwrap();
        assert!(!ui4::reference_drag(source(20)));
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
        assert!(!drag_drop::cancel(WindowOwner::Vm(20),token));
    }
    #[test]
    fn denied_ai_cannot_begin_and_admitted_ai_has_no_human_attachment_circle() {
        reset();
        world(|w| { w.human = false; w.denied = true; });
        assert_eq!(drag_drop::begin(WindowOwner::Vm(20),frame(20),source(20),vdrag::PATH_LIST_V1,"path",&paths(&["/x"])),Err(-5));
        world(|w| w.denied = false);
        let token = begin();
        assert!(!ui4::reference_drag(source(20)));
        assert!(!drag_drop::release_to_codex(source(20),(581,432)));
        assert!(codex_prompt::pending().is_none());
        assert!(!drag_drop::cancel(WindowOwner::Vm(20),token));
    }
    #[test]
    fn ordinary_local_drop_keeps_the_original_payload_and_local_action() {
        reset();
        let token = begin();
        drag_drop::pointer_released(source(20),Some((WindowOwner::Vm(20),frame(20),10,20)));
        assert!(codex_prompt::pending().is_none());
        assert!(!drag_drop::cancel(WindowOwner::Vm(20),token));
    }
}
