#!/usr/bin/env python3
"""Exercise actual native Rama UDP selection and options on real host UDP.

The selected adapter and full public Rama options are production code. The
transport is ordinary host Tokio UDP; native transport compilation is checked
by the normal Blueprint --check path, not claimed as rig verification here.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
TESTS=r'''
#[cfg(test)]mod tests{
 use super::*;
 fn options()->SocketOptions{SocketOptions::default_udp()}
 #[test]fn validates_exact_type_protocol_family_and_address_before_bind(){
  let o=options();assert_eq!(validated_address(&o).unwrap(),"0.0.0.0:0".parse::<SocketAddr>().unwrap());
  for o in [SocketOptions{r#type:Type::Stream,..options()},SocketOptions{protocol:Some(Protocol::TCP),..options()},SocketOptions{domain:Domain::Unix,..options()}]{assert!(validated_address(&o).is_err());}
  let o=SocketOptions{address:Some("[::1]:0".parse().unwrap()),..options()};assert!(validated_address(&o).is_err());
 }
 #[test]fn rejects_explicit_unavailable_options_even_false_defaults(){
  for o in [SocketOptions{broadcast:Some(false),..options()},SocketOptions{keep_alive:Some(false),..options()},SocketOptions{read_timeout:Some(std::time::Duration::from_secs(1)),..options()},SocketOptions{reuse_address:Some(false),..options()},SocketOptions{tcp_no_delay:Some(false),..options()}]{assert!(validated_address(&o).is_err());}
  for ttl in [0,256]{assert!(validated_address(&SocketOptions{ttl:Some(ttl),..options()}).is_err());}
 }
 #[test]fn preserves_real_udp_bind_port_payload_and_ttl(){
  tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
   let receiver=UdpSocket::bind("127.0.0.1:0").await.unwrap();let receiver_addr=receiver.local_addr().unwrap();
   let socket=bind_udp(Interface::Socket(SocketOptions{address:Some("127.0.0.1:0".parse().unwrap()),ttl:Some(42),..options()}.into())).await.unwrap();assert_eq!(socket.ttl().unwrap(),42);assert_ne!(socket.local_addr().unwrap().port(),0);
   socket.send_to(b"native UDP options",receiver_addr).await.unwrap();let mut bytes=[0;64];let(n,peer)=receiver.recv_from(&mut bytes).await.unwrap();assert_eq!(&bytes[..n],b"native UDP options");assert_eq!(peer,socket.local_addr().unwrap());
  });
 }
 #[test]fn address_interface_works_and_an_occupied_bind_reports_failure(){
  tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
   let socket=bind_udp(Interface::new_address("127.0.0.1:0".parse::<SocketAddr>().unwrap())).await.unwrap();let addr=socket.local_addr().unwrap();assert_ne!(addr.port(),0);
   assert!(bind_udp(Interface::Socket(SocketOptions{address:Some(addr.into()),..options()}.into())).await.is_err());
  });
 }
}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-rama-udp-')as temporary:
  work=Path(temporary);driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_socket_overlay.rs'))).replace('overlay::stage(','overlay::stage_rama_udp(');(work/'stage.rs').write_text(driver)
  subprocess.run(['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')],check=True)
  source=next((Path.home()/'.cargo/registry/src').glob('*/rama-udp-0.3.0-alpha.4'));staged=Path(subprocess.check_output([str(work/'stage'),str(work/'staged'),str(source)],text=True).strip());(out/'staged-source.json').write_text(json.dumps({'source':str(source),'staged':str(staged)},indent=2))
  manifest(work,f'''[package]\nname="native-rama-udp-provider"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nrama-core="=0.3.0-alpha.4"\nrama-net="=0.3.0-alpha.4"\nrama-error="=0.3.0-alpha.4"\nserde_json="1"\ntokio={{path={json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/tokio-1.54.1'))},features=["rt","net","time"]}}\n[patch.crates-io]\nmio={{path={json.dumps(str(ROOT/'vendor/mio-1.2.0'))}}}\n''')
  (work/'Cargo.lock').write_bytes((ROOT.parent/'codex/codex-rs/Cargo.lock').read_bytes())
  (work/'lib.rs').write_text((staged/'src/socket_trueos.rs').read_text().replace('//!','//')+TESTS)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--lib','--','--test-threads=1'];log=out/(mode+'.log')
   with log.open('w')as f:r=subprocess.run(command,cwd=work,env=environment(out/'target'),stdout=f,stderr=subprocess.STDOUT,timeout=240)
   if r.returncode:raise RuntimeError(log.read_text()[-10000:])
   checks.append({'mode':mode,'command':command,'log':str(log)})
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(SDK/'src/codex_native_socket_overlay.rs'),info(SDK/'src/rama_udp_trueos.rs')]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
