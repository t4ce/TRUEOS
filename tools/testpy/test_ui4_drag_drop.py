#!/usr/bin/env python3
"""Exercise production drag ownership/mailboxes and slot-4 preview clipping."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

from _microfont_build import build_microfont
ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT

def main():
    source = r'''
#![allow(dead_code)]
extern crate alloc;
mod graphics {pub mod primitives {
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Rgba8(u8,u8,u8,u8);
impl Rgba8 {pub fn new(r:u8,g:u8,b:u8,a:u8)->Self{Self(r,g,b,a)}}
}}
mod r {pub mod services {pub mod microfont_mask_service {
    pub static READY:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(true);
    pub fn copy_text_mask(pixels:&mut [u8],width:usize,height:usize,x:usize,y:usize,
        text:&str,cell_width:usize,cell_height:usize)->Result<(),&'static str> {
        if !READY.load(std::sync::atomic::Ordering::Relaxed) {return Err("microfont-not-ready");}
        // Transport test fixture; the real R8 copy and sampling are tested by
        // test_microfont_mask_service.py. Keep this harness raster-only too.
        for (column,ch) in text.chars().enumerate() {
            if ch==' ' {continue;}
            for row in y..(y+cell_height).min(height) {
                for col in x+column*cell_width..(x+(column+1)*cell_width).min(width) {
                    pixels[row*width+col]=if col%2==0 {64} else {192};
                }
            }
        }
        Ok(())
    }
}}}
mod shell3 {pub mod tui {pub fn vm_for_window(_:crate::ui4::WindowId)->Option<u8>{None}}}
mod ui4 {
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub enum WindowOwner {Vm(u8),KernelApp(u8)}
impl WindowOwner {pub const SHELL3_SERVICE:Self=Self::KernelApp(11);}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub struct WindowId(u32);
impl WindowId {pub fn from_raw(id:u32)->Option<Self>{(id!=0).then_some(Self(id))}}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub struct CursorFrameKey {pub owner:WindowOwner,pub window:WindowId}
impl CursorFrameKey {pub fn new(owner:WindowOwner,window:WindowId)->Self{Self{owner,window}}}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub struct Ui4CursorSource {pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,pub hid_kind:u8}
mod input_broker {pub fn notify_slot4_visual_change(){} pub fn claim_drag_gesture(_:super::Ui4CursorSource){}}
'''
    source += f'#[path="{ROOT}/src/ui4/drag_drop.rs"] mod drag_drop;\n'
    source += r'''
mod slot4 {
#[derive(Clone,Copy,Debug)]struct Rect {x:u32,y:u32,width:u32,height:u32,color:crate::graphics::primitives::Rgba8}
type Slot4Rects=Vec<Rect>;
fn push_overlay_rect(out:&mut Slot4Rects,x:u32,y:u32,width:u32,height:u32,color:crate::graphics::primitives::Rgba8){out.push(Rect{x,y,width,height,color});}
'''
    source += extract.item('src/ui4/slot4_service.rs', 'push_drag_preview')
    source += r'''
#[test]fn drag_label_is_visible_and_clipped_at_every_screen_edge(){
    let preview=super::drag_drop::DragPreview{width:12,height:10,runs:vec![(0,0,6,64),(5,9,7,192)]};
    for (x,y,w,h) in [(0,0,100,80),(99,79,100,80),(1000,1000,100,80),(0,0,3,2)] {
        let mut out=Vec::new();push_drag_preview(&mut out,x,y,&preview,w,h);
        assert!(!out.is_empty());
        assert!(out.iter().all(|r|r.width>0&&r.height>0&&r.x+r.width<=w&&r.y+r.height<=h));
    }
}
#[test]fn unready_native_atlas_does_not_cache_an_empty_preview(){
    use super::{WindowOwner,WindowId,CursorFrameKey,Ui4CursorSource,drag_drop};
    use crate::r::services::microfont_mask_service::READY;
    let owner=WindowOwner::Vm(19);
    let frame=CursorFrameKey::new(owner,WindowId::from_raw(19).unwrap());
    let source=Ui4CursorSource{controller_id:19,slot_id:19,ep_target:19,hid_kind:1};
    READY.store(false,std::sync::atomic::Ordering::Relaxed);
    let result=drag_drop::begin(owner,frame,source,1,"label",b"data");
    READY.store(true,std::sync::atomic::Ordering::Relaxed);
    assert_eq!(result,Err(-5));
    assert!(drag_drop::cursor_preview(source).is_none());
    let token=drag_drop::begin(owner,frame,source,1,"label",b"data").unwrap();
    assert!(!drag_drop::cursor_preview(source).unwrap().runs.is_empty());
    drag_drop::cancel(owner,token);
}
#[test]fn drag_text_keeps_native_coverage_at_final_pixel_size(){
    let preview=super::drag_drop::DragPreview{width:12,height:22,runs:vec![(0,0,3,64),(3,0,3,192)]};
    let mut out=Vec::new();push_drag_preview(&mut out,0,0,&preview,100,80);
    assert_eq!((out[0].width,out[0].height),(20,30));
    assert_eq!((out[1].x,out[1].y,out[1].width,out[1].height),(20,20,3,1));
    assert_eq!(out[1].color,crate::graphics::primitives::Rgba8::new(255,255,255,64));
    assert_eq!(out[2].color,crate::graphics::primitives::Rgba8::new(255,255,255,192));
}
}
}
'''
    with tempfile.TemporaryDirectory(prefix='ui4-drag-') as directory:
        path=Path(directory)
        (path/'spin.rs').write_text("pub struct Mutex<T>(std::sync::Mutex<T>);impl<T> Mutex<T>{pub const fn new(t:T)->Self{Self(std::sync::Mutex::new(t))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}")
        for name, file in [('spin',path/'spin.rs'),('microfont',ROOT/'vendor/microfont/src/lib.rs')]:
            if name == 'microfont':
                build_microfont(path / 'libmicrofont.rlib', features=('raster',))
            else:
                subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name',name,str(file),'-o',str(path/f'lib{name}.rlib')],check=True)
        (path/'tests.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'tests.rs'),'-o',str(path/'tests'),'--extern',f'spin={path}/libspin.rlib','--extern',f'microfont={path}/libmicrofont.rlib'],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)

if __name__ == '__main__': main()
