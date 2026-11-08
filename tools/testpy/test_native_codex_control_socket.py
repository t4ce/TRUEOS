#!/usr/bin/env python3
"""Check Codex control-socket native boundaries and retained lock identities.

The actual production functions run over real host files and advisory locks.
Only the native metadata accessors are adapted to optional object IDs. The
native socket entry points must reject before creating any rendezvous paths.
This is a filesystem boundary proof, not a native IPC integration test.
"""
from pathlib import Path
import argparse, json, subprocess, tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
from test_native_codex_signals import function

SOURCE = ROOT.parent / 'codex/codex-rs/app-server-transport/src/transport/unix_socket.rs'
HARNESS = r'''
#![allow(dead_code)]
use std::{fs::{self, File}, io::{self, ErrorKind, Result as IoResult}, path::{Path,PathBuf}, sync::atomic::{AtomicUsize, Ordering}};
type AbsolutePathBuf = PathBuf;
enum UnixListener {}
enum DaemonSocketFileGuard {}
static MISSING:AtomicUsize=AtomicUsize::new(0);
static ID_INDEX:AtomicUsize=AtomicUsize::new(0);
static ROOT_INDEX:AtomicUsize=AtomicUsize::new(0);
static FOREIGN_ROOT:AtomicUsize=AtomicUsize::new(0);
trait NativeIdentity {fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;}
impl NativeIdentity for std::fs::Metadata {
 fn root_id(&self)->u32{let n=ROOT_INDEX.fetch_add(1,Ordering::SeqCst)+1;let root=u32::try_from(std::os::unix::fs::MetadataExt::dev(self)).unwrap();if FOREIGN_ROOT.load(Ordering::SeqCst)==n{root.wrapping_add(1)}else{root}}
 fn file_id(&self)->Option<u64>{let n=ID_INDEX.fetch_add(1,Ordering::SeqCst)+1;if MISSING.load(Ordering::SeqCst)==n{None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}}
}
__FUNCTIONS__
#[cfg(test)]mod tests {
 use super::*;
 struct Dir(PathBuf);
 impl Dir{fn new()->Self{MISSING.store(0,Ordering::SeqCst);ID_INDEX.store(0,Ordering::SeqCst);ROOT_INDEX.store(0,Ordering::SeqCst);FOREIGN_ROOT.store(0,Ordering::SeqCst);let p=std::env::temp_dir().join(format!("native-control-lock-{}",std::process::id()));let _=fs::remove_dir_all(&p);fs::create_dir(&p).unwrap();Self(p)}fn file(&self)->(PathBuf,File){let p=self.0.join("startup.lock");fs::write(&p,b"retained").unwrap();let f=File::options().read(true).write(true).open(&p).unwrap();(p,f)}}
 impl Drop for Dir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn ready<T>(future:impl std::future::Future<Output=T>)->T{let mut f=std::pin::pin!(future);match f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())){std::task::Poll::Ready(v)=>v,_=>panic!("socket capability check must not wait")}}
 #[test]fn retained_open_and_current_path_have_real_identical_object_id(){let d=Dir::new();let(p,f)=d.file();assert!(startup_lock_file_matches_path(&f,&p).unwrap());}
 #[test]fn removed_path_is_not_the_retained_object(){let d=Dir::new();let(p,f)=d.file();fs::remove_file(&p).unwrap();assert!(!startup_lock_file_matches_path(&f,&p).unwrap());}
 #[test]fn rename_then_replace_is_detected_with_equal_payloads(){let d=Dir::new();let(p,f)=d.file();fs::rename(&p,d.0.join("original")).unwrap();fs::write(&p,b"retained").unwrap();assert!(!startup_lock_file_matches_path(&f,&p).unwrap());}
 #[test]fn identical_file_id_on_another_native_root_is_distinct(){let d=Dir::new();let(p,f)=d.file();FOREIGN_ROOT.store(2,Ordering::SeqCst);assert!(!startup_lock_file_matches_path(&f,&p).unwrap());}
 #[test]fn missing_retained_identity_fails_without_inventing_one(){let d=Dir::new();let(p,f)=d.file();MISSING.store(1,Ordering::SeqCst);assert_eq!(startup_lock_file_matches_path(&f,&p).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(fs::read(p).unwrap(),b"retained");}
 #[test]fn missing_path_identity_fails_without_inventing_one(){let d=Dir::new();let(p,f)=d.file();MISSING.store(2,Ordering::SeqCst);assert_eq!(startup_lock_file_matches_path(&f,&p).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(fs::read(p).unwrap(),b"retained");}
 #[test]fn cleanup_removes_only_the_retained_lock_object(){let d=Dir::new();let(p,f)=d.file();let lock=AppServerStartupLock{_file:f,removable_path:Some(p.clone()),remove_on_drop:false};lock.remove_file().unwrap();assert!(!p.exists());}
 #[test]fn cleanup_preserves_a_replacement_file(){let d=Dir::new();let(p,f)=d.file();fs::rename(&p,d.0.join("original")).unwrap();fs::write(&p,b"replacement").unwrap();let lock=AppServerStartupLock{_file:f,removable_path:Some(p.clone()),remove_on_drop:false};lock.remove_file().unwrap();assert_eq!(fs::read(p).unwrap(),b"replacement");}
 #[test]fn missing_cleanup_identity_returns_error_and_preserves_file(){let d=Dir::new();let(p,f)=d.file();MISSING.store(1,Ordering::SeqCst);let lock=AppServerStartupLock{_file:f,removable_path:Some(p.clone()),remove_on_drop:false};assert_eq!(lock.remove_file().unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(fs::read(p).unwrap(),b"retained");}
 #[test]fn native_bind_and_peer_verification_reject_before_any_path_effects(){let d=Dir::new();let socket=d.0.join("absent-parent").join("control.sock");assert!(matches!(ready(bind_private_daemon_socket(&socket)),Err(e)if e.kind()==ErrorKind::Unsupported));assert_eq!(verified_local_daemon_socket(&socket).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(fs::read_dir(&d.0).unwrap().count(),0);}
}
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True)
    text=SOURCE.read_text()
    parts=[function(text,start)for start in ['pub async fn bind_private_daemon_socket(', 'pub fn verified_local_daemon_socket(', 'pub struct AppServerStartupLock {', 'impl AppServerStartupLock {', 'fn startup_lock_file_matches_path(']]
    checks=[]
    with tempfile.TemporaryDirectory(prefix='trueos-control-socket-')as temporary:
        work=Path(temporary)
        manifest(work,'[package]\nname="native-codex-control-socket"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n')
        (work/'lib.rs').write_text(HARNESS.replace('__FUNCTIONS__','\n'.join(parts)))
        (out/'selected-production-functions.rs').write_text('\n'.join(parts)+'\n')
        for mode,flags in [('debug',[]),('release',['--release'])]:
            command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1']
            log=out/(mode+'.log')
            with log.open('w')as stream:
                result=subprocess.run(command,cwd=work,env=environment(out/'target',native_host=True),stdout=stream,stderr=subprocess.STDOUT,timeout=180)
            checks.append({'mode':mode,'exit':result.returncode,'command':command,'log':str(log)})
            if result.returncode:raise RuntimeError(log.read_text()[-12000:])
            assert '10 passed'in log.read_text()
    (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(SOURCE)]},indent=2)+'\n')
    print(out/'receipt.json')

if __name__=='__main__':main()
