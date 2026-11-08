#!/usr/bin/env python3
"""Run the exact pinned same-file native provider with accurate optional IDs.

Real host files supply bytes and stable identities. The narrow metadata adapter
matches custom std's root_id()->u32/file_id()->Option<u64> signature and can
withhold an identity, which must return Unsupported rather than fabricate one.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
ADAPTER=r'''
trait NativeIdentity {fn root_id(&self)->u32;fn file_id(&self)->Option<u64>;}
impl NativeIdentity for std::fs::Metadata {
 fn root_id(&self)->u32{<u32 as std::convert::TryFrom<u64>>::try_from(std::os::unix::fs::MetadataExt::dev(self)).unwrap()}
 fn file_id(&self)->Option<u64>{if crate::FIXTURE_MISSING_ID.load(std::sync::atomic::Ordering::SeqCst){None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}}
}
'''
TESTS=r'''
#[cfg(test)]mod tests{
 use same_file::Handle;use std::{fs::{self,File},io::{self,Write},collections::HashSet,path::PathBuf};
 struct Dir(PathBuf);
 impl Dir{fn new()->Self{let p=std::env::temp_dir().join(format!("native-same-file-{}",std::process::id()));let _=fs::remove_dir_all(&p);fs::create_dir(&p).unwrap();Self(p)}}
 impl Drop for Dir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 #[test]fn independent_file_opens_have_identical_identity_and_hash(){let d=Dir::new();let p=d.0.join("a");fs::write(&p,b"data").unwrap();let a=Handle::from_path(&p).unwrap();let b=Handle::from_file(File::open(&p).unwrap()).unwrap();assert_eq!(a,b);let mut set=HashSet::new();set.insert(a);set.insert(b);assert_eq!(set.len(),1);}
 #[test]fn equal_bytes_do_not_make_distinct_files_equal(){let d=Dir::new();let a=d.0.join("a");let b=d.0.join("b");fs::write(&a,b"same").unwrap();fs::write(&b,b"same").unwrap();assert_ne!(Handle::from_path(a).unwrap(),Handle::from_path(b).unwrap());}
 #[test]fn rename_retains_identity_and_replacement_is_new_object(){let d=Dir::new();let a=d.0.join("a");let b=d.0.join("b");fs::write(&a,b"old").unwrap();let original=Handle::from_path(&a).unwrap();fs::rename(&a,&b).unwrap();assert_eq!(original,Handle::from_path(&b).unwrap());fs::remove_file(&b).unwrap();fs::write(&b,b"new").unwrap();assert_ne!(original,Handle::from_path(&b).unwrap());}
 #[test]fn directory_identity_and_retained_file_are_real(){let d=Dir::new();let other=d.0.join("other");fs::create_dir(&other).unwrap();assert_eq!(Handle::from_path(&d.0).unwrap(),Handle::from_path(&d.0).unwrap());assert_ne!(Handle::from_path(&d.0).unwrap(),Handle::from_path(other).unwrap());let f=d.0.join("file");fs::write(&f,b"a").unwrap();let mut h=Handle::from_file(File::options().write(true).open(&f).unwrap()).unwrap();h.as_file_mut().write_all(b"b").unwrap();assert_eq!(fs::read(f).unwrap(),b"b");}
 #[test]fn console_stream_identity_is_unsupported(){for h in [Handle::stdin(),Handle::stdout(),Handle::stderr()]{assert_eq!(h.unwrap_err().kind(),io::ErrorKind::Unsupported);}}
 #[test]fn missing_native_file_id_is_unsupported(){let d=Dir::new();let p=d.0.join("a");fs::write(&p,b"data").unwrap();same_file::FIXTURE_MISSING_ID.store(true,std::sync::atomic::Ordering::SeqCst);let result=Handle::from_path(&p);same_file::FIXTURE_MISSING_ID.store(false,std::sync::atomic::Ordering::SeqCst);assert_eq!(result.unwrap_err().kind(),io::ErrorKind::Unsupported);assert!(Handle::from_path(&p).is_ok());}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-same-file-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-10000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/native_file_overlays.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage_same_file(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))')
  (work/'stage.rs').write_text(driver);run('stage-compile',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
  source=list((Path.home()/'.cargo/registry/src').glob('*/same-file-1.0.6'));assert len(source)==1
  path=Path(run('stage',[str(work/'stage'),str(work/'staged'),str(source[0])]).strip())
  with (path/'src/trueos.rs').open('a')as stream:stream.write(ADAPTER)
  with (path/'src/lib.rs').open('a')as stream:stream.write('\n/// Fixture boundary: withhold the optional native metadata identity.\npub static FIXTURE_MISSING_ID:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);\n')
  manifest(work,f'''[package]
name="native-same-file-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
same-file={{path={json.dumps(str(path))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="same_file":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
   result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'6 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src/native_file_overlays.rs'),info(SDK/'src/same_file_trueos.rs')]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
