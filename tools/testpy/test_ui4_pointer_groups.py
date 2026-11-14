#!/usr/bin/env python3
"""Exercise production UI4 pointer promotion and retirement without hardware.

Extract the actual frame rig and broker methods; only hardware metadata,
presentation wakeups and unrelated interaction types are fixtures. In
particular, the retirement dispatch comes from process_cursor itself.
"""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item
from test_mouse_visual_cursors import block

ROOT = Path(__file__).resolve().parents[2]
FRAME = "src/ui4/cursor_frame_inout.rs"
BROKER = "src/ui4/input_broker.rs"

PRELUDE = r'''
#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
mod shell3 {pub mod tui {
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct RemoteInputAuthority {pub account:u64,pub epoch:u64}
}}
#[macro_export] macro_rules! log_info { ($($token:tt)*) => {} }
mod graphics { pub mod primitives {
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct Rgba8(u8,u8,u8,u8);
 impl Rgba8 {pub const fn new(r:u8,g:u8,b:u8,a:u8)->Self{Self(r,g,b,a)}}
}}
mod r {
 pub mod cursor {pub const HID_KIND_MOUSE:u8=1;}
 pub mod keyboard {
  #[derive(Clone,Copy,Default)] pub struct TrueosKeyboardOutputEvent {
   pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,
  }
 }
}
mod usb2 {pub mod hid {
 pub const HID_CURSOR_EVENT_FLAG_DEVICE_LOST:u32=1<<17;
 pub const HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED:u32=1<<18;
 #[derive(Clone,Copy,Default)] pub struct TrueosHidCursorEvent {
  pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,pub hid_kind:u8,pub flags:u32,
 }
 pub mod hut {
  #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct HidKeyboardState {
   pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,
  }
  pub fn keyboards_snapshot()->Vec<HidKeyboardState>{
   vec![HidKeyboardState{controller_id:2,slot_id:200,ep_target:1}]
  }
 }
}}
mod ui4 {
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub enum WindowOwner {KernelApp(u64)}
 pub type OutputId=u32;
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct WindowId(u64);
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct WindowSessionId(u64);
 pub struct WindowState;
 pub type WindowDockTarget=();
 pub mod collapse {pub type RestorePress=();}
 pub mod window_broker {pub const MAX_WINDOWS:usize=32;}
 pub mod drag_drop {pub fn cursor_retired(_:super::cursor_frame_inout::Ui4CursorSource){}}
 pub mod context_menu {pub fn dismiss_for_source(_:super::cursor_frame_inout::Ui4CursorSource){}}
 pub mod spirit_access {
  pub fn cursor_remote_authority(s:super::cursor_frame_inout::Ui4CursorSource)->Option<crate::shell3::tui::RemoteInputAuthority> {
   super::input_broker::remote_authority(s)
  }
  pub fn cursor_is_human(s:super::cursor_frame_inout::Ui4CursorSource)->bool {
   super::input_broker::metadata(s).2
  }
  pub fn cursor_target_allowed(_:super::cursor_frame_inout::Ui4CursorSource,_:super::cursor_frame_inout::CursorFrameKey)->bool{true}
  pub fn keyboard_state_target_allowed(_: &crate::usb2::hid::hut::HidKeyboardState,_:super::cursor_frame_inout::CursorFrameKey)->bool{true}
 }
 pub(crate) use cursor_frame_inout::{CursorFrameKey,selected_frame,selected_frame_for_source};
 pub mod cursor_frame_inout {
'''

