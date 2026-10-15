#!/usr/bin/env python3
"""Compile the production byte store and CABI helpers with isolated realms."""
from pathlib import Path
import subprocess
import tempfile
import re

ROOT = Path(__file__).resolve().parents[2]

def function(path,name):
    source=path.read_text()
    start=re.search(r'^(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+(?:extern\s+"C"\s+)?)?fn '+re.escape(name)+r'(?:<[^>]*>)?\(',source,re.M)
    assert start,name
    end=re.search(r'^}\s*\n',source[start.end():],re.M)
    assert end,name
    return source[start.start():start.end()+end.end()]


def main():
    shim = ROOT / "src/std_abi_shim.rs"
    names = ["environment_vm_id", "initialize_blueprint_environment", "with_environment", "environment_bytes",
             "environment_pairs", "environ_import_address", "environment_cstr",
             "getenv", "setenv", "unsetenv", "sys_getenv"]
    functions = "\n".join(function(shim, name) for name in names)
    stage = function(ROOT / "src/hv/mod.rs", "stage_blueprint_launch")
    assert stage.index("initialize_blueprint_environment(") < stage.index("*process_slot.lock() = Some(process_context)")
    assert stage.index("for (key, value) in link.env") < stage.index("initialize_blueprint_environment(")
    assert "blueprint_initial_process_env" not in functions
    assert "clear_blueprint_child_template(vm_id);" in stage[stage.index("initialize_blueprint_environment("):]

    trusted = function(ROOT / "src/hv/mod.rs", "blueprint_process_env_var")
    io_var = (ROOT / "src/r/io.rs").read_text().split("pub fn var(key: &str)",1)[1].split("pub(crate) fn initial_snapshot",1)[0]
    assert io_var.index('key.starts_with("TRUEOS_")') < io_var.index('environment_bytes(')
    constants = "\n".join(re.search(r"^(?:pub\(crate\) )?const " + name + r": c_int = \d+;", shim.read_text(), re.M).group(0)
                          for name in ["TRUEOS_EINVAL", "TRUEOS_EFAULT", "TRUEOS_ENOMEM"])
    source = r'''
extern crate alloc;
use std::{cell::Cell,ffi::{c_char,c_int},ptr,sync::{Mutex as StdMutex,MutexGuard,atomic::{AtomicI32,AtomicUsize,Ordering}}};
use alloc::vec::Vec;
#[path="@ROOT@/src/process_environment.rs"]mod process_environment;
thread_local! {static VM:Cell<Option<u8>>=const{Cell::new(None)};static DOMAIN:Cell<usize>=const{Cell::new(0)};}
const N:usize=4096;
static BASE:[AtomicUsize;N]=[const{AtomicUsize::new(0)};N];
static SIZE:[AtomicUsize;N]=[const{AtomicUsize::new(0)};N];
static OWNER:[AtomicUsize;N]=[const{AtomicUsize::new(0)};N];
struct Alloc;
unsafe impl std::alloc::GlobalAlloc for Alloc {
 unsafe fn alloc(&self,l:std::alloc::Layout)->*mut u8 {
  let p=unsafe{std::alloc::System.alloc(l)};
  let owner=DOMAIN.try_with(Cell::get).unwrap_or(0);
  if owner!=0 && !p.is_null(){for i in 0..N{if BASE[i].compare_exchange(0,p as usize,Ordering::SeqCst,Ordering::SeqCst).is_ok(){SIZE[i].store(l.size(),Ordering::SeqCst);OWNER[i].store(owner,Ordering::SeqCst);break;}}}
  p
 }
 unsafe fn dealloc(&self,p:*mut u8,l:std::alloc::Layout){for i in 0..N{if BASE[i].load(Ordering::SeqCst)==p as usize{BASE[i].store(0,Ordering::SeqCst);break;}}unsafe{std::alloc::System.dealloc(p,l)}}
}
#[global_allocator]static ALLOC:Alloc=Alloc;
fn owner(pointer:usize)->usize{for i in 0..N{let p=BASE[i].load(Ordering::SeqCst);if p!=0&&pointer>=p&&pointer<p+SIZE[i].load(Ordering::SeqCst){return OWNER[i].load(Ordering::SeqCst);}}0}
struct Mutex<T>(StdMutex<T>);
impl<T>Mutex<T>{const fn new(v:T)->Self{Self(StdMutex::new(v))}fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()}}
struct ProcessSharedState{environment:Mutex<process_environment::Environment>}
impl ProcessSharedState{const fn new()->Self{Self{environment:Mutex::new(process_environment::Environment::new())}}}
static HOST_PROCESS_STATE:ProcessSharedState=ProcessSharedState::new();
static STATES:[ProcessSharedState;2]=[const{ProcessSharedState::new()};2];
fn blueprint_process_state(vm:u8)->Option<&'static ProcessSharedState>{STATES.get(vm as usize)}
@CONSTANTS@
static TRUEOS_ERRNO:AtomicI32=AtomicI32::new(0);
fn abi_read_bytes<'a>(p:*const u8,n:usize)->Option<&'a[u8]>{if p.is_null(){None}else{Some(unsafe{std::slice::from_raw_parts(p,n)})}}
fn copy_bytes_to_words(p:*mut u32,n:usize,bytes:&[u8])->usize{if !p.is_null(){unsafe{ptr::copy_nonoverlapping(bytes.as_ptr(),p.cast(),bytes.len().min(n*4))};}bytes.len()}
fn copy_vmcall_text_response_to_words(_:u32,_:usize,_:&[u8],_:*mut u32,_:usize,missing:usize)->usize{missing}
mod trueos_vm{pub mod vmcall{pub const PAYLOAD_CAP:usize=4096;pub const OP_BP_ENV_VAR:u32=44;pub fn call_with_payload(_:u32,_:u64,_:u64,_:&[u8],_:&mut[u8])->(u32,usize){(1,0)}}}
mod r{pub mod threads{pub fn current_vm_id()->Option<u8>{crate::VM.with(std::cell::Cell::get)}}pub mod io{pub mod env{pub fn initial_snapshot()->Vec<(String,String)>{vec![("HOME".into(),"/host".into())]}}}}
mod hv{
 use crate::Mutex;
 type AllocString=String;
 thread_local!{pub static HULL:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
 struct Context{vars:std::collections::BTreeMap<String,String>}
 struct Contexts;
 static BLUEPRINT_PROCESS_CONTEXTS:Contexts=Contexts;
 impl Contexts{fn get(&self,vm:usize)->Option<&'static Mutex<Option<Context>>>{
 static CONTEXT:std::sync::OnceLock<Mutex<Option<Context>>>=std::sync::OnceLock::new();
 (vm==0).then(||CONTEXT.get_or_init(||Mutex::new(Some(Context{vars:[("TRUEOS_FS_SCOPE".into(),"private".into())].into()}))))
 }
 }
 @TRUSTED_HELPER@
 pub fn current_guest_execution_context_vm_id()->Option<u8>{crate::VM.with(std::cell::Cell::get)}pub fn current_vm_id()->Option<u8>{None}pub fn current_hull_guest_context_vm_id()->Option<u8>{HULL.with(|h|h.get().then_some(0))}
 pub fn blueprint_initial_process_env(vm:u8)->Option<Vec<(String,String)>>{Some(vec![("HOME".into(),format!("/vm{vm}")),("TRUEOS_FS_SCOPE".into(),"private".into())])}
}
mod std_abi_shim{pub(crate) use crate::environment_bytes;}
mod allocators{
 pub fn with_hv_guest_alloc_domain<T>(vm:u8,f:impl FnOnce()->T)->Option<T>{crate::DOMAIN.with(|d|{let old=d.replace(vm as usize+1);let value=f();d.set(old);Some(value)})}
 pub fn with_host_alloc_domain<T>(f:impl FnOnce()->T)->T{crate::DOMAIN.with(|d|{let old=d.replace(0);let value=f();d.set(old);value})}
}
@FUNCTIONS@
unsafe fn value(name:&std::ffi::CStr)->Option<Vec<u8>>{let p=unsafe{getenv(name.as_ptr())};if p.is_null(){None}else{Some(unsafe{std::ffi::CStr::from_ptr(p)}.to_bytes().to_vec())}}
#[test]fn cabi_snapshot_workers_real_errors_and_guest_pointer_ownership(){
 VM.with(|v|v.set(Some(0)));
 assert!(environ_import_address().is_none());assert_eq!(unsafe{value(c"HOME")},None);
 for vm in 0..2{let home=format!("/vm{vm}");initialize_blueprint_environment(vm,[(b"HOME".as_slice(),home.as_bytes()),(b"TRUEOS_FS_SCOPE".as_slice(),b"private".as_slice())].into_iter()).unwrap();}
 let cell=environ_import_address().unwrap() as *mut *mut *mut c_char;
 assert_eq!(unsafe{value(c"HOME")},Some(b"/vm0".to_vec()));
 assert_eq!(unsafe{setenv(c"HOME".as_ptr(),c"/changed".as_ptr(),0)},0);assert_eq!(unsafe{value(c"HOME")},Some(b"/vm0".to_vec()));
 assert_eq!(unsafe{setenv(c"HOME".as_ptr(),c"/changed".as_ptr(),1)},0);
 let home=unsafe{getenv(c"HOME".as_ptr())};assert_eq!(owner(home as usize),1);
 let mut item=unsafe{*cell};assert_eq!(owner(item as usize),1);let mut found=false;
 while !unsafe{*item}.is_null(){let p=unsafe{*item};assert_eq!(owner(p as usize),1);found|=unsafe{std::ffi::CStr::from_ptr(p)}.to_bytes()==b"HOME=/changed";item=unsafe{item.add(1)};}assert!(found);
 let mut words=[0u32;32];let n=unsafe{sys_getenv(words.as_mut_ptr(),words.len(),b"HOME".as_ptr(),4)};assert_eq!(&unsafe{std::slice::from_raw_parts(words.as_ptr().cast::<u8>(),n)},&&b"/changed"[..]);
 std::thread::spawn(||{VM.with(|v|v.set(Some(0)));assert_eq!(unsafe{value(c"HOME")},Some(b"/changed".to_vec()));assert_eq!(unsafe{setenv(c"HOME".as_ptr(),c"/worker".as_ptr(),1)},0);assert_eq!(owner(unsafe{getenv(c"HOME".as_ptr())} as usize),1);}).join().unwrap();
 assert_eq!(unsafe{value(c"HOME")},Some(b"/worker".to_vec()));assert_eq!(environ_import_address().unwrap(),cell as usize);
 assert_eq!(unsafe{setenv(c"".as_ptr(),c"x".as_ptr(),1)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),22);
 assert_eq!(unsafe{setenv(c"a=b".as_ptr(),c"x".as_ptr(),1)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),22);
 assert_eq!(unsafe{setenv(c"TRUEOS_FS_SCOPE".as_ptr(),c"trueosfs".as_ptr(),1)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),1);assert_eq!(unsafe{value(c"TRUEOS_FS_SCOPE")},Some(b"private".to_vec()));
 let scope=unsafe{getenv(c"TRUEOS_FS_SCOPE".as_ptr())};unsafe{ptr::copy_nonoverlapping(b"evil\0".as_ptr(),scope.cast(),5)};
 assert_eq!(unsafe{value(c"TRUEOS_FS_SCOPE")},Some(b"evil".to_vec()));
 assert_eq!(hv::blueprint_process_env_var(0,"TRUEOS_FS_SCOPE").as_deref(),Some("private"));
 hv::HULL.with(|h|h.set(true));assert_eq!(hv::blueprint_process_env_var(0,"TRUEOS_FS_SCOPE"),None);hv::HULL.with(|h|h.set(false));
 assert_eq!(unsafe{unsetenv(c"HOME".as_ptr())},0);assert_eq!(unsafe{value(c"HOME")},None);assert_eq!(unsafe{sys_getenv(ptr::null_mut(),0,b"HOME".as_ptr(),4)},usize::MAX);
 VM.with(|v|v.set(Some(1)));assert_ne!(environ_import_address().unwrap(),cell as usize);assert_eq!(unsafe{value(c"HOME")},Some(b"/vm1".to_vec()));assert_eq!(owner(unsafe{getenv(c"HOME".as_ptr())} as usize),2);
 VM.with(|v|v.set(None));assert_eq!(unsafe{value(c"HOME")},Some(b"/host".to_vec()));assert_eq!(unsafe{setenv(c"HOME".as_ptr(),c"/host-new".as_ptr(),1)},0);
 VM.with(|v|v.set(Some(0)));assert_eq!(unsafe{value(c"HOME")},None);*STATES[0].environment.lock()=process_environment::Environment::new();assert_eq!(unsafe{value(c"HOME")},None);assert!(environ_import_address().is_none());
 initialize_blueprint_environment(0,[(b"HOME".as_slice(),b"/next-generation".as_slice())].into_iter()).unwrap();
 assert_eq!(unsafe{value(c"HOME")},Some(b"/next-generation".to_vec()));
 assert!(initialize_blueprint_environment(0,[(b"bad=key".as_slice(),b"bad".as_slice())].into_iter()).is_err());
 assert_eq!(unsafe{value(c"HOME")},Some(b"/next-generation".to_vec()));
}
'''.replace("@ROOT@", str(ROOT)).replace("@FUNCTIONS@", functions).replace("@CONSTANTS@", constants).replace("@TRUSTED_HELPER@", trusted)
    with tempfile.TemporaryDirectory(prefix="trueos-process-env-") as directory:
        path=Path(directory);(path/"test.rs").write_text(source)
        subprocess.run(["rustc","--edition=2024","--test","-Awarnings",str(path/"test.rs"),"-o",str(path/"test")],check=True)
        subprocess.run([str(path/"test"),"--test-threads=1"],check=True)


if __name__ == "__main__":
    main()
