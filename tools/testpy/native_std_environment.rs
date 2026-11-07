#![feature(negative_impls, try_trait_v2, iter_next_chunk, iter_advance_by, exact_size_is_empty)]
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_vm;
pub use std::{array, error, ffi, fmt, io, num, ops, os, path, ptr, slice, thread, time, vec};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::io::{Read, Write};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

#[path = "@ROOT@/src/process_environment.rs"] mod owned_environment;
struct Lock<T>(Mutex<T>);
impl<T> Lock<T> { fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() } }
mod native_abi {
    pub(crate) fn abi_read_bytes<'a>(p: *const u8, n: usize) -> Option<&'a [u8]> {
        if n == 0 { Some(&[]) } else if p.is_null() { None } else { Some(unsafe { std::slice::from_raw_parts(p, n) }) }
    }
    pub(crate) fn abi_write_bytes<'a>(p: *mut u8, n: usize) -> Option<&'a mut [u8]> {
        if n == 0 { Some(&mut []) } else if p.is_null() { None } else { Some(unsafe { std::slice::from_raw_parts_mut(p, n) }) }
    }
}
mod r { pub(crate) mod native_process {
    use super::super::*;
    thread_local! { static REALM: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(Some(0)) }; }
    static ENVIRONMENTS: OnceLock<Vec<Lock<owned_environment::Environment>>> = OnceLock::new();
    fn states() -> &'static Vec<Lock<owned_environment::Environment>> { ENVIRONMENTS.get_or_init(|| (0..3).map(|_| Lock(Mutex::new(owned_environment::Environment::new()))).collect()) }
    pub(crate) fn realm(vm: Option<u8>) { REALM.set(vm); }
    pub(crate) fn current_vm() -> Option<u8> { REALM.get() }
    pub(crate) fn with_environment<T>(vm: Option<u8>, f: impl FnOnce(&mut owned_environment::Environment) -> T) -> Result<T, i32> {
        let state = states().get(vm.map_or(2, usize::from)).ok_or(22)?;
        let mut state = state.lock();
        if !state.initialized() { state.initialize([(b"TRUEOS_APP_NAME".as_slice(), b"codex".as_slice())].into_iter())?; }
        Ok(f(&mut state))
    }
    pub(crate) fn reset() { for state in states() { *state.lock() = owned_environment::Environment::new(); } realm(Some(0)); }
} pub(crate) mod io {
    pub(crate) mod env { pub(crate) fn arg(index: usize) -> Option<String> { crate::launch_args().get(index).map(|bytes|String::from_utf8(bytes.clone()).unwrap()) } }
    pub(crate) mod fs_cabi {
        const FS_ERR_NO_SPACE: i32 = -3;
        @COPY_TEXT@
        @ARGS_FUNCTION@
    }
} }
#[path = "@ROOT@/src/native_environment.rs"] mod native_environment;
mod platform_env {
    mod common { include!("@LIBRARY@/std/src/sys/env/common.rs"); }
    pub mod native {
        include!("@ROOT@/tools/rust-std/trueos_env.rs");
        #[test] fn growing_capacity_probe_retries_and_rejects_contract_overruns() {
            let mut calls=0;
            let bytes=copied_bytes(10,|out,len|{calls+=1;match calls{1=>1,2=>5,_=>{assert_eq!(len,5);unsafe{core::ptr::copy_nonoverlapping(b"hello".as_ptr(),out,5)};5}}}).unwrap();
            assert_eq!(calls,3);assert_eq!(bytes,b"hello");
            assert!(copied_bytes(10,|_,_|11).is_err());
            assert_eq!(copied_bytes(10,|_,_|-12).unwrap_err().raw_os_error(),Some(12));
        }
        #[test] fn malformed_environment_snapshots_fail_closed() {
            for bytes in [vec![],vec![1,0,0],vec![1,0,0,0],vec![0,0,0,0,1]] {assert!(pairs(&bytes).is_err());}
            let mut bytes=1u32.to_le_bytes().to_vec();bytes.extend_from_slice(&1u32.to_le_bytes());bytes.extend_from_slice(&1u32.to_le_bytes());bytes.extend_from_slice(b"ab");assert!(pairs(&bytes).is_ok());
            bytes[12]=b'=';assert!(pairs(&bytes).is_err());bytes[12]=b'a';bytes[13]=0;assert!(pairs(&bytes).is_err());
        }
    }
    pub use native::*;
}
mod platform_args {
    mod common { include!("@LIBRARY@/std/src/sys/args/common.rs"); }
    #[path = "@ROOT@/tools/rust-std/trueos_args.rs"] pub mod native;
    pub use native::*;
}
pub mod env { pub fn var_os(key: impl AsRef<std::ffi::OsStr>) -> Option<std::ffi::OsString> { crate::platform_env::getenv(key.as_ref()) } }
mod sys { pub mod io { pub const DEFAULT_BUF_SIZE: usize = 8192; } }
#[path = "@ROOT@/tools/rust-std/trueos_stdio.rs"] mod native_streams;
#[path = "@ROOT@/tools/rust-std/trueos_is_terminal.rs"] mod terminal_classification;
#[path = "@ROOT@/tools/rust-std/trueos_paths.rs"] mod native_paths;
#[path = "@ROOT@/tools/rust-std/trueos_exit.rs"] mod native_exit;

