#!/usr/bin/env python3
"""Execute actual credential stdin admission/callers without reading user input.

Native tests run provider + ChatGPT + registration + executor entrypoints and a
captured command constructor. Subprocesses receive synthetic stdin and prove it
remains unread on native rejection. Host pipe/file admission still runs real
Unix FD metadata checks. Token parsing/auth construction and reader spawning
are counted boundaries; their implementations are unchanged by this patch.
"""
from pathlib import Path
import argparse
import json
import re
import shutil
import subprocess
import tempfile
import textwrap
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest

CODEX = ROOT.parent / 'codex/codex-rs'
REMOTE = CODEX / 'exec-server/src/remote.rs'
CONFIG = CODEX / 'exec-server-cli/src/config.rs'
EXECUTOR = CODEX / 'exec-server-cli/src/exec_server_credential.rs'
COMMAND = CODEX / 'exec-server-cli/src/command.rs'


def function(path, name, indent=0):
    text = path.read_text()
    match = re.search(r'^' + ' ' * indent + r'(?:pub(?:\([^)]*\))? )?fn ' + re.escape(name) + r'\(', text, re.M)
    end = text.index('\n' + ' ' * indent + '}', match.end()) + 2 + indent
    return textwrap.dedent(text[match.start():end])


HARNESS = r'''
use std::sync::atomic::{AtomicUsize,Ordering};
static PARSES:AtomicUsize=AtomicUsize::new(0);
static FACTORIES:AtomicUsize=AtomicUsize::new(0);
static SPAWNS:AtomicUsize=AtomicUsize::new(0);
#[derive(Debug,PartialEq)]struct CodexAuth{line:String,account:Option<String>}
mod remote{
 use super::*;
 type SharedAuthProvider=String;
 PROVIDER
 REGISTRATION
 fn static_bearer_auth_provider(token:String,account:Option<String>)->std::io::Result<String>{assert!(account.is_none());FACTORIES.fetch_add(1,Ordering::SeqCst);Ok(token)}
}
mod codex_exec_server{pub use crate::remote::credential_stdin;}
mod config{
 use super::*;
 use std::io::{BufRead,Read};
 CHATGPT
 fn chatgpt_auth_from_line(line:&str,account:Option<String>)->anyhow::Result<CodexAuth>{PARSES.fetch_add(1,Ordering::SeqCst);Ok(CodexAuth{line:line.into(),account})}
}
mod executor{
 use super::*;
 pub(super)struct ExecutorCredentialPipe{pub(super)provider:(),pub(super)parent_closed:()}
 impl ExecutorCredentialPipe{
  EXECUTOR_CONSTRUCTOR
  fn spawn_with_reader<R:std::io::Read,F:FnOnce()->R+Send+'static>(_environment:String,_open:F)->Self{SPAWNS.fetch_add(1,Ordering::SeqCst);Self{provider:(),parent_closed:()}}
 }
}
fn command_constructor(environment_id:String)->anyhow::Result<()>{use executor::ExecutorCredentialPipe;COMMAND_CONSTRUCTOR
 let _=(provider,parent_closed);Ok(())}
#[cfg(test)]mod tests{
 use super::*;
 use std::io::{BufRead,Write};
 use std::process::{Command,Stdio};
 const SENTINEL:&str="synthetic-credential-input-do-not-consume\n";
 fn reset(){for c in [&PARSES,&FACTORIES,&SPAWNS]{c.store(0,Ordering::SeqCst);}}
 fn unsupported(error:std::io::Error){assert_eq!(error.kind(),std::io::ErrorKind::Unsupported);assert!(error.to_string().contains("parent-owned credential pipe"));}
 #[cfg(target_os="trueos")]
 #[test]fn provider_rejects_native_input_capability(){unsupported(remote::credential_stdin().unwrap_err());}
 #[cfg(target_os="trueos")]
 #[test]fn chatgpt_gate_rejects_before_token_parse(){reset();for account in [None,Some("account".into())]{let error=config::read_chatgpt_stdin_auth(account).unwrap_err();assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),std::io::ErrorKind::Unsupported);}assert_eq!(PARSES.load(Ordering::SeqCst),0);}
 #[cfg(target_os="trueos")]
 #[test]fn registration_gate_rejects_before_auth_factory(){reset();unsupported(remote::registration_auth_from_stdin().unwrap_err());assert_eq!(FACTORIES.load(Ordering::SeqCst),0);}
 #[cfg(target_os="trueos")]
 #[test]fn executor_gate_rejects_before_starting_reader(){reset();let error=executor::ExecutorCredentialPipe::from_stdin("environment".into()).err().unwrap();unsupported(error);assert_eq!(SPAWNS.load(Ordering::SeqCst),0);}
 #[cfg(target_os="trueos")]
 #[test]fn command_constructor_propagates_provider_rejection(){reset();let error=command_constructor("environment".into()).unwrap_err();assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),std::io::ErrorKind::Unsupported);assert_eq!(SPAWNS.load(Ordering::SeqCst),0);}
 #[cfg(target_os="trueos")]
 #[test]fn all_native_gates_leave_real_stdin_bytes_unread(){run_child("native",false);}
 #[cfg(not(target_os="trueos"))]
 #[test]fn host_pipe_credentials_keep_real_metadata_admission(){run_child("chatgpt",false);}
 #[cfg(not(target_os="trueos"))]
 #[test]fn host_regular_file_is_rejected_without_consuming_bytes(){run_child("regular-file",true);}
 #[cfg(not(target_os="trueos"))]
 #[test]fn host_registration_pipe_reaches_existing_auth_factory(){run_child("registration",false);}
 #[cfg(not(target_os="trueos"))]
 #[test]fn host_executor_admits_provider_before_reader_constructor(){reset();executor::ExecutorCredentialPipe::from_stdin("environment".into()).unwrap();assert_eq!(SPAWNS.load(Ordering::SeqCst),1);}
 fn run_child(mode:&str,regular:bool){
  let mut command=Command::new(std::env::current_exe().unwrap());command.args(["--ignored","--exact","tests::stdin_child","--nocapture"]).env("CREDENTIAL_FIXTURE_CHILD",mode).stdout(Stdio::piped()).stderr(Stdio::piped());
  let mut file=None;
  if regular{let mut input=tempfile::NamedTempFile::new().unwrap();input.write_all(SENTINEL.as_bytes()).unwrap();command.stdin(input.reopen().unwrap());file=Some(input);}else{command.stdin(Stdio::piped());}
  let mut child=command.spawn().unwrap();if let Some(mut stdin)=child.stdin.take(){stdin.write_all(SENTINEL.as_bytes()).unwrap();}
  let output=child.wait_with_output().unwrap();assert!(output.status.success(),"{} {}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));drop(file);
 }
 #[test]#[ignore]fn stdin_child(){
  let mode=std::env::var("CREDENTIAL_FIXTURE_CHILD").unwrap();reset();
  #[cfg(target_os="trueos")]
  {
   assert_eq!(mode,"native");unsupported(remote::credential_stdin().unwrap_err());assert!(config::read_chatgpt_stdin_auth(Some("account".into())).is_err());unsupported(remote::registration_auth_from_stdin().unwrap_err());unsupported(executor::ExecutorCredentialPipe::from_stdin("environment".into()).err().unwrap());assert!(command_constructor("environment".into()).is_err());assert_eq!(PARSES.load(Ordering::SeqCst),0);assert_eq!(FACTORIES.load(Ordering::SeqCst),0);assert_eq!(SPAWNS.load(Ordering::SeqCst),0);let mut line=String::new();std::io::stdin().lock().read_line(&mut line).unwrap();assert_eq!(line,SENTINEL);
  }
  #[cfg(not(target_os="trueos"))]
  match mode.as_str(){
   "chatgpt"=>{let auth=config::read_chatgpt_stdin_auth(Some("account".into())).unwrap();assert_eq!(auth.line,SENTINEL);assert_eq!(auth.account.as_deref(),Some("account"));assert_eq!(PARSES.load(Ordering::SeqCst),1);},
   "regular-file"=>{let error=config::read_chatgpt_stdin_auth(None).unwrap_err();assert!(error.to_string().contains("pipe or socket"));assert_eq!(PARSES.load(Ordering::SeqCst),0);let mut line=String::new();std::io::stdin().lock().read_line(&mut line).unwrap();assert_eq!(line,SENTINEL);},
   "registration"=>{assert_eq!(remote::registration_auth_from_stdin().unwrap(),SENTINEL.trim_end());assert_eq!(FACTORIES.load(Ordering::SeqCst),1);},
   _=>panic!("unexpected child mode")
  }
 }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    constructor = re.search(r'let ExecutorCredentialPipe\s*\{\s*provider,\s*parent_closed,\s*\}\s*= ExecutorCredentialPipe::from_stdin\(environment_id.trim\(\).to_string\(\)\)\?;', COMMAND.read_text()).group()
    source = HARNESS.replace('PROVIDER', function(REMOTE, 'credential_stdin')).replace('REGISTRATION', function(REMOTE, 'registration_auth_from_stdin')).replace('CHATGPT', function(CONFIG, 'read_chatgpt_stdin_auth')).replace('EXECUTOR_CONSTRUCTOR', function(EXECUTOR, 'from_stdin', 4)).replace('COMMAND_CONSTRUCTOR', constructor)
    with tempfile.TemporaryDirectory(prefix='trueos-credential-stdin-') as temporary:
        work = Path(temporary)
        (work / 'lib.rs').write_text(source)
        manifest(work, '''[package]
