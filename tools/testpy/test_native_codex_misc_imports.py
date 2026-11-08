#!/usr/bin/env python3
"""Exercise the actual native terminal, rollout clock, doctor and credential providers.

Production staging and terminal-size code run unchanged with real host PTYs;
only native geometry is supplied by the fixture ABI. Credential transport uses
instrumented existing child endpoints and synthetic bytes, never user input.
Real time formatting and Doctor report logic run with the native source branch.
This is a host/provider proof, not a rig, allocator or kernel permission test.
"""
from pathlib import Path
import argparse
import json
import os
import pty
import re
import subprocess
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info
from test_native_rama_tcp import STAGE_DRIVER
from test_sdk_metadata_v2 import item

CODEX = ROOT.parent / 'codex/codex-rs'
SDK = ROOT.parent / 'TRUEOS-Blueprints'
TESTS = r'''
#![allow(dead_code, unused_imports)]
use std::{collections::VecDeque,io,path::{Path,PathBuf},sync::Mutex};
use time::OffsetDateTime;
extern crate self as codex_protocol;
type ThreadId=uuid::Uuid;type RolloutId=uuid::Uuid;
trait IdFromString {fn from_string(value:&str)->Result<uuid::Uuid,uuid::Error>;}
impl IdFromString for uuid::Uuid{fn from_string(value:&str)->Result<uuid::Uuid,uuid::Error>{uuid::Uuid::parse_str(value)}}
mod compression{pub fn parse_rollout_file_name(name:&str)->Option<&str>{Some(name)}}
mod rollout_file_name { use crate::IdFromString; @FILENAME@ }
use rollout_file_name::RolloutFileName;
struct Config(PathBuf);trait RolloutConfigView{fn codex_home(&self)->&Path;}
impl RolloutConfigView for Config{fn codex_home(&self)->&Path{&self.0}}
const SESSIONS_SUBDIR:&str="sessions";type IoError=io::Error;
@PRECOMPUTE@
mod doctor {
 use super::*;
 #[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)] @STATUS@
 #[derive(Clone,Debug)] @CHECK@
 #[derive(Clone,Debug)] @ISSUE@
 @CHECK_IMPL@ @ISSUE_IMPL@
 const GIB:u64=1024*1024*1024;const FAIL_THRESHOLD:u64=GIB;const WARNING_THRESHOLD:u64=5*GIB;
 @DISK@
 #[test]fn native_capacity_unavailable_is_warning_without_fake_success(){let temp=tempfile::tempdir().unwrap();let check=check_with_paths(Some(temp.path()),temp.path(),available_space);assert_eq!(check.status,CheckStatus::Warning);assert_eq!(check.issues.len(),2);assert_eq!(check.summary,"disk capacity could not be fully verified");assert!(check.details.iter().filter(|x|x.contains("available:")).all(|x|x.contains("Unsupported")));}
 #[test]fn real_capacity_report_thresholds_unchanged(){let temp=tempfile::tempdir().unwrap();for(space,status)in[(0,CheckStatus::Fail),(2*GIB,CheckStatus::Warning),(10*GIB,CheckStatus::Ok)]{let check=check_with_paths(Some(temp.path()),temp.path(),|_|Ok(space));assert_eq!(check.status,status);}}
}
#[path="read_api_key.rs"]mod read_api_key;
#[derive(Default)]struct Pipe {terminal:bool,status:i32,bytes:VecDeque<u8>,reads:usize,empty_reads:usize,error:isize,invalid_count:bool,disappear:bool}
static PIPE:Mutex<Pipe>=Mutex::new(Pipe{terminal:false,status:2,bytes:VecDeque::new(),reads:0,empty_reads:0,error:0,invalid_count:false,disappear:false});
fn reset(){*PIPE.lock().unwrap()=Pipe{status:2,..Pipe::default()};unsafe{std::env::set_var("TRUEOS_MCP_STDIO","1")};}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_stdio_is_terminal_v1(_:u32)->i32{PIPE.lock().unwrap().terminal as i32}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_blueprint_child_status_v1(handle:u64)->i32{assert_eq!(handle,0);PIPE.lock().unwrap().status}
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_blueprint_child_read_v1(handle:u64,out:*mut u8,cap:usize)->isize{assert_eq!(handle,0);let mut s=PIPE.lock().unwrap();s.reads+=1;if s.error!=0{return s.error;}if s.invalid_count{return cap as isize+1;}if s.empty_reads!=0{s.empty_reads-=1;if s.disappear{s.status=-2;}return 0;}let n=cap.min(s.bytes.len());for i in 0..n{unsafe{out.add(i).write(s.bytes.pop_front().unwrap())}}n as isize}
#[test]fn native_rollout_names_use_real_utc_date_and_timestamp(){let root=PathBuf::from("/apps/codex/.codex");let id=uuid::Uuid::new_v4();let before=OffsetDateTime::now_utc();let(path,timestamp)=precompute_new_rollout_path(&Config(root.clone()),id,None).unwrap();let after=OffsetDateTime::now_utc();assert!(timestamp>=before&&timestamp<=after);assert_eq!(timestamp.offset(),time::UtcOffset::UTC);let expected=root.join("sessions").join(timestamp.year().to_string()).join(format!("{:02}",u8::from(timestamp.month()))).join(format!("{:02}",timestamp.day()));assert_eq!(path.parent().unwrap(),expected);assert_eq!(path.file_name().unwrap(),RolloutFileName::new(timestamp,id,id).render().unwrap().as_str());}
#[test]fn native_reverted_rollout_preserves_distinct_ids(){let id=uuid::Uuid::new_v4();let rollout=uuid::Uuid::new_v4();let(path,timestamp)=precompute_new_rollout_path(&Config(PathBuf::from("home")),id,Some(rollout)).unwrap();let parsed=RolloutFileName::parse(path.file_name().unwrap().to_str().unwrap()).unwrap();assert_eq!(parsed.thread_id(),id);assert_eq!(parsed.rollout_id(),rollout);assert_eq!(timestamp.offset(),time::UtcOffset::UTC);}
'''.replace('extern "C"fn', 'extern "C" fn')

