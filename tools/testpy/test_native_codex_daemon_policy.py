#!/usr/bin/env python3
"""Check native daemon capability boundaries and actual retained File locks.

Production Rust bodies are selected for TRUEOS; host files supply real advisory
locks. Process, install and terminal fixtures record effects instead of spawning
anything. The target-selection body preserves explicit WebSocket endpoints.
"""
from pathlib import Path
import argparse,json,re,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_sdk_metadata_v2 import item
CODEX=ROOT.parent/'codex/codex-rs'
HARNESS=r'''
#![allow(dead_code,unused_imports)]
use std::{io,path::{Path,PathBuf},sync::atomic::{AtomicUsize,Ordering}};
use anyhow::{Context,Result};
static EFFECTS:AtomicUsize=AtomicUsize::new(0);
mod daemon {
 use super::*;
 struct Daemon{pid_file:PathBuf}
 type LifecycleCommand=();type LifecycleOutput=();type BootstrapOptions=();type BootstrapOutput=();
 impl Daemon{
  fn from_environment()->Result<Self>{EFFECTS.fetch_add(1,Ordering::SeqCst);Ok(Self{pid_file:PathBuf::new()})}
  async fn run(self,_:LifecycleCommand)->Result<LifecycleOutput>{EFFECTS.fetch_add(1,Ordering::SeqCst);Ok(())}
  async fn bootstrap(self,_:BootstrapOptions)->Result<BootstrapOutput>{EFFECTS.fetch_add(1,Ordering::SeqCst);Ok(())}
 }
 mod backend{pub mod stderr_log{pub fn start(_:std::path::PathBuf){super::super::super::EFFECTS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);}}}
 @SUPPORT@
 @RUN@
 @BOOTSTRAP@
 @MAINTAIN@
 #[tokio::test]async fn explicit_lifecycle_refuses_before_environment_install_or_log_effects(){
  for result in [run(()).await,bootstrap(()).await,maintain_stderr_log()]{assert_eq!(result.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::Unsupported);}
  assert_eq!(EFFECTS.load(Ordering::SeqCst),0);
 }
}
mod operation_lock {use super::*;type DaemonLockFile=std::fs::File;@OPERATION_LOCK@
 #[test]fn native_operation_lock_preserves_real_contention(){let temp=tempfile::tempdir().unwrap();let path=temp.path().join("lock");std::fs::write(&path,b"retained metadata").unwrap();let a=std::fs::OpenOptions::new().write(true).open(&path).unwrap();let b=std::fs::OpenOptions::new().write(true).open(&path).unwrap();assert!(try_lock_file(&a).unwrap());assert!(!try_lock_file(&b).unwrap());a.unlock().unwrap();assert!(try_lock_file(&b).unwrap());assert_eq!(std::fs::read(path).unwrap(),b"retained metadata");}
}
mod pid {use super::*;type DaemonLockFile=std::fs::File;struct PidRecord;
 @PID_LOCK@ @PROBE@ @PROBE_GUARD@ @PROBE_DROP@ @PROCESS@
 #[test]fn pid_reservation_probes_unlock_retained_clones(){let temp=tempfile::tempdir().unwrap();let path=temp.path().join("lock");std::fs::write(&path,b"pid metadata").unwrap();let a=std::fs::OpenOptions::new().write(true).open(&path).unwrap();let b=std::fs::OpenOptions::new().write(true).open(&path).unwrap();assert!(try_lock_file(&a).unwrap());assert!(probe_reservation_lock(b.try_clone().unwrap()).unwrap());assert!(!try_lock_file(&b).unwrap());assert!(!probe_reservation_lock(a.try_clone().unwrap()).unwrap());assert!(try_lock_file(&b).unwrap());b.unlock().unwrap();assert_eq!(std::fs::read(path).unwrap(),b"pid metadata");}
 #[test]fn cancelled_empty_pid_probe_releases_real_lock(){let temp=tempfile::tempdir().unwrap();let path=temp.path().join("lock");std::fs::write(&path,b"retained").unwrap();let a=std::fs::OpenOptions::new().write(true).open(&path).unwrap();let b=std::fs::OpenOptions::new().write(true).open(&path).unwrap();assert!(try_lock_file(&a).unwrap());let guard=EmptyPidProbeLock(a.try_clone().unwrap());assert!(!try_lock_file(&b).unwrap());drop(guard);assert!(try_lock_file(&b).unwrap());}
 #[tokio::test]async fn detached_process_probes_and_termination_never_fake_results(){
  for result in [terminate_process(1),force_terminate_process(1),terminate_process_group(1),force_terminate_process_group(1)]{assert_eq!(result.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::Unsupported);}
  assert_eq!(process_matches_record(&PidRecord).await.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::Unsupported);
  assert_eq!(read_process_start_time(1).await.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::Unsupported);
  assert_eq!(read_process_details(1).await.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(),io::ErrorKind::Unsupported);
 }
}
mod startup {use super::*;
 #[derive(Default)]struct Cli{no_daemon:bool,oss:bool,config_profile_v2:Option<String>,strict_config:bool,bypass_hook_trust:bool}
 struct LoaderOverrides;
 fn config_exclusion(_: &[(String,toml::Value)],_: &LoaderOverrides,_:bool,_:bool)->Option<&'static str>{None}
 type AbsolutePathBuf=PathBuf;
 #[derive(Clone,Debug,PartialEq,Eq)]enum RemoteAppServerEndpoint{UnixSocket{socket_path:PathBuf},WebSocket{url:String}}
 #[derive(Clone,Debug,PartialEq,Eq)]@TARGET@
 @EXCLUSION@ @SELECT@
 #[test]fn native_default_and_no_daemon_choose_embedded_before_discovery(){for cli in [Cli::default(),Cli{no_daemon:true,..Default::default()}]{let excluded=exclusion(&cli,&[],&LoaderOverrides,false,None);assert!(excluded.is_some());assert_eq!(app_server_target_for_launch(None,Some(PathBuf::from("must-not-open.sock")),excluded.is_none(),false,None).unwrap(),AppServerTarget::Embedded);}}
 #[test]fn explicit_remote_websocket_keeps_its_selected_endpoint(){let endpoint=RemoteAppServerEndpoint::WebSocket{url:"wss://fixture.test/codex".into()};assert_eq!(app_server_target_for_launch(Some(endpoint.clone()),None,false,false,None).unwrap(),AppServerTarget::Remote{endpoint});}
}
'''
def native_item(text,signature):
    start=text.index('#[cfg(target_os = "trueos")]\n'+signature)
    return item(text[start:],signature)