#[derive(Default)] struct Console {
    input: std::collections::VecDeque<u8>, output: Vec<(u32, Vec<u8>)>, generation: u32,
    claims: usize, waits: usize, inject: bool, read_error: isize, overread: bool,
    backpressure: usize, send_error: isize, child_terminal: i32,
}
static CONSOLE: OnceLock<Mutex<Console>> = OnceLock::new();
fn console() -> &'static Mutex<Console> { CONSOLE.get_or_init(Default::default) }
fn reset() { r::native_process::reset(); *console().lock().unwrap() = Console::default(); HULL.store(false, Ordering::Relaxed); }
fn child(enabled: bool) { r::native_process::with_environment(Some(0), |env| env.initialize(core::iter::empty())).unwrap().unwrap(); r::native_process::with_environment(Some(0), |env| { *env = owned_environment::Environment::new(); env.initialize(if enabled { vec![(b"TRUEOS_MCP_STDIO".as_slice(), b"1".as_slice())] } else { vec![] }.into_iter()).unwrap(); }).unwrap(); }
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_write(stream: u32, p: *const u8, n: usize) { console().lock().unwrap().output.push((stream, unsafe { std::slice::from_raw_parts(p, n) }.to_vec())); }
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_blueprint_terminal_lease_current_v1(_: u64, epoch: *mut u64) -> i32 { console().lock().unwrap().claims += 1; unsafe { epoch.write(4); } 0 }
#[unsafe(no_mangle)] extern "C" fn trueos_cabi_shell_attached_observe_v1() -> u32 { console().lock().unwrap().generation }
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_shell_attached_read_v1(p: *mut u8, n: usize) -> isize {
    let mut c=console().lock().unwrap(); if c.read_error != 0 { return c.read_error; } if c.overread { return (n+1) as isize; }
    if c.inject { c.inject=false; c.input.extend(b"arrived"); c.generation += 1; return 0; }
    let count=n.min(c.input.len()); for i in 0..count { unsafe { p.add(i).write(c.input.pop_front().unwrap()); } } count as isize
}
#[unsafe(no_mangle)] extern "C" fn trueos_cabi_shell_attached_wait_after_v1(observed: u32, _: u64) -> i32 { let mut c=console().lock().unwrap(); c.waits += 1; assert_ne!(observed,c.generation,"lost input generation"); 1 }
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_blueprint_child_read_v1(_:u64,p:*mut u8,n:usize)->isize { unsafe { trueos_cabi_shell_attached_read_v1(p,n) } }
#[unsafe(no_mangle)] extern "C" fn trueos_cabi_blueprint_child_status_v1(_:u64)->i32 { console().lock().unwrap().child_terminal }
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_blueprint_child_send_v1(_:u64,p:*const u8,n:usize)->isize {
    let mut c=console().lock().unwrap(); if c.send_error!=0{return c.send_error;} if c.backpressure>0 {c.backpressure-=1;return -3;}
    let count=n.min(3); c.output.push((7,unsafe{std::slice::from_raw_parts(p,count)}.to_vec())); count as isize
}
fn launch_args()->Vec<Vec<u8>> { vec![b"codex".to_vec(),vec![],"λ café".as_bytes().to_vec(),vec![b'x';113]] }
#[unsafe(no_mangle)] extern "C" fn trueos_cabi_env_args_count()->usize {launch_args().len()}
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_env_arg(i:usize,p:*mut u8,n:usize)->isize {
    unsafe { r::io::fs_cabi::trueos_cabi_env_arg(i,p,n) }
}
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_env_current_dir(p:*mut u8,n:usize)->isize {
    let bytes=b"/apps/codex";if !p.is_null() && n>=bytes.len(){unsafe{std::ptr::copy_nonoverlapping(bytes.as_ptr(),p,bytes.len());}} bytes.len() as isize
}
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_blueprint_shutdown(p:*const u8,n:usize)->i32 {
    if std::env::var("TRUEOS_TEST_EXIT_MODE").as_deref()==Ok("reason") {println!("shutdown-reason:{}",String::from_utf8_lossy(unsafe{std::slice::from_raw_parts(p,n)}));std::process::exit(0);}
    0
}
#[unsafe(no_mangle)] unsafe extern "C" fn sys_halt()->! {std::process::exit(89)}

