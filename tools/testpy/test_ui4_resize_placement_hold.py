#!/usr/bin/env python3
"""Host-test the real broker placement update during producer resize holds."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
extract.ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
struct WindowPlacement {x:i32,y:i32,width:u32,height:u32,z:u32,opacity:u8,visible:bool}
impl WindowPlacement {fn valid(self)->bool {self.width>0 && self.height>0}}
#[derive(Clone,Copy)] struct WindowInteraction {receives_input:bool,resize_on_maximize:bool}
#[derive(Debug)] enum WindowBrokerError {EmptyExtent}
type WindowOwner=u32; type WindowId=u32;
struct Window {placement:WindowPlacement,replacement_presentation:Option<WindowPlacement>,
interaction:WindowInteraction,committed_resize_extent:(u32,u32),resize_epoch:u64,
dock_target:Option<u32>,restore_placement:Option<WindowPlacement>,
pending_resize_extent:Option<(u32,u32)>,open_transition:Option<u32>,revision:u64}
struct Broker {window:Window}
impl Broker {
fn checked_window_mut(&mut self,_:u32,_:u32)->Result<&mut Window,WindowBrokerError> {Ok(&mut self.window)}
fn mark_composition_changed(&mut self) {}
}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {const fn new(t:T)->Self {Self(std::sync::Mutex::new(t))}
fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
const OLD:WindowPlacement=WindowPlacement{x:20,y:30,width:800,height:600,z:1,opacity:255,visible:true};
static WINDOW_BROKER:Mutex<Broker>=Mutex::new(Broker{window:Window{placement:OLD,
replacement_presentation:None,interaction:WindowInteraction{receives_input:true,resize_on_maximize:true},
committed_resize_extent:(800,600),resize_epoch:0,dock_target:None,restore_placement:None,
pending_resize_extent:None,open_transition:None,revision:0}});
static RESIZES:Mutex<Vec<(u32,u32)>>=Mutex::new(Vec::new());
fn next_serial(s:u64)->u64 {s+1}
mod ui4 {pub fn output_dimensions()->Option<(u32,u32)> {Some((1920,1080))}}
mod input_broker {pub fn enqueue_window_resize(_:u32,_:u32,_:u64,_:u32,_:u32,w:u32,h:u32) {super::RESIZES.lock().push((w,h));}}
mod cursor_frame_inout {pub fn selection_strip_stack_changed() {}}
mod production {use super::*; ITEMS
#[test] fn new_extent_holds_old_position_and_drag_still_works() {
let desired=WindowPlacement{x:300,y:250,width:400,height:300,..OLD};
assert_eq!(update_window_placement(1,1,|_|desired).unwrap(),desired);
{let b=WINDOW_BROKER.lock();assert_eq!(b.window.placement,desired);
assert_eq!(b.window.replacement_presentation,Some(OLD));
assert_eq!(b.window.pending_resize_extent,Some((400,300)));}
assert_eq!(*RESIZES.lock(),vec![(400,300)]);
update_window_placement(1,1,|p|WindowPlacement{x:p.x+40,y:p.y+50,..p}).unwrap();
let moved=WindowPlacement{x:60,y:80,..OLD};
assert_eq!(WINDOW_BROKER.lock().window.replacement_presentation,Some(moved));
update_window_placement(1,1,|p|WindowPlacement{x:500,y:400,width:300,height:200,..p}).unwrap();
assert_eq!(WINDOW_BROKER.lock().window.replacement_presentation,Some(moved));
assert_eq!(*RESIZES.lock(),vec![(400,300),(300,200)]);
}
}
'''
def main():
    items = '\n'.join(extract.item('src/ui4/window_broker.rs', name) for name in
                      ['update_window_placement', 'translated_resize_presentation', 'producer_resize_required'])
    with tempfile.TemporaryDirectory(prefix='ui4-resize-placement-') as directory:
        root=Path(directory); source=root/'test.rs'; binary=root/'test'
        source.write_text(HARNESS.replace('ITEMS',items))
        subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)
if __name__ == '__main__': main()
