#!/usr/bin/env python3
"""Exercise production video resize staging, publication, and retirement on host."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[2]
PATH = 'src/ui4/video_frame.rs'


def main():
    harness = r'''#![allow(dead_code)]
use std::cell::RefCell;
type FrameHandle = u32;
type WindowId = u32;
type WindowSessionId = u32;
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowPlacement { width: u32, height: u32 }
#[derive(Clone, Copy, Debug, PartialEq)]
enum WindowBrokerError { Closed, Capacity, StaleResize }
struct DamageRect;
impl DamageRect { const FULL: Self = Self; }
const VIDEO_OWNER: u32 = 1;
#[derive(Default)]
struct Broker {
    extent: (u32, u32), epoch: u64, frame: u32, next: u32,
    fail_allocation: bool, closed: bool, published: usize, retired: Vec<u32>,
}
thread_local! { static BROKER: RefCell<Broker> = RefCell::new(Broker::default()); }
fn window_resize_state(_:u32, _:u32) -> Result<(WindowPlacement,u64),WindowBrokerError> {
    BROKER.with(|b| { let b=b.borrow();
        if b.closed { return Err(WindowBrokerError::Closed); }
        Ok((WindowPlacement{width:b.extent.0,height:b.extent.1},b.epoch))
    })
}
fn commit_window_frame_replacement(_:u32,_:u32,frame:u32,p:WindowPlacement,e:u64,_:DamageRect)
    -> Result<u64,WindowBrokerError> {
    BROKER.with(|b| { let mut b=b.borrow_mut();
        if b.closed { return Err(WindowBrokerError::Closed); }
        if e!=b.epoch && (p.width,p.height)!=b.extent { return Err(WindowBrokerError::StaleResize); }
        b.frame=frame; b.published+=1; Ok(b.published as u64)
    })
}
fn publish_window_frame(_:u32,_:u32,_:DamageRect)->Result<u64,WindowBrokerError> {
    BROKER.with(|b| {let mut b=b.borrow_mut(); b.published+=1;Ok(b.published as u64)})
}
fn create_video_frame(_:u32,_:u32)->Result<u32,()> {
    BROKER.with(|b| {let mut b=b.borrow_mut(); if b.fail_allocation {return Err(());}
        b.next+=1;Ok(b.next)})
}
fn retire_video_frame(frame:u32) {BROKER.with(|b| b.borrow_mut().retired.push(frame));}
mod video {
use super::*;
'''
    # Keep the real stream, geometry, ownership guard, and transaction helpers.
    for name in ['VideoStream', 'NativeViewportLayout', 'RetireVideoFrame',
                 'stage_video_resize', 'publish_video_conversion', 'native_viewport_layout']:
        harness += item(PATH, name) + '\n'
    harness += r'''
fn setup(width:u32,height:u32) -> VideoStream {
    BROKER.with(|b| *b.borrow_mut()=Broker{extent:(width,height),epoch:1,frame:1,next:1,..Default::default()});
    VideoStream{session:1,frame:1,window:1,source_width:1920,source_height:1080,
        visible_width:1920,visible_height:1080,frame_width:1920,frame_height:1080,
        pan_x:0,pan_y:0,active_pan_source:None}
}
#[test] fn commit_waits_for_completed_picture_and_retires_old_frame() {
    let mut live=setup(960,540);let mut rendered=live;
    let (mut guard,p,e)=stage_video_resize(&mut rendered).unwrap();
    BROKER.with(|b| assert_eq!((b.borrow().frame,b.borrow().published),(1,0)));
    live.pan_x=100;
    publish_video_conversion(&mut live,rendered,&mut guard,p,e).unwrap();
    assert_eq!((live.frame_width,live.frame_height,live.pan_x),(960,540,100));
    BROKER.with(|b| assert_eq!(b.borrow().frame,live.frame));
    drop(guard);BROKER.with(|b| assert_eq!(b.borrow().retired,vec![1]));
}
#[test] fn superseded_resize_keeps_live_picture_and_cleans_candidate() {
    let mut live=setup(960,540);let mut rendered=live;
    let (mut guard,p,e)=stage_video_resize(&mut rendered).unwrap();
    BROKER.with(|b| {let mut b=b.borrow_mut();b.extent=(800,600);b.epoch+=1;});
    assert_eq!(publish_video_conversion(&mut live,rendered,&mut guard,p,e),Err(WindowBrokerError::StaleResize));
    assert_eq!(live.frame,1);drop(guard);
    BROKER.with(|b| {let b=b.borrow();assert_eq!(b.frame,1);assert_eq!(b.retired,vec![2]);});
    let mut retry=live;let (mut guard,p,e)=stage_video_resize(&mut retry).unwrap();
    publish_video_conversion(&mut live,retry,&mut guard,p,e).unwrap();
    assert_eq!((live.frame_width,live.frame_height),(800,600));
}
#[test] fn preceding_lane_replacement_forces_old_lane_to_reconvert() {
    let mut live=setup(960,540);let old=live;let mut rendered=live;
    let (mut guard,p,e)=stage_video_resize(&mut rendered).unwrap();
    publish_video_conversion(&mut live,rendered,&mut guard,p,e).unwrap();
    assert_eq!(publish_video_conversion(&mut live,old,&mut RetireVideoFrame(None),p,e),Err(WindowBrokerError::StaleResize));
}
#[test] fn allocation_failure_and_close_preserve_live_frame() {
    let mut live=setup(960,540);let mut rendered=live;
    BROKER.with(|b| b.borrow_mut().fail_allocation=true);
    let (mut fallback,p,e)=stage_video_resize(&mut rendered).unwrap();
    assert!(fallback.0.is_none());assert_eq!(rendered.frame,1);
    publish_video_conversion(&mut live,rendered,&mut fallback,p,e).unwrap();
    BROKER.with(|b| b.borrow_mut().fail_allocation=false);
    let (mut guard,p,e)=stage_video_resize(&mut rendered).unwrap();
    BROKER.with(|b| b.borrow_mut().closed=true);
    assert_eq!(publish_video_conversion(&mut live,rendered,&mut guard,p,e),Err(WindowBrokerError::Closed));
    drop(guard);BROKER.with(|b| assert_eq!(b.borrow().retired,vec![2]));
}
#[test] fn unchanged_extent_avoids_allocation_and_larger_viewport_centers() {
    let mut live=setup(1920,1080);
    let (guard,_,_)=stage_video_resize(&mut live).unwrap();assert!(guard.0.is_none());
    let layout=native_viewport_layout(1920,1080,2560,1440,99,99).unwrap();
    assert_eq!((layout.destination_x,layout.destination_y,layout.width,layout.height),(320,180,1920,1080));
    assert_eq!((layout.source_x,layout.source_y),(0,0));
    let crop=native_viewport_layout(1920,1080,960,540,2000,2000).unwrap();
    assert_eq!((crop.source_x,crop.source_y,crop.width,crop.height),(960,540,960,540));
}
}
type Ui4CursorSource = u32;
'''
    with tempfile.TemporaryDirectory(prefix='ui4-video-resize-') as directory:
        path = Path(directory) / 'tests.rs'
        path.write_text(harness)
        binary = Path(directory) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
