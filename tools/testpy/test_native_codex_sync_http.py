#!/usr/bin/env python3
"""Stage actual tiny_http native transport selection; test optional native hostname.

The native HTTP check compiles the real crate against the new custom std. The
host control runs real loopback HTTP with the same staged crate. This does not
claim native packets or VM execution; std native ABI behavior has separate proof.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile,os,re
from test_native_mio import ROOT,TOOLCHAIN,info,environment,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
CODEX=ROOT.parent/'codex/codex-rs'
HOST=r'''
use std::io::{Read,Write};
fn main(){let server=tiny_http::Server::http("127.0.0.1:0").unwrap();let addr=server.server_addr().to_ip().unwrap();let worker=std::thread::spawn(move||{let request=server.recv().unwrap();assert_eq!(request.url(),"/native-http-proof");request.respond(tiny_http::Response::from_string("native-http-proof-ok")).unwrap();});let mut stream=std::net::TcpStream::connect(addr).unwrap();write!(stream,"GET /native-http-proof HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();let mut response=String::new();stream.read_to_string(&mut response).unwrap();assert!(response.starts_with("HTTP/1.0 200"));assert!(response.ends_with("native-http-proof-ok"));worker.join().unwrap();println!("ok real host loopback HTTP callback");}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--std-selection',type=Path,required=True);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 def run(label,command,env=None,timeout=180):
  log=out/(label+'.log')
  with log.open('w')as f:r=subprocess.run(command,cwd=work,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=timeout)
  checks.append({'label':label,'command':command,'exit':r.returncode,'log':str(log)})
  assert r.returncode==0,log.read_text()[-16000:]
  return log.read_text()
 source=next((Path.home()/'.cargo/registry/src').glob('*/tiny_http-0.12.0'));overlay=SDK/'src/codex_native_sync_http_overlay.rs'
 with tempfile.TemporaryDirectory(prefix='trueos-sync-http-')as tmp:
  work=Path(tmp);driver=work/'stage.rs';driver.write_text(STAGE_DRIVER.replace('@MODULE@',json.dumps(str(overlay))).replace('fn main(){','fn io_string(error:std::io::Error)->String{error.to_string()}\nfn main(){'))
  stage=work/'stage';run('compile-overlay',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(driver),'-o',str(stage)])
  staged=Path(run('stage-overlay',[str(stage),str(out/'staging'),str(source)]).strip())
  host=work/'host';manifest(host,f'[package]\nname="native-sync-http-host"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\ntiny_http={{path={json.dumps(str(staged))}}}\n[[bin]]\nname="proof"\npath="main.rs"\n');(host/'main.rs').write_text(HOST)
  run('host-http',['rustup','run',TOOLCHAIN,'cargo','run','--offline','--manifest-path',str(host/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu'],environment(out/'host-target'))
  hostname=work/'hostname.rs';hostname.write_text('extern crate self as pretty_assertions;pub use std::assert_eq;\nmod actual{include!('+json.dumps(str(CODEX/'config/src/host_name.rs'))+');#[test]fn native_has_no_invented_hostname_or_fqdn(){assert_eq!(host_name(),None);assert_eq!(os_host_name(),None);assert_eq!(local_fqdn_for_hostname("ignored"),None);}}')
  for mode,opts in [('debug',[]),('release',['-O'])]:
   exe=out/('hostname-'+mode);run('hostname-compile-'+mode,['rustup','run',TOOLCHAIN,'rustc','--edition=2024','--test','--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags',str(hostname),'-o',str(exe),*opts]);text=run('hostname-'+mode,[str(exe)]);assert re.search(r'test result: ok\. 4 passed',text)
  native=out/'native';manifest(native,f'[package]\nname="native-sync-http-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\ntiny_http={{path={json.dumps(str(staged))}}}\n[lib]\npath="lib.rs"\n');(native/'lib.rs').write_text('pub fn callback_server()->Result<tiny_http::Server,Box<dyn std::error::Error+Send+Sync>>{tiny_http::Server::http("127.0.0.1:0")}')
  env=environment(out/'native-target');env['RUSTC']=json.loads(a.std_selection.read_text())['rustc_wrapper'];env['RUSTC_BOOTSTRAP_SYNTHETIC_TARGET']='1'
  run('native-tiny-http',['rustup','run',TOOLCHAIN,'cargo','check','--offline','--manifest-path',str(native/'Cargo.toml'),'--target',str(ROOT.parent/'TRUEOS-Buildins/tools/trueos-app.json'),'-Zbuild-std=core,compiler_builtins,alloc,std,panic_abort','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec','-j','2','--config','patch.crates-io.libc.path='+json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/libc-0.2.186'))],env)
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(overlay),info(CODEX/'config/src/host_name.rs'),info(ROOT/'tools/rust-std/trueos_net.rs')],'limits':['Host loopback checks HTTP logic; native custom-target check is compilation only.','No native rig reset, authentication data, or external HTTP request.']},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
