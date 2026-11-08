#!/usr/bin/env python3
"""Check production MCP OAuth and CA storage with native policy boundaries.

Real host files, advisory locks and renameat2(RENAME_NOREPLACE) supply I/O.
The kernel private-path query is an explicit allow/deny fixture; its authorization
is independently covered by the real-engine tests. CA key generation is counted
here to prove admission ordering; cryptographic providers are unchanged.
"""
from pathlib import Path
import argparse,json,re,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_sdk_metadata_v2 import item
CODEX=ROOT.parent/'codex/codex-rs'
HARNESS=r'''
#![allow(dead_code,unused_imports)]
extern crate self as trueos;
extern crate self as codex_utils_paths;
use std::{cell::RefCell,path::{Path,PathBuf},io,collections::BTreeMap};
#[derive(Default)]struct Boundary{home:PathBuf,private:PathBuf,checks:Vec<PathBuf>,generated:usize,renames:usize}
thread_local!{static BOUNDARY:RefCell<Boundary>=RefCell::new(Boundary::default());}
pub mod home_dir{pub fn find_codex_home()->io::Result<PathBuf>{Ok(BOUNDARY.with(|b|b.borrow().home.clone()))}use super::*;}
pub mod async_fs{
 use super::*;
 pub fn block_on<T>(future:impl std::future::Future<Output=T>)->T{let mut future=std::pin::pin!(future);let mut context=std::task::Context::from_waker(std::task::Waker::noop());match future.as_mut().poll(&mut context){std::task::Poll::Ready(value)=>value,std::task::Poll::Pending=>panic!("fixture boundary yielded")}}
 pub async fn assert_private_path(bytes:&[u8])->Result<(),i32>{let path=PathBuf::from(std::str::from_utf8(bytes).unwrap());BOUNDARY.with(|b|{let mut b=b.borrow_mut();b.checks.push(path.clone());if !path.starts_with(&b.private){return Err(-19)};for ancestor in path.ancestors(){if let Ok(metadata)=std::fs::symlink_metadata(ancestor){if metadata.file_type().is_symlink(){return Err(-20)}}}Ok(())})}
 pub async fn rename(source:&[u8],destination:&[u8])->Result<(),i32>{BOUNDARY.with(|b|b.borrow_mut().renames+=1);let source=std::ffi::CString::new(source).unwrap();let destination=std::ffi::CString::new(destination).unwrap();let result=unsafe{libc::renameat2(libc::AT_FDCWD,source.as_ptr(),libc::AT_FDCWD,destination.as_ptr(),libc::RENAME_NOREPLACE)};if result==0{Ok(())}else if io::Error::last_os_error().kind()==io::ErrorKind::AlreadyExists{Err(-9)}else{Err(-2)}}
}
fn setup()->tempfile::TempDir{let temp=tempfile::tempdir().unwrap();let home=temp.path().join("private");std::fs::create_dir(&home).unwrap();BOUNDARY.with(|b|*b.borrow_mut()=Boundary{home:home.clone(),private:home,..Default::default()});temp}
fn home()->PathBuf{BOUNDARY.with(|b|b.borrow().home.clone())}
fn deny_to(path:PathBuf){BOUNDARY.with(|b|b.borrow_mut().home=path);}
fn io_kind(error:&anyhow::Error)->io::ErrorKind{error.downcast_ref::<io::Error>().unwrap().kind()}
mod oauth{
 use super::*;use anyhow::{Context,Result};use std::{fs,io::{ErrorKind,Write}};
 type FallbackFile=BTreeMap<String,String>;
 #[path="@OAUTH_POLICY@"]mod native_storage;
 #[path="@REFRESH@"]mod refresh_lock;
 #[path="@STORE@"]mod store_lock;
 #[path="@GENERATION@"]mod enterprise_generation;
 use refresh_lock::RefreshCredentialLock;
 fn compute_store_key(name:&str,url:&str)->Result<String>{Ok(format!("{name}|{url}"))}
 fn fallback_file_path()->Result<PathBuf>{Ok(home_dir::find_codex_home()?.join(".credentials.json"))}
 @FALLBACK@
 #[test]fn native_fallback_roundtrips_and_deletes_real_data(){let _temp=setup();let store=BTreeMap::from([("server".into(),"oauth-token-fixture".into())]);write_fallback_file(&store).unwrap();assert_eq!(read_fallback_file_unlocked().unwrap(),Some(store.clone()));assert_eq!(serde_json::from_slice::<FallbackFile>(&fs::read(fallback_file_path().unwrap()).unwrap()).unwrap(),store);write_fallback_file(&BTreeMap::new()).unwrap();assert!(!fallback_file_path().unwrap().exists());}
 #[test]fn public_credential_read_write_and_delete_refuse_without_effects(){let temp=setup();let outside=temp.path().join("public");fs::create_dir(&outside).unwrap();let path=outside.join(".credentials.json");let bytes=br#"{"retained":"token"}"#;fs::write(&path,bytes).unwrap();deny_to(outside.clone());assert_eq!(io_kind(&read_fallback_file_unlocked().unwrap_err()),io::ErrorKind::PermissionDenied);for store in [BTreeMap::new(),BTreeMap::from([("new".into(),"must-not-save".into())])]{assert_eq!(io_kind(&write_fallback_file(&store).unwrap_err()),io::ErrorKind::PermissionDenied);assert_eq!(fs::read(&path).unwrap(),bytes);}let missing=temp.path().join("missing-parent");deny_to(missing.clone());assert!(write_fallback_file(&BTreeMap::from([("x".into(),"y".into())])).is_err());assert!(!missing.exists());}
 #[tokio::test]async fn aggregate_and_refresh_locks_require_private_home_before_mkdir(){let temp=setup();let outside=temp.path().join("not-created");deny_to(outside.clone());assert!(store_lock::OAuthStoreLock::acquire_for_write(store_lock::OAuthStore::File).is_err());assert!(RefreshCredentialLock::acquire_for_server("server","https://fixture.test").await.is_err());assert!(!outside.exists());}
 #[tokio::test]async fn private_fences_persist_versions_and_unprotected_fences_refuse(){let temp=setup();let lock=RefreshCredentialLock::acquire_for_server("server","https://fixture.test").await.unwrap();let file=enterprise_generation::EnterpriseOAuthGenerationFile::open("server","https://fixture.test",enterprise_generation::EnterpriseOAuthGenerationKind::Credential,&lock).unwrap();assert!(file.current().unwrap().is_none());let generation=file.replace().unwrap();assert!(file.current().unwrap().as_ref()==Some(&generation));let reopened=enterprise_generation::EnterpriseOAuthGenerationFile::open("server","https://fixture.test",enterprise_generation::EnterpriseOAuthGenerationKind::Credential,&lock).unwrap();assert!(reopened.current().unwrap().as_ref()==Some(&generation));let outside=temp.path().join("missing-public");deny_to(outside.clone());assert!(enterprise_generation::EnterpriseOAuthGenerationFile::open("server","https://fixture.test",enterprise_generation::EnterpriseOAuthGenerationKind::Credential,&lock).is_err());assert!(!outside.exists());}
 #[test]fn nonregular_fallback_and_host_symlink_fixture_never_truncate_targets(){let temp=setup();let path=fallback_file_path().unwrap();fs::create_dir(&path).unwrap();assert!(open_fallback_file_for_write(&path).is_err());fs::remove_dir(&path).unwrap();let target=temp.path().join("retained");fs::write(&target,b"retained").unwrap();std::os::unix::fs::symlink(&target,&path).unwrap();assert!(open_fallback_file_for_write(&path).is_err());assert_eq!(fs::read(&target).unwrap(),b"retained");}
 #[test]fn private_aggregate_lock_has_actual_contention(){let _temp=setup();let held=store_lock::OAuthStoreLock::acquire_for_write(store_lock::OAuthStore::File).unwrap();assert!(store_lock::OAuthStoreLock::try_acquire_for_read(store_lock::OAuthStore::File).is_err());drop(held);assert!(store_lock::OAuthStoreLock::try_acquire_for_read(store_lock::OAuthStore::File).is_ok());}
}
mod ca{
 use super::*;use anyhow::{Context,Result,anyhow};use std::{fs,fs::{File,OpenOptions},io::{Read,Write},time::{SystemTime,UNIX_EPOCH},marker::PhantomData};use sha2::{Digest,Sha256};use tracing::{info,warn};
 #[path="@CA_POLICY@"]mod native_storage;
 struct KeyPair;struct Issuer<'a,T>(PhantomData<&'a T>);impl Issuer<'static,KeyPair>{fn from_ca_cert_pem(_: &str,_:KeyPair)->Result<Self>{Ok(Self(PhantomData))}}
 struct ManagedMitmCa{issuer:Issuer<'static,KeyPair>,certificate_path:PathBuf,_artifact_lease:Option<File>}
 fn generate_ca()->Result<(String,KeyPair)>{BOUNDARY.with(|b|b.borrow_mut().generated+=1);Ok(("public-certificate-fixture".into(),KeyPair))}
 fn prune_managed_ca_artifacts(_: &Path){}
 @CONSTANTS@
 impl ManagedMitmCa{@CREATE@}
 @CA_FUNCTIONS@
 #[test]fn denied_ca_creation_happens_before_directory_and_key_generation(){let temp=setup();let outside=temp.path().join("not-created");assert!(ManagedMitmCa::create(&outside).is_err());assert!(!outside.exists());assert_eq!(BOUNDARY.with(|b|b.borrow().generated),0);}
 #[test]fn native_ca_generation_retains_real_lock_and_only_public_certificate_is_persisted(){let _temp=setup();let directory=home().join("proxy");let created=ManagedMitmCa::create(&directory).unwrap();assert_eq!(fs::read(&created.certificate_path).unwrap(),b"public-certificate-fixture");let lock=managed_ca_certificate_lock_path(&created.certificate_path).unwrap();assert!(matches!(open_managed_ca_lock(&lock).unwrap().try_lock(),Err(std::fs::TryLockError::WouldBlock)));assert!(!fs::read_dir(&directory).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().contains("key")));drop(created);assert!(open_managed_ca_lock(&lock).unwrap().try_lock().is_ok());}
 #[test]fn atomic_publication_never_replaces_existing_data(){let _temp=setup();let path=home().join("published.pem");write_atomic_create_new(&path,b"first",0o644).unwrap();assert!(write_atomic_create_new(&path,b"second",0o644).is_err());assert_eq!(fs::read(&path).unwrap(),b"first");assert_eq!(fs::read_dir(home()).unwrap().count(),1);assert_eq!(BOUNDARY.with(|b|b.borrow().renames),2);}
 #[test]fn matching_certificate_can_be_reused_but_mismatch_is_preserved(){let _temp=setup();let path=home().join("bundle.pem");write_atomic_create_new_or_reuse(&path,b"public",0o644).unwrap();write_atomic_create_new_or_reuse(&path,b"public",0o644).unwrap();assert!(write_atomic_create_new_or_reuse(&path,b"mismatch",0o644).is_err());assert_eq!(fs::read(path).unwrap(),b"public");}
 #[test]fn public_certificate_reads_remain_allowed_while_private_key_reads_require_native_scope(){let temp=setup();let public=temp.path().join("public.pem");fs::write(&public,b"public fixture").unwrap();assert_eq!(read_external_ca_file(&public,"certificate",false).unwrap(),"public fixture");assert_eq!(io_kind(&read_external_ca_file(&public,"private key",true).unwrap_err()),io::ErrorKind::PermissionDenied);let private=home().join("key.pem");fs::write(&private,b"private fixture").unwrap();assert_eq!(read_external_ca_file(&private,"private key",true).unwrap(),"private fixture");}
 #[test]fn denied_ca_locks_and_publication_do_not_create_public_files(){let temp=setup();let outside=temp.path().join("public");fs::create_dir(&outside).unwrap();assert!(open_managed_ca_lock(&outside.join("lock")).is_err());assert!(write_atomic_create_new(&outside.join("cert"),b"payload",0o644).is_err());assert_eq!(fs::read_dir(outside).unwrap().count(),0);}
}
'''
def selected(text):
 return text.replace('#[cfg(all(unix, not(target_os = "trueos")))]','#[cfg(any())]').replace('#[cfg(not(target_os = "trueos"))]','#[cfg(any())]').replace('#[cfg(target_os = "trueos")]','#[cfg(all())]')
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 oauth=CODEX/'rmcp-client/src/oauth.rs';certs=CODEX/'network-proxy/src/certs.rs';paths=[oauth,certs,CODEX/'rmcp-client/src/oauth/native_storage.rs',CODEX/'network-proxy/src/native_ca_storage.rs']+[CODEX/'rmcp-client/src/oauth'/name for name in ['refresh_lock.rs','store_lock.rs','enterprise_generation.rs']]
 with tempfile.TemporaryDirectory(prefix='native-oauth-ca-')as tmp:
  work=Path(tmp);generated=HARNESS
  for marker,path in [('@OAUTH_POLICY@',paths[2]),('@CA_POLICY@',paths[3]),('@REFRESH@',paths[4]),('@STORE@',paths[5]),('@GENERATION@',paths[6])]:
   copy=work/path.name;source=path.read_text();source=source.split('#[cfg(test)]',1)[0];copy.write_text(selected(source));generated=generated.replace(marker,str(copy))
  generated=generated.replace('@FALLBACK@','\n'.join(selected(item(oauth.read_text(),signature))for signature in ['fn read_fallback_file_unlocked(','fn open_fallback_file_for_write(','fn write_fallback_file(']))
  source=certs.read_text();constants='\n'.join(re.search(r'const '+name+r'[^;]*;',source)[0]for name in ['MAX_EXTERNAL_CA_FILE_BYTES','MANAGED_MITM_CA_ARTIFACT_LOCK','MANAGED_MITM_CA_CERT_PREFIX','MANAGED_MITM_CA_TRUST_BUNDLE_PREFIX'])
  generated=generated.replace('@CONSTANTS@',constants).replace('@CREATE@',selected(item(source,'fn create(proxy_dir:')))
  signatures=['fn read_external_ca_file(','fn persist_managed_ca_certificate(','fn lock_managed_ca_certificate(','fn lock_managed_ca_artifacts(','fn managed_ca_certificate_lock_path(','fn open_managed_ca_lock(','fn write_atomic_create_new(','fn write_atomic_create_new_or_reuse(','fn sync_parent_dir(']
  functions='\n'.join(selected(item(source,signature))for signature in signatures)
  native=source[source.index('#[cfg(target_os = "trueos")]\nfn open_create_new_with_mode'):]
  functions+='\n'+item(native,'fn open_create_new_with_mode(')
  generated=generated.replace('@CA_FUNCTIONS@',functions);(work/'lib.rs').write_text(generated)
  manifest(work,'''[package]
name="native-codex-oauth-ca-storage"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
tokio={version="=1.54.1",features=["rt","macros","time"]}
tempfile="3"
serde_json="1"
sha2="0.10"
oauth2={version="5",default-features=false}
thiserror="2"
tracing="0.1"
libc="0.2"
''')
  for profile,flags in [('debug',[]),('release',['--release'])]:
   command=['cargo','+'+TOOLCHAIN,'test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(work/'Cargo.toml'),'--','--test-threads=1']
   result=subprocess.run(command,cwd=work,env=environment(out/'target'),capture_output=True,text=True,timeout=240);log=out/(profile+'.log');log.write_text(result.stdout+result.stderr)
   if result.returncode:raise RuntimeError(log.read_text()[-14000:])
   count=int(re.search(r'test result: ok\. (\d+) passed',result.stdout)[1]);assert count==12,count;checks.append({'profile':profile,'tests':count,'log':str(log),'command':command})
 receipt=out/'receipt.json';receipt.write_text(json.dumps({'scope':__doc__,'sources':[info(path)for path in paths],'checks':checks},indent=2)+'\n');print(receipt)
if __name__=='__main__':main()
