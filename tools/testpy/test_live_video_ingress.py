#!/usr/bin/env python3
"""Exercise production live-video admission and transactions with host mocks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'src/r/services/live_video_service.rs').read_text()
source = source.replace('#[trueos_executor::task(pool_size = 3)]', '')
source = source.replace('async fn playback_task(', 'async fn playback_body(')
source = source.replace('"live_video_queue.rs"', '"' + str(ROOT / 'src/r/services/live_video_queue.rs') + '"')
head = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
pub use std::time::Duration;
pub struct Timer;
impl Timer { pub async fn after(_: Duration) {} }
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
use std::sync::atomic::{AtomicU32,Ordering};
static CANCELLED: AtomicU32 = AtomicU32::new(0);
static NEXT_SESSION: AtomicU32 = AtomicU32::new(0);
#[macro_export] macro_rules! log_warn { ($($args:tt)*) => {} }
mod ui4 {
    #[derive(Clone,Copy,PartialEq,Eq)] pub struct VideoPlaybackSession(pub u32);
    impl VideoPlaybackSession {
        pub fn cancel(self) { crate::CANCELLED.fetch_or(1 << self.0,crate::Ordering::SeqCst); }
        pub fn is_cancelled(self)->bool { crate::CANCELLED.load(crate::Ordering::SeqCst) & (1 << self.0) != 0 }
        pub fn finish(self,_:&str) { self.cancel(); }
    }
    pub fn reserve_shell_decoded_video_player()->Option<VideoPlaybackSession> {
        Some(VideoPlaybackSession(crate::NEXT_SESSION.fetch_add(1,crate::Ordering::SeqCst)))
    }
}
mod workers {
    pub struct Worker;
    pub fn pick_background_spawner()->Option<Worker> { Some(Worker) }
    impl Worker { pub fn spawn<T>(&self,_:T) {} }
}
mod intel {pub mod media {pub mod hw_vid {
    pub async fn run_live_ui4_video_playback(_:crate::ui4::VideoPlaybackSession,_:u32,_:u16)->Result<(),&'static str> { Ok(()) }
}}}
mod service {
'''
tail = r'''
fn playback_task(_:u32,_:VideoPlaybackSession,_:u16)->Result<(),()> { Ok(()) }
#[cfg(test)] mod tests {
    use super::*;
    const IDR:&[u8] = &[0,0,0,1,0x67,1,0,0,1,0x68,1,0,0,1,0x65,1];
    fn reset() { STREAMS.lock().clear(); NEXT.store(1,Ordering::Relaxed); crate::CANCELLED.store(0,Ordering::Relaxed); crate::NEXT_SESSION.store(0,Ordering::Relaxed); }
    fn call(owner:u32,op:u32,a:u64,b:u64,input:&[u8])->i32 { command(owner,op,a,b,input,&mut []) }
    fn open(owner:u32)->u64 { let id=call(owner,10,30,0,&[]);assert!(id>0);id as u64 }
    #[test] fn commands_are_scoped_to_the_creating_owner() {
        reset();let id=open(1);
        for op in 11..=16 { assert_eq!(call(2,op,id,0,&[]),-1); }
        assert_eq!(call(1,14,id,0,&[]),0);
        assert_eq!(call(1,11,id,IDR.len() as u64,&[]),0);
        assert_eq!(call(1,12,id,0,IDR),0);
        assert_eq!(call(1,13,id,0,&[]),0);
        assert_eq!(STREAMS.lock()[0].units.pop().unwrap(),IDR);
    }
    #[test] fn close_and_owner_teardown_revoke_queued_and_staged_bytes() {
        reset();let first=open(1);let second=open(2);
        assert_eq!(call(1,11,first,IDR.len() as u64,&[]),0);
        assert_eq!(call(1,12,first,0,IDR),0);
        assert_eq!(call(1,13,first,0,&[]),0);
        assert_eq!(call(1,11,first,IDR.len() as u64,&[]),0);
        assert_eq!(call(1,15,first,0,&[]),0);
        assert_eq!(call(1,14,first,0,&[]),2);
        assert_eq!(call(1,13,first,0,&[]),-2);
        assert!(STREAMS.lock()[0].units.pop().is_none());
        assert_eq!(call(2,14,second,0,&[]),0);
        release_owner(2);
        assert_eq!(call(2,14,second,0,&[]),2);
    }
    #[test] fn validates_rate_chunk_size_and_global_admission() {
        reset();assert_eq!(call(1,10,0,0,&[]),INVALID);assert_eq!(call(1,10,61,0,&[]),INVALID);
        let id=open(1);open(2);open(3);assert_eq!(call(4,10,30,0,&[]),BUSY);
        assert_eq!(call(1,11,id,4096,&[]),0);
        assert_eq!(call(1,12,id,0,&vec![0;3073]),INVALID);
        assert_eq!(call(1,16,id,0,&[]),0);
        assert_eq!(call(1,13,id,0,&[]),INVALID);
    }
    #[test] fn status_reports_worker_phase_and_terminal_failure() {
        reset();let id=open(1);assert_eq!(call(1,14,id,0,&[]),0);
        decoder_started(id as u32);assert_eq!(call(1,14,id,0,&[]),1);
        STREAMS.lock()[0].error=-2;assert_eq!(call(1,14,id,0,&[]),-2);
        STREAMS.lock()[0].ended=true;assert_eq!(call(1,15,id,0,&[]),0);
        assert_eq!(call(1,14,id,0,&[]),-1);
    }
}}
'''
with tempfile.TemporaryDirectory(prefix='live-video-ingress-') as directory:
    path = Path(directory)
    (path / 'test.rs').write_text(head + source + tail)
    subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)
