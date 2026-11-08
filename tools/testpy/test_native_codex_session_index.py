#!/usr/bin/env python3
"""Exercise actual Codex session-index append/remove/locking over real files.

Only protocol ID/config types and the native access-equivalence service boundary
are adapted. Tempfile publication, bytes, retained handles and locks are real
host filesystem operations. Native key enforcement itself has the independent
real-engine opcode15 matrix in test_native_file_objects.py.
"""
from pathlib import Path
import argparse
import json
import re
import shutil
import subprocess
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest

SOURCE = ROOT.parent / 'codex/codex-rs/rollout/src/session_index.rs'


def item(source, name):
    match = re.search(r'^(?:pub(?:\([^)]*\))? )?(?:async )?fn ' + re.escape(name) + r'\(', source, re.M)
    if not match:
        raise ValueError(name)
    return source[match.start():source.index('\n}', match.end()) + 2]


HARNESS = r'''
use std::fs::File;
use std::io::{ErrorKind,Write};
use std::path::{Path,PathBuf};
use std::time::{Duration,Instant};
use serde::{Deserialize,Serialize};
type ThreadId=uuid::Uuid;
const SESSION_INDEX_FILE:&str="session_index.jsonl";
const SESSION_INDEX_LOCK_FILE:&str="session_index.jsonl.lock";
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub struct SessionIndexEntry{pub id:ThreadId,pub thread_name:String,pub updated_at:String}

#[cfg(target_os="trueos")]
trait NativeAccess{fn has_same_access_key(&self,other:&File)->std::io::Result<bool>;}
#[cfg(target_os="trueos")]
impl NativeAccess for File{
 fn has_same_access_key(&self,other:&File)->std::io::Result<bool>{
  use std::os::unix::fs::MetadataExt;
  let mut state=STATE.lock().unwrap();
  state.calls+=1;
  let call=state.calls;
  assert_eq!(self.metadata()?.ino(),state.original_inode,"policy must inspect the exact source whose bytes were read");
  if call==1{assert_eq!(other.metadata()?.len(),0,"private source bytes reached staging before access proof");}
  if call==2{if state.expect_empty{assert_eq!(other.metadata()?.len(),0);}else{assert!(other.metadata()?.len()>0,"publication proof must occur after staging bytes");}}
  match state.mode{
   1=>Ok(false),
   2=>Ok(call==1),
   3=>Err(std::io::Error::new(ErrorKind::PermissionDenied,"epoch retired")),
   4=>{
    let replacement=state.path.with_extension("replacement");
    std::fs::write(&replacement,b"concurrent replacement must survive\n")?;
    std::fs::rename(replacement,&state.path)?;
    assert_ne!(self.metadata()?.ino(),std::fs::metadata(&state.path)?.ino());
    Ok(false)
   },
   _=>Ok(true)
  }
 }
}
#[derive(Default)]struct PolicyState{mode:u8,calls:usize,original_inode:u64,path:PathBuf,expect_empty:bool}
static STATE:std::sync::Mutex<PolicyState>=std::sync::Mutex::new(PolicyState{mode:0,calls:0,original_inode:0,path:PathBuf::new(),expect_empty:false});

#[cfg(test)]mod tests{
 use super::*;
 use std::os::unix::fs::{MetadataExt,PermissionsExt};
 fn entry(id:ThreadId,name:&str)->SessionIndexEntry{SessionIndexEntry{id,thread_name:name.into(),updated_at:"2026-11-08T00:00:00Z".into()}}
 fn configure(path:&Path,mode:u8){let inode=std::fs::metadata(path).unwrap().ino();*STATE.lock().unwrap()=PolicyState{mode,calls:0,original_inode:inode,path:path.into(),expect_empty:false};}
 fn temporary_count(path:&Path)->usize{std::fs::read_dir(path).unwrap().filter_map(Result::ok).filter(|entry|entry.file_name().to_string_lossy().starts_with("session-index-")).count()}
 #[tokio::test]async fn append_then_remove_preserves_other_names_and_malformed_lines(){
  let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();let b=ThreadId::new_v4();
  append_session_index_entry(d.path(),entry(a,"old")).await.unwrap();
  append_session_index_entry(d.path(),entry(b,"keep")).await.unwrap();
  append_session_index_entry(d.path(),entry(a,"new")).await.unwrap();
  let path=session_index_path(d.path());std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"opaque legacy row\n").unwrap();configure(&path,0);
  remove_thread_name_entries(d.path(),a).await.unwrap();
  let bytes=std::fs::read_to_string(&path).unwrap();let rows=bytes.lines().collect::<Vec<_>>();assert_eq!(rows.len(),2);
  assert_eq!(serde_json::from_str::<SessionIndexEntry>(rows[0]).unwrap(),entry(b,"keep"));assert_eq!(rows[1],"opaque legacy row");assert_eq!(temporary_count(d.path()),0);
 }
 #[tokio::test]async fn nonexistent_index_is_noop_without_lock_or_staging(){let d=tempfile::tempdir().unwrap();remove_thread_name_entries(d.path(),ThreadId::new_v4()).await.unwrap();assert_eq!(std::fs::read_dir(d.path()).unwrap().count(),0);}
 #[tokio::test]async fn removing_all_entries_publishes_valid_empty_index(){let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();append_session_index_entry(d.path(),entry(a,"only")).await.unwrap();let path=session_index_path(d.path());configure(&path,0);STATE.lock().unwrap().expect_empty=true;remove_thread_name_entries(d.path(),a).await.unwrap();assert!(std::fs::read(&path).unwrap().is_empty());assert_eq!(temporary_count(d.path()),0);}
 #[tokio::test]async fn absent_thread_keeps_exact_original_file(){let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();append_session_index_entry(d.path(),entry(a,"keep")).await.unwrap();let path=session_index_path(d.path());configure(&path,1);let before=std::fs::read(&path).unwrap();let inode=std::fs::metadata(&path).unwrap().ino();remove_thread_name_entries(d.path(),ThreadId::new_v4()).await.unwrap();assert_eq!(std::fs::read(&path).unwrap(),before);assert_eq!(std::fs::metadata(&path).unwrap().ino(),inode);assert_eq!(STATE.lock().unwrap().calls,0);}
 #[tokio::test(flavor="multi_thread",worker_threads=4)]async fn concurrent_locked_appends_and_removal_are_complete(){let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();let b=ThreadId::new_v4();let mut jobs=Vec::new();for n in 0..32{let home=d.path().to_path_buf();jobs.push(tokio::spawn(async move{append_session_index_entry(&home,entry(if n%2==0{a}else{b},&format!("name-{n}"))).await.unwrap();}));}for job in jobs{job.await.unwrap();}let path=session_index_path(d.path());configure(&path,0);remove_thread_name_entries(d.path(),a).await.unwrap();let rows=std::fs::read_to_string(&path).unwrap();assert_eq!(rows.lines().count(),16);for line in rows.lines(){assert_eq!(serde_json::from_str::<SessionIndexEntry>(line).unwrap().id,b);}}
 #[cfg(not(target_os="trueos"))]
 #[tokio::test]async fn host_rewrite_preserves_actual_permission_bits(){let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();append_session_index_entry(d.path(),entry(a,"remove")).await.unwrap();let path=session_index_path(d.path());std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o640)).unwrap();remove_thread_name_entries(d.path(),a).await.unwrap();assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode()&0o777,0o640);}
 #[cfg(target_os="trueos")]
 async fn denied(mode:u8,kind:ErrorKind){let d=tempfile::tempdir().unwrap();let a=ThreadId::new_v4();let b=ThreadId::new_v4();append_session_index_entry(d.path(),entry(a,"remove")).await.unwrap();append_session_index_entry(d.path(),entry(b,"secret remains private")).await.unwrap();let path=session_index_path(d.path());configure(&path,mode);let before=std::fs::read(&path).unwrap();let error=remove_thread_name_entries(d.path(),a).await.unwrap_err();assert_eq!(error.kind(),kind);if mode==4{assert_eq!(std::fs::read(&path).unwrap(),b"concurrent replacement must survive\n");}else{assert_eq!(std::fs::read(&path).unwrap(),before);}assert_eq!(temporary_count(d.path()),0);}
 #[cfg(target_os="trueos")]
 #[tokio::test]async fn distinct_source_policy_refuses_before_copy(){denied(1,ErrorKind::PermissionDenied).await;assert_eq!(STATE.lock().unwrap().calls,1);}
 #[cfg(target_os="trueos")]
 #[tokio::test]async fn policy_revocation_before_publish_keeps_original(){denied(2,ErrorKind::PermissionDenied).await;assert_eq!(STATE.lock().unwrap().calls,2);}
 #[cfg(target_os="trueos")]
 #[tokio::test]async fn access_service_error_propagates_without_publication(){denied(3,ErrorKind::PermissionDenied).await;}
 #[cfg(target_os="trueos")]
 #[tokio::test]async fn pathname_replacement_cannot_substitute_source_policy(){denied(4,ErrorKind::PermissionDenied).await;}
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    source = SOURCE.read_text()
    functions = ['append_session_index_entry', 'remove_thread_name_entries',
                 'verify_session_index_access', 'with_session_index_lock', 'session_index_path']
    extracted = '\n\n'.join(('#[cfg(target_os="trueos")]\n' if name == 'verify_session_index_access' else '')
                            + item(source, name) for name in functions)
    # Keep Cargo's invocation outside TRUEOS so its native build-std defaults do
    # not mix rebuilt core/alloc with the host std used by these real-file tests.
    directory = tempfile.TemporaryDirectory(prefix='trueos-session-index-')
    work = Path(directory.name)
    (work / 'lib.rs').write_text(HARNESS + '\n' + extracted)
    manifest(work, '''[package]