FRAME_FIXTURE = r'''
static CURSOR_FRAME_RIG: Mutex<CursorFrameRig> = Mutex::new(CursorFrameRig::new());
fn signal_visual_change() {}
#[cfg(test)] pub(super) fn reset() {*CURSOR_FRAME_RIG.lock()=CursorFrameRig::new();}
#[cfg(test)] pub(super) fn select_for_test(source:Ui4CursorSource,window:u64,icon:Ui4CursorIcon) {
 let key=CursorFrameKey::new(super::WindowOwner::KernelApp(1),super::WindowId(window));
 let mut rig=CURSOR_FRAME_RIG.lock();
 rig.frame_opened(key,super::WindowSessionId(1)).unwrap();
 rig.select(Some(key),source,crate::graphics::primitives::Rgba8::new(1,2,3,255));
 rig.set_cursor(key,Some(source),icon).unwrap();
}
#[cfg(test)] pub(super) fn icon_for_test(source:Ui4CursorSource)->Option<Ui4CursorIcon> {
 CURSOR_FRAME_RIG.lock().cursor_presentation_for_source(source).map(|(_,icon,_)|icon)
}
#[cfg(test)] mod retarget_tests {
 use super::*;
 fn source(slot:u32)->Ui4CursorSource {Ui4CursorSource{controller_id:1,slot_id:slot,ep_target:1,hid_kind:1}}
 fn rig()->(CursorFrameRig,CursorFrameKey) {
  let mut rig=CursorFrameRig::new();
  let key=CursorFrameKey::new(super::super::WindowOwner::KernelApp(1),super::super::WindowId(1));
  rig.frame_opened(key,super::super::WindowSessionId(1)).unwrap();(rig,key)
 }
 #[test] fn override_only_retarget_reports_change_and_keeps_the_next_selection_icon() {
  let (mut rig,key)=rig();let old=source(1);let next=source(2);
  rig.set_cursor(key,Some(old),Ui4CursorIcon::Loading).unwrap();
  assert!(rig.retarget_source(old,next));
  assert!(!rig.retarget_source(old,next));
  rig.select(Some(key),next,crate::graphics::primitives::Rgba8::new(1,2,3,255));
  assert_eq!(rig.cursor_icon(key,next),Ui4CursorIcon::Loading);
  assert_eq!(rig.frames[0].overrides.len(),1);
 }
 #[test] fn override_collision_keeps_destination_presentation_and_reports_old_removal() {
  let (mut rig,key)=rig();let old=source(1);let next=source(2);
  rig.set_cursor(key,Some(old),Ui4CursorIcon::Loading).unwrap();
  rig.set_cursor(key,Some(next),Ui4CursorIcon::ResizeHorizontal).unwrap();
  assert!(rig.retarget_source(old,next));
  assert_eq!(rig.frames[0].overrides.len(),1);
  assert_eq!(rig.frames[0].overrides[0].source,next);
  rig.select(Some(key),next,crate::graphics::primitives::Rgba8::new(1,2,3,255));
  assert_eq!(rig.cursor_icon(key,next),Ui4CursorIcon::ResizeHorizontal);
 }
 #[test] fn parking_releases_selection_but_retains_cursor_registration_for_restore() {
  let (mut rig,key)=rig();let first=source(1);let second=source(2);
  rig.set_cursor(key,Some(first),Ui4CursorIcon::Loading).unwrap();
  let image=Arc::new(cursor_image_from_rgba(1,1,0,0,&[50,80,120,255]));
  rig.register_cursor_image(key,1,Arc::clone(&image)).unwrap();
  rig.select_cursor_image(key,1).unwrap();
  rig.set_center_snapped_mouse(key,true).unwrap();
  rig.select(Some(key),first,crate::graphics::primitives::Rgba8::new(1,2,3,255));
  rig.select(Some(key),second,crate::graphics::primitives::Rgba8::new(4,5,6,255));
  assert!(rig.frame_parked(key));
  assert!(rig.selected.is_none());assert!(rig.selecting_cursors.is_empty());
  assert_eq!(rig.frames.len(),1);assert_eq!(rig.frames[0].images.len(),1);
  assert_eq!(rig.frames[0].selected_image,Some(1));
  assert!(rig.frames[0].center_snapped_mouse);
  assert!(Arc::ptr_eq(&rig.frames[0].images[0].image,&image));
  assert!(!rig.frame_parked(key),"parking is idempotent");
  rig.select(Some(key),first,crate::graphics::primitives::Rgba8::new(1,2,3,255));
  assert_eq!(rig.cursor_icon(key,first),Ui4CursorIcon::Loading);
  assert_eq!(rig.center_snapped_frame_for_source(first),Some(key));
 }
 #[test] fn parking_preserves_other_windows_focus_and_cursor_ownership() {
  let (mut rig,key)=rig();let first=source(1);let second=source(2);
  let other=CursorFrameKey::new(key.owner,super::super::WindowId(2));
  rig.frame_opened(other,super::super::WindowSessionId(1)).unwrap();
  rig.select(Some(key),first,crate::graphics::primitives::Rgba8::new(1,2,3,255));
  rig.select(Some(other),second,crate::graphics::primitives::Rgba8::new(4,5,6,255));
  assert!(rig.frame_parked(key));
  assert_eq!(rig.selected,Some(other));
  assert_eq!(rig.selected_frame_for_source(first),None);
  assert_eq!(rig.selected_frame_for_source(second),Some(other));
  assert_eq!(rig.frames.len(),2);
 }
}
'''

