#!/usr/bin/env python3
"""Execute production UI4 queue/poll admission while a peer holds its runtime.

GPU preparation/probing tails are unreachable hardware mocks. Validation,
try-lock admission, pending-owner lookup, and the public completion wrappers
come from the driver, so replacing try_lock with lock makes these tests time out.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / 'src/intel/gpgpu/operations/ui4.rs').read_text()


def function(name):
    start = SOURCE.index(f'fn {name}(')
    if SOURCE[max(0, start - 11):start] == 'pub(crate) ':
        start -= 11
    opening = SOURCE.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (SOURCE[end] == '{') - (SOURCE[end] == '}')
        end += 1
    return SOURCE[start:end]


def admission(name, hardware_start):
    code = function(name)
    return code[:code.index(hardware_start)] + 'panic!("unexpected hardware access in contention test")\n}\n'


head = r'''
#![allow(dead_code,unused_variables,unused_mut)]
use std::collections::VecDeque;
use std::sync::{mpsc,Mutex as HostMutex,MutexGuard,atomic::{AtomicUsize,Ordering}};
use std::time::Duration;
struct Mutex<T>(HostMutex<T>);
impl<T> Mutex<T> {
 const fn new(value:T)->Self{Self(HostMutex::new(value))}
 fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()}
 fn try_lock(&self)->Option<MutexGuard<'_,T>>{self.0.try_lock().ok()}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]struct Ui4CompositorSubmission{serial:u64}
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]struct Probe{completion_polls:u64}
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]struct GpgpuWorklistSubmitStats{probe:Probe}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Ui4CompositorSubmitError{Busy,InvalidWorklist}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Ui4CompositorCompletion{Pending,Complete(GpgpuWorklistSubmitStats),InvalidSubmission}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Ui4VideoFrameCompletion{Pending,Complete{stats:GpgpuWorklistSubmitStats,release:u64},Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Ui4SpriteSceneCompletion{Pending,Complete{stats:GpgpuWorklistSubmitStats,release:u64},Failed}
#[derive(Clone,Copy)]struct Ui4CompositorPending{submission:Ui4CompositorSubmission,job_slot:usize,stats:GpgpuWorklistSubmitStats}
struct Ui4CompositorRuntime{pending:VecDeque<Ui4CompositorPending>,completions:VecDeque<(Ui4CompositorSubmission,Ui4CompositorCompletion)>}
static UI4_COMPOSITOR_RUNTIME:Mutex<Ui4CompositorRuntime>=Mutex::new(Ui4CompositorRuntime{pending:VecDeque::new(),completions:VecDeque::new()});
static RELEASES:AtomicUsize=AtomicUsize::new(0);
static OBSERVATIONS:AtomicUsize=AtomicUsize::new(0);
const UI4_COMPOSITOR_RCS_JOB_SLOTS:usize=2;
const UI4_COMPOSITOR_NV12_SOURCE_MAX_BYTES:usize=64*1024*1024;
const ALPHA_BLEND_WORKLIST_FLAG_COPY:u32=1;
const ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER:u32=2;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB:u32=4;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA:u32=8;
const ALPHA_BLEND_WORKLIST_FLAG_PREMUL_SRC:u32=16;
const ALPHA_BLEND_WORKLIST_MAX_DESCS:usize=256;
#[derive(Clone,Copy)]struct GpgpuRgba8Surface{gpu:u64,phys:u64,bytes:usize,width:u32,height:u32}
impl GpgpuRgba8Surface{fn is_valid(self)->bool{self.width>0&&self.height>0}}
#[derive(Clone,Copy)]struct GpgpuNv12Tile64Surface{phys:u64,bytes:usize,width:u32,height:u32}
impl GpgpuNv12Tile64Surface{fn is_valid(self)->bool{self.width>0&&self.height>0}}
#[derive(Clone,Copy)]struct GpgpuAlphaBlendWorklistDesc{src_xy:u32,dst_xy:u32,size:u32,flags:u32}
fn observe_ui4_pending_h2g_consumption(_: &mut Ui4CompositorPending){OBSERVATIONS.fetch_add(1,Ordering::SeqCst);}
fn gpgpu_rgba8_release(_:GpgpuRgba8Surface)->u64{RELEASES.fetch_add(1,Ordering::SeqCst);42}
'''
production = '\n'.join([
    function('gpu_ranges_overlap'),
    admission('queue_ui4_blueprint_alpha_rects', '    let dev ='),
    admission('queue_ui4_video_frame_nv12_tile64_to_rgba8', '    let source_gpu ='),
    admission('poll_ui4_compositor_submission', '    let Some(shared_state) ='),
    function('poll_ui4_video_frame_submission'),
    function('poll_ui4_blueprint_sprite_scene'),
])
tests = r'''
fn rgba(base:u64)->GpgpuRgba8Surface{GpgpuRgba8Surface{gpu:base,phys:base,bytes:65536,width:128,height:128}}
fn nv12()->GpgpuNv12Tile64Surface{GpgpuNv12Tile64Surface{phys:0x100000,bytes:32768,width:128,height:128}}
fn descriptor()->GpgpuAlphaBlendWorklistDesc{GpgpuAlphaBlendWorklistDesc{src_xy:0,dst_xy:0,size:128|(128<<16),flags:ALPHA_BLEND_WORKLIST_FLAG_COPY}}
fn submission(serial:u64)->Ui4CompositorSubmission{Ui4CompositorSubmission{serial}}
fn reset(){let mut runtime=UI4_COMPOSITOR_RUNTIME.lock();runtime.pending.clear();runtime.completions.clear();RELEASES.store(0,Ordering::SeqCst);OBSERVATIONS.store(0,Ordering::SeqCst);}
fn held_by_peer<T:Send+'static>(call:impl FnOnce()->T+Send+'static)->T {
 let(held_tx,held_rx)=mpsc::channel();let(release_tx,release_rx)=mpsc::channel();
 let owner=std::thread::spawn(move||{let _guard=UI4_COMPOSITOR_RUNTIME.lock();held_tx.send(()).unwrap();release_rx.recv().unwrap();});
 held_rx.recv_timeout(Duration::from_secs(2)).unwrap();
 let(result_tx,result_rx)=mpsc::channel();
 let caller=std::thread::spawn(move||{let result=call();let _=result_tx.send(result);});
 let result=result_rx.recv_timeout(Duration::from_millis(250));
 // Always release the peer before asserting, so a lock regression fails cleanly
 // instead of leaving a permanently blocked test process.
 release_tx.send(()).unwrap();owner.join().unwrap();caller.join().unwrap();
 result.expect("production GPU admission/poll spun behind a peer runtime lock")
}
#[test]fn blueprint_queue_returns_busy_without_waiting_for_peer_preparation(){
 reset();assert_eq!(held_by_peer(||queue_ui4_blueprint_alpha_rects(rgba(0x100000),rgba(0x200000),&[descriptor()])),Err(Ui4CompositorSubmitError::Busy));
 assert!(UI4_COMPOSITOR_RUNTIME.lock().pending.is_empty());
}
#[test]fn video_queue_returns_busy_without_waiting_for_peer_preparation(){
 reset();assert_eq!(held_by_peer(||queue_ui4_video_frame_nv12_tile64_to_rgba8(nv12(),rgba(0x200000),0,0,128,128,0,0,false,1)),Err(Ui4CompositorSubmitError::Busy));
 assert!(UI4_COMPOSITOR_RUNTIME.lock().pending.is_empty());
}
#[test]fn contention_preserves_exact_pending_submission_and_mints_no_release(){
 reset();let original=Ui4CompositorPending{submission:submission(7),job_slot:0,stats:GpgpuWorklistSubmitStats::default()};
 UI4_COMPOSITOR_RUNTIME.lock().pending.push_back(original);
 assert_eq!(held_by_peer(||poll_ui4_compositor_submission(submission(7))),Ui4CompositorCompletion::Pending);
 assert_eq!(held_by_peer(||poll_ui4_video_frame_submission(submission(7),rgba(0x200000))),Ui4VideoFrameCompletion::Pending);
 assert_eq!(held_by_peer(||poll_ui4_blueprint_sprite_scene(submission(7),rgba(0x200000))),Ui4SpriteSceneCompletion::Pending);
 let runtime=UI4_COMPOSITOR_RUNTIME.lock();assert_eq!(runtime.pending.len(),1);assert_eq!(runtime.pending[0].submission,original.submission);assert!(runtime.completions.is_empty());assert_eq!(RELEASES.load(Ordering::SeqCst),0);assert_eq!(OBSERVATIONS.load(Ordering::SeqCst),0);
}
#[test]fn cached_retirement_remains_visible_after_contention_without_consuming_it(){
 reset();let completion=Ui4CompositorCompletion::Complete(GpgpuWorklistSubmitStats::default());
 UI4_COMPOSITOR_RUNTIME.lock().completions.push_back((submission(9),completion));
 assert_eq!(held_by_peer(||poll_ui4_compositor_submission(submission(9))),Ui4CompositorCompletion::Pending);
 assert_eq!(poll_ui4_compositor_submission(submission(9)),completion);
 assert!(matches!(poll_ui4_video_frame_submission(submission(9),rgba(0x200000)),Ui4VideoFrameCompletion::Complete{..}));
 assert_eq!(UI4_COMPOSITOR_RUNTIME.lock().completions.len(),1);
}
#[test]fn unknown_submission_is_pending_only_during_contention(){
 reset();assert_eq!(held_by_peer(||poll_ui4_compositor_submission(submission(55))),Ui4CompositorCompletion::Pending);assert_eq!(poll_ui4_compositor_submission(submission(55)),Ui4CompositorCompletion::InvalidSubmission);
}
#[test]fn invalid_inputs_are_rejected_without_runtime_admission(){
 reset();assert_eq!(held_by_peer(||queue_ui4_blueprint_alpha_rects(rgba(0x100000),rgba(0x200000),&[])),Err(Ui4CompositorSubmitError::InvalidWorklist));
 assert_eq!(held_by_peer(||queue_ui4_video_frame_nv12_tile64_to_rgba8(nv12(),rgba(0x200000),0,0,129,128,0,0,false,1)),Err(Ui4CompositorSubmitError::InvalidWorklist));
}
'''
with tempfile.TemporaryDirectory(prefix='ui4-contention-') as directory:
    path = Path(directory)
    (path / 'test.rs').write_text(head + production + tests)
    subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test'), '--test-threads=1'], check=True, timeout=20)
