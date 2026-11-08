#!/usr/bin/env python3
"""Compare actual trusted launch admission with native SQLite argv/script parsing."""
from pathlib import Path
import hashlib,json,re,subprocess,tempfile
from test_sdk_metadata_v2 import item
ROOT=Path(__file__).resolve().parents[2];BLUE=ROOT.parent/'TRUEOS-Blueprints'
kernel=ROOT/'src/hv/blueprint/blueprint.rs';app=BLUE/'apps/sqlite/src/main.rs';private=BLUE/'apps/sqlite/src/private_storage.rs'
fragments=[item(kernel.read_text(),needle)for needle in ['fn safe_archive_stem(', 'pub(crate) fn codex_sqlite_service_requested(', 'pub(crate) fn build_process_args(']]
app_parts=[item(app.read_text(),needle)for needle in ['fn valid_name(', 'fn port_for(', 'fn options(']]
private_parts=[item(private.read_text(),needle)for needle in ['pub fn resolve(']]
fixture=r'''
extern crate self as trueos;
use std::{path::PathBuf,sync::Mutex};
static ARGS:Mutex<Vec<String>>=Mutex::new(Vec::new());static SCRIPT:Mutex<Option<Vec<u8>>>=Mutex::new(None);static TRUSTED:Mutex<bool>=Mutex::new(false);
pub mod env{pub fn args()->std::vec::IntoIter<String>{crate::ARGS.lock().unwrap().clone().into_iter()}pub fn var(key:&str)->Result<String,std::env::VarError>{if *crate::TRUSTED.lock().unwrap(){match key{"TRUEOS_CODEX_SQLITE_STORAGE"=>return Ok(crate::private_storage::DIRECTORY.into()),"TRUEOS_CODEX_SQLITE_ENDPOINT"=>return Ok(crate::private_storage::ENDPOINT.into()),_=>{}}}Err(std::env::VarError::NotPresent)}}
pub mod async_fs{pub async fn read_file(_: &[u8])->Result<Vec<u8>,i32>{crate::SCRIPT.lock().unwrap().clone().ok_or(-8)}pub fn block_on<T>(future:impl core::future::Future<Output=T>)->T{let mut f=core::pin::pin!(future);let mut cx=core::task::Context::from_waker(core::task::Waker::noop());match f.as_mut().poll(&mut cx){core::task::Poll::Ready(v)=>v,_=>panic!("unexpected pending")}}}
mod private_storage {use std::path::PathBuf;pub const DIRECTORY:&str="/apps/codex/.codex/sqlite-service/codex";pub const ENDPOINT:&str="http://127.0.0.1:42116";@PRIVATE@}
@KERNEL@
@APP@
fn arguments(args:&[&str])->Vec<String>{args.iter().map(|s|s.to_string()).collect()}
fn evaluate(archive:&str,args:&[&str],script:Option<&str>,trusted:bool)->(Result<bool,&'static str>,Result<(String,PathBuf),String>){let args=arguments(args);let admission=codex_sqlite_service_requested(archive,&args,script);*ARGS.lock().unwrap()=build_process_args(archive,&args);*SCRIPT.lock().unwrap()=script.map(|s|s.as_bytes().to_vec());*TRUSTED.lock().unwrap()=trusted;(admission,options())}
#[test]fn real_launch_stream_overrides_argv_in_both_parsers(){for(args,script,expected)in[(&["--instance","public"][..],Some("instance codex"),true),(&["instance","codex"][..],Some("--instance public"),false),(&["instance","public","--instance","codex"][..],None,true),(&[][..],None,false)]{let(admission,actual)=evaluate("sqlite.bp",args,script,expected);assert_eq!(admission,Ok(expected));assert_eq!(actual.unwrap().0=="codex",expected);}}
#[test]fn private_custom_directory_is_rejected_in_every_option_order(){for(args,script)in[(&["--data-dir","/common/bad","instance","codex"][..],None),(&["instance","codex","--data-dir","/apps/codex/.codex/sqlite-service"][..],None),(&["--data-dir","/common/bad"][..],Some("instance codex"))]{let(admission,actual)=evaluate("sqlite.bp",args,script,true);assert!(admission.is_err());assert!(actual.is_err());}}
#[test]fn malformed_or_unknown_options_never_receive_a_private_purpose(){for args in [&["instance"][..],&["--instance"][..],&["--data-dir"][..],&["instance","codex","unknown"][..]]{let(admission,actual)=evaluate("sqlite.bp",args,None,true);assert!(admission.is_err());assert!(actual.is_err());}for invalid in ["", "codex/other", "codex..", "cødex"]{let(admission,actual)=evaluate("sqlite.bp",&["instance",invalid],None,false);assert_ne!(admission,Ok(true));assert!(actual.is_err());}}
#[test]fn package_aliases_match_the_existing_exact_app_normalization(){for archive in ["sqlite","sqlite.bp","SQLITE.bp","sqlite.bp.bp"]{let(admission,actual)=evaluate(archive,&["instance","codex"],None,true);assert_eq!(safe_archive_stem(archive),"sqlite");assert_eq!(admission,Ok(true));assert_eq!(actual.unwrap().0,"codex");}for archive in ["sqlite-tools.bp","sqlite_other.bp","other.bp"]{assert_eq!(codex_sqlite_service_requested(archive,&arguments(&["instance","codex"]),None),Ok(false));}}
#[test]fn native_codex_cannot_silently_fall_back_to_public_storage(){let(admission,actual)=evaluate("sqlite.bp",&["instance","codex"],None,false);assert_eq!(admission,Ok(true));assert!(actual.is_err());let(_,actual)=evaluate("sqlite.bp",&["instance","codex"],None,true);assert_eq!(actual.unwrap().1,PathBuf::from(private_storage::DIRECTORY));}
#[test]fn explicitly_public_instances_keep_their_options_and_receive_no_delegate(){let(admission,actual)=evaluate("sqlite.bp",&["instance","public","--data-dir","/common/custom"],None,false);assert_eq!(admission,Ok(false));assert_eq!(actual.unwrap().1,PathBuf::from("/common/custom/public"));let(admission,actual)=evaluate("sqlite.bp",&["instance","CODEX"],None,false);assert_eq!(admission,Ok(false));assert_eq!(actual.unwrap().0,"CODEX");assert_ne!(port_for("CODEX"),42116);}
#[test]fn reserved_endpoint_is_the_actual_service_hash_and_trusted_values_cannot_leak_to_other_instances(){assert_eq!(port_for("codex"),42116);let(admission,actual)=evaluate("sqlite.bp",&["instance","public"],None,true);assert_eq!(admission,Ok(false));assert!(actual.is_err());}
'''
def main():
 out=ROOT/'bld/codex-native-migration-20261107/private-sqlite-launch-parser';out.mkdir(parents=True,exist_ok=True)
 source=fixture.replace('@KERNEL@','\n'.join(fragments)).replace('@APP@','\n'.join(app_parts)).replace('@PRIVATE@','\n'.join(private_parts));(out/'fixture.rs').write_text(source)
 channel=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1];checks=[]
 for mode,flags in [('debug',[]),('release',['-O'])]:
  binary=out/mode;commands=[['rustc','+'+channel,'--edition=2024','--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','--test',str(out/'fixture.rs'),*flags,'-o',str(binary)],[str(binary),'--test-threads=1']];log=out/(mode+'.log')
  with log.open('w')as stream:
   for command in commands:subprocess.run(command,cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=90,check=True)
  assert'7 passed'in log.read_text();checks.append({'mode':mode,'commands':commands,'log':str(log)})
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'source_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in [kernel,app,private]},'boundary':'Actual parser/storage-resolution bodies; SDK launch argv/script and immutable trusted values are fixture inputs. No guest, user data or service touched.'},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