BROKER_FIXTURE = r'''
 }
 pub mod input_broker {
 use heapless::Vec;
 use super::{WindowOwner,WindowId,CursorFrameKey};
 use super::cursor_frame_inout::Ui4CursorSource;
 const MAX_OWNER_EVENTS:usize=32;
 const MAX_CURSOR_ROUTES:usize=32;
 type Ui4KeyboardEvent=u32;
 type WindowPlacement=();
 type Ui4SpiritDockCircle=();
 type Ui4DockPreview=();
 type DesktopContextMenuAction=();
 #[derive(Clone,Copy)] struct QuadResizeGesture {source:Ui4CursorSource}
 thread_local! {static HUT:std::cell::RefCell<std::vec::Vec<(Ui4CursorSource,u32,bool,bool)>>=const{std::cell::RefCell::new(std::vec::Vec::new())};}
 thread_local! {static REMOTE:std::cell::RefCell<std::vec::Vec<(Ui4CursorSource,crate::shell3::tui::RemoteInputAuthority)>>=const{std::cell::RefCell::new(std::vec::Vec::new())};}
 pub(super) fn remote_authority(s:Ui4CursorSource)->Option<crate::shell3::tui::RemoteInputAuthority> {
  REMOTE.with(|remote|remote.borrow().iter().find(|(source,_)|*source==s).map(|(_,authority)|*authority))
 }
 pub(super) fn metadata(s:Ui4CursorSource)->(u32,bool,bool) {
  HUT.with(|hut|hut.borrow().iter().find(|(source,_,_,_)|*source==s).map(|(_,c,v,h)|(*c,*v,*h)).unwrap_or((0,false,false)))
 }
 fn cursor_hut_metadata(s:Ui4CursorSource)->(u32,bool) {let(c,v,_)=metadata(s);(c,v)}
 fn window_snapshot_for_target(_:WindowTarget)->Option<()>{Some(())}
'''

