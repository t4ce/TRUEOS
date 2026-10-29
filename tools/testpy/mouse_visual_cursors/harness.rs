#![allow(dead_code)]
extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};
static VISUAL_WAKES: AtomicUsize = AtomicUsize::new(0);
static SNAPSHOT_WRITES: AtomicUsize = AtomicUsize::new(0);
static HUT_WRITES: AtomicUsize = AtomicUsize::new(0);
static HID_WRITES: AtomicUsize = AtomicUsize::new(0);

#[macro_export]
macro_rules! log_info { ($($tt:tt)*) => {} }

pub mod graphics { pub mod primitives {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Rgba8 { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
    impl Rgba8 { pub const fn new(r:u8,g:u8,b:u8,a:u8)->Self { Self {r,g,b,a} } }
} }

pub mod ui4 {
    use crate::*;
    use graphics::primitives::Rgba8;
    pub fn output_dimensions()->Option<(u32,u32)> { Some((320,200)) }
    pub fn physical_extent_pixels(_:u32,_:u32)->Option<(u32,u32)> { Some((64,64)) }
    pub fn notify_cursor_visual_change() { VISUAL_WAKES.fetch_add(1, Ordering::Relaxed); }
    #[derive(Clone, Copy, Eq, PartialEq)]
    pub struct Ui4CursorSource { pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,pub hid_kind:u8 }
    #[derive(Clone, Copy)]
    pub struct WindowTarget { owner:u8, window:u32 }
    impl WindowTarget { fn cursor_frame_key(self)->Self {self} }
    #[derive(Clone, Copy)]
    struct CursorRoute { source:Ui4CursorSource, visual_only:bool, color:Rgba8, selection_serial:u64 }
    impl CursorRoute { fn clear_frame_interaction(&mut self) {} }
    struct InputBroker { cursors:heapless::Vec<CursorRoute,64>, selection_serial:u64 }
    impl InputBroker {
        fn new()->Self {Self {cursors:heapless::Vec::new(),selection_serial:0}}
        fn cursor_index(&mut self,source:Ui4CursorSource,_:u32,_:u32)->usize {
            self.cursors.push(CursorRoute {source,visual_only:false,color:Rgba8::new(0,0,0,255),selection_serial:0}).ok();
            self.cursors.len()-1
        }
    }
    fn cursor_hut_metadata(_:Ui4CursorSource)->(u32,bool) {(0,true)}
    pub mod cursor_frame_inout {
        use super::*;
        pub struct SelectionChange {pub changed:bool,pub focus_changed:bool,pub selected:Option<WindowTarget>}
        pub fn source_selected(_:Ui4CursorSource)->bool {false}
        pub fn select_frame(_:Option<WindowTarget>,_:Ui4CursorSource,_:Rgba8)->SelectionChange {
            panic!("visual-only route reached frame selection")
        }
    }
    pub mod context_menu {pub fn dismiss_for_source(_:super::Ui4CursorSource) {}}
    pub mod window_broker {pub fn note_window_focused(_:u8,_:u32)->Result<(),()> {Ok(())}}
    mod routing { use super::*; include!("routing.rs"); }
}

pub mod usb2 { pub mod hid {
    use crate::*;
    pub const HID_CURSOR_EVENT_FLAG_VISUAL_ONLY:u32=1<<19;
    const HID_KIND_VIRTUAL_CURSOR:u8=0;
    fn now_ms_u32()->u32 {0}
    fn clamp01(value:f64)->f64 {value.clamp(0.0,1.0)}
    fn push_cursor_event(_:TrueosHidCursorEvent) { HID_WRITES.fetch_add(1,Ordering::Relaxed); }
    fn notify_cursor_event_ready() {}
    pub fn retire_virtual_cursor(_:u32) {}
    pub mod hut {
        pub enum HidSourceKind {Ai}
        pub fn upsert_mouse_state(_:u32,_:u32,_:u32,_:f64,_:f64,_:u32,_:HidSourceKind,_:&str,_:bool) {
            crate::HUT_WRITES.fetch_add(1,crate::Ordering::Relaxed);
        }
    }
    include!("hid_injection.rs");
} }

pub mod intel {
    pub(crate) use crate::damage::{DamageRect as CompositionDamageRect,
        DamageRegion as CompositionDamageRegion};
    #[derive(Clone, Copy, Debug)]
    pub struct LiveOverlayRect {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:crate::graphics::primitives::Rgba8}
    impl LiveOverlayRect {pub fn new(x:u32,y:u32,width:u32,height:u32,color:crate::graphics::primitives::Rgba8)->Self {Self{x,y,width,height,color}}}
}

pub mod r {
    pub mod cursor {
        pub const HID_KIND_VIRTUAL_CURSOR:u8=0;
        pub fn slot_id_in_use(_:u32)->bool {false}
        pub fn upsert_snapshot(_:u32,_:u32,_:u32,_:u8,_:f64,_:f64,_:u32) {
            crate::SNAPSHOT_WRITES.fetch_add(1,crate::Ordering::Relaxed);
        }
    }
    pub mod keyboard { #[derive(Default)] pub struct TrueosKeyboardOutputEvent {pub controller_id:u32,pub slot_id:u32} }
    pub mod services {
        pub use crate::{microfont_mask_service, mouse_motion_service};
    }
}

#[path="font.rs"] pub mod microfont_mask_service;
#[path="mouse.rs"] pub mod mouse_motion_service;
#[path="damage.rs"] mod damage;

#[cfg(test)]
#[path="render.rs"] mod render;

#[cfg(test)] mod integration_tests {
    use super::*;
    use graphics::primitives::Rgba8;
    use r::services::mouse_motion_service::*;
    const OWNER: MouseControlPrincipal = MouseControlPrincipal::KernelApp(0x53);

