#!/usr/bin/env python3
"""Exercise real image admission with deterministic worker and launcher services."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'src/r/services/image_open_service.rs').read_text()
source = source[source.index('use alloc::'):].replace(
    '#[trueos_executor::task(pool_size = 4)]\n', '').replace(
    'async fn launch_task(', 'async fn launch_task_run(')
harness = r'''
extern crate alloc;
use std::sync::{Mutex, Arc};
struct State { scope:bool, context:bool, worker:bool, task:bool, missing:bool, fail:bool, args:Vec<String>, launches:usize, messages:Vec<String> }
static S:Mutex<State> = Mutex::new(State {scope:true,context:false,worker:true,task:true,missing:false,fail:false,args:Vec::new(),launches:0,messages:Vec::new()});
fn run(f:impl std::future::Future<Output=()>) {
    struct Wake; impl std::task::Wake for Wake {fn wake(self:Arc<Self>) {}}
    let w=std::task::Waker::from(Arc::new(Wake)); let mut c=std::task::Context::from_waker(&w);
    assert!(Box::pin(f).as_mut().poll(&mut c).is_ready());
}
mod trueos_vm {pub mod vmcall {pub const PAYLOAD_CAP:usize=65536;}}
mod workers {
    pub struct Worker;
    pub fn pick_background_spawner()->Option<Worker> {crate::S.lock().unwrap().worker.then_some(Worker)}
    impl Worker {pub fn spawn(&self,f:impl std::future::Future<Output=()>) {crate::run(f)}}
}
mod shell3 {
    pub struct MatrixTarget;
    pub fn matrix_target_print_line(_:&MatrixTarget,text:&str) {crate::S.lock().unwrap().messages.push(text.into());}
}
mod log_os {pub fn blueprint_important_line(_:std::fmt::Arguments) {}}
mod app_db {
    pub fn get(name:&str)->Result<Option<Vec<u8>>,String> {
        assert_eq!(name,"img.bp"); assert!(!crate::S.lock().unwrap().context);
        Ok((!crate::S.lock().unwrap().missing).then(|| vec![42]))
    }
}
mod hv {
    pub fn with_guest_broker_context<T>(vm:u8,f:impl FnOnce()->T)->T {
        assert_eq!(vm,7); crate::S.lock().unwrap().context=true;
        let value=f(); crate::S.lock().unwrap().context=false; value
    }
    pub fn blueprint_console_target(_:u8)->Option<crate::shell3::MatrixTarget> {Some(crate::shell3::MatrixTarget)}
    pub mod launcher {
        pub struct LaunchRequest {pub args:Vec<String>,pub script:Option<String>}
        impl LaunchRequest {pub fn new(name:String,bytes:Vec<u8>)->Self {
            assert_eq!(name,"img.bp");assert_eq!(bytes,vec![42]); Self {args:vec![],script:None}
        }}
        pub fn enqueue(request:LaunchRequest,observer:Option<()>)->Result<(),String> {
            assert!(observer.is_none()); assert_eq!(request.script.as_deref(),Some("fs-scope trueosfs\n"));
            let mut s=crate::S.lock().unwrap(); if s.fail {return Err("launcher unavailable".into());}
            s.args=request.args;s.launches+=1;Ok(())
        }
    }
}
mod r {
    pub mod path {
        pub struct FsPath;
        impl FsPath {pub fn parse(path:&str,_:bool)->Result<Self,()> {
            if path.trim().is_empty() || path.split('/').any(|p|p=="..") {Err(())} else {Ok(Self)}
        }}
    }
    pub mod io {pub mod env {
        pub fn trueosfs_scope_granted()->bool {let s=crate::S.lock().unwrap(); assert!(s.context); s.scope}
        pub fn resolve_fs_path(path:&str,_:bool)->Option<String> {
            assert!(crate::S.lock().unwrap().context);Some(path.trim_start_matches('/').into())
        }
    }}
    pub mod services {pub mod image_open_service {
        use crate::trueos_vm;
'''
tests = r'''
    fn launch_task(paths:Vec<String>,target:Option<crate::shell3::MatrixTarget>)->Result<impl std::future::Future<Output=()>,()> {
        if crate::S.lock().unwrap().task {Ok(launch_task_run(paths,target))} else {Err(())}
    }
}}}
#[test] fn image_admission_paths_permissions_capacity_and_errors() {
    use r::services::image_open_service::enqueue;
    assert_eq!(enqueue(7,b"trueosfs:disc7/Pictures/my image.png\0/Pictures/folder"),0);
    assert_eq!(S.lock().unwrap().args,vec!["trueosfs:disc7/Pictures/my image.png","Pictures/folder"]);
    for payload in [b"".as_slice(),b"\0a",b"a\0",b"a\0\0b",b"\xff",b"/../a"] {
        assert_eq!(enqueue(7,payload),-1);
    }
    assert_eq!(S.lock().unwrap().launches,1);
    assert_eq!(enqueue(7,vec!["a";32].join("\0").as_bytes()),0);
    assert_eq!(S.lock().unwrap().args.len(),32);
    assert_eq!(enqueue(7,vec!["a";33].join("\0").as_bytes()),-1);
    assert_eq!(enqueue(7,&vec![b'a';65537]),-1);
    S.lock().unwrap().scope=false; assert_eq!(enqueue(7,b"/a"),-13); S.lock().unwrap().scope=true;
    S.lock().unwrap().worker=false; assert_eq!(enqueue(7,b"/a"),-3); S.lock().unwrap().worker=true;
    S.lock().unwrap().task=false; assert_eq!(enqueue(7,b"/a"),-16); S.lock().unwrap().task=true;
    S.lock().unwrap().missing=true; assert_eq!(enqueue(7,b"/a"),0); S.lock().unwrap().missing=false;
    assert!(S.lock().unwrap().messages.last().unwrap().contains("img.bp is missing"));
    S.lock().unwrap().fail=true; assert_eq!(enqueue(7,b"/a"),0);
    assert!(S.lock().unwrap().messages.last().unwrap().contains("launcher unavailable"));
}
'''
with tempfile.TemporaryDirectory(prefix='image-open-') as temp:
    folder = Path(temp)
    (folder / 'test.rs').write_text(harness + source + tests)
    subprocess.run(['rustc', '--edition=2024', '--test', str(folder / 'test.rs'), '-o', str(folder / 'test')], check=True)
    subprocess.run([str(folder / 'test')], check=True)
