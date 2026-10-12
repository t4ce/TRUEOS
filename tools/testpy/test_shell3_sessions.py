#!/usr/bin/env python3
"""Run the production session registry/worker with deterministic host terminals."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as v;
#[path="API_PATH"] mod command_session;
pub mod vcabi {
    use crate::shell3::sessions;
    pub unsafe fn trueos_cabi_shell3_session_open_v1(p:*const u8,n:usize,c:u32,r:u32,f:u32)->i64 {
        sessions::open(1,unsafe {std::slice::from_raw_parts(p,n)},c as usize,r as usize,f!=0)
    }
    pub unsafe fn trueos_cabi_shell3_session_read_v1(id:u64,p:*mut u8,n:usize)->isize {
        sessions::read(1,id,unsafe {std::slice::from_raw_parts_mut(p,n)})
    }
    pub unsafe fn trueos_cabi_shell3_session_write_v1(id:u64,p:*const u8,n:usize)->isize {
        sessions::write(1,id,unsafe {std::slice::from_raw_parts(p,n)})
    }
    pub unsafe fn trueos_cabi_shell3_session_control_v1(id:u64,op:u32,c:u32,r:u32)->i32 {
        sessions::control(1,id,op,c as usize,r as usize)
    }
}
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
mod hv {
    pub fn blueprint_console_target(vm:u8)->Option<crate::shell3::MatrixSlotLease> {
        (vm==1 || vm==2).then(||crate::shell3::MatrixSlotLease {slot:vm as u32,generation:crate::shell3::GEN.load(std::sync::atomic::Ordering::SeqCst)})
    }
}
mod shell3 {
    use std::sync::atomic::{AtomicU64,AtomicUsize,Ordering};
    pub const MIN_COLUMNS:usize=20; pub const MIN_ROWS:usize=5; pub const MAX_SHELL3_INSTANCES:usize=4;
    pub static GEN:AtomicU64=AtomicU64::new(1);
    pub static RESERVED:AtomicUsize=AtomicUsize::new(0);
    pub static DROPPED:crate::Mutex<Vec<u32>>=crate::Mutex::new(Vec::new());
    pub static SIZE:crate::Mutex<(usize,usize)>=crate::Mutex::new((0,0));
    pub static INPUT:crate::Mutex<Vec<u8>>=crate::Mutex::new(Vec::new());
    #[derive(Clone,PartialEq,Eq)] pub struct MatrixSlotLease {pub slot:u32,pub generation:u64}
    pub fn matrix_target_slot_lease(target:&MatrixSlotLease)->MatrixSlotLease {target.clone()}
    pub struct MatrixSlots;
    impl MatrixSlots {pub fn drop_lease(lease:&MatrixSlotLease){DROPPED.lock().push(lease.slot);}}
    pub mod matrix_target {pub fn matrix_slot_is_live(lease:&super::MatrixSlotLease)->bool {lease.generation==super::GEN.load(std::sync::atomic::Ordering::SeqCst)}}
    pub mod service {
        pub fn refresh_appdb_names(){}
        pub fn reserve_terminal_slot()->Result<u32,()> {super::RESERVED.fetch_add(1,std::sync::atomic::Ordering::SeqCst);Ok(7)}
        pub fn release_shell_on_executor(_:u32){super::RESERVED.fetch_sub(1,std::sync::atomic::Ordering::SeqCst);}
        pub fn notify_work(){}
    }
    pub struct Shell3 {slot:u32,selected:MatrixSlotLease}
    impl Shell3 {
        pub fn new_terminal_sized_reserved(slot:u32,_:Option<()>,_:usize,_:usize)->Self {Self{slot,selected:MatrixSlotLease{slot:100,generation:GEN.load(Ordering::SeqCst)}}}
        pub fn active_matrix_lease(&self)->Option<MatrixSlotLease>{Some(self.selected.clone())}
    }
    impl Drop for Shell3 {fn drop(&mut self){service::release_shell_on_executor(self.slot);}}
    pub mod tty {
        pub const OUTPUT_LIMIT:usize=1024*1024;
        pub struct Terminal {shell:super::Shell3,pub output:Vec<u8>,pub closing:bool,pub overflow:bool}
        impl Terminal {
            pub fn new_ssh(shell:super::Shell3)->Self {Self{shell,output:Vec::new(),closing:false,overflow:false}}
            pub fn resize(&mut self,c:usize,r:usize){*super::SIZE.lock()=(c,r);}
            pub fn input(&mut self,bytes:&[u8]){
                super::INPUT.lock().extend_from_slice(bytes);
                self.output.extend_from_slice(bytes);
                if bytes.windows(2).any(|w|w==[0xc2,0xa7]) {self.shell.selected.slot=999;}
                if bytes.contains(&4) {self.closing=true;}
            }
            pub fn reconcile_matrix_selection(&mut self){}
        }
    }
    #[path="SESSION_PATH"] pub mod sessions;
#[test]
fn production_registry_and_worker() {
    use self::sessions::*;
    use std::sync::atomic::Ordering;
    let id=open(1,b"run",80,24,true) as u64;
    assert!(id>0); assert_eq!(RESERVED.load(Ordering::SeqCst),1);
    let mut out=[0u8;64];
    assert_eq!(read(2,id,&mut out),-3); assert_eq!(write(2,id,b"x"),-3); assert_eq!(control(2,id,4,0,0),-3);
    assert_eq!(write(1,id,"\t§".as_bytes()),3);
    let mut worker=Worker::new(7); assert!(worker.pending()); worker.poll();
    assert_eq!(&*INPUT.lock(),b"run\r\t\xc2\xa7");
    assert_eq!(control(1,id,2,0,0),0); worker.poll();
    assert_eq!(write(1,id,b"later"),-3); assert!(DROPPED.lock().is_empty());
    assert_eq!(control(1,id,4,0,0),0); worker.poll();
    assert_eq!(RESERVED.load(Ordering::SeqCst),0);
    assert_eq!(&*DROPPED.lock(),&[100]); // selected unrelated slot 999 survives
    let mut collected=Vec::new(); let mut short=[0u8;10];
    loop {let n=read(1,id,&mut short);assert!(n>=8);collected.extend_from_slice(&short[8..n as usize]);
        if u32::from_le_bytes(short[..4].try_into().unwrap())!=0 {assert_eq!(i32::from_le_bytes(short[4..8].try_into().unwrap()),137);break;}
        assert!(n>8,"exit must become readable once bytes drain");
    }
    assert_eq!(collected,b"run\r\t\xc2\xa7");
    assert_eq!(control(1,id,5,0,0),0);worker.poll();assert_eq!(read(1,id,&mut out),-3);assert!(worker.is_empty());
    let natural=open(1,b"",80,24,true) as u64;
    assert_eq!(write(1,natural,b"done\x04"),5);worker.poll();
    let n=read(1,natural,&mut out);assert_eq!(n,13);assert_eq!(&out[8..13],b"done\x04");
    assert_eq!(u32::from_le_bytes(out[..4].try_into().unwrap()),1);
    assert_eq!(i32::from_le_bytes(out[4..8].try_into().unwrap()),0);
    assert_eq!(control(1,natural,5,0,0),0);worker.poll();
    assert_eq!(RESERVED.load(Ordering::SeqCst),0);
    let stale=open(1,b"",80,24,true) as u64;worker.poll();GEN.store(2,Ordering::SeqCst);
    assert_eq!(read(1,stale,&mut out),-3);assert_eq!(write(1,stale,b"x"),-3);assert_eq!(control(1,stale,4,0,0),-3);
    worker.poll();assert_eq!(RESERVED.load(Ordering::SeqCst),0);assert!(worker.is_empty());
    let pending=open(1,b"",80,24,true) as u64;assert_eq!(control(1,pending,5,0,0),0);worker.poll();
    assert_eq!(RESERVED.load(Ordering::SeqCst),0);assert!(worker.is_empty());
    // Compile and exercise the production API client against the production registry.
    use crate::command_session::{Session,KernelBackend,Launch,Geometry,State,Error,Signal};
    let mut api=Session::open(&KernelBackend,Launch {script:"api".into(),terminal:None,stdin_open:true}).unwrap();
    api.transport().resize(Geometry {cols:90,rows:30}).unwrap();
    assert_eq!(api.transport().resize(Geometry {cols:1,rows:30}),Err(Error::InvalidRequest));
    let big=vec![b'x';65536];
    assert_eq!(api.transport().write(&big).unwrap(),65532); // four initial script bytes occupy queue
    assert_eq!(api.transport().write(b"full").unwrap(),0);
    worker.poll();
    assert_eq!(*SIZE.lock(),(90,30));
    api.transport().signal(Signal::Interrupt).unwrap();worker.poll();
    api.transport().close_stdin().unwrap();
    assert_eq!(api.transport().write(b"closed"),Err(Error::Closed));
    api.transport().signal(Signal::Terminate).unwrap();worker.poll();
    let mut decoded=Vec::new();let mut body=[0u8;17];
    loop {let packet=api.transport().read(&mut body).unwrap();
        assert_eq!(packet.stream,crate::command_session::Stream::Stdout);
        decoded.extend_from_slice(&body[..packet.len]);
        if packet.state==State::Exited(137) {break;}
        assert_eq!(packet.state,State::Running);assert!(packet.len>0);
    }
    assert!(decoded.starts_with(b"api\r"));assert!(decoded.contains(&3));
    api.transport().close();api.transport().close();
    assert_eq!(api.transport().read(&mut body),Err(Error::Closed));
    drop(api);worker.poll();assert!(worker.is_empty());assert_eq!(RESERVED.load(Ordering::SeqCst),0);
    let api=Session::open(&KernelBackend,Launch {script:String::new(),terminal:None,stdin_open:true}).unwrap();
    drop(api);worker.poll();assert!(worker.is_empty());assert_eq!(RESERVED.load(Ordering::SeqCst),0);
    assert_eq!(open(1,b"bad\0",80,24,true),-1);assert_eq!(open(1,b"x",1,24,true),-1);
}
}
'''

def main():
    source = SOURCE.replace('SESSION_PATH', str(ROOT / 'src/shell3/sessions.rs')).replace('API_PATH', str(ROOT.parent / 'TRUEOS-Blueprints/api/src/command_session.rs'))
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '-Aexplicit_builtin_cfgs_in_flags', '--cfg', 'target_os="trueos"', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)

if __name__ == '__main__':
    main()
