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
 #[test]fn sequential_and_parallel_ignore_filtering_use_real_native_ids(){let d=Dir::new();fs::create_dir(d.0.join("child")).unwrap();fs::write(d.0.join(".ignore"),b"skip*\n").unwrap();fs::write(d.0.join("child/visible"),b"yes").unwrap();fs::write(d.0.join("child/skip_this"),b"no").unwrap();let mut builder=ignore::WalkBuilder::new(&d.0);builder.hidden(false).same_file_system(true).threads(2);let expected=builder.build().map(|e|e.unwrap().path().to_path_buf()).collect::<std::collections::BTreeSet<_>>();assert!(expected.contains(&d.0.join("child/visible")));assert!(!expected.contains(&d.0.join("child/skip_this")));let paths=std::sync::Mutex::new(std::collections::BTreeSet::new());builder.build_parallel().run(||Box::new(|e|{paths.lock().unwrap().insert(e.unwrap().path().to_path_buf());ignore::WalkState::Continue}));assert_eq!(paths.into_inner().unwrap(),expected);}
 #[test]fn parallel_ignore_reports_missing_native_identity(){let d=Dir::new();ignore::FIXTURE_MISSING_ID.store(true,Ordering::SeqCst);let errors=std::sync::Mutex::new(Vec::new());ignore::WalkBuilder::new(&d.0).build_parallel().run(||Box::new(|e|{if let Err(e)=e{errors.lock().unwrap().push(e.io_error().unwrap().kind());}ignore::WalkState::Continue}));ignore::FIXTURE_MISSING_ID.store(false,Ordering::SeqCst);assert_eq!(errors.into_inner().unwrap(),vec![io::ErrorKind::Unsupported]);}
 #[test]fn file_id_uses_native_root_and_optional_file_identity(){let d=Dir::new();let a=d.0.join("a");let b=d.0.join("b");fs::write(&a,b"data").unwrap();let md=fs::metadata(&a).unwrap();let id=file_id::get_file_id(&a).unwrap();assert_eq!(id,file_id::FileId::new_inode(std::os::unix::fs::MetadataExt::dev(&md),std::os::unix::fs::MetadataExt::ino(&md)));fs::rename(&a,&b).unwrap();assert_eq!(id,file_id::get_file_id(&b).unwrap());file_id::FIXTURE_MISSING_ID.store(true,Ordering::SeqCst);let result=file_id::get_file_id(&b);file_id::FIXTURE_MISSING_ID.store(false,Ordering::SeqCst);assert_eq!(result.unwrap_err().kind(),io::ErrorKind::Unsupported);}
 #[test]fn actual_git_discovery_preserves_native_ownership_trust(){let d=Dir::new();let git=d.0.join(".git");fs::create_dir_all(git.join("objects")).unwrap();fs::create_dir_all(git.join("refs/heads")).unwrap();fs::write(git.join("HEAD"),b"ref: refs/heads/main\n").unwrap();let nested=d.0.join("src/module");fs::create_dir_all(&nested).unwrap();policy(0);let(path,trust)=gix_discover::upwards(&nested).unwrap();assert_eq!(trust,Trust::Full);assert_eq!(path.into_repository_and_work_tree_directories().0,git);policy(-19);assert_eq!(gix_discover::upwards(&nested).unwrap().1,Trust::Reduced);}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--ignore-version',choices=['0.4.25','0.4.32'],default='0.4.25');args=p.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-git-identity-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-12000:])
   return log.read_text()
  staged={}
  for name,version,method,module in [('walkdir','2.5.0','stage_walkdir','codex_native_data_overlay.rs'),('ignore',args.ignore_version,'stage_ignore'if args.ignore_version=='0.4.25'else'stage_ignore_version','codex_native_data_overlay.rs'),('file-id','0.2.3','stage_file_id','codex_native_data_overlay.rs'),('gix-discover','0.57.0','stage_gix_discover','codex_native_data_overlay.rs'),('gix-sec','0.16.0','stage_gix_sec','codex_native_data_overlay.rs'),('same-file','1.0.6','stage_same_file','native_file_overlays.rs')]:
   extra=',&PathBuf::from(&args[3])'if name=='gix-sec'else''
   if name=='ignore'and args.ignore_version!='0.4.25':extra=','+json.dumps(args.ignore_version)
   driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src'/module))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))',f'overlay::{method}(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]){extra})')
   (work/'stage.rs').write_text(driver);run('compile-'+name,['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
   source=list((Path.home()/'.cargo/registry/src').glob(f'*/{name}-{version}'));assert len(source)==1
   staged[name]=Path(run('stage-'+name,[str(work/'stage'),str(work/'staged'),str(source[0]),str(SDK/'api')]).strip())
   # Path dependencies inside this fixture can become workspace members.
   # Their upstream development-only packages are outside this library probe.
   cargo_manifest=staged[name]/'Cargo.toml';lines=[];skip=False
   for line in cargo_manifest.read_text().splitlines():
    if line.startswith('['):skip='dev-dependencies'in line
    if not skip:lines.append(line)
   cargo_manifest.write_text('\n'.join(lines)+'\n')
  for relative in ['src/dent.rs','src/util.rs']:
   with(staged['walkdir']/relative).open('a')as stream:stream.write(METADATA)
  with(staged['ignore']/'src/walk.rs').open('a')as stream:stream.write(METADATA)
  for name in ['same-file','walkdir','ignore','file-id','gix-discover']:
   with(staged[name]/'src/lib.rs').open('a')as stream:stream.write('\n/// Fixture: native metadata has no identity.\npub static FIXTURE_MISSING_ID:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);\n')
  with(staged['same-file']/'src/trueos.rs').open('a')as stream:stream.write(SAME_FILE_ADAPTER)
  with(staged['file-id']/'src/lib.rs').open('a')as stream:stream.write(METADATA)
  with(staged['gix-discover']/'src/upwards/util.rs').open('a')as stream:stream.write(METADATA)
  api=work/'policy';manifest(api,'[package]\nname="trueos"\nversion="0.1.0"\nedition="2024"\n[lib]\npath="lib.rs"\n');(api/'lib.rs').write_text(POLICY)
  path=staged['gix-sec']/'Cargo.toml';text=path.read_text().replace('[target.\'cfg(target_os = "trueos")\'.dependencies.trueos]','[dependencies.trueos]').replace(json.dumps(str(SDK/'api')),json.dumps(str(api)));path.write_text(text)
  manifest(work,f'''[package]
name="native-git-identity-proof"
version="0.0.0"
edition="2024"
[workspace]
exclude=["staged/source-overlay-crates/*"]
[dependencies]
gix-sec={{path={json.dumps(str(staged['gix-sec']))}}}
walkdir={{path={json.dumps(str(staged['walkdir']))}}}
ignore={{path={json.dumps(str(staged['ignore']))}}}
file-id={{path={json.dumps(str(staged['file-id']))}}}
gix-discover={{path={json.dumps(str(staged['gix-discover']))}}}
gix-hash={{version="=0.28.0",features=["sha1"]}}
trueos={{path={json.dumps(str(api))}}}
[patch.crates-io]
same-file={{path={json.dumps(str(staged['same-file']))}}}
walkdir={{path={json.dumps(str(staged['walkdir']))}}}
gix-sec={{path={json.dumps(str(staged['gix-sec']))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]in["gix_sec","walkdir","same_file","ignore","file_id","gix_discover"]:args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
   result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'12 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'ignore_version':args.ignore_version,'checks':checks,'sources':[info(SDK/'src'/p)for p in ['codex_native_data_overlay.rs','gix_sec_trueos.rs','same_file_trueos.rs']]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