PIPE_TESTS = r'''
#[cfg(test)]mod native_pipe_tests{
 use super::*;use crate::{reset,PIPE};
 #[test]fn refuses_console_before_consuming_any_bytes(){reset();let mut s=PIPE.lock().unwrap();s.terminal=true;s.bytes.extend(b"must-remain");drop(s);assert_eq!(read(&mut[0;32]).unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(PIPE.lock().unwrap().reads,0);}
 #[test]fn requires_reserved_launch_marker_before_read(){reset();unsafe{std::env::remove_var("TRUEOS_MCP_STDIO")};assert_eq!(read(&mut[0;4]).unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(PIPE.lock().unwrap().reads,0);}
 #[test]fn missing_actual_parent_endpoint_is_not_a_pipe(){reset();PIPE.lock().unwrap().status=-2;assert_eq!(read(&mut[0;4]).unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(PIPE.lock().unwrap().reads,0);}
 #[test]fn actual_child_bytes_reach_existing_credential_validator(){reset();PIPE.lock().unwrap().bytes.extend(b"fixture-key_123\n");assert_eq!(super::super::read_auth_header_from_stdin().unwrap(),"Bearer fixture-key_123");}
 #[test]fn live_pipe_waits_then_delivers_without_buffered_reader(){reset();let mut s=PIPE.lock().unwrap();s.empty_reads=2;s.bytes.extend(b"ok");drop(s);let mut bytes=[0;4];assert_eq!(read(&mut bytes).unwrap(),2);assert_eq!(&bytes[..2],b"ok");assert_eq!(PIPE.lock().unwrap().reads,3);}
 #[test]fn parent_exit_drains_retained_bytes_then_reports_eof(){reset();let mut s=PIPE.lock().unwrap();s.status=4;s.bytes.extend(b"final");drop(s);let mut bytes=[0;8];assert_eq!(read(&mut bytes).unwrap(),5);assert_eq!(read(&mut bytes).unwrap(),0);}
 #[test]fn transport_error_and_invalid_length_are_not_eof(){reset();PIPE.lock().unwrap().error=-2;assert_eq!(read(&mut[0;4]).unwrap_err().kind(),io::ErrorKind::BrokenPipe);reset();PIPE.lock().unwrap().invalid_count=true;assert_eq!(read(&mut[0;4]).unwrap_err().kind(),io::ErrorKind::InvalidData);}
 #[test]fn disappearance_during_empty_read_is_an_error(){reset();let mut s=PIPE.lock().unwrap();s.empty_reads=1;s.disappear=true;drop(s);assert_eq!(read(&mut[0;4]).unwrap_err().kind(),io::ErrorKind::BrokenPipe);}
 #[test]fn empty_live_pipe_has_a_bounded_wait(){reset();let started=Instant::now();assert_eq!(read_with_timeout(&mut[0;4],Duration::from_millis(2)).unwrap_err().kind(),io::ErrorKind::TimedOut);assert!(started.elapsed()<Duration::from_millis(100));}
}
'''

