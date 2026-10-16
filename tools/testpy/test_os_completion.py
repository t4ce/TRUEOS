#!/usr/bin/env python3
"""Exercise production Matrix completion and OS observer across VM retirement/reuse."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
matrix = '\n'.join(extract.item('src/hv/matrix.rs', name) for name in (
    'Entry', 'observe_completion', 'take_completion', 'retire_vm', 'discard'))
service = (ROOT / 'src/shell3/service.rs').read_text()
observer = re.search(r'struct ShellLaunch;\nimpl .*?^}', service, re.M | re.S).group()
trait = (ROOT / 'src/hv/launcher.rs').read_text()
trait = re.search(r'pub\(crate\) trait LaunchObserver.*?^}', trait, re.M | re.S).group()
harness = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::{Mutex as StdMutex, MutexGuard};
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(v:T)->Self {Self(StdMutex::new(v))}
    fn lock(&self)->MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone,Debug,PartialEq,Eq)] struct MatrixSlotLease(u64);
#[derive(Clone)] struct MatrixTarget(MatrixSlotLease);
fn matrix_target_slot_lease(t:&MatrixTarget)->MatrixSlotLease {t.0.clone()}
struct AppReceipt;
static LIVE:Mutex<Vec<MatrixSlotLease>>=Mutex::new(Vec::new());
static ACTIONS:Mutex<Vec<String>>=Mutex::new(Vec::new());
mod hv {
    use super::*;
    struct Vm {matrix_owner:Mutex<Option<MatrixSlotLease>>}
    static VM:Vm=Vm {matrix_owner:Mutex::new(None)};
    static REASON:Mutex<Option<String>>=Mutex::new(None);
    fn vm_slot(_:u8)->Option<&'static Vm>{Some(&VM)}
    fn blueprint_console_exit_reason(_:u8)->Option<String>{REASON.lock().clone()}
    pub mod launcher { __TRAIT__ }
    pub mod matrix {
        use super::*;
        static APPS:Mutex<Vec<Entry>>=Mutex::new(Vec::new());
        __MATRIX__
        pub fn reserve(id:u64, observed:bool)->MatrixTarget {
            let target=MatrixTarget(MatrixSlotLease(id));
            LIVE.lock().push(target.0.clone());
            APPS.lock().push(Entry {lease:target.0.clone(),receipt:AppReceipt,observed:false,completion:None});
            if observed {observe_completion(&target);}
            *VM.matrix_owner.lock()=Some(target.0.clone());
            target
        }
        pub fn remove(lease:&MatrixSlotLease) {
            APPS.lock().retain(|entry|entry.lease!=*lease);
        }
        pub fn exit(reason:Option<&str>) {
            *REASON.lock()=reason.map(str::to_owned);
            retire_vm(0);
        }
    }
}
mod shell3 {
    use super::*;
    pub(crate) use crate::MatrixTarget;
    pub struct MatrixSlots;
    impl MatrixSlots {pub fn drop_lease(lease:&MatrixSlotLease){
        LIVE.lock().retain(|v|v!=lease);
        hv::matrix::remove(lease); // attachment retirement after slot deletion
    }}
    mod os {use super::*;pub fn complete(target:MatrixTarget,reason:String){
        assert!(LIVE.lock().contains(&target.0)); ACTIONS.lock().push(reason);
    }}
    mod service {
    use super::*;
    __OBSERVER__
    pub fn finish(target:&MatrixTarget, reason:Option<&str>, error:Option<&str>) {
        use hv::launcher::LaunchObserver;
        ShellLaunch.finished(target,Some(0),error,reason);
    }
    }
    pub use service::finish;
}
fn main(){
    for (i,reason) in ["os:reboot","os:shutdown","os:update:live","os:update:lan","os:install:local:7"].iter().enumerate(){
        let target=hv::matrix::reserve(i as u64,true);
        assert_eq!(hv::matrix::take_completion(&target),None); // still running
        hv::matrix::exit(Some(reason));
        assert!(LIVE.lock().contains(&target.0)); // survives normal teardown
        let reused=hv::matrix::reserve(100+i as u64,false);
        hv::matrix::exit(Some("os:cancel")); // same VM ID, different result
        assert!(!LIVE.lock().contains(&reused.0));
        let result=hv::matrix::take_completion(&target).unwrap();
        assert_eq!(result.as_deref(),Some(*reason));
        shell3::finish(&target,result.as_deref(),None);
        assert_eq!(ACTIONS.lock().last().map(String::as_str),Some(*reason));
        assert_eq!(hv::matrix::take_completion(&target),None); // consumed once
        hv::matrix::discard(&target);
    }
    for reason in [None,Some("os:quit"),Some("os:cancel")] {
        let target=hv::matrix::reserve(200,true);
        hv::matrix::exit(reason);
        let result=hv::matrix::take_completion(&target).unwrap();
        shell3::finish(&target,result.as_deref(),None);
        assert!(!LIVE.lock().contains(&target.0));
    }
    let target=hv::matrix::reserve(300,true);
    hv::matrix::discard(&target); // explicit deletion cancels pending action
    hv::matrix::exit(Some("os:reboot"));
    assert!(!LIVE.lock().contains(&target.0));
    assert_eq!(hv::matrix::take_completion(&target),None);
    let target=hv::matrix::reserve(400,true);
    shell3::finish(&target,None,Some("launch failed"));
    assert!(!LIVE.lock().contains(&target.0));
    assert_eq!(ACTIONS.lock().len(),5);
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-os-completion-') as tmp:
    path = Path(tmp)
    (path / 'test.rs').write_text(harness.replace('__MATRIX__', matrix).replace('__TRAIT__', trait).replace('__OBSERVER__', observer))
    subprocess.run(['rustc', '--edition=2024', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test')], check=True)
print('OS completion: retirement, VM reuse, cancellation, and launch failure passed')