name="credential-stdin-fixture"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
tempfile="3"
rustix={version="1",features=["process"]}
''')
        wrapper = work / 'native-rustc.py'
        wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if os.environ.get("FIXTURE_NATIVE")=="1" and "--crate-name" in args and args[args.index("--crate-name")+1]=="credential_stdin_fixture":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''')
        wrapper.chmod(0o755)
        shutil.copytree(work, out / 'fixture', dirs_exist_ok=True)
        checks = []
        for native, count in [(False, 4), (True, 6)]:
            for mode, flags in [('debug', []), ('release', ['--release'])]:
                env = environment(out / ('native-target' if native else 'host-target'))
                env['RUSTC_WRAPPER'] = str(wrapper)
                env['FIXTURE_NATIVE'] = '1' if native else '0'
                command = ['rustup', 'run', TOOLCHAIN, 'cargo', 'test', '--offline', *flags,
                           '--target', 'x86_64-unknown-linux-gnu', '--lib', '--', '--test-threads=1']
                log = out / (('native-' if native else 'host-') + mode + '.log')
                with log.open('w') as stream:
                    result = subprocess.run(command, cwd=work, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=240)
                checks.append({'native': native, 'mode': mode, 'tests': count, 'command': command, 'exit': result.returncode, 'log': str(log)})
                if result.returncode or f'{count} passed' not in log.read_text():
                    raise RuntimeError(log.read_text()[-14000:])
    receipt = {'status': 'passed', 'scope': __doc__, 'checks': checks,
               'sources': [info(path) for path in [REMOTE, CONFIG, EXECUTOR, COMMAND]],
               'fixture': info(out / 'fixture/lib.rs'),
               'native_result': 'All credential stdin modes return Unsupported before parsing/auth construction/reader spawning; real synthetic stdin bytes remain untouched.',
               'host_result': 'Real pipe passes Unix metadata admission; real regular-file stdin rejects before read; registration factory and executor constructor retain host paths.',
               'limits': 'Token parser, auth-header factory and reader-thread constructor are counted boundaries. Existing parsing/rotation tests cover those unchanged algorithms separately.'}
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(out / 'receipt.json')


if __name__ == '__main__':
    main()