    #[test]
    fn visual_hid_delivery_never_enrolls_input_devices_or_snapshots() {
        usb2::hid::inject_virtual_cursor_event(123,0.1,0.2,0,0,usb2::hid::HID_CURSOR_EVENT_FLAG_VISUAL_ONLY);
        assert_eq!(HID_WRITES.load(Ordering::Relaxed),1);
        assert_eq!(HUT_WRITES.load(Ordering::Relaxed),0);
        assert_eq!(SNAPSHOT_WRITES.load(Ordering::Relaxed),0);
        usb2::hid::inject_virtual_cursor_event(124,0.1,0.2,0,0,0);
        assert_eq!(HID_WRITES.load(Ordering::Relaxed),2);
        assert_eq!(HUT_WRITES.load(Ordering::Relaxed),1);
        assert_eq!(SNAPSHOT_WRITES.load(Ordering::Relaxed),1);
    }

    #[test]
    fn matching_glyphs_share_registration_and_metadata_updates_are_quiet() {
        microfont_mask_service::warm_fixture_atlas();
        let black=Rgba8::new(0,0,0,255);
        let blue=Rgba8::new(0,160,255,255);
        let a=request_glyph_cursor(OWNER,"agent-a",'§',48,black).unwrap();
        let b=request_glyph_cursor(OWNER,"agent-b",'§',48,black).unwrap();
        let first=cursor_visual_metadata(a.slot_id).unwrap();
        let second=cursor_visual_metadata(b.slot_id).unwrap();
        assert!(alloc::sync::Arc::ptr_eq(first.glyph.as_ref().unwrap(),second.glyph.as_ref().unwrap()));
        assert_eq!(VISUAL_WAKES.load(Ordering::Relaxed),0);
        set_cursor_visual_color(OWNER,a.handle,black).unwrap();
        set_cursor_visible(OWNER,a.handle,true).unwrap();
        assert_eq!(VISUAL_WAKES.load(Ordering::Relaxed),0);
        set_cursor_visual_color(OWNER,a.handle,blue).unwrap();
        set_cursor_visual_color(OWNER,a.handle,blue).unwrap();
        set_cursor_visible(OWNER,a.handle,false).unwrap();
        set_cursor_visible(OWNER,a.handle,false).unwrap();
        assert_eq!(VISUAL_WAKES.load(Ordering::Relaxed),2);
        let updated=cursor_visual_metadata(a.slot_id).unwrap();
        assert_eq!(updated.color,Some(blue));
        assert!(!updated.visible);
        assert!(updated.visual_only);
        assert!(alloc::sync::Arc::ptr_eq(first.glyph.as_ref().unwrap(),updated.glyph.as_ref().unwrap()));
        assert_eq!(set_cursor_visual_color(MouseControlPrincipal::Vm(1),a.handle,black),Err(MouseControlError::Denied));
        release_cursor(OWNER,a.handle).unwrap();
        assert!(cursor_visual_metadata(a.slot_id).is_none());
        assert!(cursor_visual_metadata(b.slot_id).is_some());
    }

    #[test]
    fn marker_uses_papers_resident_coverage_without_another_atlas_production() {
        let atlas = microfont_mask_service::warm_fixture_atlas();
        let source = atlas.glyph('§').unwrap();
        for height in [1u16, 5, 10, 11, 16, 48, 253, 320] {
            let cursor = request_glyph_cursor(OWNER, "paper-size", '§', height,
                Rgba8::new(0,0,0,255)).unwrap();
            let visual = cursor_visual_metadata(cursor.slot_id).unwrap();
            let glyph = visual.glyph.unwrap();
            assert_eq!(u32::from(glyph.width), (u32::from(height) * 6 + 5) / 11);
            assert_eq!(glyph.height, height);
            let mut actual = alloc::vec![0; usize::from(glyph.width) * usize::from(height)];
            for run in glyph.runs.iter() {
                assert!(run.width != 0 && run.coverage != 0);
                assert!(run.x + run.width <= glyph.width && run.y < height);
                for x in run.x..run.x + run.width {
                    actual[usize::from(run.y) * usize::from(glyph.width) + usize::from(x)] = run.coverage;
                }
            }
            for y in 0..u32::from(height) {
                for x in 0..u32::from(glyph.width) {
                    assert_eq!(actual[y as usize * usize::from(glyph.width) + x as usize],
                        atlas.sample(source, x, y, u32::from(glyph.width), u32::from(height)));
                }
            }
            set_cursor_visual_color(OWNER, cursor.handle, Rgba8::new(0,160,255,255)).unwrap();
            release_cursor(OWNER, cursor.handle).unwrap();
        }
        assert!(core::ptr::eq(atlas, microfont_mask_service::ready_atlas().unwrap()));
        assert_eq!(microfont_mask_service::fixture_productions(), 1);
        assert_eq!(request_glyph_cursor(OWNER,"unsupported",'\u{10ffff}',48,
            Rgba8::new(0,0,0,255)), Err(MouseControlError::Invalid));
    }

