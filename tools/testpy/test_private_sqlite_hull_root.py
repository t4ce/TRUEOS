#!/usr/bin/env python3
"""Execute the actual staged root snapshot and Hull selection/projection bodies.

Host process maps and guest allocation admission are modeled boundaries. The
production snapshot statement, guest selection, guest environment constructor
call, local launch stack, native path parser and resolver execute unchanged.
"""
from pathlib import Path
import hashlib,json,re,subprocess
from test_sdk_metadata_v2 import item
ROOT=Path(__file__).resolve().parents[2]

def main():
 host=(ROOT/'src/hv/mod.rs').read_text();guest=(ROOT/'src/hv/guest_run.rs').read_text();io=(ROOT/'src/r/io.rs').read_text()
 start=host.index('    let Some(process_fs_root) =');end=host.index('    guest_state.process_fs_root = process_fs_root;',start)+len('    guest_state.process_fs_root = process_fs_root;')
 snapshot=host[start:end]
 start=guest.index('    let Some(app_fs_root) =');end=guest.index('    if module.is_filesystem_independent()',start);selection=guest[start:end]
 start=guest.index('        let mut process_env =');end=guest.index('        if timing.request != 0',start);rebuild=guest[start:end].replace('let mut process_env','let process_env',1)
 functions='\n'.join(item(io,needle)for needle in ['pub(crate) fn with_launch_context_console_and_fs_root<','pub(crate) fn current_app_fs_root(', 'fn normalize_app_path(', 'pub(crate) fn resolve_fs_path('])
 code=r'''
#![allow(dead_code,unused_imports)]
extern crate alloc;
use std::{collections::BTreeMap,sync::{Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}}};
type AllocString=String;type MatrixTarget=();
static DOMAIN_OK:AtomicBool=AtomicBool::new(true);static DOMAIN_CALLS:AtomicUsize=AtomicUsize::new(0);static HOST_READS:AtomicUsize=AtomicUsize::new(0);
struct Lock<T>(Mutex<T>);impl<T>Lock<T>{fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
#[derive(Clone)]struct LaunchState{archive:String,module_bytes:Vec<u8>,identity:(),launch_script:Option<String>,app_fs_root:String,process_fs_root:Option<String>}
struct ProcessContext{vars:BTreeMap<String,String>}
#[derive(Debug,PartialEq,Eq)]enum StartError{GuestMemoryUnavailable}
fn clear_blueprint_child_template(_:u8){}fn log(_:&str){}
mod allocators{pub fn with_hv_guest_alloc_domain<T>(_:u8,f:impl FnOnce()->T)->Option<T>{crate::DOMAIN_CALLS.fetch_add(1,crate::Ordering::SeqCst);crate::DOMAIN_OK.load(crate::Ordering::SeqCst).then(f)}}
mod hv{
 pub fn blueprint_process_env_var(_:u8,_:&str)->Option<String>{crate::HOST_READS.fetch_add(1,crate::Ordering::SeqCst);panic!("Hull must not walk the host process map")}
 pub mod blueprint{use std::collections::BTreeMap;
 pub fn app_fs_root_for_archive(archive:&str,_:&[u8])->String{format!("apps/{}",archive.trim_end_matches(".bp"))}
 pub fn build_process_env(archive:&str,_:&[u8],root:Option<&str>,_:Option<&()>,_:Option<&str>,_:bool)->BTreeMap<String,String>{BTreeMap::from([("TRUEOS_APP_FS_ROOT".into(),root.unwrap().into()),("TRUEOS_APP_ARCHIVE".into(),archive.into())])}
 }
}
mod r{#[path="@PATH@"]pub mod path;pub mod io{pub mod kfs{pub fn exists(_:&str)->Result<bool,()>{panic!("path-only fixture")}}}}
#[derive(Clone)]struct LaunchContext{args:Vec<String>,vars:BTreeMap<String,String>,app_fs_root:Option<String>}
static STACK:Lock<Vec<LaunchContext>>=Lock(Mutex::new(Vec::new()));
fn context_stack()->&'static Lock<Vec<LaunchContext>>{&STACK}
fn process_vm_id()->Option<u8>{Some(1)}fn insert_kernel_locale_env(_:&mut BTreeMap<String,String>){}
fn var(key:&str)->Option<String>{STACK.lock().last().and_then(|ctx|ctx.vars.get(key).cloned())}
fn trueosfs_scope_granted()->bool{false}
@CONTEXT_FUNCTIONS@
fn staged(state:&LaunchState,process_context:&ProcessContext)->Result<LaunchState,StartError>{let vm_id=1;let mut guest_state=state.clone();@SNAPSHOT@ Ok(guest_state)}
struct Module;impl Module{fn has_trueosfs_scope(&self)->bool{false}}
fn projected(state:&LaunchState,out:&mut Option<String>,requested:&str)->bool{let vm_id=1;let module=Module;@SELECTION@ @REBUILD@
 // The real invoke_host_rel receives this exact selected root and environment.
 *out=Some(with_launch_context_console_and_fs_root(Vec::new(),process_env,None,Some(app_fs_root),||resolve_fs_path(requested,false).unwrap()));true}
fn state()->LaunchState{LaunchState{archive:"sqlite.bp".into(),module_bytes:Vec::new(),identity:(),launch_script:None,app_fs_root:"apps/sqlite".into(),process_fs_root:None}}
fn reset(){DOMAIN_OK.store(true,Ordering::SeqCst);DOMAIN_CALLS.store(0,Ordering::SeqCst);HOST_READS.store(0,Ordering::SeqCst);STACK.lock().clear();}
#[test]fn authoritative_service_view_survives_host_map_release_and_shadows_container_path(){reset();let identity=state();let mut context=ProcessContext{vars:BTreeMap::from([("TRUEOS_APP_FS_ROOT".into(),"apps/codex/.codex/sqlite-service/codex".into())])};let selected=staged(&identity,&context).unwrap();assert_eq!(selected.app_fs_root,"apps/sqlite");context.vars.clear();let mut result=None;assert!(projected(&selected,&mut result,"/apps/codex/.codex/sqlite-service/codex/state.sqlite"));assert_eq!(result.as_deref(),Some("apps/codex/.codex/sqlite-service/codex/state.sqlite"));assert_eq!(HOST_READS.load(Ordering::SeqCst),0);assert_eq!(DOMAIN_CALLS.load(Ordering::SeqCst),2);}
#[test]fn guest_relative_paths_and_common_alias_use_the_selected_view(){reset();let context=ProcessContext{vars:BTreeMap::from([("TRUEOS_APP_FS_ROOT".into(),"apps/codex/.codex/sqlite-service/codex".into())])};let selected=staged(&state(),&context).unwrap();for(path,expected)in[("state.sqlite","apps/codex/.codex/sqlite-service/codex/state.sqlite"),("/common/sqlite-service/codex","apps/common/sqlite-service/codex")]{let mut result=None;assert!(projected(&selected,&mut result,path));assert_eq!(result.as_deref(),Some(expected));}assert_eq!(HOST_READS.load(Ordering::SeqCst),0);}
#[test]fn absent_snapshot_preserves_ordinary_container_fallback(){reset();let selected=staged(&state(),&ProcessContext{vars:BTreeMap::new()}).unwrap();assert!(selected.process_fs_root.is_none());let mut result=None;assert!(projected(&selected,&mut result,"state.sqlite"));assert_eq!(result.as_deref(),Some("apps/sqlite/state.sqlite"));}
#[test]fn guest_allocation_failure_never_publishes_a_host_pointer_or_partial_view(){reset();DOMAIN_OK.store(false,Ordering::SeqCst);assert!(matches!(staged(&state(),&ProcessContext{vars:BTreeMap::new()}),Err(StartError::GuestMemoryUnavailable)));let mut result=None;assert!(!projected(&state(),&mut result,"state.sqlite"));assert!(result.is_none());assert_eq!(HOST_READS.load(Ordering::SeqCst),0);}
'''.replace('@PATH@',str(ROOT/'src/r/path.rs')).replace('@SNAPSHOT@',snapshot).replace('@SELECTION@',selection).replace('@REBUILD@',rebuild).replace('@CONTEXT_FUNCTIONS@',functions)
 out=ROOT/'bld/codex-native-migration-20261107/private-sqlite-hull-root';out.mkdir(parents=True,exist_ok=True);(out/'fixture.rs').write_text(code)
 channel=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1];checks=[]
 for mode,flags in [('debug',[]),('release',['-O'])]:
  binary=out/mode;commands=[['rustc','+'+channel,'--edition=2024','--test',str(out/'fixture.rs'),*flags,'-o',str(binary)],[str(binary),'--test-threads=1']];logfile=out/(mode+'.log')
  with logfile.open('w')as stream:
   for command in commands:subprocess.run(command,cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=90,check=True)
  assert'4 passed'in logfile.read_text();checks.append({'mode':mode,'commands':commands,'log':str(logfile)})
 # Conservation check: the former container-only selection must reproduce the
 # observed double prefix and fail these behavioral projection assertions.
 needle='state.process_fs_root.as_ref().unwrap_or(&state.app_fs_root).clone()'
 assert code.count(needle)==1
 previous=out/'previous-container-only.rs';previous.write_text(code.replace(needle,'state.app_fs_root.clone()',1));binary=out/'previous-container-only'
 compile_command=['rustc','+'+channel,'--edition=2024','--test',str(previous),'-o',str(binary)];run_command=[str(binary),'--test-threads=1'];logfile=out/'previous-container-only.log'
 with logfile.open('w')as stream:
  subprocess.run(compile_command,cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=90,check=True)
  result=subprocess.run(run_command,cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=90)
 assert result.returncode!=0 and '2 passed; 2 failed'in logfile.read_text()
 checks.append({'mode':'previous-container-only-negative-control','expected':'2 behavioral projection failures','exit':result.returncode,'commands':[compile_command,run_command],'log':str(logfile)})
 paths=[ROOT/path for path in ['src/hv/mod.rs','src/hv/guest_run.rs','src/r/io.rs','src/r/path.rs']]
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'source_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in paths},'fragment_sha256':{key:hashlib.sha256(value.encode()).hexdigest()for key,value in [('stage_snapshot',snapshot),('guest_selection',selection),('guest_environment_call',rebuild),('launch_context_and_projection',functions)]}},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