name="codex-session-index-fixture"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
serde={version="1",features=["derive"]}
serde_json="1"
uuid={version="1",features=["v4","serde"]}
tempfile="3"
tokio={version="=1.54.1",features=["rt","macros","rt-multi-thread"]}
''')
    wrapper = work / 'native-rustc.py'
    wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if os.environ.get("FIXTURE_NATIVE")=="1" and "--crate-name" in args and args[args.index("--crate-name")+1]=="codex_session_index_fixture":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''')
    wrapper.chmod(0o755)
    shutil.copytree(work, out / 'fixture', dirs_exist_ok=True)
    checks = []
    for native, count in [(False, 6), (True, 9)]:
        for label, flags in [('debug', []), ('release', ['--release'])]:
            env = environment(out / ('native-target' if native else 'host-target'))
            env['RUSTC_WRAPPER'] = str(wrapper)
            env['FIXTURE_NATIVE'] = '1' if native else '0'
            command = ['rustup', 'run', TOOLCHAIN, 'cargo', 'test', '--offline', *flags,
                       '--target', 'x86_64-unknown-linux-gnu', '--lib', '--', '--test-threads=1']
            log = out / (('native-' if native else 'host-') + label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=300)
            checks.append({'native': native, 'profile': label, 'tests': count,
                           'command': command, 'exit': result.returncode, 'log': str(log)})
            if result.returncode or f'{count} passed' not in log.read_text():
                raise RuntimeError(log.read_text()[-12000:])
    receipt = {'status': 'passed', 'scope': __doc__, 'source': info(SOURCE),
               'actual_functions': functions, 'fixture': info(out / 'fixture/lib.rs'),
               'checks': checks, 'key_enforcement_receipt': str(ROOT / 'bld/codex-native-migration-20261107/native-file-objects-access-equivalence.json')}
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    directory.cleanup()
    print(out / 'receipt.json')


if __name__ == '__main__':
    main()
