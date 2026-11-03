#![allow(dead_code)]
extern crate alloc;
extern crate self as v;
extern crate self as trueos_time;
use core::sync::atomic::{AtomicU64, Ordering};
static NOW:AtomicU64=AtomicU64::new(0);
#[derive(Clone,Copy)] pub struct Instant(u64);
impl Instant {pub fn now()->Self{Self(NOW.load(Ordering::Relaxed))} pub fn as_millis(self)->u64{self.0}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct Duration(u64);
impl Duration {pub fn from_millis(ms:u64)->Self{Self(ms)}}
pub mod bp_abi { #[derive(Clone,Copy,Debug,Default)] REGION_RECORD }
pub mod graphics {pub mod primitives {
    #[derive(Clone,Copy,Debug)] pub struct Rgba8(pub u8,pub u8,pub u8,pub u8);
    impl Rgba8 {pub fn new(r:u8,g:u8,b:u8,a:u8)->Self{Self(r,g,b,a)}}
}}
pub mod intel {
    pub struct LiveOverlayRect {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:crate::graphics::primitives::Rgba8}
}
mod ui4 {
    pub type WindowId=u32; pub type WindowOwner=u32; pub type WindowSessionId=u32; pub type Ui4CursorSource=u32;
    pub struct OutputId; impl OutputId {pub fn from_slot(_:u32)->Option<Self>{Some(Self)}}
    #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum WindowState{Ready,Closing}
    #[derive(Clone,Copy)] pub struct Plane(usize); impl Plane{pub fn slot(self)->usize{self.0}}
    #[derive(Clone,Copy)] pub struct Placement{pub x:i32,pub y:i32,pub width:u32,pub height:u32,pub z:i32}
    #[derive(Clone,Copy)] pub struct Interaction{pub hit_testable:bool}
    #[derive(Clone,Copy)] pub struct WindowSnapshot{pub id:u32,pub owner:u32,pub session:u32,pub plane:Plane,pub placement:Placement,pub presentation_placement:Placement,pub state:WindowState,pub interaction:Interaction,pub arc:u16}
    #[derive(Clone,Copy,PartialEq,Eq)] pub struct CursorFrameKey(u32,u32);
    impl CursorFrameKey{pub fn new(owner:u32,id:u32)->Self{Self(owner,id)}}
    static WINDOWS:spin::Mutex<Vec<WindowSnapshot>>=spin::Mutex::new(Vec::new());
    static SELECTED:spin::Mutex<Vec<(u32,CursorFrameKey)>>=spin::Mutex::new(Vec::new());
    pub fn visible_windows_for_output(_:OutputId)->Vec<WindowSnapshot>{WINDOWS.lock().clone()}
    pub fn selected_frame_for_source(source:u32)->Option<CursorFrameKey>{SELECTED.lock().iter().find(|(s,_)|*s==source).map(|(_,key)|*key)}
    pub fn notify_cursor_visual_change(){}
    mod input_broker {
        pub static CURSORS:spin::Mutex<Vec<super::button_outline::Cursor>>=spin::Mutex::new(Vec::new());
        pub fn button_outline_cursors()->Vec<super::button_outline::Cursor>{CURSORS.lock().iter().map(|c|super::button_outline::Cursor{source:c.source,x:c.x,y:c.y,pressed:c.pressed}).collect()}
        pub fn placement_contains(p:super::Placement,_:u16,x:u32,y:u32)->bool{x as i64>=p.x as i64 && y as i64>=p.y as i64 && (x as i64)<p.x as i64+p.width as i64 && (y as i64)<p.y as i64+p.height as i64}
    }
    #[path="GEOMETRY_PATH"] mod button_outline_geometry;
    #[path="STATE_PATH"] mod button_outline;

