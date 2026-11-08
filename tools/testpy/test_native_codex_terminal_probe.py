#!/usr/bin/env python3
"""Run the actual native terminal probe and Crossterm replay parser together.

Only existing native console/lease/wait ABI calls are instrumented. The full
Codex probe, startup filtering and selected Crossterm event parser are real.
This checks deadline, byte-budget, partial-write and lease handoff behavior;
it does not claim physical terminal integration.
"""
from pathlib import Path
import argparse,json,re,shutil,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
CODEX=ROOT.parent/'codex/codex-rs'
CROSSTERM=ROOT.parent/'TRUEOS-Buildins/vendor/crossterm-0.29.0-trueos'
HARNESS=r'''
#![allow(dead_code)]
use std::{collections::VecDeque,sync::{Mutex,OnceLock},time::{Duration,Instant}};
struct State{lease:u64,generation:u32,bytes:VecDeque<u8>,output:Vec<u8>,write_limit:usize,read_error:isize,wait_error:i32,after_observe:Vec<u8>,on_wait:Vec<u8>,waits:Vec<(u32,u64)>}
fn state()->&'static Mutex<State>{static STATE:OnceLock<Mutex<State>>=OnceLock::new();STATE.get_or_init(||Mutex::new(State{lease:1,generation:1,bytes:VecDeque::new(),output:Vec::new(),write_limit:usize::MAX,read_error:0,wait_error:0,after_observe:Vec::new(),on_wait:Vec::new(),waits:Vec::new()}))}
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_blueprint_terminal_lease_current_v1(expected:u64,epoch:*mut u64)->i32{let s=state().lock().unwrap();if s.lease==0||expected!=0&&expected!=s.lease{return -3;}unsafe{epoch.write(s.lease)};0}
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_blueprint_terminal_surface_snapshot_v1(g:*mut u64,c:*mut u32,r:*mut u32)->i32{let s=state().lock().unwrap();if s.lease==0{return -3;}unsafe{g.write(s.lease);c.write(120);r.write(40)};0}
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_shell_attached_read_v1(out:*mut u8,cap:usize)->isize{let mut s=state().lock().unwrap();if s.read_error!=0{return s.read_error;}let n=cap.min(s.bytes.len());for i in 0..n{unsafe{out.add(i).write(s.bytes.pop_front().unwrap())}}n as isize}
#[unsafe(no_mangle)]unsafe extern "C"fn trueos_cabi_shell_attached_write(bytes:*const u8,len:usize)->usize{let mut s=state().lock().unwrap();let n=len.min(s.write_limit);s.output.extend_from_slice(unsafe{std::slice::from_raw_parts(bytes,n)});n}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_shell_attached_observe_v1()->u32{let mut s=state().lock().unwrap();let g=s.generation;let input=std::mem::take(&mut s.after_observe);if !input.is_empty(){s.bytes.extend(input);s.generation+=1;}g}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_shell_attached_wait_after_v1(g:u32,ms:u64)->i32{let mut s=state().lock().unwrap();s.waits.push((g,ms));if s.wait_error!=0{return s.wait_error;}let input=std::mem::take(&mut s.on_wait);if !input.is_empty(){s.bytes.extend(input);s.generation+=1;}let changed=s.generation!=g;drop(s);if !changed{std::thread::sleep(Duration::from_millis(ms.min(20)));}i32::from(changed)}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_env_var(_: *const u8,_:usize,_:*mut u8,_:usize)->isize{-1}
#[unsafe(no_mangle)]extern "C"fn trueos_cabi_log(_:u32,_:*const u8,_:usize,_:*const u8,_:usize)->i32{0}
#[unsafe(no_mangle)]extern "Rust"fn trueos_platform_monotonic_nanos()->u64{static ORIGIN:OnceLock<Instant>=OnceLock::new();1_000_000_000+ORIGIN.get_or_init(Instant::now).elapsed().as_nanos()as u64}
#[unsafe(no_mangle)]extern "Rust"fn trueos_tokio_platform_wait_observe(_:u64)->u32{0}
#[unsafe(no_mangle)]extern "Rust"fn trueos_tokio_platform_wait_after(_:u64,_:u32,ms:u64)->bool{std::thread::sleep(Duration::from_millis(ms.min(1)));false}
#[unsafe(no_mangle)]extern "Rust"fn trueos_tokio_platform_wake_all(_:u64)->usize{0}
#[unsafe(no_mangle)]extern "Rust"fn trueos_tokio_platform_wake_one(_:u64)->bool{false}
mod terminal_probe;
'''
TESTS=r'''
#[cfg(test)]mod native_transport_tests{
 use super::*;use crate::state;use std::time::Instant;use crossterm::event::{self,Event,KeyCode};
 fn reset(){let mut s=state().lock().unwrap();s.lease=1;s.generation+=1;s.bytes.clear();s.output.clear();s.write_limit=usize::MAX;s.read_error=0;s.wait_error=0;s.after_observe.clear();s.on_wait.clear();s.waits.clear();drop(s);event::discard_buffered_input().unwrap();while event::poll(Duration::ZERO).unwrap(){let _=event::read().unwrap();}}
 fn events()->Vec<Event>{let mut out=Vec::new();while event::poll(Duration::ZERO).unwrap(){let e=event::read().unwrap();if !matches!(e,Event::Resize(..)){out.push(e);}}out}
 #[test]fn native_probe_keeps_deadline_and_byte_budget_without_draining_typeahead(){reset();state().lock().unwrap().bytes.extend(b"still queued");let mut tty=native_tty::Tty::open().unwrap();let mut out=Vec::new();tty.read_available(&mut out,Instant::now(),5).unwrap();assert!(out.is_empty());assert_eq!(state().lock().unwrap().bytes.len(),12);tty.read_available(&mut out,Instant::now()+Duration::from_secs(1),5).unwrap();assert_eq!(out,b"still");assert_eq!(state().lock().unwrap().bytes.iter().copied().collect::<Vec<_>>(),b" queued");}
 #[test]fn observation_read_race_does_not_park_or_lose_input(){reset();state().lock().unwrap().after_observe=b"x".to_vec();let mut tty=native_tty::Tty::open().unwrap();assert!(tty.poll_readable(Duration::from_millis(10)).unwrap());assert!(state().lock().unwrap().waits.is_empty());let mut bytes=Vec::new();tty.read_available(&mut bytes,Instant::now()+Duration::from_secs(1),1).unwrap();assert_eq!(bytes,b"x");}
 #[test]fn wait_registration_race_is_preserved(){reset();state().lock().unwrap().on_wait=b"z".to_vec();let mut tty=native_tty::Tty::open().unwrap();assert!(tty.poll_readable(Duration::from_millis(10)).unwrap());assert_eq!(state().lock().unwrap().waits.len(),1);let mut bytes=Vec::new();tty.read_available(&mut bytes,Instant::now()+Duration::from_secs(1),1).unwrap();assert_eq!(bytes,b"z");}
 #[test]fn native_writes_handle_short_writes_and_report_zero_progress(){reset();state().lock().unwrap().write_limit=2;let mut tty=native_tty::Tty::open().unwrap();tty.write_all(b"terminal-query").unwrap();assert_eq!(state().lock().unwrap().output,b"terminal-query");state().lock().unwrap().write_limit=0;assert_eq!(tty.write_all(b"x").unwrap_err().kind(),std::io::ErrorKind::WriteZero);}
 #[test]fn actual_probe_parses_responses_and_replays_typed_input_and_paste(){reset();state().lock().unwrap().bytes.extend(b"typed\x1b[200~paste\x1b[201~\x1b[20;10R\x1b]10;rgb:eeee/eeee/eeee\x1b\\\x1b]11;rgb:1111/1111/1111\x07\x1b[?64;1;2c\x1b[?7u");let p=startup(Duration::from_millis(50),StartupKeyboardEnhancementProbe::Query).unwrap();assert_eq!(p.cursor_position,Some(ratatui::layout::Position{x:9,y:19}));assert_eq!(p.default_colors,Some(DefaultColors{fg:(238,238,238),bg:(17,17,17)}));assert_eq!(p.keyboard_enhancement_supported,Some(true));let output=events();let typed=output.iter().filter_map(|e|match e{Event::Key(k)=>match k.code{KeyCode::Char(c)=>Some(c),_=>None},_=>None}).collect::<String>();assert_eq!(typed,"typed");assert!(output.contains(&Event::Paste("paste".into())));}
 #[test]fn incomplete_startup_paste_continues_in_the_actual_parser(){reset();let tty=native_tty::Tty::open().unwrap();tty.replay_input(b"\x1b[200~partial").unwrap();assert!(events().is_empty());state().lock().unwrap().bytes.extend(b" paste\x1b[201~");assert_eq!(events(),vec![Event::Paste("partial paste".into())]);}
 #[test]fn stale_probe_lease_cannot_replay_into_reentered_terminal(){reset();let tty=native_tty::Tty::open().unwrap();state().lock().unwrap().lease=2;assert!(tty.replay_input(b"sensitive").is_err());assert!(events().is_empty());}
 #[test]fn replay_queued_before_handoff_is_discarded_before_reentry_delivery(){reset();let tty=native_tty::Tty::open().unwrap();tty.replay_input(b"old-session-input").unwrap();state().lock().unwrap().lease=2;assert!(events().is_empty(),"old lease input must not survive reentry");}
 #[test]fn native_read_and_wait_errors_are_not_eof_or_ready_success(){reset();let mut tty=native_tty::Tty::open().unwrap();state().lock().unwrap().read_error=-3;assert!(tty.poll_readable(Duration::from_millis(5)).is_err());state().lock().unwrap().read_error=0;state().lock().unwrap().wait_error=-3;assert!(tty.poll_readable(Duration::from_millis(5)).is_err());}
 #[test]fn unavailable_terminal_open_fails_and_empty_probe_has_a_bounded_deadline(){reset();state().lock().unwrap().lease=0;assert!(native_tty::Tty::open().is_err());state().lock().unwrap().lease=1;let before=Instant::now();let p=startup(Duration::from_millis(5),StartupKeyboardEnhancementProbe::Query).unwrap();assert!(p.cursor_position.is_none());assert!(before.elapsed()<Duration::from_millis(100));assert!(state().lock().unwrap().waits.iter().all(|(_,ms)|*ms<=5));}
}
'''
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-terminal-probe-')as temporary:
  work=Path(temporary)
  text=(CROSSTERM/'Cargo.toml').read_text();text=text.replace('path = "src/lib.rs"','path = '+json.dumps(str(CROSSTERM/'src/lib.rs')))
  for relative in re.findall(r'^path = "(\.\.[^"]+)"$',text,re.M):text=text.replace('path = '+json.dumps(relative),'path = '+json.dumps(str((CROSSTERM/relative).resolve())))
  text+='\n[dependencies.trueos-runtime]\npath='+json.dumps(str(ROOT/'crates/trueos-runtime'))+'\n[dependencies.trueos-io]\npath='+json.dumps(str(ROOT/'crates/trueos-io'))+'\n'
  manifest(work/'crossterm',text)
  detection=CODEX/'terminal-detection/src/lib.rs';manifest(work/'terminal-detection',f'[package]\nname="codex-terminal-detection"\nversion="0.0.0"\nedition="2024"\n[lib]\npath={json.dumps(str(detection))}\n[dependencies]\ntracing="0.1"\n')
  manifest(work,f'''[package]
name="native-codex-terminal-probe"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
crossterm={{path="crossterm"}}
codex-terminal-detection={{path="terminal-detection"}}
ratatui={{package="ratatui-core",version="=0.1.2",default-features=false}}
pretty_assertions="1"
[patch.crates-io]
mio={{path={json.dumps(str(ROOT/'vendor/mio-1.2.0'))}}}
''')
  shutil.copytree(CODEX/'tui/src/terminal_probe',work/'terminal_probe');(work/'terminal_probe.rs').write_text((CODEX/'tui/src/terminal_probe.rs').read_text()+TESTS);(work/'lib.rs').write_text(HARNESS.replace('extern "C"fn','extern "C" fn').replace('extern "Rust"fn','extern "Rust" fn'))
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]in{"native_codex_terminal_probe","crossterm"}:args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'];log=out/(mode+'.log')
   with log.open('w')as stream:result=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=240)
   checks.append({'mode':mode,'exit':result.returncode,'command':command,'log':str(log)})
   if result.returncode:raise RuntimeError(log.read_text()[-16000:])
   assert 'native_transport_tests::'in log.read_text()and'0 failed'in log.read_text()
 sources=[CODEX/'tui/src/terminal_probe.rs',*[p for p in(CODEX/'tui/src/terminal_probe').glob('*.rs')],CROSSTERM/'src/trueos.rs',CROSSTERM/'src/event.rs',CROSSTERM/'src/event/read.rs',CROSSTERM/'src/event/source.rs',CROSSTERM/'src/event/source/unix/trueos.rs']
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(p)for p in sources]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
