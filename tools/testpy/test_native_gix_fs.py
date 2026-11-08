#!/usr/bin/env python3
"""Check pinned Git filesystem helpers select actual native capabilities.

TRUEOS has no executable mode bits or symlinks. File bytes, directory probes and
no-follow reads remain available; explicit Unix sharing policy is unsupported.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
TESTS=r'''
#[cfg(test)]mod tests{
 use std::{fs,io::{self,Read},path::PathBuf};
 struct Dir(PathBuf);impl Dir{fn new()->Self{let p=std::env::temp_dir().join(format!("native-gix-fs-{}",std::process::id()));let _=fs::remove_dir_all(&p);fs::create_dir(&p).unwrap();Self(p)}}impl Drop for Dir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 #[test]fn defaults_and_probes_report_real_native_capabilities(){let d=Dir::new();let defaults=gix_fs::Capabilities::default();assert!(!defaults.executable_bit);assert!(!defaults.symlink);assert!(!defaults.ignore_case);let detected=gix_fs::Capabilities::probe_dir(&d.0);assert!(!detected.executable_bit);assert!(!detected.symlink);assert!(!detected.ignore_case);assert_eq!(fs::read_dir(&d.0).unwrap().count(),0);}
 #[test]fn existing_file_bytes_are_read_through_native_no_follow_contract(){let d=Dir::new();let p=d.0.join("file");fs::write(&p,b"Git file bytes").unwrap();match gix_fs::open_read_only_no_follow(&p).unwrap(){gix_fs::FileOrSymlink::File(mut f)=>{let mut s=String::new();f.read_to_string(&mut s).unwrap();assert_eq!(s,"Git file bytes");},_=>panic!("real file classified as symlink")};assert_eq!(gix_fs::open_read_only_no_follow(&d.0.join("absent")).unwrap_err().kind(),io::ErrorKind::NotFound);}
 #[test]fn unix_shared_policy_is_explicitly_unsupported(){let d=Dir::new();let p=d.0.join("file");fs::write(&p,b"unchanged").unwrap();let before=fs::metadata(&p).unwrap().permissions();gix_fs::set_shared_repository_permissions(&p,0).unwrap();for mode in [0o660,0o664,-0o640]{assert_eq!(gix_fs::set_shared_repository_permissions(&p,mode).unwrap_err().kind(),io::ErrorKind::Unsupported);}assert_eq!(fs::metadata(&p).unwrap().permissions(),before);assert_eq!(fs::read(&p).unwrap(),b"unchanged");}
 #[test]fn host_executable_metadata_is_never_misread_as_native_permission(){use std::os::unix::fs::PermissionsExt;let d=Dir::new();let p=d.0.join("script");fs::write(&p,b"echo native").unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o755)).unwrap();assert!(!gix_fs::is_executable(&fs::metadata(&p).unwrap()));}
}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-gix-fs-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-12000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_data_overlay.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage_gix_fs(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))');(work/'stage.rs').write_text(driver)
  run('compile',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')]);source=list((Path.home()/'.cargo/registry/src').glob('*/gix-fs-0.24.0'));assert len(source)==1
  staged=Path(run('stage',[str(work/'stage'),str(work/'staged'),str(source[0])]).strip());manifest(work,f'''[package]
name="native-gix-fs-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
gix-fs={{path={json.dumps(str(staged))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="gix_fs":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'4 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src/codex_native_data_overlay.rs')]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