TESTS = r'''
#[cfg(test)] mod tests {
 use super::*;
 use super::super::cursor_frame_inout::{self as frames,Ui4CursorIcon};
 static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
 fn source(slot:u32)->Ui4CursorSource {Ui4CursorSource{controller_id:1,slot_id:slot,ep_target:1,hid_kind:1}}
 fn hut(s:Ui4CursorSource,combo:u32,virtual_:bool,human:bool) {
  HUT.with(|hut| {let mut hut=hut.borrow_mut();hut.retain(|(id,_,_,_)|*id!=s);hut.push((s,combo,virtual_,human));});
 }
 fn reset()->std::sync::MutexGuard<'static,()> {
  let lock=SERIAL.lock().unwrap_or_else(|error|error.into_inner());
  frames::reset();HUT.with(|hut|hut.borrow_mut().clear());REMOTE.with(|remote|remote.borrow_mut().clear());lock
 }
 fn keyboard()->KeyboardSource {KeyboardSource{controller_id:2,slot_id:200,ep_target:1}}
 fn event(s:Ui4CursorSource,flags:u32)->crate::usb2::hid::TrueosHidCursorEvent {
  crate::usb2::hid::TrueosHidCursorEvent{controller_id:s.controller_id,slot_id:s.slot_id,ep_target:s.ep_target,hid_kind:s.hid_kind,flags}
 }
 fn frame(window:u64)->CursorFrameKey {CursorFrameKey::new(WindowOwner::KernelApp(1),WindowId(window))}

 #[test] fn promotion_preserves_human_selection_focus_keyboard_and_ai_cursor() {
  let _lock=reset();let old=source(1);let next=source(2);let ai=source(9);
  hut(old,7,false,true);hut(ai,8,true,false);
  let mut broker=InputBroker::new();
  let human=broker.cursor_index(old,37,81);let ai_index=broker.cursor_index(ai,91,18);
  broker.cursors[human].selection_serial=41;
  broker.cursors[human].keyboard_source=Some(keyboard());
  broker.cursors[human].capture=Some(WindowTarget::from(frame(1)));
  broker.cursors[human].context_menu=Some((3,4));
  broker.cursors[ai_index].selection_serial=99;
  frames::select_for_test(ai,2,Ui4CursorIcon::ResizeHorizontal);
  frames::select_for_test(old,1,Ui4CursorIcon::Loading);
  HUT.with(|hut|hut.borrow_mut().retain(|(id,_,_,_)|*id!=old));hut(next,7,false,true);
  assert_eq!(broker.cursor_index(next,0,0),human);
  assert_eq!(broker.cursors.len(),2);
  let route=&broker.cursors[human];
  assert_eq!((route.x,route.y,route.selection_serial),(37,81,41));
  assert_eq!(route.keyboard_source,Some(keyboard()));
  assert_eq!(route.capture,None);assert_eq!(route.context_menu,None);
  assert_eq!(frames::selected_frame_for_source(old),None);
  assert_eq!(frames::selected_frame_for_source(next),Some(frame(1)));
  assert_eq!(frames::selected_frame(),Some(frame(1)));
  assert_eq!(frames::icon_for_test(next),Some(Ui4CursorIcon::Loading));
  assert_eq!(frames::selected_frame_for_source(ai),Some(frame(2)));
  assert_eq!(frames::icon_for_test(ai),Some(Ui4CursorIcon::ResizeHorizontal));
  assert!(broker.focused_keyboard_state(WindowTarget::from(frame(1))).is_some());
  let key=crate::r::keyboard::TrueosKeyboardOutputEvent::default();
  assert_eq!(broker.keyboard_route_index(&key,7,true),Some(human));
  assert_eq!(broker.keyboard_route_index(&key,8,false),Some(ai_index));
 }

 #[test] fn split_promotion_before_reused_source_preserves_old_group_selection() {
  let _lock=reset();let mouse=source(1);let pad=source(2);
  hut(mouse,1,false,true);let mut broker=InputBroker::new();
  let original=broker.cursor_index(mouse,50,60);
  frames::select_for_test(mouse,1,Ui4CursorIcon::Loading);
  hut(mouse,2,false,true);hut(pad,1,false,true);
  // Producer order: surviving old persona first, then newly split persona.
  assert_eq!(broker.cursor_index(pad,50,60),original);
  let split=broker.cursor_index(mouse,50,60);
  assert_ne!(split,original);assert_eq!(broker.cursors.len(),2);
  assert_eq!(frames::selected_frame_for_source(pad),Some(frame(1)));
  assert_eq!(frames::selected_frame_for_source(mouse),None);
  assert_eq!(frames::selected_frame(),Some(frame(1)));
 }

 #[test] fn virtual_and_ai_pointers_never_retarget_a_human_group() {
  let _lock=reset();let human=source(1);let ai=source(2);let virtual_human=source(3);
  hut(human,7,false,true);hut(ai,7,false,false);hut(virtual_human,7,true,true);
  let mut broker=InputBroker::new();let first=broker.cursor_index(human,1,2);
  frames::select_for_test(human,1,Ui4CursorIcon::Loading);
  assert_ne!(broker.cursor_index(ai,3,4),first);
  assert_ne!(broker.cursor_index(virtual_human,5,6),first);
  assert_eq!(broker.cursors.len(),3);
  assert_eq!(frames::selected_frame_for_source(human),Some(frame(1)));
  assert_eq!(frames::selected_frame_for_source(ai),None);
  assert_eq!(frames::selected_frame_for_source(virtual_human),None);
 }

 #[test] fn group_retirement_keeps_same_slot_keyboard_and_other_endpoint() {
  let _lock=reset();let retiring=source(1);let mut other=retiring;other.ep_target=2;
  hut(retiring,7,false,true);hut(other,8,false,true);
  let mut broker=InputBroker::new();broker.cursor_index(retiring,1,2);
  let survivor=broker.cursor_index(other,3,4);
  let composite_key=KeyboardSource{controller_id:retiring.controller_id,slot_id:retiring.slot_id,ep_target:3};
  broker.cursors[survivor].keyboard_source=Some(composite_key);
  broker.quad_cancel_key=Some(composite_key);
  broker.physical_keys.push((WindowOwner::KernelApp(1),42)).unwrap();
  broker.quad_resize=Some(QuadResizeGesture{source:retiring});
  frames::select_for_test(retiring,1,Ui4CursorIcon::Loading);
  frames::select_for_test(other,2,Ui4CursorIcon::Default);
  broker.process_cursor(event(retiring,crate::usb2::hid::HID_CURSOR_EVENT_FLAG_POINTER_GROUP_RETIRED),0);
  assert_eq!(broker.cursors.len(),1);assert_eq!(broker.cursors[0].source,other);
  assert_eq!(broker.cursors[0].keyboard_source,Some(composite_key));
  assert_eq!(broker.quad_cancel_key,Some(composite_key));assert_eq!(broker.physical_keys.len(),1);
  assert!(broker.quad_resize.is_none());
  assert_eq!(frames::selected_frame_for_source(retiring),None);
  assert_eq!(frames::selected_frame_for_source(other),Some(frame(2)));
  assert_eq!(frames::selected_frame(),Some(frame(2)));
 }

 #[test] fn actual_device_loss_still_retires_all_pointer_endpoints_and_keyboard_cache() {
  let _lock=reset();let lost=source(1);let mut endpoint=lost;endpoint.ep_target=2;let surviving=source(9);
  hut(lost,7,false,true);hut(endpoint,8,false,true);hut(surviving,9,false,true);
  let mut broker=InputBroker::new();broker.cursor_index(lost,1,2);broker.cursor_index(endpoint,3,4);
  let survivor=broker.cursor_index(surviving,5,6);
  broker.cursors[survivor].keyboard_source=Some(KeyboardSource{controller_id:lost.controller_id,slot_id:lost.slot_id,ep_target:3});
  broker.process_cursor(event(lost,crate::usb2::hid::HID_CURSOR_EVENT_FLAG_DEVICE_LOST),0);
  assert_eq!(broker.cursors.len(),1);assert_eq!(broker.cursors[0].source,surviving);
  assert_eq!(broker.cursors[0].keyboard_source,None);
 }

 #[test] fn remote_session_change_clears_retained_gestures_but_same_token_preserves_them() {
  let _lock=reset();let remote=source(11);let other=source(12);
  let original=crate::shell3::tui::RemoteInputAuthority{account:0,epoch:1};
  REMOTE.with(|state|state.borrow_mut().push((remote,original)));
  let mut broker=InputBroker::new();let index=broker.cursor_index(remote,37,81);
  let other_index=broker.cursor_index(other,91,18);
  broker.cursors[other_index].buttons_down=2;
  broker.refresh_remote_input_route(index);
  let target=WindowTarget::from(frame(7));
  fn arm(broker:&mut InputBroker,index:usize,target:WindowTarget){
   let route=&mut broker.cursors[index];
   route.capture=Some(target);route.resize_latch=Some((target,99));
   route.collapse_drag=Some(target);route.collapse_press=Some((target,()));
   route.screenshot_latch=true;route.selection_anchor=Some((7,8));
   route.secondary_anchor=Some((9,10));route.secondary_dragged=true;
   route.context_menu=Some((11,12));route.context_menu_owns_gesture=true;
   route.requested_context_menu_gesture=Some(100);route.buttons_down=3;route.absorb_select=true;
   broker.quad_resize=Some(QuadResizeGesture{source:route.source});
  }
  arm(&mut broker,index,target);
  broker.refresh_remote_input_route(index);
  let route=&broker.cursors[index];
  assert_eq!(route.remote_input_authority,Some(original));
  assert_eq!(route.capture,Some(target));assert_eq!(route.resize_latch,Some((target,99)));
  assert_eq!(route.collapse_drag,Some(target));assert_eq!(route.collapse_press,Some((target,())));
  assert_eq!(route.buttons_down,3);assert!(route.screenshot_latch);assert!(route.absorb_select);
  assert_eq!(broker.quad_resize.unwrap().source,remote);
  let next=crate::shell3::tui::RemoteInputAuthority{epoch:2,..original};
  REMOTE.with(|state|state.borrow_mut()[0].1=next);
  broker.refresh_remote_input_route(index);
  let route=&broker.cursors[index];
  assert_eq!(route.remote_input_authority,Some(next));
  assert_eq!((route.x,route.y),(37,81));
  assert_eq!(route.capture,None);assert_eq!(route.resize_latch,None);
  assert_eq!(route.collapse_drag,None);assert_eq!(route.collapse_press,None);
  assert!(!route.screenshot_latch);assert_eq!(route.selection_anchor,None);
  assert_eq!(route.secondary_anchor,None);assert!(!route.secondary_dragged);
  assert_eq!(route.context_menu,None);assert!(!route.context_menu_owns_gesture);
  assert_eq!(route.requested_context_menu_gesture,None);
  assert_eq!(route.buttons_down,0);assert!(!route.absorb_select);assert!(broker.quad_resize.is_none());
  assert_eq!(broker.cursors[other_index].buttons_down,2);
  arm(&mut broker,index,target);
  REMOTE.with(|state|state.borrow_mut().clear());
  broker.refresh_remote_input_route(index);
  assert_eq!(broker.cursors[index].remote_input_authority,None);
  assert_eq!(broker.cursors[index].buttons_down,0);
  assert_eq!(broker.cursors[index].capture,None);assert!(broker.quad_resize.is_none());
 }
}
}}
'''