    #[cfg(test)] mod tests {
        use super::*; use crate::{bp_abi::TrueosUi4ButtonRegionV1 as Region,Ordering};
        static LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
        fn setup()->std::sync::MutexGuard<'static,()> {
            let guard=LOCK.lock().unwrap();
            button_outline::remove_owner(1);button_outline::remove_owner(2);
            WINDOWS.lock().clear();SELECTED.lock().clear();input_broker::CURSORS.lock().clear();crate::NOW.store(0,Ordering::Relaxed);
            guard
        }
        fn window(id:u32,slot:usize,z:i32)->WindowSnapshot{
            let placement=Placement{x:20,y:30,width:200,height:120,z};
            WindowSnapshot{id,owner:1,session:1,plane:Plane(slot),placement,presentation_placement:placement,state:WindowState::Ready,interaction:Interaction{hit_testable:true},arc:0}
        }
        fn region(x:i32)->Region{Region{x,y:10,width:70,height:30,clip_x:0,clip_y:0,clip_width:200,clip_height:120,border_rgba:0xff000000}}
        fn cursor(source:u32,x:u32,pressed:bool){
            input_broker::CURSORS.lock().push(button_outline::Cursor{source,x,y:50,pressed});
            SELECTED.lock().push((source,CursorFrameKey::new(1,1)));
        }
        fn draw()->Vec<crate::intel::LiveOverlayRect>{let mut r=Vec::new();button_outline::append(&mut r,600,400);r}
        fn pixels(rects:&[crate::intel::LiveOverlayRect])->std::collections::BTreeSet<(u32,u32)>{
            rects.iter().flat_map(|r|(r.y..r.y+r.height).flat_map(move|y|(r.x..r.x+r.width).map(move|x|(x,y)))).collect()
        }
        #[test] fn one_effect_per_cursor_and_shared_button_alpha_does_not_stack(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);button_outline::register(w,&[region(10),region(100)]).unwrap();
            cursor(1,40,false);let once=draw();cursor(2,45,false);assert_eq!(pixels(&once),pixels(&draw()));assert_eq!(once.len(),draw().len());
            cursor(3,135,false);assert!(draw().len()>once.len());assert!(draw().iter().all(|r|r.color.3==128));
        }
        #[test] fn press_wins_for_shared_button_and_stops_dash_timer(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);button_outline::register(w,&[region(10)]).unwrap();cursor(1,40,false);draw();
            assert_eq!(button_outline::wait(crate::Instant(0)),Some(crate::Duration(67)));
            assert!(!button_outline::due(crate::Instant(66)));assert!(button_outline::due(crate::Instant(67)));
            cursor(2,45,true);let full=draw();assert_eq!(button_outline::wait(crate::Instant(0)),None);assert_eq!(full.len(),4);
        }
        #[test] fn link_hover_uses_ink_and_stays_dashed_when_pressed(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);
            let link=Region{border_rgba:u32::from_le_bytes([0,0,255,255]),..region(10)};
            button_outline::register_hover(w,&[link]).unwrap();cursor(1,40,false);
            let hover=draw();assert!(!hover.is_empty());
            assert!(hover.iter().all(|r|(r.color.0,r.color.1,r.color.2,r.color.3)==(0,0,255,128)));
            input_broker::CURSORS.lock()[0].pressed=true;
            assert_eq!(pixels(&hover),pixels(&draw()));assert_eq!(hover.len(),draw().len());
            assert_eq!(button_outline::wait(crate::Instant(0)),Some(crate::Duration(67)));
            cursor(2,45,true);assert_eq!(hover.len(),draw().len());
            crate::NOW.store(67,Ordering::Relaxed);assert_ne!(pixels(&hover),pixels(&draw()));
            input_broker::CURSORS.lock().clear();assert!(draw().is_empty());assert_eq!(button_outline::wait(crate::Instant(67)),None);
        }
        #[test] fn buttons_take_priority_and_region_lists_update_independently(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);
            button_outline::register_hover(w,&[region(10)]).unwrap();
            button_outline::register(w,&[region(10)]).unwrap();cursor(1,40,true);
            assert_eq!(draw().len(),4);assert_eq!(button_outline::wait(crate::Instant(0)),None);
            button_outline::register(w,&[]).unwrap();assert!(draw().len()>4);
            assert_eq!(button_outline::wait(crate::Instant(0)),Some(crate::Duration(67)));
            let original=pixels(&draw());assert!(button_outline::register_hover(w,&[Region{width:0,..region(10)}]).is_err());
            assert_eq!(original,pixels(&draw()));
            button_outline::register_hover(w,&[]).unwrap();assert!(draw().is_empty());
            button_outline::register(w,&[region(10)]).unwrap();button_outline::register_hover(w,&[]).unwrap();assert_eq!(draw().len(),4);
        }
        #[test] fn link_hover_respects_clip_selection_and_session_lifetime(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);
            button_outline::register_hover(w,&[Region{clip_x:15,..region(10)}]).unwrap();cursor(1,40,false);
            let rects=draw();assert!(!rects.is_empty());assert!(rects.iter().all(|r|r.x>=35));
            SELECTED.lock().clear();assert!(draw().is_empty());SELECTED.lock().push((1,CursorFrameKey::new(1,1)));
            WINDOWS.lock()[0].session=2;assert!(draw().is_empty());
            let recycled=WINDOWS.lock()[0];button_outline::register(recycled,&[]).unwrap();assert!(draw().is_empty());
            button_outline::register_hover(recycled,&[region(10)]).unwrap();assert!(!draw().is_empty());
            button_outline::remove_window(1,1);assert!(draw().is_empty());
        }
        #[test] fn top_window_occludes_outline_even_when_pointer_is_on_visible_half(){
            let _guard=setup();let w=window(1,1,0);let mut upper=window(2,2,-100);
            upper.presentation_placement.x=70;upper.presentation_placement.width=100;
            WINDOWS.lock().extend([w,upper]);button_outline::register(w,&[region(10)]).unwrap();cursor(1,40,true);
            let rects=draw();assert!(!rects.is_empty());assert!(rects.iter().all(|r|r.x+r.width<=70));
            input_broker::CURSORS.lock()[0].x=80;assert!(draw().is_empty());
        }
        #[test] fn clear_close_and_recycled_session_remove_feedback(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);button_outline::register(w,&[region(10)]).unwrap();cursor(1,40,false);assert!(!draw().is_empty());
            button_outline::register(w,&[]).unwrap();assert!(draw().is_empty());
            button_outline::register(w,&[region(10)]).unwrap();WINDOWS.lock()[0].session=2;assert!(draw().is_empty());
            WINDOWS.lock()[0]=w;button_outline::remove_window(1,1);assert!(draw().is_empty());
            button_outline::register(w,&[region(10)]).unwrap();button_outline::remove_owner(1);assert!(draw().is_empty());
        }
        #[test] fn selection_and_content_clip_are_enforced_and_invalid_updates_are_atomic(){
            let _guard=setup();let w=window(1,1,0);WINDOWS.lock().push(w);let mut r=region(10);r.clip_x=15;button_outline::register(w,&[r]).unwrap();cursor(1,40,true);
            assert!(draw().iter().all(|r|r.x>=35));let original=pixels(&draw());let bad=Region{width:0,..r};assert!(button_outline::register(w,&[bad]).is_err());assert_eq!(original,pixels(&draw()));
            SELECTED.lock().clear();assert!(draw().is_empty());
        }
    }
}
