#!/usr/bin/env python3
"""Check actual Codex native lifecycle and callback cancellation branches.

Functions are taken directly from production sources. Real host Tokio drives
native-selected service policy; callback parsing uses actual Crossterm event
types with a fixture event stream and terminal-mode boundary. No VM is stopped.
"""
from pathlib import Path
import argparse,json,re,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
CODEX=ROOT.parent/'codex/codex-rs'

def function(text,start):
 begin=text.index(start);brace=text.index('{',begin);depth=1;end=brace+1
 # These selected production bodies contain no brace characters in string data.
 while depth:
  if text[end]=='{':depth+=1
  elif text[end]=='}':depth-=1
  end+=1
 return text[begin:end]

HARNESS=r'''
#![allow(dead_code)]
extern crate self as codex_utils_cli;
#[path="__LIFECYCLE__"]pub mod native_lifecycle;
use std::{io::{self,BufRead,Write},collections::VecDeque,sync::Mutex};
use anyhow::{Context,Result,bail};
use futures::{Stream,StreamExt};
use std::{pin::Pin,task::{Context as TaskContext,Poll}};
use crossterm::event::{Event,KeyCode,KeyEventKind,KeyModifiers,KeyEvent};
use tokio::{sync::{watch,oneshot},task::JoinHandle};
const MAX_CALLBACK_BYTES:usize=64*1024;
type IoResult<T>=io::Result<T>;
enum ShutdownSignal { Forceable,GracefulOnly }
static INPUT:Mutex<VecDeque<Event>>=Mutex::new(VecDeque::new());
static MODE:Mutex<bool>=Mutex::new(false);
mod terminal {
 pub fn enable_raw_mode()->std::io::Result<()>{*crate::MODE.lock().unwrap()=true;Ok(())}
 pub fn disable_raw_mode()->std::io::Result<()>{*crate::MODE.lock().unwrap()=false;Ok(())}
}
struct EventStream;
impl EventStream { fn new()->Self {Self} }
impl Stream for EventStream {
 type Item=io::Result<Event>;
 fn poll_next(self:Pin<&mut Self>,_:&mut TaskContext<'_>)->Poll<Option<Self::Item>>{Poll::Ready(INPUT.lock().unwrap().pop_front().map(Ok))}
}
__FUNCTIONS__
#[cfg(test)]mod tests {
 use super::*;
 #[tokio::test]async fn native_foreground_does_not_stop_or_close_from_missing_unix_signals(){
  let(mut rx,task)=foreground_stop_signal();tokio::task::yield_now().await;
  assert!(!*rx.borrow());assert!(tokio::time::timeout(std::time::Duration::from_millis(10),rx.changed()).await.is_err());
  task.abort();let _=task.await;assert!(!*rx.borrow());
 }
 #[tokio::test]async fn real_transport_eof_can_finish_while_native_shutdown_waits(){
  let(tx,rx)=oneshot::channel::<()>();tx.send(()).unwrap();
  tokio::select! { biased; _=shutdown_signal()=>panic!("native lifecycle must not manufacture a signal"), _=rx=>{} }
 }
 #[tokio::test]async fn terminal_ctrl_c_remains_real_oauth_cancellation(){
  INPUT.lock().unwrap().push_back(Event::Key(KeyEvent::new(KeyCode::Char('c'),KeyModifiers::CONTROL)));
  assert_eq!(read_terminal_callback().await.unwrap_err().to_string(),"OAuth login cancelled");assert!(!*MODE.lock().unwrap());
 }
 #[tokio::test]async fn terminal_ctrl_d_remains_oauth_input_eof(){
  INPUT.lock().unwrap().push_back(Event::Key(KeyEvent::new(KeyCode::Char('d'),KeyModifiers::CONTROL)));
  assert_eq!(read_terminal_callback().await.unwrap_err().to_string(),"No OAuth callback URL received before input closed");assert!(!*MODE.lock().unwrap());
 }
 #[tokio::test]async fn terminal_pasted_callback_is_preserved(){
  INPUT.lock().unwrap().extend([Event::Paste("https://fixture.test/callback?code=test".into()),Event::Key(KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE))]);
  assert_eq!(read_terminal_callback().await.unwrap(),"https://fixture.test/callback?code=test");assert!(!*MODE.lock().unwrap());
 }
 #[test]fn captured_input_eof_cancels_and_complete_url_succeeds(){
  assert_eq!(read_callback_line(io::Cursor::new("")).unwrap_err().to_string(),"No OAuth callback URL received before input closed");
  assert_eq!(read_callback_line(io::Cursor::new("https://fixture.test/callback\n")).unwrap(),"https://fixture.test/callback");
 }
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 remote=(CODEX/'cli/src/remote_control_cmd.rs').read_text();server=(CODEX/'app-server/src/lib.rs').read_text();mcp=(CODEX/'cli/src/mcp_login.rs').read_text()
 parts=[function(remote,'fn foreground_stop_signal()'),function(server,'async fn shutdown_signal()'),function(mcp,'fn read_callback_line(')]
 guard=mcp[mcp.index('struct TerminalInputGuard;'):mcp.index('#[cfg(test)]',mcp.index('struct TerminalInputGuard;'))]
 parts.append(guard)
 assert 'if !cfg!(target_os = "trueos") && matches!(mode, McpLoginMode::PasteCallback)'in mcp
 stdio=(CODEX/'app-server-transport/src/transport/stdio.rs').read_text();assert '#[cfg(all(unix, not(target_os = "trueos")))]\n    if install_shutdown_signal_handler'in stdio
 with tempfile.TemporaryDirectory(prefix='trueos-codex-signals-')as temp:
  work=Path(temp);manifest(work,f'''[package]
name="codex-native-signal-policy"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
futures="0.3"
crossterm={{path={json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/crossterm-0.29.0-trueos'))}}}
tokio={{path={json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/tokio-1.54.1'))},features=["rt","macros","sync","time","signal"]}}
[patch.crates-io]
mio={{path={json.dumps(str(ROOT/'vendor/mio-1.2.0'))}}}
''')
  (work/'lib.rs').write_text(HARNESS.replace('__LIFECYCLE__',str(CODEX/'utils/cli/src/native_lifecycle.rs')).replace('__FUNCTIONS__','\n'.join(parts)))
  wrapper=work/'root-native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name" in args and args[args.index("--crate-name")+1]=="codex_native_signal_policy":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);cmd=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'];log=out/(mode+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':mode,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-12000:])
   assert '6 passed'in log.read_text()
 paths=['utils/cli/src/native_lifecycle.rs','utils/cli/src/lib.rs','cli/src/remote_control_cmd.rs','cli/src/mcp_login.rs','app-server/src/lib.rs','app-server-transport/src/transport/stdio.rs']
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(CODEX/p)for p in paths]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