def harness() -> str:
    frame = (ROOT / FRAME).read_text()
    broker = (ROOT / BROKER).read_text()
    # Keep the complete production pure state machine, excluding global services.
    core = frame[frame.index("use alloc::sync::Arc;"):frame.index("static CURSOR_FRAME_RIG:")]
    source = PRELUDE + core + FRAME_FIXTURE
    for name in ("selected_frame", "source_selected", "selected_frame_for_source",
                 "retarget_source", "cursor_retired"):
        source += item(FRAME, name)
    source += BROKER_FIXTURE
    for name in ("WindowTarget", "KeyboardSource", "CursorRoute", "InputBroker"):
        source += item(BROKER, name)
    for marker in ("impl WindowTarget {", "impl From<super::CursorFrameKey> for WindowTarget {"):
        source += block(broker, marker)
    source += "impl CursorRoute {\n"
    for marker in ("fn new(source:", "fn clear_frame_interaction(", "fn clear_window_interaction("):
        source += block(broker, marker)
    source += "}\nimpl InputBroker {\n"
    impl = broker[broker.index("impl InputBroker {"):]
    for marker in ("const fn new()", "fn refresh_remote_input_route(", "fn cursor_index(", "fn retire_pointer_group(",
                   "fn retire_hid_slot(", "fn keyboard_route_index(", "    fn focused_keyboard_state("):
        source += block(impl, marker)
    prefix = impl[impl.index("fn process_cursor("):impl.index("        let Some((width, height))")]
    source += prefix + '\n panic!("only retirement events belong in this harness");\n}\n}\n'
    return source + TESTS


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-pointer-groups-") as temporary:
        work = Path(temporary)
        (work / "src").mkdir()
        (work / "src/lib.rs").write_text(harness())
        (work / "Cargo.toml").write_text('''[package]
name="trueos-ui4-pointer-groups"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
heapless="=0.9.1"
spin="=0.10.1"
''')
        subprocess.run(["cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
                        "--quiet", "--", "--test-threads=1"], cwd=work, check=True)


if __name__ == "__main__":
    main()
