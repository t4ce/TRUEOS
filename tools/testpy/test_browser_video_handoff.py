#!/usr/bin/env python3
"""Exercise the production browser playback coordinator with host GPU mocks."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'src/r/services/video_open_service.rs').read_text()
source = re.sub(r'#\[trueos_executor::task\(pool_size\s*=\s*3\)\]', '', source)
source = source.replace('async fn browser_prepare_task(', 'async fn browser_prepare_body(')
source = source.replace('async fn playback_task(', 'async fn playback_body(')
head = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
pub use std::time::{Duration,Instant};
pub struct Timer;
impl Timer {pub async fn after(_:Duration){}}
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{pub const fn new(value:T)->Self{Self(std::sync::Mutex::new(value))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
use std::sync::atomic::{AtomicU64,Ordering};
static CANCELLED:AtomicU64=AtomicU64::new(0);
static STOPPED:AtomicU64=AtomicU64::new(0);
static PLAYBACK_FAILURE:Mutex<Option<&'static str>>=Mutex::new(None);
static PLAYBACK_SOURCES:Mutex<Vec<(String,Option<String>)>>=Mutex::new(Vec::new());
static LOG_LINES:Mutex<Vec<String>>=Mutex::new(Vec::new());
mod allcaps{pub mod hv{pub const VM_ID_LIMIT:usize=64;}}
mod ui4{
 #[derive(Clone,Copy,PartialEq,Eq,Debug)]pub struct VideoPlaybackSession(pub u8);
 impl VideoPlaybackSession{
  pub fn cancel(self){crate::CANCELLED.fetch_or(1<<self.0,crate::Ordering::SeqCst);}
  pub fn is_cancelled(self)->bool{crate::CANCELLED.load(crate::Ordering::SeqCst)&(1<<self.0)!=0}
  pub fn finish(self,_:&str)->bool{self.cancel();true}
 }
 pub fn reserve_shell_decoded_video_player()->Option<VideoPlaybackSession>{None}
 pub fn open_shell_decoded_video_player(_:VideoPlaybackSession,_:u32,_:u32)->bool{true}
 pub fn stop_decoded_nv12_stream(s:VideoPlaybackSession,_:&str)->bool{s.cancel();crate::STOPPED.fetch_or(1<<s.0,crate::Ordering::SeqCst);true}
 pub const DEFAULT_FRAME_WIDTH:u32=1280;pub const DEFAULT_FRAME_HEIGHT:u32=720;
}
mod workers{pub struct Worker;pub fn pick_background_spawner()->Option<Worker>{Some(Worker)}impl Worker{pub fn spawn<T>(&self,_:T){}}}
mod shell3{#[derive(Clone)]pub struct MatrixTarget;pub fn matrix_target_print_line(_:&MatrixTarget,_:&str){}}
mod hv{pub fn with_guest_broker_context<T>(_:u8,f:impl FnOnce()->T)->T{f()}pub fn blueprint_console_target(_:u8)->Option<crate::shell3::MatrixTarget>{None}}
mod r{pub mod io{pub mod env{pub fn trueosfs_scope_granted()->bool{true}pub fn resolve_fs_path(s:&str,_:bool)->Option<String>{Some(s.into())}}pub mod async_fs_cabi{pub fn owner_for_vm(vm:u8)->u32{0x80000000|vm as u32}}}pub mod path{pub struct FsPath;impl FsPath{pub fn parse(_:&str,_:bool)->Result<(),()>{Ok(())}}}}
mod log_os{pub fn blueprint_important_line(line:std::fmt::Arguments<'_>){crate::LOG_LINES.lock().push(line.to_string());}}
mod intel{pub mod media{pub mod hw_vid{
 pub struct Asset;impl Asset{pub fn visible_extent(&self)->(u32,u32){(1280,720)}}
 pub async fn prepare_trueosfs_ui4_video(_:crate::ui4::VideoPlaybackSession,_:&str)->Result<Asset,&'static str>{Ok(Asset)}
 pub async fn run_prepared_trueosfs_ui4_video(_:crate::ui4::VideoPlaybackSession,_:&str,_:Asset)->Result<(),&'static str>{Ok(())}
 pub async fn run_resolved_ui4_framed_video_playback(_:crate::ui4::VideoPlaybackSession,_:&str)->Result<(),&'static str>{Ok(())}
 pub async fn run_resolved_ui4_framed_video_playback_with_referrer(_:crate::ui4::VideoPlaybackSession,url:&str,referrer:Option<&str>)->Result<(),&'static str>{
  crate::PLAYBACK_SOURCES.lock().push((url.into(),referrer.map(String::from)));
  match *crate::PLAYBACK_FAILURE.lock(){Some(error)=>Err(error),None=>Ok(())}
 }
}}}
mod service {
'''
tail = r'''
fn browser_prepare_task(_:u32)->Result<(),()>{Ok(())}
fn playback_task(_:Source,_:Session,_:Option<crate::shell3::MatrixTarget>)->Result<(),()>{Ok(())}
#[cfg(test)]mod tests{
 use super::*;use crate::{CANCELLED,STOPPED};
 fn reset(){BROWSERS.lock().clear();NEXT_REQUEST.store(1,Ordering::Relaxed);CANCELLED.store(0,Ordering::SeqCst);STOPPED.store(0,Ordering::SeqCst);*crate::PLAYBACK_FAILURE.lock()=None;crate::PLAYBACK_SOURCES.lock().clear();crate::LOG_LINES.lock().clear();}
 fn request(owner:u32)->u32{let id=browser_request(owner,b"https://example.test/video.mp4");assert!(id>0);id as u32}
 fn candidate(owner:u32,active:Option<u8>,pending:u8){let mut states=BROWSERS.lock();let s=states.iter_mut().find(|s|s.owner==owner).unwrap();s.active=active.map(VideoPlaybackSession);s.pending=Some(VideoPlaybackSession(pending));s.queued=None;}
 #[test]fn switches_only_after_successful_first_publication(){
  reset();let id=request(1);candidate(1,Some(1),2);
  let result=publish_browser_frame(VideoPlaybackSession(2),||{assert_eq!(STOPPED.load(Ordering::SeqCst),0);Err::<(),_>("not ready")});
  assert_eq!(result,Some(Err("not ready")));assert_eq!(browser_status(1,id),0);assert!(!VideoPlaybackSession(1).is_cancelled());
  assert_eq!(publish_browser_frame(VideoPlaybackSession(2),||Ok::<_,()>(42)),Some(Ok(42)));
  assert_eq!(browser_status(1,id),1);assert!(VideoPlaybackSession(1).is_cancelled());assert!(!VideoPlaybackSession(2).is_cancelled());
  assert_eq!(STOPPED.load(Ordering::SeqCst),1<<1);
 }
 #[test]fn newest_candidate_wins_and_failure_preserves_active(){
  reset();let first=request(1);candidate(1,Some(1),2);let newer=request(1);
  assert!(VideoPlaybackSession(2).is_cancelled());assert!(!VideoPlaybackSession(1).is_cancelled());assert_eq!(browser_status(1,first),-125);
  assert_eq!(publish_browser_frame(VideoPlaybackSession(2),||panic!("stale publication")),None::<Result<(),()>>);
  candidate(1,Some(1),3);browser_finished(VideoPlaybackSession(3));assert_eq!(browser_status(1,newer),-2);assert!(!VideoPlaybackSession(1).is_cancelled());
 }
 #[test]fn late_retirement_and_other_owners_do_not_cancel_replacement(){
  reset();let id=request(1);candidate(1,Some(1),2);let other=request(2);candidate(2,Some(3),4);
  publish_browser_frame(VideoPlaybackSession(2),||Ok::<_,()>(()));browser_finished(VideoPlaybackSession(1));
  assert_eq!(browser_status(1,id),1);assert_eq!(browser_status(2,other),0);assert!(!VideoPlaybackSession(3).is_cancelled());
  assert_eq!(BROWSERS.lock()[0].active,Some(VideoPlaybackSession(2)));
 }
 #[test]fn source_validation_and_pending_requests_are_bounded(){
  reset();for bad in ["http://example.test/video.mp4","https://","https://user@example.test/a","https://example.test/a\n"]{assert!(browser_request(1,bad.as_bytes())<0);}
  for _ in 0..300{request(1);}assert_eq!(BROWSERS.lock().len(),1);assert!(BROWSERS.lock()[0].queued.is_some());
 }
 fn finish_immediate(future:impl core::future::Future<Output=()>){
  let mut future=Box::pin(future);
  let mut context=core::task::Context::from_waker(core::task::Waker::noop());
  assert!(future.as_mut().poll(&mut context).is_ready(),"mocked native playback should complete in one poll");
 }
 #[test]fn native_fetch_or_container_failure_reaches_browser_status_and_keeps_old_frame(){
  for error in ["online media fetch failed","HLS encrypted, fragmented or discontinuous stream unsupported"]{
   reset();let id=request(1);candidate(1,Some(1),2);*crate::PLAYBACK_FAILURE.lock()=Some(error);
   finish_immediate(playback_body(Source::Browser(browser_source(b"https://cdn.example.test/video.mp4\0https://page.example.test/watch").unwrap()),Session(VideoPlaybackSession(2)),None));
   assert_eq!(browser_status(1,id),-2);
   assert_eq!(BROWSERS.lock()[0].pending,None);
   assert_eq!(BROWSERS.lock()[0].active,Some(VideoPlaybackSession(1)));
   assert!(!VideoPlaybackSession(1).is_cancelled());assert!(VideoPlaybackSession(2).is_cancelled());
   assert_eq!(&*crate::PLAYBACK_SOURCES.lock(),&[("https://cdn.example.test/video.mp4".into(),Some("https://page.example.test/watch".into()))]);
   assert!(crate::LOG_LINES.lock().iter().any(|line|line.contains(error)));
  }
 }
 #[test]fn browser_referrer_is_validated_without_modifying_media_url(){
  reset();
  for bad in ["https://cdn.example.test/video.mp4\0http://page.example.test/","https://cdn.example.test/video.mp4\0https://page.example.test/#fragment","https://cdn.example.test/video.mp4\0https://page.example.test/\0extra"]{assert!(browser_request(1,bad.as_bytes())<0);}
  assert!(browser_request(1,b"https://cdn.example.test/video.mp4?signature=unchanged\0https://page.example.test/watch")>0);
  let states=BROWSERS.lock();let source=states[0].queued.as_ref().unwrap();
  assert_eq!(source.url,"https://cdn.example.test/video.mp4?signature=unchanged");
  assert_eq!(source.referrer.as_deref(),Some("https://page.example.test/watch"));
 }
}}
'''
with tempfile.TemporaryDirectory(prefix='browser-video-policy-') as directory:
    path = Path(directory)
    (path / 'test.rs').write_text(head + source + tail)
    subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)
