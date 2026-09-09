#!/usr/bin/env python3
"""Host-test the production two-producer resize commit and rollback boundary."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[1]
SOURCE = "src/ui4/blueprint_text.rs"

HARNESS = r'''
#![allow(dead_code)]
use std::cell::RefCell;
type WindowOwner = u32;
type FrameHandle = u32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] struct WindowId(u32);
impl WindowId { fn raw(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] struct WindowPlacement { x:i32, y:i32, width:u32, height:u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] enum WindowBrokerError { StaleResize, InvalidHandle }
#[derive(Clone, Copy)] struct BlueprintPendingResize {
    previous_frame:FrameHandle, previous_width:u32, previous_height:u32,
    previous_placement:WindowPlacement, placement:WindowPlacement, resize_epoch:u64,
}
struct BlueprintSceneSurface {
    owner:WindowOwner, window:WindowId, render_target:u32, frame:FrameHandle,
    width:u32, height:u32, placement:WindowPlacement,
    pending_resize:Option<BlueprintPendingResize>, pending_resize_ready:bool,
}
const ERROR_STATE:i32 = -1;
const ERROR_UI4:i32 = -2;
thread_local! {
    static RETIRED:RefCell<Vec<u32>> = RefCell::new(Vec::new());
    static COMMITS:RefCell<Vec<(u32,u32)>> = RefCell::new(Vec::new());
    static RESULT:RefCell<Result<(),WindowBrokerError>> = RefCell::new(Ok(()));
}
fn retire_frame_when_released(frame:u32) { RETIRED.with(|r|r.borrow_mut().push(frame)); }
mod window_broker {
    use super::*;
    pub(super) fn commit_window_layered_replacement(_:WindowOwner,_:WindowId,foreground:u32,background:u32,_:WindowPlacement,_:u64)->Result<(),WindowBrokerError> {
        COMMITS.with(|c|c.borrow_mut().push((foreground,background)));
        RESULT.with(|r|*r.borrow())
    }
}
mod producer {
    use super::*;
    ITEMS
    pub(super) fn commit(s:&mut [BlueprintSceneSurface])->i32 { commit_layered_resize_if_ready(s,1,WindowId(1)) }
}
fn pair()->Vec<BlueprintSceneSurface> {
    (0..2).map(|layer| {
        let old = WindowPlacement{x:5,y:6,width:100,height:100};
        let new = WindowPlacement{x:7,y:8,width:200,height:150};
        BlueprintSceneSurface {
            owner:1,window:WindowId(1),render_target:if layer==0 {1}else{0x8001},
            frame:20+layer,width:200,height:150,placement:new,pending_resize_ready:false,
            pending_resize:Some(BlueprintPendingResize{previous_frame:10+layer,
                previous_width:100,previous_height:100,previous_placement:old,placement:new,resize_epoch:9}),
        }
    }).collect()
}
#[test] fn either_producer_may_finish_first_without_replacing_half_the_window() {
    for first in 0..2 {
        RETIRED.with(|r|r.borrow_mut().clear()); COMMITS.with(|r|r.borrow_mut().clear());
        let mut s=pair(); s[first].pending_resize_ready=true;
        assert_eq!(producer::commit(&mut s),0);
        COMMITS.with(|c|assert!(c.borrow().is_empty())); RETIRED.with(|r|assert!(r.borrow().is_empty()));
        assert!(s.iter().all(|s|s.pending_resize.is_some()));
        s[1-first].pending_resize_ready=true;
        assert_eq!(producer::commit(&mut s),0);
        COMMITS.with(|c|assert_eq!(*c.borrow(),vec![(20,21)]));
        RETIRED.with(|r|assert_eq!(*r.borrow(),vec![10,11]));
        assert!(s.iter().all(|s|s.pending_resize.is_none()&&!s.pending_resize_ready));
        assert_eq!((s[0].frame,s[1].frame),(20,21));
    }
}
#[test] fn superseded_resize_restores_both_old_fronts_and_remains_recoverable() {
    RESULT.with(|r|*r.borrow_mut()=Err(WindowBrokerError::StaleResize));
    let mut s=pair(); for member in &mut s { member.pending_resize_ready=true; }
    assert_eq!(producer::commit(&mut s),0);
    RETIRED.with(|r|assert_eq!(*r.borrow(),vec![20,21]));
    assert_eq!((s[0].frame,s[1].frame),(10,11));
    assert!(s.iter().all(|s|s.width==100&&s.height==100&&s.placement.x==5&&s.pending_resize.is_none()&&!s.pending_resize_ready));
}
#[test] fn failed_commit_retires_replacements_but_keeps_both_live_fronts() {
    RESULT.with(|r|*r.borrow_mut()=Err(WindowBrokerError::InvalidHandle));
    let mut s=pair(); for member in &mut s { member.pending_resize_ready=true; }
    assert_eq!(producer::commit(&mut s),ERROR_UI4);
    RETIRED.with(|r|assert_eq!(*r.borrow(),vec![20,21]));
    assert_eq!((s[0].frame,s[1].frame),(10,11));
}
#[test] fn mismatched_resize_epochs_cannot_commit_a_pair() {
    let mut s=pair(); for member in &mut s { member.pending_resize_ready=true; }
    s[1].pending_resize.as_mut().unwrap().resize_epoch+=1;
    assert_eq!(producer::commit(&mut s),ERROR_STATE);
    COMMITS.with(|c|assert!(c.borrow().is_empty()));
    RETIRED.with(|r|assert!(r.borrow().is_empty()));
}
'''

def main():
    functions = "\n".join(item(SOURCE, name) for name in (
        "commit_layered_resize_if_ready", "revert_blueprint_pending_resize"))
    with tempfile.TemporaryDirectory(prefix="ui4-layered-resize-") as directory:
        source = Path(directory) / "tests.rs"
        source.write_text(HARNESS.replace("ITEMS", functions))
        binary = Path(directory) / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

if __name__ == "__main__":
    main()
