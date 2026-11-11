#![allow(dead_code)]
use core::sync::atomic::{AtomicU64,Ordering};
#[derive(Clone,Copy,Debug,Eq,PartialEq)] struct WindowOwner(u8);
#[derive(Clone,Copy,Debug,Eq,PartialEq)] struct WindowId(u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)] struct Ui4CursorSource(u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)] struct CursorFrameKey { owner:WindowOwner,window:WindowId }
impl CursorFrameKey { fn new(owner:WindowOwner,window:WindowId)->Self{Self{owner,window}} }
#[derive(Clone,Copy)] struct RawKey {slot:u32,key:u32}
mod spirit_access {
 use super::*;
 pub static SERIAL:AtomicU64=AtomicU64::new(1);
 #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct InputStamp(u64);
 pub fn stamp_cursor(source:Ui4CursorSource,frame:CursorFrameKey)->Option<InputStamp>{
  if source.0==1{return Some(InputStamp(0))}let serial=SERIAL.load(Ordering::SeqCst);
  (source.0==2&&serial!=0&&frame.window==WindowId(7)).then_some(InputStamp(serial))
 }
 pub fn stamp_keyboard(key:&RawKey,frame:CursorFrameKey)->Option<InputStamp>{stamp_cursor(Ui4CursorSource(key.slot),frame)}
 pub fn stamp_valid(stamp:InputStamp,frame:CursorFrameKey)->bool{stamp.0==0||(frame.window==WindowId(7)&&stamp.0==SERIAL.load(Ordering::SeqCst))}
 pub fn cursor_target_allowed(source:Ui4CursorSource,frame:CursorFrameKey)->bool{stamp_cursor(source,frame).is_some()}
 pub fn keyboard_target_allowed(key:&RawKey,frame:CursorFrameKey)->bool{stamp_keyboard(key,frame).is_some()}
 pub fn keyboard_state_target_allowed(key:&crate::usb2::hid::hut::HidKeyboardState,frame:CursorFrameKey)->bool{stamp_cursor(Ui4CursorSource(key.slot_id),frame).is_some()}
}
mod window_broker { pub fn collapse_state(_:super::WindowOwner,_:super::WindowId)->Option<bool>{Some(false)} }
#[derive(Clone,Copy)]struct WindowTarget{owner:WindowOwner,window:WindowId}
impl WindowTarget{fn cursor_frame_key(self)->CursorFrameKey{CursorFrameKey::new(self.owner,self.window)}}
struct SnapshotState{focus:Option<CursorFrameKey>,selected:std::vec::Vec<(Ui4CursorSource,CursorFrameKey)>,keyboards:std::vec::Vec<usb2::hid::hut::HidKeyboardState>}
static SNAPSHOTS:std::sync::Mutex<SnapshotState>=std::sync::Mutex::new(SnapshotState{focus:None,selected:std::vec::Vec::new(),keyboards:std::vec::Vec::new()});
fn selected_frame()->Option<CursorFrameKey>{SNAPSHOTS.lock().unwrap().focus}
fn selected_frame_for_source(source:Ui4CursorSource)->Option<CursorFrameKey>{SNAPSHOTS.lock().unwrap().selected.iter().find(|(s,_)|*s==source).map(|(_,f)|*f)}
mod cursor_frame_inout{pub fn source_selected(source:super::Ui4CursorSource)->bool{super::selected_frame_for_source(source).is_some_and(|f|super::selected_frame()==Some(f))}}
mod usb2{pub mod hid{pub mod hut{
 #[derive(Clone)]pub struct HidKeyboardState{pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,pub combo_id:u32,pub key_down_bits:[u32;8]}
 pub fn keyboards_snapshot()->std::vec::Vec<HidKeyboardState>{crate::SNAPSHOTS.lock().unwrap().keyboards.clone()}
}}}
mod snapshots{
 use super::*;use heapless::Vec;const MAX_CURSOR_ROUTES:usize=4;
 #[derive(Clone,Copy)]struct KeyboardSource{controller_id:u32,slot_id:u32,ep_target:u32}
 struct Route{source:Ui4CursorSource,visual_only:bool,selection_serial:u64,keyboard_source:Option<KeyboardSource>}
 impl Route{fn color(&self)->u32{0}}
 struct InputBroker{cursors:Vec<Route,MAX_CURSOR_ROUTES>}
 struct Ui4WindowInputRoute{source:Ui4CursorSource,combo_id:u32,color:u32,vcursor:bool,selected_for_window:bool,app_focus:bool,keyboard:Option<crate::usb2::hid::hut::HidKeyboardState>}
 fn window_snapshot_for_target(_:WindowTarget)->Option<()>{Some(())}
 fn cursor_hut_metadata(source:Ui4CursorSource)->(u32,bool){(source.0,false)}
 impl InputBroker{@SNAPSHOTS@}
 #[cfg(test)]mod tests{
  use super::*;
  fn setup(source:u32)->(InputBroker,WindowTarget){
   let target=WindowTarget{owner:WindowOwner(3),window:WindowId(7)};spirit_access::SERIAL.store(1,Ordering::SeqCst);
   let mut state=SNAPSHOTS.lock().unwrap();state.focus=Some(target.cursor_frame_key());state.selected=vec![(Ui4CursorSource(source),target.cursor_frame_key())];
   state.keyboards=vec![crate::usb2::hid::hut::HidKeyboardState{controller_id:0,slot_id:source,ep_target:0,combo_id:source,key_down_bits:[0xfeed;8]}];drop(state);
   let mut cursors=Vec::new();cursors.push(Route{source:Ui4CursorSource(source),visual_only:false,selection_serial:1,keyboard_source:Some(KeyboardSource{controller_id:0,slot_id:source,ep_target:0})}).ok();
   (InputBroker{cursors},target)
  }
  #[test]fn window_routes_never_return_human_keys_from_another_selected_window(){
   let(broker,target)=setup(1);let foreign=WindowTarget{window:WindowId(8),..target};
   let routes=broker.window_input_routes(foreign);assert_eq!(routes.len(),1);assert!(!routes[0].selected_for_window);assert!(routes[0].keyboard.is_none());
   let selected=broker.window_input_routes(target);assert_eq!(selected[0].keyboard.as_ref().unwrap().key_down_bits,[0xfeed;8]);
  }
  #[test]fn revoked_ai_routes_and_focused_keyboard_snapshots_disappear(){
   let(broker,target)=setup(2);assert!(broker.focused_keyboard_state(target).is_some());assert_eq!(broker.window_input_routes(target).len(),1);
   spirit_access::SERIAL.store(0,Ordering::SeqCst);assert!(broker.focused_keyboard_state(target).is_none());assert!(broker.window_input_routes(target).is_empty());
  }
  #[test]fn human_cursor_does_not_lend_revoked_ai_keyboard_state(){
   let(mut broker,target)=setup(1);broker.cursors[0].keyboard_source=Some(KeyboardSource{controller_id:0,slot_id:2,ep_target:0});
   let mut state=SNAPSHOTS.lock().unwrap();state.keyboards[0].slot_id=2;drop(state);spirit_access::SERIAL.store(0,Ordering::SeqCst);
   assert!(broker.focused_keyboard_state(target).is_none());assert!(broker.window_input_routes(target)[0].keyboard.is_none());
  }
 }
}
mod broker {
 use super::*;use heapless::Vec;use spin::Mutex;
 const MAX_OWNER_EVENTS:usize=4;const MAX_OWNER_QUEUES:usize=4;
 #[derive(Clone,Copy)] struct Ui4PointerEvent {source:Ui4CursorSource,window:WindowId,buttons_down:u32,buttons_pressed:u32,buttons_released:u32,dx:i32,dy:i32,wheel:i16}
 #[derive(Clone,Copy)] struct Ui4ButtonEvent {source:Ui4CursorSource,window:WindowId}
 #[derive(Clone,Copy,Eq,PartialEq)] enum Ui4PanPhase{Begin,Update,End}
 #[derive(Clone,Copy)] struct Ui4PanEvent {source:Ui4CursorSource,window:WindowId,phase:Ui4PanPhase,dx:i32,dy:i32}
 #[derive(Clone,Copy)] struct Ui4KeyboardEvent {window:WindowId,event:RawKey}
 #[derive(Clone,Copy)] struct Ui4ResizeEvent {window:WindowId}
 #[derive(Clone,Copy)] pub enum Ui4InputEvent {Pointer(Ui4PointerEvent),Button(Ui4ButtonEvent),Pan(Ui4PanEvent),Keyboard(Ui4KeyboardEvent),Resize(Ui4ResizeEvent)}
 struct ResizeState {owner:WindowOwner,pending:Option<Ui4ResizeEvent>}
 static OWNER_QUEUES:Mutex<Vec<OwnerQueue,MAX_OWNER_QUEUES>>=Mutex::new(Vec::new());
 static OWNER_RESIZE_STATES:Mutex<Vec<ResizeState,4>>=Mutex::new(Vec::new());
 static PHYSICAL_OWNER_QUEUES:Mutex<Vec<PhysicalOwnerQueue,MAX_OWNER_QUEUES>>=Mutex::new(Vec::new());
 fn enqueue_owner_resize(owner:WindowOwner,event:Ui4ResizeEvent){OWNER_RESIZE_STATES.lock().push(ResizeState{owner,pending:Some(event)}).ok();}
 fn note_owner_queue_drop(_:WindowOwner,_:Ui4InputEvent,_:&str){}
 @PRODUCTION@
 #[cfg(test)]mod tests{
  use super::*;
  const OWNER:WindowOwner=WindowOwner(3);
  fn reset(){OWNER_QUEUES.lock().clear();OWNER_RESIZE_STATES.lock().clear();PHYSICAL_OWNER_QUEUES.lock().clear();spirit_access::SERIAL.store(1,Ordering::SeqCst);}
  fn key(slot:u32,value:u32)->Ui4KeyboardEvent{Ui4KeyboardEvent{window:WindowId(7),event:RawKey{slot,key:value}}}
  fn pointer(dx:i32)->Ui4InputEvent{Ui4InputEvent::Pointer(Ui4PointerEvent{source:Ui4CursorSource(2),window:WindowId(7),buttons_down:0,buttons_pressed:0,buttons_released:0,dx,dy:0,wheel:0})}
  fn regrant(){spirit_access::SERIAL.store(0,Ordering::SeqCst);spirit_access::SERIAL.store(2,Ordering::SeqCst);}
  #[test]fn queued_key_never_borrows_regranted_permit(){
   reset();enqueue_owner_event(OWNER,Ui4InputEvent::Keyboard(key(2,41)));regrant();
   enqueue_owner_event(OWNER,Ui4InputEvent::Keyboard(key(2,42)));
   let out=take_owner_input_events(OWNER);assert_eq!(out.len(),1);
   let Ui4InputEvent::Keyboard(event)=out[0]else{panic!("keyboard")};assert_eq!(event.event.key,42);
  }
  #[test]fn pointer_samples_do_not_coalesce_across_permit_generations(){
   reset();enqueue_owner_event(OWNER,pointer(100));regrant();enqueue_owner_event(OWNER,pointer(7));
   assert_eq!(OWNER_QUEUES.lock()[0].events.len(),2);
   let out=take_owner_input_events(OWNER);assert_eq!(out.len(),1);
   let Ui4InputEvent::Pointer(event)=out[0]else{panic!("pointer")};assert_eq!(event.dx,7);
  }
  #[test]fn physical_queue_retains_the_original_admission_stamp(){
   reset();enqueue_physical_owner_event(OWNER,key(2,41));regrant();enqueue_physical_owner_event(OWNER,key(2,42));
   let(reset,out)=take_physical_owner_events(OWNER);assert!(!reset);assert_eq!(out.len(),1);assert_eq!(out[0].event.key,42);
  }
  #[test]fn deferred_event_keeps_stamp_when_resize_delivery_fills_output(){
   reset();for n in 0..MAX_OWNER_EVENTS{enqueue_owner_resize(OWNER,Ui4ResizeEvent{window:WindowId(n as u32)});}
   enqueue_owner_event(OWNER,Ui4InputEvent::Keyboard(key(2,41)));
   assert_eq!(take_owner_input_events(OWNER).len(),MAX_OWNER_EVENTS);assert_eq!(OWNER_QUEUES.lock()[0].events.len(),1);
   regrant();assert!(take_owner_input_events(OWNER).is_empty());
  }
  #[test]fn human_events_remain_deliverable_and_denied_events_never_enter_queues(){
   reset();spirit_access::SERIAL.store(0,Ordering::SeqCst);
   enqueue_owner_event(OWNER,Ui4InputEvent::Keyboard(key(2,1)));enqueue_physical_owner_event(OWNER,key(2,1));
   enqueue_owner_event(OWNER,Ui4InputEvent::Keyboard(key(1,2)));enqueue_physical_owner_event(OWNER,key(1,2));
   assert_eq!(take_owner_input_events(OWNER).len(),1);let(_,out)=take_physical_owner_events(OWNER);assert_eq!(out.len(),1);assert_eq!(out[0].event.key,2);
  }
 }
}