static HULL: AtomicBool=AtomicBool::new(false);
#[derive(Clone,Copy)] enum BlueprintTerminalLeaseState { Unsupported, Reserved, Active, Parked }
struct Context { console_attached:bool,console_target:Option<()>,terminal_lease:BlueprintTerminalLeaseState,vars:std::collections::BTreeMap<String,String> }
impl Context { fn fresh()->Self {Self{console_attached:true,console_target:Some(()),terminal_lease:BlueprintTerminalLeaseState::Reserved,vars:Default::default()}} }
struct Contexts;
static CONTEXTS:OnceLock<Vec<Lock<Option<Context>>>>=OnceLock::new();
impl Contexts { fn get(&self,i:usize)->Option<&'static Lock<Option<Context>>> {CONTEXTS.get_or_init(||(0..3).map(|_|Lock(Mutex::new(Some(Context::fresh())))).collect()).get(i)} }
struct ChildLink{parent_vm_id:u8,parent_generation:u64,child_generation:u64}
struct Links;
static LINKS:OnceLock<Vec<Lock<Option<ChildLink>>>>=OnceLock::new();
impl Links {fn get(&self,i:usize)->Option<&'static Lock<Option<ChildLink>>>{LINKS.get_or_init(||(0..3).map(|_|Lock(Mutex::new(None))).collect()).get(i)}}
struct Vm{running:AtomicBool,generation:AtomicU64}
static VMS:OnceLock<Vec<Vm>>=OnceLock::new();
fn vm_slot(i:u8)->Option<&'static Vm>{VMS.get_or_init(||(0..3).map(|_|Vm{running:AtomicBool::new(true),generation:AtomicU64::new(1)}).collect()).get(i as usize)}
fn vm_run_generation(i:u8)->Option<u64>{Some(vm_slot(i)?.generation.load(Ordering::Acquire))}
mod hv {
    use super::*;
    static BLUEPRINT_PROCESS_CONTEXTS:Contexts=Contexts;
    static BLUEPRINT_CHILD_LINKS:Links=Links;
    pub(crate) fn current_hull_guest_context_vm_id()->Option<u8>{HULL.load(Ordering::Relaxed).then_some(0)}
    @TERMINAL_QUERY@
    @PROCESS_QUERY@
}
pub mod vmcall {
    pub const OP_BP_STDIO_IS_TERMINAL_V1:u32=0x254;pub const STATUS_OK:u32=0;
    pub const OP_BP_PROCESS_IDENTITY_V1:u32=0x256;
    pub const OP_BP_ENV_ARG:u32=0x2b;pub const PAYLOAD_CAP:usize=16;
    pub fn call(op:u32,arg:u64,_:u64)->(u32,u64){(STATUS_OK,match op{OP_BP_STDIO_IS_TERMINAL_V1=>crate::hv::blueprint_stdio_is_terminal(0,arg as u32) as u64,OP_BP_PROCESS_IDENTITY_V1=>crate::hv::blueprint_native_process_identity(0,arg as u32) as u64,_=>panic!("unknown vmcall")})}
    pub fn call_with_payload_reported_len(op:u32,index:u64,offset:u64,_:&[u8],out:&mut[u8])->(u32,u64,usize){assert_eq!(op,OP_BP_ENV_ARG);let args=crate::launch_args();let Some(bytes)=args.get(index as usize) else{return(1,0,0);};let Some(chunk)=bytes.get(offset as usize..) else{return(1,0,0);};let n=chunk.len().min(PAYLOAD_CAP).min(out.len());out[..n].copy_from_slice(&chunk[..n]);(STATUS_OK,bytes.len() as u64,n)}
}
#[path = "@ROOT@/src/native_stdio.rs"] mod native_stdio;
#[path = "@ROOT@/src/native_process_api.rs"] mod native_process_api;

#[test] fn empty_non_utf8_get_set_remove_and_missing_are_distinct(){
    reset(); let key=OsStr::from_bytes(b"\xff"); unsafe{platform_env::setenv(key,OsStr::from_bytes(b"\xfe")).unwrap();}
    assert_eq!(platform_env::getenv(key).unwrap().as_bytes(),b"\xfe");
    unsafe{platform_env::setenv(OsStr::new("EMPTY"),OsStr::new("")).unwrap();}
    assert_eq!(platform_env::getenv(OsStr::new("EMPTY")),Some(OsString::new()));
    assert_eq!(platform_env::getenv(OsStr::new("MISSING")),None);
    unsafe{platform_env::unsetenv(key).unwrap();platform_env::unsetenv(key).unwrap();} assert_eq!(platform_env::getenv(key),None);
}
#[test] fn mutations_reject_invalid_reserved_and_oversized_values_without_changing_old_value(){
    reset(); unsafe{platform_env::setenv(OsStr::new("KEY"),OsStr::new("old")).unwrap();}
    for key in ["","a=b","a\0b","TRUEOS_APP_NAME"] {assert!(unsafe{platform_env::setenv(OsStr::new(key),OsStr::new("x"))}.is_err());}
    assert!(unsafe{platform_env::unsetenv(OsStr::new("TRUEOS_APP_NAME"))}.is_err());
    assert!(unsafe{platform_env::setenv(OsStr::new("KEY"),OsStr::new("bad\0value"))}.is_err());
    assert!(unsafe{platform_env::setenv(OsStr::new("KEY"),OsStr::from_bytes(&vec![b'x';65537]))}.is_err());
    assert_eq!(platform_env::getenv(OsStr::new("KEY")).unwrap(),"old");
}
#[test] fn environment_snapshot_is_consistent_owned_and_per_realm(){
    reset(); unsafe{platform_env::setenv(OsStr::new("A"),OsStr::new("one")).unwrap();}
    let snapshot:Vec<_>=platform_env::env().collect(); r::native_process::realm(Some(1));
    assert_eq!(platform_env::getenv(OsStr::new("A")),None); unsafe{platform_env::setenv(OsStr::new("A"),OsStr::new("two")).unwrap();}
    r::native_process::realm(Some(0)); assert_eq!(platform_env::getenv(OsStr::new("A")).unwrap(),"one");
    unsafe{platform_env::setenv(OsStr::new("A"),OsStr::new("three")).unwrap();}
    assert!(snapshot.contains(&(OsString::from("A"),OsString::from("one"))));
}
#[test] fn logical_threads_observe_the_same_process_mutation(){
    reset(); let worker=std::thread::spawn(|| {r::native_process::realm(Some(0));unsafe{platform_env::setenv(OsStr::new("DOTENV"),OsStr::new("loaded")).unwrap();}}); worker.join().unwrap();
    assert_eq!(platform_env::getenv(OsStr::new("DOTENV")).unwrap(),"loaded");
}
#[test] fn snapshot_short_buffer_does_not_write_and_raw_errors_are_checked(){
    reset(); let required=unsafe{native_environment::trueos_cabi_env_snapshot_v1(ptr::null_mut(),0)};assert!(required>=4);
    let mut bytes=vec![0xa5;required as usize-1];assert_eq!(unsafe{native_environment::trueos_cabi_env_snapshot_v1(bytes.as_mut_ptr(),bytes.len())},required);assert!(bytes.iter().all(|b|*b==0xa5));
    assert_eq!(unsafe{native_environment::trueos_cabi_env_set_v1(ptr::null(),1,b"x".as_ptr(),1)},-22);
    assert_eq!(unsafe{native_environment::trueos_cabi_env_get_v1(b"missing".as_ptr(),7,ptr::null_mut(),0)},-2);
}
#[test] fn native_args_preserve_zero_empty_unicode_and_multi_chunk_values_without_c_startup(){reset();for hull in[false,true]{HULL.store(hull,Ordering::Relaxed);let args:Vec<_>=platform_args::args().map(|s|s.into_vec()).collect();assert_eq!(args,launch_args());}}
#[test] fn attached_read_waits_for_input_instead_of_returning_eof(){
    reset();console().lock().unwrap().inject=true;let mut input=native_streams::Stdin::new();let mut bytes=[0;7];assert_eq!(input.read(&mut bytes).unwrap(),7);assert_eq!(&bytes,b"arrived");let c=console().lock().unwrap();assert_eq!(c.waits,1);assert_eq!(c.claims,1);
}
#[test] fn empty_read_and_transcript_writes_do_not_claim_input(){
    reset();assert_eq!(native_streams::Stdin::new().read(&mut []).unwrap(),0);native_streams::Stdout::new().write_all(b"stdout").unwrap();native_streams::Stderr::new().write_all(b"stderr").unwrap();let c=console().lock().unwrap();assert_eq!(c.claims,0);assert_eq!(c.output,vec![(1,b"stdout".to_vec()),(2,b"stderr".to_vec())]);
}
#[test] fn attached_errors_and_impossible_counts_are_not_eof(){
    reset();console().lock().unwrap().read_error=-2;assert!(native_streams::Stdin::new().read(&mut [0;4]).is_err());console().lock().unwrap().read_error=0;console().lock().unwrap().overread=true;assert_eq!(native_streams::Stdin::new().read(&mut [0;4]).unwrap_err().kind(),io::ErrorKind::InvalidData);
}
#[test] fn child_pipe_eof_partial_writes_and_backpressure_preserve_bytes(){
    reset();child(true);console().lock().unwrap().child_terminal=4;assert_eq!(native_streams::Stdin::new().read(&mut [0;4]).unwrap(),0);console().lock().unwrap().backpressure=2;native_streams::Stdout::new().write_all(b"abcdefghi").unwrap();let c=console().lock().unwrap();assert_eq!(c.claims,0);assert_eq!(c.output.iter().flat_map(|(_,b)|b.iter().copied()).collect::<Vec<_>>(),b"abcdefghi");drop(c);console().lock().unwrap().send_error=-1;assert_eq!(native_streams::Stdout::new().write(b"x").unwrap_err().kind(),io::ErrorKind::BrokenPipe);
}
#[test] fn terminal_query_is_live_read_only_and_distinguishes_child_pipes(){
    reset();let slot=Contexts.get(0).unwrap();*slot.lock()=Some(Context::fresh());
    for hull in [false,true]{HULL.store(hull,Ordering::Relaxed);for s in 0..3{assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(s),1);}assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(3),0);}
    slot.lock().as_mut().unwrap().vars.insert("TRUEOS_MCP_STDIO".into(),"1".into());
    for hull in [false,true]{HULL.store(hull,Ordering::Relaxed);assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(0),0);assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(1),0);assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(2),1);}
    slot.lock().as_mut().unwrap().console_attached=false;assert_eq!(native_stdio::trueos_cabi_stdio_is_terminal_v1(2),0);assert_eq!(console().lock().unwrap().claims,0);
}
#[test] fn paths_use_native_scope_env_and_reject_unsupported_mutations(){
    reset();assert_eq!(native_paths::getcwd().unwrap(),std::path::PathBuf::from("/apps/codex"));assert_eq!(native_paths::chdir(std::path::Path::new("/elsewhere")).unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(native_paths::getcwd().unwrap(),std::path::PathBuf::from("/apps/codex"));assert_eq!(native_paths::current_exe().unwrap_err().kind(),io::ErrorKind::Unsupported);
    unsafe{platform_env::setenv(OsStr::new("HOME"),OsStr::new("/home/native")).unwrap();platform_env::setenv(OsStr::new("TMPDIR"),OsStr::new("/scratch")).unwrap();}assert_eq!(native_paths::home_dir().unwrap(),std::path::PathBuf::from("/home/native"));assert_eq!(native_paths::temp_dir(),std::path::PathBuf::from("/scratch"));
    let values:Vec<_>=native_paths::split_paths(OsStr::from_bytes(b"/a::/\xff")).collect();assert_eq!(values.len(),3);assert_eq!(native_paths::join_paths(values.iter()).unwrap().as_bytes(),b"/a::/\xff");
}
#[test] fn process_identity_uses_vm_owner_and_only_live_generation_checked_parent(){
    reset();let link=Links.get(0).unwrap();*link.lock()=Some(ChildLink{parent_vm_id:1,parent_generation:1,child_generation:1});vm_slot(0).unwrap().generation.store(1,Ordering::Relaxed);vm_slot(1).unwrap().generation.store(1,Ordering::Relaxed);vm_slot(1).unwrap().running.store(true,Ordering::Relaxed);
    for hull in[false,true]{HULL.store(hull,Ordering::Relaxed);assert_eq!(native_process_api::trueos_cabi_process_identity_v1(0),1);assert_eq!(native_process_api::trueos_cabi_process_identity_v1(1),2);assert_eq!(native_process_api::trueos_cabi_process_identity_v1(2),0);}
    vm_slot(1).unwrap().generation.store(2,Ordering::Relaxed);assert_eq!(native_process_api::trueos_cabi_process_identity_v1(1),0);vm_slot(1).unwrap().generation.store(1,Ordering::Relaxed);vm_slot(1).unwrap().running.store(false,Ordering::Relaxed);assert_eq!(native_process_api::trueos_cabi_process_identity_v1(1),0);
}
#[test] fn exit_child_entry(){if std::env::var_os("TRUEOS_TEST_EXIT_MODE").is_some(){native_exit::exit(-17);}}
#[test] fn native_exit_records_code_and_does_not_return_on_shutdown_failure(){
    let executable=std::env::current_exe().unwrap();let output=std::process::Command::new(&executable).args(["--exact","exit_child_entry","--nocapture"]).env("TRUEOS_TEST_EXIT_MODE","reason").output().unwrap();assert!(output.status.success());assert!(String::from_utf8_lossy(&output.stdout).contains("shutdown-reason:Rust process exit code=-17"));
    let status=std::process::Command::new(executable).args(["--exact","exit_child_entry"]).env("TRUEOS_TEST_EXIT_MODE","fallback").status().unwrap();assert_eq!(status.code(),Some(89));
}
