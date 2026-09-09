#!/usr/bin/env python3
"""Exercise real restore-hold geometry and background fallback admission."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

HARNESS = r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,Eq,PartialEq)] struct WindowPlacement { x:i32,y:i32,width:u32,height:u32 }
#[derive(Clone,Copy)] struct WindowPlane(usize);
impl WindowPlane { fn slot(self)->usize {self.0} }
#[derive(Clone,Copy,PartialEq)] enum WindowState {Ready,Closing}
#[derive(Clone,Copy)] struct Interaction {resize_on_maximize:bool}
#[derive(Clone,Copy)] struct WindowSnapshot {placement:WindowPlacement,plane:WindowPlane,interaction:Interaction,state:WindowState}
#[derive(Clone,Copy)] struct FrameRgbaView {width:u32,height:u32}
#[derive(Clone,Copy)] enum FrameBuffering {Single,Double,Triple,Quad}
#[derive(Clone,Copy)] enum FrameCadence {Dirty,Streaming,Immutable}
#[derive(Clone,Copy)] enum FrameContent {BlueprintScene,FontScene2d,RenderScene3d,Image}
struct FramePlan {buffering:FrameBuffering,cadence:FrameCadence,content:FrameContent}
mod intel { pub fn active_scanout_dimensions()->Option<(u32,u32)> {Some((2560,1440))} }
mod production {use super::*; ITEMS}
fn eligible(p:WindowPlacement,slot:usize)->bool {
    production::direct_overlay_geometry_eligible(WindowSnapshot{placement:p,plane:WindowPlane(slot),interaction:Interaction{resize_on_maximize:true},state:WindowState::Ready},FrameRgbaView{width:p.width,height:p.height})
}
#[test] fn restore_then_drag_cannot_translate_old_fullscreen_pair_outside_pipe() {
    let full=WindowPlacement{x:0,y:0,width:2560,height:1440};
    // Previous code translated the held fullscreen allocation by the delta of
    // the much smaller logical restored window. Neither direct layer fits.
    let old=WindowPlacement{x:80,y:50,..full};
    assert!(!eligible(old,2));assert!(!eligible(old,3));
    let held=production::translated_resize_presentation(full,80,50,Some((2560,1440)));
    assert_eq!(held,full);assert!(eligible(held,2));assert!(eligible(held,3));
}
#[test] fn partial_hold_moves_only_within_bounds_and_keeps_exact_source_extent() {
    let old=WindowPlacement{x:100,y:200,width:1280,height:720};
    let held=production::translated_resize_presentation(old,2000,2000,Some((2560,1440)));
    assert_eq!(held,WindowPlacement{x:1280,y:720,..old});
    assert!(eligible(held,2));assert!(eligible(held,3));
    assert_eq!(production::translated_resize_presentation(old,-2000,-2000,Some((2560,1440))),WindowPlacement{x:0,y:0,..old});
    assert_eq!(production::translated_resize_presentation(old,200,200,None),old);
}
#[test] fn released_double_background_has_compositor_fallback_like_its_foreground() {
    assert!(production::frame_plan_shares_compositor_plane(FramePlan{content:FrameContent::BlueprintScene,cadence:FrameCadence::Dirty,buffering:FrameBuffering::Double}));
    assert!(production::frame_plan_shares_compositor_plane(FramePlan{content:FrameContent::BlueprintScene,cadence:FrameCadence::Streaming,buffering:FrameBuffering::Triple}));
}
'''

def main():
    functions=[]
    for path,name in [('src/ui4/window_broker.rs','translated_resize_presentation'),('src/ui4/compositor_service.rs','direct_overlay_geometry_eligible'),('src/ui4/compositor_service.rs','bounded_direct_plane_scale'),('src/ui4/mod.rs','frame_plan_shares_compositor_plane')]:
        function=item(path,name)
        function=function.replace('fn '+name,'pub(super) fn '+name) if not function.startswith('pub') else function
        functions.append(function)
    source=HARNESS.replace('ITEMS','\n'.join(functions))
    with tempfile.TemporaryDirectory(prefix='ui4-restore-') as directory:
        path=Path(directory)/'tests.rs';path.write_text(source)
        binary=Path(directory)/'tests'
        subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)

if __name__=='__main__':main()
