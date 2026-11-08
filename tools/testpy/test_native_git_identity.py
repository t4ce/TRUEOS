#!/usr/bin/env python3
"""Exercise native Git trust and traversal using exact pinned source overlays.

Real host files and native-shaped optional metadata IDs drive walkdir/same-file.
The SDK policy boundary returns explicit owner assertions or errors; it never
derives ownership from readability. Kernel access-key enforcement is separate.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
from test_native_same_file import ADAPTER as SAME_FILE_ADAPTER
SDK=ROOT.parent/'TRUEOS-Blueprints'
METADATA=r'''
trait NativeIdentity {fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;}
impl NativeIdentity for std::fs::Metadata {
 fn root_id(&self)->u32{<u32 as std::convert::TryFrom<u64>>::try_from(std::os::unix::fs::MetadataExt::dev(self)).unwrap()}
 fn file_id(&self)->Option<u64>{if crate::FIXTURE_MISSING_ID.load(std::sync::atomic::Ordering::SeqCst){None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}}
}
'''
POLICY=r'''
pub static RESULT:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
pub static CALLS:std::sync::Mutex<Vec<Vec<u8>>>=std::sync::Mutex::new(Vec::new());
pub mod async_fs {
 pub async fn assert_private_path(path:&[u8])->Result<(),i32>{crate::CALLS.lock().unwrap().push(path.to_vec());match crate::RESULT.load(std::sync::atomic::Ordering::SeqCst){0=>Ok(()),code=>Err(code)}}
 pub fn block_on<T>(future:impl core::future::Future<Output=T>)->T{let mut f=core::pin::pin!(future);let mut cx=core::task::Context::from_waker(core::task::Waker::noop());match f.as_mut().poll(&mut cx){core::task::Poll::Ready(v)=>v,_=>panic!("policy fixture unexpectedly pending")}}
}
'''
TESTS=r'''
#[cfg(test)]mod tests {
 use std::{fs,io,path::PathBuf,sync::atomic::Ordering};
 use gix_sec::Trust;
 struct Dir(PathBuf);
 impl Dir{fn new()->Self{let p=std::env::temp_dir().join(format!("native-git-identity-{}",std::process::id()));let _=fs::remove_dir_all(&p);fs::create_dir(&p).unwrap();Self(p)}}
 impl Drop for Dir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn policy(code:i32){trueos::RESULT.store(code,Ordering::SeqCst);trueos::CALLS.lock().unwrap().clear();}
 #[test]fn native_owner_assertion_grants_full_git_trust(){let d=Dir::new();let p=d.0.join("private-repo");fs::create_dir(&p).unwrap();policy(0);assert_eq!(Trust::from_path_ownership(&p).unwrap(),Trust::Full);assert_eq!(*trueos::CALLS.lock().unwrap(),vec![fs::canonicalize(&p).unwrap().as_os_str().as_encoded_bytes().to_vec()]);}
 #[test]fn readable_ffa_external_and_foreign_paths_stay_reduced(){let d=Dir::new();for name in ["ffa","external","foreign-owner"]{let p=d.0.join(name);fs::write(&p,b"readable data").unwrap();assert_eq!(fs::read(&p).unwrap(),b"readable data");policy(-19);assert_eq!(Trust::from_path_ownership(&p).unwrap(),Trust::Reduced);assert_eq!(trueos::CALLS.lock().unwrap().len(),1);}}
 #[test]fn unavailable_policy_and_io_failures_are_not_ownership_denials(){let d=Dir::new();for(code,kind)in[(-20,io::ErrorKind::Unsupported),(-8,io::ErrorKind::NotFound),(-4,io::ErrorKind::InvalidInput),(-2,io::ErrorKind::Other)]{policy(code);assert_eq!(Trust::from_path_ownership(&d.0).unwrap_err().kind(),kind);}}
 #[test]fn missing_path_never_uses_creation_assertion_to_claim_ownership(){let d=Dir::new();policy(0);assert_eq!(Trust::from_path_ownership(&d.0.join("absent")).unwrap_err().kind(),io::ErrorKind::NotFound);assert!(trueos::CALLS.lock().unwrap().is_empty());}
 #[test]fn real_recursive_entries_retain_native_file_identity(){use walkdir::DirEntryExt;let d=Dir::new();fs::create_dir(d.0.join("child")).unwrap();let file=d.0.join("child/data");fs::write(&file,b"bytes").unwrap();let entries=walkdir::WalkDir::new(&d.0).sort_by_file_name().into_iter().collect::<Result<Vec<_>,_>>().unwrap();assert_eq!(entries.len(),3);for e in entries{assert_eq!(e.ino(),std::os::unix::fs::MetadataExt::ino(&fs::symlink_metadata(e.path()).unwrap()));}}
 #[test]fn unknown_native_id_yields_error_without_inventing_inode(){let d=Dir::new();walkdir::FIXTURE_MISSING_ID.store(true,Ordering::SeqCst);let result=walkdir::WalkDir::new(&d.0).into_iter().next().unwrap();walkdir::FIXTURE_MISSING_ID.store(false,Ordering::SeqCst);assert_eq!(result.unwrap_err().io_error().unwrap().kind(),io::ErrorKind::Unsupported);}
 #[test]fn same_filesystem_option_uses_native_root_identity(){let d=Dir::new();fs::create_dir(d.0.join("child")).unwrap();fs::write(d.0.join("child/data"),b"native").unwrap();let entries=walkdir::WalkDir::new(&d.0).same_file_system(true).into_iter().collect::<Result<Vec<_>,_>>().unwrap();assert_eq!(entries.len(),3);}
 #[test]fn followed_directory_cycle_remains_detected(){let d=Dir::new();fs::create_dir(d.0.join("child")).unwrap();std::os::unix::fs::symlink(&d.0,d.0.join("child/cycle")).unwrap();let errors=walkdir::WalkDir::new(&d.0).follow_links(true).into_iter().filter_map(Result::err).collect::<Vec<_>>();assert_eq!(errors.len(),1);assert_eq!(errors[0].loop_ancestor(),Some(d.0.as_path()));}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-git-identity-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-12000:])
   return log.read_text()
  staged={}
  for name,version,method,module in [('walkdir','2.5.0','stage_walkdir','codex_native_data_overlay.rs'),('gix-sec','0.16.0','stage_gix_sec','codex_native_data_overlay.rs'),('same-file','1.0.6','stage_same_file','native_file_overlays.rs')]:
   extra=',&PathBuf::from(&args[3])'if name=='gix-sec'else''
   driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src'/module))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))',f'overlay::{method}(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]){extra})')
   (work/'stage.rs').write_text(driver);run('compile-'+name,['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
   source=list((Path.home()/'.cargo/registry/src').glob(f'*/{name}-{version}'));assert len(source)==1
   staged[name]=Path(run('stage-'+name,[str(work/'stage'),str(work/'staged'),str(source[0]),str(SDK/'api')]).strip())
  for relative in ['src/dent.rs','src/util.rs']:
   with(staged['walkdir']/relative).open('a')as stream:stream.write(METADATA)
  for name in ['same-file','walkdir']:
   with(staged[name]/'src/lib.rs').open('a')as stream:stream.write('\n/// Fixture: native metadata has no identity.\npub static FIXTURE_MISSING_ID:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);\n')
  with(staged['same-file']/'src/trueos.rs').open('a')as stream:stream.write(SAME_FILE_ADAPTER)
  api=work/'policy';manifest(api,'[package]\nname="trueos"\nversion="0.1.0"\nedition="2024"\n[lib]\npath="lib.rs"\n');(api/'lib.rs').write_text(POLICY)
  path=staged['gix-sec']/'Cargo.toml';text=path.read_text().replace('[target.\'cfg(target_os = "trueos")\'.dependencies.trueos]','[dependencies.trueos]').replace(json.dumps(str(SDK/'api')),json.dumps(str(api)));path.write_text(text)
  manifest(work,f'''[package]
name="native-git-identity-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
gix-sec={{path={json.dumps(str(staged['gix-sec']))}}}
walkdir={{path={json.dumps(str(staged['walkdir']))}}}
trueos={{path={json.dumps(str(api))}}}
[patch.crates-io]
same-file={{path={json.dumps(str(staged['same-file']))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]in["gix_sec","walkdir","same_file"]:args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
   result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'8 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src'/p)for p in ['codex_native_data_overlay.rs','gix_sec_trueos.rs','same_file_trueos.rs']]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
