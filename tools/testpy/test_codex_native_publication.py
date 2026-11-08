#!/usr/bin/env python3
"""Exercise exact Codex native object, socket and rollout publication providers.

The host supplies actual files, Tokio and Zstd. Metadata identity uses actual
host file IDs; the native key-comparison call is an explicit policy fixture.
The kernel comparison itself is covered by test_native_file_objects.py.
"""
from pathlib import Path
import argparse,json,re,subprocess,tempfile
from test_sdk_metadata_v2 import item
from test_native_mio import ROOT,TOOLCHAIN,environment,info
CODEX=ROOT.parent/'codex/codex-rs'
TESTS=r'''
extern crate alloc;
use std::{collections::BTreeMap,fs::{File,Metadata},io::{self,Read,Write},path::Path,sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}}};
use std::os::unix::fs::MetadataExt;
static NO_ID:AtomicBool=AtomicBool::new(false);
static POLICIES:Mutex<BTreeMap<u64,Vec<u8>>>=Mutex::new(BTreeMap::new());
trait HostMetadata {fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;}
impl HostMetadata for Metadata {fn root_id(&self)->u32{1}fn file_id(&self)->Option<u64>{(!NO_ID.load(Ordering::SeqCst)).then(||self.ino())}}
trait HostAccess {fn has_same_access_key(&self,other:&File)->io::Result<bool>;}
impl HostAccess for File {fn has_same_access_key(&self,other:&File)->io::Result<bool>{let keys=POLICIES.lock().unwrap();Ok(keys.get(&self.metadata()?.ino())==keys.get(&other.metadata()?.ino()))}}
fn policy(path:&Path,keys:Vec<u8>){POLICIES.lock().unwrap().insert(std::fs::metadata(path).unwrap().ino(),keys);}
#[derive(Debug)]pub struct FileMetadata {pub content_type:Option<()>,pub is_directory:bool,pub is_file:bool,pub is_symlink:bool,pub size:u64,pub created_at_ms:i64,pub modified_at_ms:i64}
mod output {
 use std::{io,path::Path};
 fn invalid_output(message:&'static str)->io::Error{io::Error::new(io::ErrorKind::InvalidInput,message)}
 #[path="@NATIVE_OBJECT@"]mod native;
 use native as imp;
 @BOUND_OUTPUT@
}
#[path="@NATIVE_SOCKET@"]mod native_socket;
mod migration {
 use super::*;
 use std::fs::Permissions;
 use std::path::PathBuf;
 use std::time::SystemTime;
 use tokio::fs::File;
 use std::fs::File as StdFile;
 type ThreadStoreResult<T>=io::Result<T>;
 struct MigrationLocks;
 fn migration_error(error:impl std::fmt::Display)->io::Error{io::Error::other(error.to_string())}
 @MIGRATION@
 #[tokio::test]
 async fn staging_checks_policy_before_truncation_and_writes_real_data(){
  let temp=tempfile::tempdir().unwrap();let source=temp.path().join("source");let stage=temp.path().join("stage");
  std::fs::write(&source,b"source payload").unwrap();std::fs::write(&stage,b"retained stage").unwrap();
  policy(&source,vec![1,2]);policy(&stage,vec![1]);
  assert_eq!(open_staged_rollout(&source,&stage,std::fs::metadata(&source).unwrap().permissions()).await.unwrap_err().kind(),io::ErrorKind::PermissionDenied);
  assert_eq!(std::fs::read(&stage).unwrap(),b"retained stage");assert_eq!(std::fs::read(&source).unwrap(),b"source payload");
  policy(&stage,vec![1,2]);let mut file=open_staged_rollout(&source,&stage,std::fs::metadata(&source).unwrap().permissions()).await.unwrap();
  assert!(std::fs::read(&stage).unwrap().is_empty());tokio::io::AsyncWriteExt::write_all(&mut file,b"new history").await.unwrap();file.sync_all().await.unwrap();assert_eq!(std::fs::read(&stage).unwrap(),b"new history");
 }
 #[tokio::test]
 async fn actual_compress_and_decompress_preserve_policy_and_existing_bytes_on_refusal(){
  let temp=tempfile::tempdir().unwrap();let plain=temp.path().join("plain");let packed=temp.path().join("packed");let unpacked=temp.path().join("unpacked");
  let payload=b"native rollout compression and policy\n".repeat(1000);std::fs::write(&plain,&payload).unwrap();std::fs::write(&packed,b"old packed").unwrap();std::fs::write(&unpacked,b"old unpacked").unwrap();
  policy(&plain,vec![5]);policy(&packed,vec![6]);policy(&unpacked,vec![6]);let locks=Arc::new(MigrationLocks);
  assert!(compress_rollout_to_path(&plain,&packed,std::fs::metadata(&plain).unwrap().permissions(),None,&locks).await.is_err());assert_eq!(std::fs::read(&packed).unwrap(),b"old packed");
  policy(&packed,vec![5]);compress_rollout_to_path(&plain,&packed,std::fs::metadata(&plain).unwrap().permissions(),None,&locks).await.unwrap();assert_eq!(zstd::stream::decode_all(&std::fs::read(&packed).unwrap()[..]).unwrap(),payload);
  assert!(decompress_rollout_to_path(&packed,&unpacked,&locks).await.is_err());assert_eq!(std::fs::read(&unpacked).unwrap(),b"old unpacked");
  policy(&unpacked,vec![5]);decompress_rollout_to_path(&packed,&unpacked,&locks).await.unwrap();assert_eq!(std::fs::read(&unpacked).unwrap(),payload);assert_eq!(std::fs::read(&plain).unwrap(),payload);
 }
}
#[test]fn bound_output_retains_real_object_and_rejects_visible_replacement(){
 let temp=tempfile::tempdir().unwrap();let path=temp.path().join("output");std::fs::write(&path,b"old output").unwrap();
 let output=output::BoundOutput::open(&path).unwrap();assert_eq!(std::fs::read(&path).unwrap(),b"old output");output.write(b"new").unwrap();assert_eq!(std::fs::read(&path).unwrap(),b"new");
 let output=output::BoundOutput::open(&path).unwrap();std::fs::rename(&path,temp.path().join("retained")).unwrap();std::fs::write(&path,b"replacement").unwrap();
 assert_eq!(output.write(b"must not truncate replacement").unwrap_err().kind(),io::ErrorKind::InvalidInput);assert_eq!(std::fs::read(&path).unwrap(),b"replacement");assert_eq!(std::fs::read(temp.path().join("retained")).unwrap(),b"new");
}
#[test]fn absent_object_identity_and_descriptor_output_are_honestly_unavailable(){
 let temp=tempfile::tempdir().unwrap();let path=temp.path().join("output");std::fs::write(&path,b"retained").unwrap();NO_ID.store(true,Ordering::SeqCst);
 assert_eq!(output::BoundOutput::open(&path).err().unwrap().kind(),io::ErrorKind::Unsupported);NO_ID.store(false,Ordering::SeqCst);assert_eq!(std::fs::read(&path).unwrap(),b"retained");
 for path in ["/dev/stdout","/dev/stderr","/dev/fd/2","/proc/self/fd/2"]{assert_eq!(output::BoundOutput::open(Path::new(path)).err().unwrap().kind(),io::ErrorKind::Unsupported);}
}
#[tokio::test]async fn native_socket_operations_refuse_before_creating_paths(){
 let temp=tempfile::tempdir().unwrap();let path=temp.path().join("socket-directory");
 assert_eq!(native_socket::prepare_private_socket_directory(&path).await.unwrap_err().kind(),io::ErrorKind::Unsupported);assert!(!path.exists());
 assert_eq!(native_socket::bind_listener(&path).await.err().unwrap().kind(),io::ErrorKind::Unsupported);assert!(!path.exists());
 assert_eq!(native_socket::connect_stream(&path).await.err().unwrap().kind(),io::ErrorKind::Unsupported);
 assert_eq!(native_socket::is_stale_socket_path(&path).await.unwrap_err().kind(),io::ErrorKind::Unsupported);
 assert_eq!(native_socket::shared_daemon_socket_directory().unwrap_err().kind(),io::ErrorKind::Unsupported);
 assert_eq!(native_socket::prepare_shared_daemon_socket_directory().unwrap_err().kind(),io::ErrorKind::Unsupported);
}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
 sources=[CODEX/'exec-server/src/no_follow/native.rs',CODEX/'exec-server/src/no_follow/mod.rs',CODEX/'uds/src/native.rs',CODEX/'thread-store/src/local/rollout_migration/publish.rs']
 with tempfile.TemporaryDirectory(prefix='trueos-codex-native-publication-')as tmp:
  work=Path(tmp);native=work/'native_object.rs';text=sources[0].read_text();text=text.replace('use crate::FileMetadata;','use crate::{FileMetadata,HostMetadata};');native.write_text(text)
  bound='\n'.join(item(sources[1].read_text(),marker)for marker in ['pub struct BoundOutput','enum Destination','impl BoundOutput'])
  bound=bound.replace('all(unix, not(target_os = "trueos"))','any()').replace('target_os = "trueos"','all()')
  published=sources[3].read_text();selected='\n'.join(item(published,marker)for marker in ['pub(super) fn verify_native_access(','pub(super) async fn verify_native_open_access(','pub(super) async fn open_staged_rollout(','pub(super) async fn decompress_rollout_to_path(','pub(super) async fn compress_rollout_to_path('])
  selected=selected.replace('#[cfg(not(target_os = "trueos"))]','#[cfg(any())]').replace('#[cfg(all(unix, not(target_os = "trueos"))))]','#[cfg(any())]').replace('#[cfg(all(unix, not(target_os = "trueos")))]','#[cfg(any())]').replace('#[cfg(target_os = "trueos")]','#[cfg(all())]')
  generated=TESTS.replace('@NATIVE_OBJECT@',str(native)).replace('@NATIVE_SOCKET@',str(sources[2])).replace('@BOUND_OUTPUT@',bound).replace('@MIGRATION@',selected)
  (work/'lib.rs').write_text(generated)
  (work/'Cargo.toml').write_text('''[package]
name="codex-native-publication-proof"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
tokio={version="=1.54.1",features=["fs","io-util","macros","rt","sync","time"]}
zstd="=0.13.3"
tempfile="=3.27.0"
''')
  checks=[]
  for mode,flags in [('debug',[]),('release',['--release'])]:
   command=['cargo','+'+TOOLCHAIN,'test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(work/'Cargo.toml'),'--','--test-threads=1']
   result=subprocess.run(command,cwd=work,env=environment(out/'target'),text=True,capture_output=True,timeout=240);log=out/(mode+'.log');log.write_text(result.stdout+result.stderr)
   if result.returncode:raise RuntimeError(log.read_text()[-14000:])
   count=int(re.search(r'test result: ok\. (\d+) passed',result.stdout)[1]);assert count==5,count;checks.append({'profile':mode,'tests':count,'log':str(log),'command':command})
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'sources':[info(p)for p in sources],'checks':checks},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
