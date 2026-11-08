#!/usr/bin/env python3
"""Exercise pinned which's real public API with honest native executable capability errors.

The complete upstream library and production staging code are unchanged by the
fixture. Actual host files carry real executable/non-executable permissions.
Native selection refuses file executable classification; host controls retain
the original access checks. Shell3's independent launch path is source-audited,
not executed or represented by an invented filesystem executable.
"""
from pathlib import Path
import argparse
import json
import re
import subprocess
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info
from test_native_rama_tcp import STAGE_DRIVER

SDK=ROOT.parent/'TRUEOS-Blueprints'
CODEX=ROOT.parent/'codex/codex-rs'
TESTS=r'''
use std::{fs,io,path::{Path,PathBuf}};
use std::os::unix::fs::PermissionsExt;
use which::sys::Sys;
fn candidate()->(tempfile::TempDir,PathBuf){let temp=tempfile::tempdir().unwrap();let path=temp.path().join("native-probe-helper");fs::write(&path,b"#!/bin/sh\nprintf synthetic\n").unwrap();fs::set_permissions(&path,fs::Permissions::from_mode(0o755)).unwrap();(temp,path)}
#[cfg(target_os="trueos")]
#[test]fn native_regular_and_executable_files_are_not_claimed_executable(){let(_temp,path)=candidate();let sys=which::sys::RealSys;for mode in[0o755,0o644]{fs::set_permissions(&path,fs::Permissions::from_mode(mode)).unwrap();let error=sys.is_valid_executable(&path).unwrap_err();assert_eq!(error.kind(),io::ErrorKind::Unsupported);assert!(error.to_string().contains("Shell3"));assert_eq!(fs::read(&path).unwrap(),b"#!/bin/sh\nprintf synthetic\n");}}
#[cfg(target_os="trueos")]
#[test]fn native_public_path_search_has_no_candidates(){let(temp,path)=candidate();let paths=Some(temp.path().as_os_str());assert_eq!(which::which_in("native-probe-helper",paths,temp.path()),Err(which::Error::CannotFindBinaryPath));assert_eq!(which::which_in(&path,paths,temp.path()),Err(which::Error::CannotFindBinaryPath));assert_eq!(which::which_in_all("native-probe-helper",paths,temp.path()).unwrap().count(),0);}
#[cfg(target_os="trueos")]
#[test]fn public_nonfatal_handler_receives_actual_unsupported_reason(){let(temp,_path)=candidate();let mut errors=Vec::new();let found=which::WhichConfig::new().binary_name("native-probe-helper".into()).custom_path_list(temp.path().as_os_str().into()).custom_cwd(temp.path().into()).nonfatal_error_handler(|e|errors.push(e)).all_results().unwrap().collect::<Vec<_>>();assert!(found.is_empty());assert_eq!(errors.len(),1);assert!(matches!(&errors[0],which::NonFatalError::Io(e)if e.kind()==io::ErrorKind::Unsupported));}
#[cfg(target_os="trueos")]
#[test]fn missing_file_never_becomes_a_candidate(){let(temp,_)=candidate();assert_eq!(which::which_in("absent",Some(temp.path().as_os_str()),temp.path()),Err(which::Error::CannotFindBinaryPath));}
#[cfg(not(target_os="trueos"))]
#[test]fn host_keeps_real_executable_classification(){let(_temp,path)=candidate();let sys=which::sys::RealSys;assert!(sys.is_valid_executable(&path).unwrap());fs::set_permissions(&path,fs::Permissions::from_mode(0o644)).unwrap();assert!(sys.is_valid_executable(&path).is_err());}
#[cfg(not(target_os="trueos"))]
#[test]fn host_public_path_search_still_finds_actual_executable(){let(temp,path)=candidate();assert_eq!(which::which_in("native-probe-helper",Some(temp.path().as_os_str()),temp.path()).unwrap(),path);}
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[];counts={}
    module=SDK/'src/codex_native_executable_overlay.rs'
    with tempfile.TemporaryDirectory(prefix='trueos-native-which-')as tmp:
        work=Path(tmp)
        def run(label,command,env=None):
            result=subprocess.run(command,cwd=work,env=env,text=True,capture_output=True,timeout=240);log=out/(label+'.log');log.write_text(result.stdout+result.stderr);checks.append({'label':label,'command':command,'exit':result.returncode,'log':str(log)})
            if result.returncode:raise RuntimeError(log.read_text()[-12000:])
            return result.stdout
        (work/'stage.rs').write_text(STAGE_DRIVER.replace('@MODULE@',json.dumps(str(module))))
        run('compile-production-stage',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
        candidates=list((Path.home()/'.cargo/registry/src').glob('*/which-8.0.0'));assert len(candidates)==1;source=candidates[0];before=info(source/'src/sys.rs')
        staged=Path(run('stage-which',[str(work/'stage'),str(work/'staged'),str(source)]).strip());assert before==info(source/'src/sys.rs')
        (work/'lib.rs').write_text(TESTS)
        (work/'Cargo.toml').write_text(f'''[package]\nname="codex-native-executable-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nwhich={{path={json.dumps(str(staged))}}}\ntempfile="=3.27.0"\n''')
        wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\nif "--crate-name"in a and a[a.index("--crate-name")+1]in{"codex_native_executable_proof","which"}:a += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]\nos.execvp(a[0],a)\n''');wrapper.chmod(0o755)
        for target in ['native','host']:
            env=environment(out/(target+'-target'))
            if target=='native':env['RUSTC_WRAPPER']=str(wrapper)
            for profile,flags in [('debug',[]),('release',['--release'])]:
                result=run(target+'-'+profile,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);count=int(re.search(r'test result: ok\. (\d+) passed',result)[1]);assert count==(4 if target=='native'else 2);counts[target+'-'+profile]=count
                if target=='native':
                    libraries=list((out/'native-target/x86_64-unknown-linux-gnu'/profile/'deps').glob('libwhich-*.rlib'));assert len(libraries)==1
                    imports=run('native-no-unix-access-'+profile,['nm','-u',str(libraries[0])]);assert not re.search(r'\b(?:access|faccessat|faccessat2)$',imports,re.M)
    paths=[module,CODEX/'rmcp-client/src/program_resolver.rs',CODEX/'rmcp-client/src/stdio_server_launcher.rs',CODEX/'utils/pty/src/trueos.rs',CODEX/'core/src/unified_exec/process_manager.rs',CODEX/'shell-command/src/shell_detect.rs']
    text=paths[2].read_text();assert '#[cfg(not(target_os = "trueos"))]\n    fn launch_server('in text and 'VmxStdioTransport::spawn(&request)'in text
    assert 'spawn_shell3_script('in paths[4].read_text() and 'pub async fn spawn_script('in paths[3].read_text()
    (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'cases':counts,'checks':checks,'sources':[info(p)for p in paths],'caller_findings':['Native MCP selects VmxStdioTransport before the host program_resolver/which path.','Native local unified_exec selects spawn_shell3_script before Unix argv spawning.','Shell discovery optional which candidates disappear; existing typed Sh fallback remains.','Doctor helper discovery may report unavailable; a native data file is never advertised as executable.']},indent=2)+'\n');print(out/'receipt.json')

if __name__=='__main__':main()
