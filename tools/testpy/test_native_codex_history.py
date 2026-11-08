#!/usr/bin/env python3
"""Run the actual native Codex history module over real host files.

Only native metadata IDs and the kernel private-workspace assertion are adapted.
File bytes, append, clone cursors, rename and advisory locks use real host std.
The config fixture supplies data types; original history algorithms are unchanged.
"""
from pathlib import Path
import argparse
import json
import shutil
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
import subprocess

CODEX = ROOT.parent / "codex/codex-rs"
ADAPTER = r'''
trait NativeIdentity {fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;}
impl NativeIdentity for std::fs::Metadata {
 fn root_id(&self)->u32{u32::try_from(std::os::unix::fs::MetadataExt::dev(self)).unwrap()}
 fn file_id(&self)->Option<u64>{if FIXTURE_MISSING_ID.load(std::sync::atomic::Ordering::SeqCst){None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}}
}
static FIXTURE_MISSING_ID:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]mod native_tests {
 use super::*;use std::sync::atomic::Ordering;
 fn config(path:&Path)->HistoryConfig{trueos::RESULT.store(0,Ordering::SeqCst);HistoryConfig{codex_home:path.to_path_buf(),persistence:HistoryPersistence::SaveAll,max_bytes:None}}
 #[tokio::test]async fn append_lookup_and_batch_share_the_native_identity(){let d=tempfile::tempdir().unwrap();let c=config(d.path());append_entry("first","one",&c).await.unwrap();let(id,n)=history_metadata(&c).await;assert_ne!(id,0);assert_eq!(n,1);append_entry("second","two",&c).await.unwrap();assert_eq!(history_metadata(&c).await,(id,2));assert_eq!(lookup(id,1,&c).unwrap().text,"second");let b=lookup_batch(id,HistoryBatchCursor::new(1),&c).unwrap();assert_eq!(b.entries.len(),2);assert_eq!(b.entries[0].entry.as_ref().unwrap().text,"second");}
 #[tokio::test]async fn capped_history_trims_under_the_existing_file_lock(){let d=tempfile::tempdir().unwrap();let mut c=config(d.path());append_entry(&"a".repeat(200),"one",&c).await.unwrap();let before=std::fs::metadata(history_filepath(&c)).unwrap().len();c.max_bytes=Some(before as usize+10);append_entry(&"b".repeat(200),"two",&c).await.unwrap();let contents=std::fs::read_to_string(history_filepath(&c)).unwrap();let rows=contents.lines().collect::<Vec<_>>();assert_eq!(rows.len(),1);assert!(rows[0].contains(&"b".repeat(200)));assert!(contents.len()<=c.max_bytes.unwrap());}
 #[tokio::test]async fn replacement_changes_identity_and_rejects_old_cursor(){let d=tempfile::tempdir().unwrap();let c=config(d.path());append_entry("original","one",&c).await.unwrap();let(id,_)=history_metadata(&c).await;let p=d.path().join("replacement");std::fs::write(&p,b"replacement\n").unwrap();std::fs::rename(&p,history_filepath(&c)).unwrap();assert_ne!(history_metadata(&c).await.0,id);assert!(lookup(id,0,&c).is_none());assert!(lookup_batch(id,HistoryBatchCursor::new(0),&c).unwrap().entries.is_empty());}
 #[tokio::test]async fn private_policy_denial_precedes_all_file_creation(){let d=tempfile::tempdir().unwrap();let c=config(&d.path().join("absent-parent"));trueos::RESULT.store(-19,Ordering::SeqCst);assert_eq!(append_entry("secret","one",&c).await.unwrap_err().kind(),std::io::ErrorKind::PermissionDenied);assert!(!c.codex_home.exists());trueos::RESULT.store(0,Ordering::SeqCst);}
 #[test]fn native_identity_does_not_invent_missing_or_zero_file_ids(){let d=tempfile::tempdir().unwrap();let p=d.path().join("history");std::fs::write(&p,b"bytes").unwrap();FIXTURE_MISSING_ID.store(true,Ordering::SeqCst);assert_eq!(log_identity(&std::fs::metadata(&p).unwrap()),None);FIXTURE_MISSING_ID.store(false,Ordering::SeqCst);assert_eq!(trueos_log_identity(1,0),None);assert_ne!(trueos_log_identity(1,7),trueos_log_identity(2,7));assert_ne!(trueos_log_identity(1,7),trueos_log_identity(1,8));}
 #[test]fn private_policy_errors_keep_their_meaning(){let p=Path::new("/fixture/history.jsonl");for(code,kind)in[(-4,std::io::ErrorKind::InvalidInput),(-8,std::io::ErrorKind::NotFound),(-19,std::io::ErrorKind::PermissionDenied),(-20,std::io::ErrorKind::Unsupported),(-2,std::io::ErrorKind::Other)]{trueos::RESULT.store(code,Ordering::SeqCst);assert_eq!(assert_private_history_path(p).unwrap_err().kind(),kind);}trueos::RESULT.store(0,Ordering::SeqCst);}
 #[tokio::test]async fn concurrent_real_locked_appends_remain_complete(){let d=tempfile::tempdir().unwrap();let c=config(d.path());let mut jobs=Vec::new();for worker in 0..4{let c=c.clone();jobs.push(tokio::spawn(async move{for n in 0..12{append_entry(&format!("worker{worker}-{n}"),worker,&c).await.unwrap();}}));}for job in jobs{job.await.unwrap();}assert_eq!(history_metadata(&c).await.1,48);let lines=std::fs::read_to_string(history_filepath(&c)).unwrap();for line in lines.lines(){let _:HistoryEntry=serde_json::from_str(line).unwrap();}}
}
'''
POLICY = r'''
pub static RESULT:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
pub mod async_fs {
 pub async fn assert_private_path(_path:&[u8])->Result<(),i32>{match crate::RESULT.load(std::sync::atomic::Ordering::SeqCst){0=>Ok(()),code=>Err(code)}}
 pub fn block_on<T>(future:impl core::future::Future<Output=T>)->T{let mut f=core::pin::pin!(future);let mut cx=core::task::Context::from_waker(core::task::Waker::noop());match f.as_mut().poll(&mut cx){core::task::Poll::Ready(v)=>v,_=>panic!("unexpected pending policy fixture")}}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-codex-history-')as directory:
  work=Path(directory);shutil.copytree(CODEX/'message-history/src',work/'src')
  for path in(work/'src').rglob('*.rs'):
   text=path.read_text().replace('#[cfg(test)]\nmod tests;','#[cfg(any())]\nmod tests;');path.write_text(text)
  with(work/'src/lib.rs').open('a')as stream:stream.write(ADAPTER)
  manifest(work/'config','[package]\nname="codex-config"\nversion="0.0.0"\nedition="2024"\n[lib]\npath="lib.rs"\n');(work/'config/lib.rs').write_text('pub mod types {#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum HistoryPersistence{SaveAll,None}pub struct History{pub persistence:HistoryPersistence,pub max_bytes:Option<usize>}}\n')
  manifest(work/'policy','[package]\nname="trueos"\nversion="0.1.0"\nedition="2024"\n[lib]\npath="lib.rs"\n');(work/'policy/lib.rs').write_text(POLICY)
  manifest(work,'''[package]
name="codex-message-history"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
codex-config={path="config"}
trueos={path="policy"}
memchr="2.7"
serde={version="1",features=["derive"]}
serde_json="1"
tokio={version="=1.54.1",features=["fs","io-util","rt","macros","rt-multi-thread"]}
tracing="0.1"
tempfile="3"
''')
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="codex_message_history":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
  for label,flags in [('debug',[]),('release',['--release'])]:
   command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','native_tests','--','--test-threads=1'];log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=240)
   checks.append({'mode':label,'exit':r.returncode,'command':command,'log':str(log)})
   if r.returncode:raise RuntimeError(log.read_text()[-14000:])
   assert'7 passed'in log.read_text()
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(CODEX/'message-history'/path)for path in ['src/lib.rs','src/batch.rs','Cargo.toml']]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
