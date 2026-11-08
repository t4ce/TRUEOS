#!/usr/bin/env python3
"""Check exact native tempfile reopen rejects absent stable object identities.

The production provider uses real host files with only metadata accessors
adapted to native std's optional identity API. No native transport is emulated.
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
 fn file_id(&self)->Option<u64>{
  let index=crate::FIXTURE_META_INDEX.fetch_add(1,std::sync::atomic::Ordering::SeqCst)+1;
  if crate::FIXTURE_MISSING_META.load(std::sync::atomic::Ordering::SeqCst)==index {None}else{Some(std::os::unix::fs::MetadataExt::ino(self))}
 }
}
'''
TESTS=r'''
#[cfg(test)]mod tests {
 use std::{fs,io::{self,Read,Seek,SeekFrom,Write},sync::atomic::Ordering};
 fn reset(missing:usize){tempfile::FIXTURE_META_INDEX.store(0,Ordering::SeqCst);tempfile::FIXTURE_MISSING_META.store(missing,Ordering::SeqCst);}
 #[test]fn absent_original_identity_rejects_before_opening_name(){let d=tempfile::tempdir().unwrap();let t=tempfile::NamedTempFile::new_in(d.path()).unwrap();reset(1);assert_eq!(t.reopen().unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(tempfile::FIXTURE_META_INDEX.load(Ordering::SeqCst),1);reset(0);assert!(t.reopen().is_ok());}
 #[test]fn absent_reopened_identity_rejects_without_changing_original(){let d=tempfile::tempdir().unwrap();let mut t=tempfile::NamedTempFile::new_in(d.path()).unwrap();t.write_all(b"original").unwrap();t.rewind().unwrap();reset(2);assert_eq!(t.reopen().unwrap_err().kind(),io::ErrorKind::Unsupported);reset(0);let mut s=String::new();t.read_to_string(&mut s).unwrap();assert_eq!(s,"original");}
 #[test]fn known_identity_preserves_independent_cursors(){let d=tempfile::tempdir().unwrap();let mut t=tempfile::NamedTempFile::new_in(d.path()).unwrap();t.write_all(b"abcdef").unwrap();t.seek(SeekFrom::Start(4)).unwrap();reset(0);let mut reopened=t.reopen().unwrap();assert_eq!(reopened.stream_position().unwrap(),0);let mut b=[0;2];reopened.read_exact(&mut b).unwrap();assert_eq!(&b,b"ab");assert_eq!(t.stream_position().unwrap(),4);}
 #[test]fn replaced_name_does_not_reopen_another_object(){let d=tempfile::tempdir().unwrap();let t=tempfile::NamedTempFile::new_in(d.path()).unwrap();fs::remove_file(t.path()).unwrap();fs::write(t.path(),b"replacement").unwrap();reset(0);assert_eq!(t.reopen().unwrap_err().kind(),io::ErrorKind::NotFound);}
}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-tempfile-reopen-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-10000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/native_file_overlays.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage_tempfile(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))')
  (work/'stage.rs').write_text(driver);run('stage-compile',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
  source=list((Path.home()/'.cargo/registry/src').glob('*/tempfile-3.27.0'));assert len(source)==1
  path=Path(run('stage',[str(work/'stage'),str(work/'staged'),str(source[0])]).strip())
  with (path/'src/file/imp/trueos.rs').open('a')as stream:stream.write(ADAPTER)
  with (path/'src/lib.rs').open('a')as stream:stream.write('\n#[doc(hidden)]pub static FIXTURE_META_INDEX:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);\n#[doc(hidden)]pub static FIXTURE_MISSING_META:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);\n')
  manifest(work,f'''[package]
name="native-tempfile-reopen-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
tempfile={{path={json.dumps(str(path))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="tempfile":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
   result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'4 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src/native_file_overlays.rs'),info(SDK/'src/tempfile_trueos.rs')]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