TERMINAL = r'''
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_konsole_size(cols:*mut u32,rows:*mut u32)->i32{let values:Vec<i64>=std::env::var("GEOMETRY").unwrap().split(',').map(|x|x.parse().unwrap()).collect();unsafe{cols.write(values[0]as u32);rows.write(values[1]as u32)};values[2]as i32}
fn main(){println!("GEOMETRY_RESULT={:?}",terminal_size::terminal_size());}
'''.replace('extern "C"fn','extern "C" fn')

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
    sources=[CODEX/'responses-api-proxy/src/read_api_key.rs',CODEX/'responses-api-proxy/src/native_stdin.rs',CODEX/'rollout/src/recorder.rs',CODEX/'rollout/src/rollout_file_name.rs',CODEX/'cli/src/doctor/disk.rs',SDK/'src/codex_native_terminal_overlay.rs',SDK/'src/terminal_size_trueos.rs']
    with tempfile.TemporaryDirectory(prefix='trueos-codex-misc-')as tmp:
        work=Path(tmp)
        def run(label,command,env=None,input=None):
            result=subprocess.run(command,cwd=work,env=env,input=input,text=True,capture_output=True,timeout=240)
            log=out/(label+'.log');log.write_text(result.stdout+result.stderr);checks.append({'label':label,'command':command,'exit':result.returncode,'log':str(log)})
            if result.returncode:raise RuntimeError(log.read_text()[-15000:])
            return result.stdout
        stage=work/'stage.rs';stage.write_text(STAGE_DRIVER.replace('@MODULE@',json.dumps(str(sources[5]))))
        run('compile-production-stage',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(stage),'-o',str(work/'stage')])
        registry=list((Path.home()/'.cargo/registry/src').glob('*/terminal_size-0.4.3'));assert len(registry)==1
        before=info(registry[0]/'src/lib.rs');staged=Path(run('stage-terminal-size',[str(work/'stage'),str(work/'staged'),str(registry[0])]).strip());assert before==info(registry[0]/'src/lib.rs')
        recorder=sources[2].read_text();doctor=(CODEX/'cli/src/doctor.rs').read_text();disk=sources[4].read_text()
        filename=sources[3].read_text();filename=re.sub(r'#\[cfg\(test\)\]\n#\[path = "rollout_file_name_tests.rs"\]\nmod tests;','',filename)
        selected='\n'.join(item(disk,marker)for marker in ['fn check_with_paths(','fn format_capacity('])
        selected+='\n'+item(disk,'fn available_space(_path:')
        text=TESTS.replace('@FILENAME@',filename).replace('@PRECOMPUTE@',item(recorder,'fn precompute_new_rollout_path(')).replace('@DISK@',selected)
        for key,marker in [('STATUS','enum CheckStatus'),('CHECK','struct DoctorCheck'),('ISSUE','struct DoctorIssue'),('CHECK_IMPL','impl DoctorCheck'),('ISSUE_IMPL','impl DoctorIssue')]:text=text.replace('@'+key+'@',item(doctor,marker))
        (work/'lib.rs').write_text(text);(work/'terminal.rs').write_text(TERMINAL)
        (work/'read_api_key.rs').write_text(sources[0].read_text());(work/'native_stdin.rs').write_text(sources[1].read_text()+PIPE_TESTS)
        (work/'Cargo.toml').write_text(f'''[package]\nname="codex-native-misc-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[[bin]]\nname="native-terminal-size"\npath="terminal.rs"\n[dependencies]\nanyhow="1"\nzeroize="1"\ntempfile="=3.27.0"\nuuid={{version="1",features=["v4"]}}\ntime={{version="=0.3.47",features=["formatting","parsing","macros","local-offset"]}}\nterminal_size={{path={json.dumps(str(staged))}}}\n''')
        wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\nif "--crate-name"in a and a[a.index("--crate-name")+1]in{"codex_native_misc_proof","terminal_size"}:a += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]\nos.execvp(a[0],a)\n''');wrapper.chmod(0o755)
        env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
        counts={}
        for profile,flags in [('debug',[]),('release',['--release'])]:
            cargo=['rustup','run',TOOLCHAIN,'cargo'];result=run(profile,cargo+['test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);counts[profile]=int(re.search(r'test result: ok\. (\d+) passed',result)[1]);assert counts[profile]>=19
            run('terminal-build-'+profile,cargo+['build','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--bin','native-terminal-size'],env)
            binary=out/'target/x86_64-unknown-linux-gnu'/profile/'native-terminal-size'
            for label,geometry,attached,expected in [('attached','132,48,0',True,'Some((Width(132), Height(48)))'),('changed','99,22,0',True,'Some((Width(99), Height(22)))'),('invalid','0,22,0',True,'None'),('unavailable','90,22,-3',True,'None'),('large','80000,90000,0',True,'Some((Width(65535), Height(65535)))'),('pipes','132,48,0',False,'None')]:
                child_env=dict(env,GEOMETRY=geometry)
                if attached:
                    master,slave=pty.openpty()
                    child=subprocess.Popen([str(binary)],env=child_env,stdin=slave,stdout=slave,stderr=slave);os.close(slave);output=b''
                    while True:
                        try:part=os.read(master,4096)
                        except OSError:break
                        if not part:break
                        output+=part
                    os.close(master);assert child.wait(timeout=10)==0;output=output.decode()
                else:
                    child=subprocess.run([str(binary)],env=child_env,stdin=subprocess.DEVNULL,text=True,capture_output=True,timeout=10);assert child.returncode==0;output=child.stdout
                assert 'GEOMETRY_RESULT='+expected in output,(label,output);checks.append({'label':profile+'-terminal-'+label,'expected':expected,'actual':output.strip()})
        host=work/'host-control';host.mkdir()
        (host/'Cargo.toml').write_text('''[package]\nname="codex-host-credential-control"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="../read_api_key.rs"\n[[bin]]\nname="host-credential-input"\npath="main.rs"\n[dependencies]\nanyhow="1"\nzeroize="1"\nlibc="0.2"\n''')
        (host/'main.rs').write_text('#[path="../read_api_key.rs"]mod read_api_key;\nfn main(){let value=read_api_key::read_auth_header_from_stdin().unwrap();assert_eq!(value,"Bearer host-fixture-key");println!("HOST_STDIN_VALIDATED=true");}\n')
        host_env=environment(out/'host-target')
        for profile,flags in [('debug',[]),('release',['--release'])]:
            command=['rustup','run',TOOLCHAIN,'cargo'];result=run('host-control-'+profile,command+['test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(host/'Cargo.toml'),'--lib'],host_env)
            counts['host-'+profile]=int(re.search(r'test result: ok\. (\d+) passed',result)[1])
            result=run('host-real-stdin-'+profile,command+['run','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(host/'Cargo.toml'),'--bin','host-credential-input'],host_env,'host-fixture-key\n')
            assert 'HOST_STDIN_VALIDATED=true' in result
    resident=[ROOT/'src/phys.rs',ROOT/'src/hv/memory.rs',ROOT/'src/hv/store.rs']
    (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'cases':counts,'checks':checks,'sources':[info(p)for p in sources],'residency_contract_sources':[info(p)for p in resident]},indent=2)+'\n');print(out/'receipt.json')

if __name__=='__main__':main()
