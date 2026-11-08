#!/usr/bin/env python3
"""Run native daemon installer locks and selection identity over real host files.

The production install-lock module uses actual Tokio and std advisory locks.
Selection functions use native-shaped optional IDs/revisions; only the metadata
accessor boundary is adapted. No Unix daemon is spawned or package pruned.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_codex_signals import function
CODEX=ROOT.parent/'codex/codex-rs'
SOURCE=CODEX/'app-server-daemon/src/release_pruning.rs'
LOCK=CODEX/'app-server-daemon/src/install_lock.rs'
HARNESS=r'''
#![allow(dead_code)]
use std::{path::Path,io,sync::atomic::{AtomicUsize,AtomicU64,Ordering}};
use anyhow::{Context,Result};
#[path="__LOCK__"]mod install_lock;
static MISSING:AtomicUsize=AtomicUsize::new(0);
static REVISION:AtomicU64=AtomicU64::new(1);
static ROOT_OFFSET:AtomicUsize=AtomicUsize::new(0);
trait NativeIdentity{fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;fn revision(&self)->Option<u64>;}
impl NativeIdentity for std::fs::Metadata{
 fn root_id(&self)->u32{u32::try_from(std::os::unix::fs::MetadataExt::dev(self)).unwrap().wrapping_add(ROOT_OFFSET.load(Ordering::SeqCst)as u32)}
 fn file_id(&self)->Option<u64>{if MISSING.load(Ordering::SeqCst)==1{None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}}
 fn revision(&self)->Option<u64>{if MISSING.load(Ordering::SeqCst)==2{None}else{Some(REVISION.load(Ordering::SeqCst))}}
}
__FUNCTIONS__
#[cfg(test)]mod tests{
 use super::*;use std::{fs,time::Duration};
 fn reset(){MISSING.store(0,Ordering::SeqCst);REVISION.store(1,Ordering::SeqCst);ROOT_OFFSET.store(0,Ordering::SeqCst);}
 #[tokio::test]async fn native_installer_uses_file_lock_without_shell_or_pid_directory(){let d=tempfile::tempdir().unwrap();let lock=install_lock::acquire_install_lock(d.path()).await.unwrap();let names=fs::read_dir(d.path()).unwrap().map(|p|p.unwrap().file_name()).collect::<Vec<_>>();assert_eq!(names,vec![std::ffi::OsString::from("install.lock")]);drop(lock);}
 #[tokio::test]async fn retained_lock_serializes_an_independent_contender_until_release(){let d=tempfile::tempdir().unwrap();let held=install_lock::acquire_install_lock(d.path()).await.unwrap();let mut other=Box::pin(install_lock::acquire_install_lock(d.path()));assert!(tokio::time::timeout(Duration::from_millis(15),&mut other).await.is_err());drop(held);let acquired=tokio::time::timeout(Duration::from_secs(1),other).await.unwrap().unwrap();drop(acquired);}
 #[tokio::test]async fn acquiring_native_lock_preserves_existing_file_bytes(){let d=tempfile::tempdir().unwrap();let path=d.path().join("install.lock");fs::write(&path,b"installer metadata").unwrap();let held=install_lock::acquire_install_lock(d.path()).await.unwrap();assert_eq!(fs::read(path).unwrap(),b"installer metadata");drop(held);}
 #[tokio::test]async fn failed_open_propagates_without_directory_lock_fallback(){let d=tempfile::tempdir().unwrap();fs::create_dir(d.path().join("install.lock")).unwrap();assert!(install_lock::acquire_install_lock(d.path()).await.is_err());assert_eq!(fs::read_dir(d.path()).unwrap().count(),1);}
 #[test]fn native_selection_identity_changes_when_object_or_revision_changes(){reset();let d=tempfile::tempdir().unwrap();let p=d.path().join("current");fs::write(&p,b"one").unwrap();let original=selection_identity(&p).unwrap();fs::write(&p,b"two").unwrap();REVISION.store(2,Ordering::SeqCst);assert_ne!(selection_identity(&p).unwrap(),original);let revised=selection_identity(&p).unwrap();fs::rename(&p,d.path().join("old")).unwrap();fs::write(&p,b"two").unwrap();assert_ne!(selection_identity(&p).unwrap(),revised);}
 #[test]fn selection_identity_includes_root_and_requires_id_and_revision(){reset();let d=tempfile::tempdir().unwrap();let p=d.path().join("current");fs::write(&p,b"one").unwrap();let original=selection_identity(&p).unwrap();ROOT_OFFSET.store(1,Ordering::SeqCst);assert_ne!(selection_identity(&p).unwrap(),original);for missing in[1,2]{MISSING.store(missing,Ordering::SeqCst);assert!(selection_identity(&p).is_err());}reset();}
 #[test]fn native_mount_boundary_uses_real_root_for_files_and_directories(){reset();let d=tempfile::tempdir().unwrap();let p=d.path().join("file");fs::write(&p,b"one").unwrap();assert_eq!(owned_mount(d.path()).unwrap(),owned_mount(&p).unwrap());let original=owned_mount(&p).unwrap();ROOT_OFFSET.store(1,Ordering::SeqCst);assert_ne!(owned_mount(&p).unwrap(),original);reset();}
 #[test]fn absent_selection_propagates_actual_missing_path_error(){reset();let d=tempfile::tempdir().unwrap();assert_eq!(selection_identity(&d.path().join("missing")).unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::NotFound);}
}
'''
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 text=SOURCE.read_text();parts=[function(text,'fn selection_identity('),function(text,'fn owned_mount(')]
 with tempfile.TemporaryDirectory(prefix='trueos-install-lock-')as temporary:
  work=Path(temporary);manifest(work,'''[package]
name="native-codex-install-lock"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
tokio={version="=1.54.1",features=["fs","rt","macros","time"]}
tempfile="3"
''');(work/'lib.rs').write_text(HARNESS.replace('__LOCK__',str(LOCK)).replace('__FUNCTIONS__','\n'.join(parts)))
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="native_codex_install_lock":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'];log=out/(mode+'.log')
   with log.open('w')as stream:result=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'mode':mode,'exit':result.returncode,'command':command,'log':str(log)})
   if result.returncode:raise RuntimeError(log.read_text()[-12000:])
   assert '8 passed'in log.read_text()
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(SOURCE),info(LOCK)]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