    #[test]
    fn text_cursor_uses_the_same_resident_cells_and_preserves_its_neutral_name() {
        let atlas = microfont_mask_service::warm_fixture_atlas();
        let black = Rgba8::new(0,0,0,255);
        let neutral = black;
        for text in ["§Galileo XVII", "§Joker", "§🃏"] {
            for height in [11u16, 38, 44] {
                let cursor = request_text_cursor(OWNER,"nickname",text,height,1,black,neutral).unwrap();
                let before = cursor_visual_metadata(cursor.slot_id).unwrap();
                let glyph = before.glyph.as_ref().unwrap();
                let (width, measured_height) = cursor_text_dimensions(text,height).unwrap();
                assert_eq!((glyph.width, glyph.height), (width,measured_height));
                assert_eq!(glyph.text.as_ref(),text);
                let cell = (u32::from(height)*6+5)/11;
                assert_eq!(glyph.prefix_width,u16::try_from(cell).unwrap());
                assert_eq!(u32::from(width),cell*text.chars().count() as u32);
                let mut actual = alloc::vec![0; usize::from(width)*usize::from(height)];
                for run in glyph.runs.iter() {
                    assert!(run.width != 0 && run.coverage != 0);
                    assert!(run.x+run.width <= width && run.y < height);
                    assert!((run.x < glyph.prefix_width) == (run.x+run.width <= glyph.prefix_width));
                    for x in run.x..run.x+run.width {
                        actual[usize::from(run.y)*usize::from(width)+usize::from(x)] = run.coverage;
                    }
                }
                for (column,ch) in text.chars().enumerate() {
                    let mask = atlas.glyph(ch).or_else(||atlas.glyph('?')).unwrap();
                    for y in 0..u32::from(height) {
                        for x in 0..cell {
                            assert_eq!(actual[y as usize*usize::from(width)+column*cell as usize+x as usize],
                                atlas.sample(mask,x,y,cell,u32::from(height)));
                        }
                    }
                }
                let same = request_text_cursor(OWNER,"same-name",text,height,1,
                    Rgba8::new(0,160,255,255),Rgba8::new(200,200,200,255)).unwrap();
                assert!(alloc::sync::Arc::ptr_eq(glyph,
                    cursor_visual_metadata(same.slot_id).unwrap().glyph.as_ref().unwrap()));
                set_cursor_visual_color(OWNER,cursor.handle,Rgba8::new(0,220,120,255)).unwrap();
                let after = cursor_visual_metadata(cursor.slot_id).unwrap();
                assert_eq!(after.text_color,Some(neutral));
                assert!(alloc::sync::Arc::ptr_eq(glyph,after.glyph.as_ref().unwrap()));
                release_cursor(OWNER,cursor.handle).unwrap();
                release_cursor(OWNER,same.handle).unwrap();
            }
        }
        assert_eq!(microfont_mask_service::fixture_productions(),1);
    }

    #[test]
    fn text_cursor_measurement_is_ready_before_the_atlas_and_bounds_registration() {
        assert!(microfont_mask_service::ready_atlas().is_none());
        assert_eq!(cursor_text_dimensions("§Joker",38),Ok((126,38)));
        assert_eq!(request_text_cursor(OWNER,"cold","§Joker",38,1,
            Rgba8::new(0,0,0,255),Rgba8::new(235,235,235,255)),Err(MouseControlError::Unavailable));
        for text in ["", "§line\nbreak", "§\tTab", "§\0Zero"] {
            assert_eq!(cursor_text_dimensions(text,38),Err(MouseControlError::Invalid));
        }
        for height in [0,4097] {
            assert_eq!(cursor_text_dimensions("§Joker",height),Err(MouseControlError::Invalid));
        }
        assert_eq!(cursor_text_dimensions(&"N".repeat(128),38),Ok((2688,38)));
        assert_eq!(cursor_text_dimensions(&"N".repeat(129),38),Err(MouseControlError::Capacity));
        assert_eq!(cursor_text_dimensions(&"N".repeat(128),4096),Err(MouseControlError::Capacity));
        assert_eq!(request_text_cursor(OWNER,"bad-prefix","§Joker",38,7,
            Rgba8::new(0,0,0,255),Rgba8::new(235,235,235,255)),Err(MouseControlError::Invalid));
    }
}
