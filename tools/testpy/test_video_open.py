#!/usr/bin/env python3
"""Exercise the host video bridge with deterministic playback/session services."""
from pathlib import Path
import subprocess
import tempfile
root=Path(__file__).resolve().parents[2]
source=(root/'src/r/services/video_open_service.rs').read_text()
source=source[source.index('use alloc::'):].replace('#[trueos_executor::task(pool_size = 3)]\n','').replace('async fn playback_task(', 'async fn playback_task_run(')
harness=r'''
extern crate alloc;
use std::sync::{Mutex,Arc};
struct State {scope:bool,occupied:usize,worker:bool,task:bool,cancelled:bool,window:bool,laps:usize,urls:usize,messages:Vec<String>}
static S:Mutex<State>=Mutex::new(State{scope:true,occupied:0,worker:true,task:true,cancelled:false,window:true,laps:0,urls:0,messages:Vec::new()});
fn run(f:impl std::future::Future<Output=()>) {struct Wake;impl std::task::Wake for Wake{fn wake(self:Arc<Self>){}}let w=std::task::Waker::from(Arc::new(Wake));let mut c=std::task::Context::from_waker(&w);assert!(Box::pin(f).as_mut().poll(&mut c).is_ready());}
mod workers {pub struct Worker;pub fn pick_background_spawner()->Option<Worker>{ready().then_some(Worker)} fn ready()->bool{crate::S.lock().unwrap().worker} impl Worker{pub fn spawn(&self,f:impl std::future::Future<Output=()>){crate::run(f)}}}
mod shell3 {#[derive(Clone)]pub struct MatrixTarget;pub fn matrix_target_print_line(_:&MatrixTarget,text:&str){crate::S.lock().unwrap().messages.push(text.into());}}
mod hv {pub fn with_guest_broker_context<T>(_:u8,f:impl FnOnce()->T)->T{f()}pub fn blueprint_console_target(_:u8)->Option<crate::shell3::MatrixTarget>{Some(crate::shell3::MatrixTarget)}}
mod log_os {pub fn blueprint_important_line(_:std::fmt::Arguments){}}
mod r {
    pub mod path {pub struct FsPath;impl FsPath{pub fn parse(path:&str,_:bool)->Result<Self,()>{if path.split('/').any(|s|s=="..") {Err(())} else {Ok(Self)}}}}
    pub mod io {pub mod env {pub fn trueosfs_scope_granted()->bool{crate::S.lock().unwrap().scope}pub fn resolve_fs_path(path:&str,_:bool)->Option<String>{Some(path.into())}}}
    pub mod services {pub mod video_open_service {
'''
stubs=r'''
    fn playback_task(source:Source,session:Session,target:Option<crate::shell3::MatrixTarget>)->Result<impl std::future::Future<Output=()>,()> {
        let future=playback_task_run(source,session,target);
        if crate::S.lock().unwrap().task {Ok(future)} else {Err(())}
    }
}}}
mod ui4 {
    pub const DEFAULT_FRAME_WIDTH:u32=640;pub const DEFAULT_FRAME_HEIGHT:u32=480;
    #[derive(Clone,Copy)]pub struct VideoPlaybackSession;
    impl VideoPlaybackSession {pub fn is_cancelled(self)->bool{crate::S.lock().unwrap().cancelled}pub fn finish(self,_:&str)->bool{crate::S.lock().unwrap().occupied-=1;true}}
    pub fn reserve_shell_decoded_video_player()->Option<VideoPlaybackSession>{let mut s=crate::S.lock().unwrap();if s.occupied==3 {None} else {s.occupied+=1;s.cancelled=false;Some(VideoPlaybackSession)}}
    pub fn open_shell_decoded_video_player(_:VideoPlaybackSession,w:u32,h:u32)->bool{assert!(matches!((w,h),(320,240)|(640,480)));crate::S.lock().unwrap().window}
}
mod intel {pub mod media {pub mod hw_vid {
    pub struct Prepared;impl Prepared{pub fn visible_extent(&self)->(u32,u32){(320,240)}}
    pub async fn prepare_trueosfs_ui4_video(_:crate::ui4::VideoPlaybackSession,_:&str)->Result<Prepared,&'static str>{Ok(Prepared)}
    pub async fn run_prepared_trueosfs_ui4_video(_:crate::ui4::VideoPlaybackSession,_:&str,_:Prepared)->Result<(),&'static str>{let mut s=crate::S.lock().unwrap();s.laps+=1;if s.laps==2{s.cancelled=true;}Ok(())}
    pub async fn run_resolved_ui4_framed_video_playback(_:crate::ui4::VideoPlaybackSession,_:&str)->Result<(),&'static str>{crate::S.lock().unwrap().urls+=1;Ok(())}
}}}
#[test]fn admission_validation_shared_cap_looping_and_cleanup(){
    use r::services::video_open_service::enqueue;
    assert_eq!(enqueue(1,b"trueosfs:disc7/movies/map.mp4",true),0);
    assert_eq!(S.lock().unwrap().laps,2);assert_eq!(S.lock().unwrap().occupied,0);
    assert_eq!(enqueue(1,b"https://media.example/video.mp4",true),0);
    assert_eq!(S.lock().unwrap().urls,1);assert_eq!(S.lock().unwrap().occupied,0);
    for (path,qualified) in [("relative.mp4",false),("trueosfs:discX/a.mp4",true),("trueosfs:disc7/../a.mp4",true),("https://user@host/a.mp4",true),("https://",true),("https://host/a b",true)] {assert_eq!(enqueue(1,path.as_bytes(),qualified),-1);}
    S.lock().unwrap().scope=false;assert_eq!(enqueue(1,b"/a.mp4",false),-13);S.lock().unwrap().scope=true;
    S.lock().unwrap().occupied=3;assert_eq!(enqueue(1,b"/a.mp4",false),-11);S.lock().unwrap().occupied=0;
    S.lock().unwrap().task=false;assert_eq!(enqueue(1,b"/a.mp4",false),-11);assert_eq!(S.lock().unwrap().occupied,0);S.lock().unwrap().task=true;
    S.lock().unwrap().window=false;assert_eq!(enqueue(1,b"/a.mp4",false),0);assert_eq!(S.lock().unwrap().occupied,0);assert!(S.lock().unwrap().messages[0].contains("UI4 video window unavailable"));
}
'''
with tempfile.TemporaryDirectory(prefix='video-open-') as temp:
    p=Path(temp);(p/'test.rs').write_text(harness+source+stubs)
    subprocess.run(['rustc','--edition=2024','--test',str(p/'test.rs'),'-o',str(p/'test')],check=True)
    subprocess.run([str(p/'test')],check=True)