def selected(text):
    return text.replace('#[cfg(all(unix, not(target_os = "trueos")))]','#[cfg(any())]').replace('#[cfg(not(target_os = "trueos"))]','#[cfg(any())]').replace('#[cfg(target_os = "trueos")]','#[cfg(all())]')
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True)
    paths=[CODEX/'app-server-daemon/src/lib.rs',CODEX/'app-server-daemon/src/backend/pid.rs',CODEX/'tui/src/daemon_startup.rs',CODEX/'tui/src/lib.rs',CODEX/'cli/src/main.rs',CODEX/'app-server-daemon/src/launch.rs']
    lib,pid,startup,tui,cli,launch=[path.read_text()for path in paths]
    mappings={'@SUPPORT@':native_item(lib,'fn ensure_supported_platform('),'@RUN@':item(lib,'pub async fn run('),'@BOOTSTRAP@':item(lib,'pub async fn bootstrap('),'@MAINTAIN@':item(lib,'pub fn maintain_stderr_log('),'@OPERATION_LOCK@':native_item(lib,'fn try_lock_file('),'@PID_LOCK@':native_item(pid,'fn try_lock_file('),'@PROBE@':selected(item(pid,'fn probe_reservation_lock(')),'@PROBE_GUARD@':'struct EmptyPidProbeLock(DaemonLockFile);','@PROBE_DROP@':selected(item(pid,'impl Drop for EmptyPidProbeLock')),'@PROCESS@':'\n'.join(native_item(pid,signature)for signature in ['fn native_process_unavailable','fn terminate_process(','fn force_terminate_process(','fn terminate_process_group(','fn force_terminate_process_group(','async fn process_matches_record(','async fn read_process_start_time(','async fn read_process_details(']),'@TARGET@':item(tui,'pub(crate) enum AppServerTarget'),'@EXCLUSION@':item(startup,'pub(super) fn exclusion(').replace('pub(super)','pub(crate)').replace('cfg!(target_os = "trueos")','true'),'@SELECT@':item(tui,'fn app_server_target_for_launch(')}
    for source,names in [(lib,['pub async fn run(','pub async fn bootstrap(','pub async fn ensure_remote_control_ready(','pub async fn enable_remote_control_on_socket(','pub async fn start_remote_control_pairing(','pub async fn set_remote_control(','pub async fn run_pid_update_loop(','pub async fn update(','pub fn maintain_stderr_log(','pub async fn probe_app_server_version(']),(launch,['pub async fn start_with_features_classified(','pub async fn restart_with_features('])]:
        for name in names:assert item(source,name).split('{',1)[1].strip().startswith('ensure_supported_platform()?;'),name
    assert cli.index('if managed_daemon {')<cli.index('let transport = if stdio {')
    generated=HARNESS
    for marker,value in mappings.items():generated=generated.replace(marker,value)
    checks=[]
    with tempfile.TemporaryDirectory(prefix='trueos-daemon-policy-')as tmp:
        work=Path(tmp);manifest(work,'''[package]
name="native-codex-daemon-policy"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
tokio={version="=1.54.1",features=["rt","macros"]}
tempfile="3"
tracing="0.1.44"
toml="0.9.5"
''');(work/'lib.rs').write_text(generated)
        for profile,flags in [('debug',[]),('release',['--release'])]:
            command=['cargo','+'+TOOLCHAIN,'test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(work/'Cargo.toml'),'--','--test-threads=1']
            result=subprocess.run(command,cwd=work,env=environment(out/'target'),text=True,capture_output=True,timeout=180);log=out/(profile+'.log');log.write_text(result.stdout+result.stderr)
            if result.returncode:raise RuntimeError(log.read_text()[-15000:])
            count=int(re.search(r'test result: ok\. (\d+) passed',result.stdout)[1]);assert count==7;checks.append({'profile':profile,'tests':count,'log':str(log),'command':command})
    receipt=out/'receipt.json';receipt.write_text(json.dumps({'scope':__doc__,'sources':[info(path)for path in paths],'checks':checks},indent=2)+'\n');print(receipt)
if __name__=='__main__':main()
