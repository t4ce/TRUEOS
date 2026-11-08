#!/usr/bin/env python3
"""Exercise both pinned Socket2 stages with actual native std networking code.

Native tests retain production staging, facade and synchronous NativeSocket
implementation. Only std-internal imports and its thin public wrapper methods
are supplied for the host test harness; native ABI transport uses the existing
socket simulator. Host controls use the unchanged original Socket2 library and
real TCP/UDP loopback. No packets, grants, VM scheduling or rig are claimed for
the native simulator; separate custom-target compilation proves std wiring.
"""
from pathlib import Path
import argparse,json,os,re,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info
from test_native_rama_tcp import STAGE_DRIVER
from test_native_std_sync_net import HARNESS,SOURCE,BOUNDARY
SDK=ROOT.parent/'TRUEOS-Blueprints'

WRAPPERS=r'''
pub(crate) mod native_test_net {
 pub use super::connection::native::{NativeSocket,TcpStream,TcpListener,UdpSocket};
 macro_rules! public_socket {($($ty:ident),*)=>{$(impl $ty{
  pub fn native_socket(&self)->&NativeSocket{self.native()}
  pub fn into_native_socket(self)->NativeSocket{self.into_native()}
  pub fn from_native_socket(socket:NativeSocket)->Self{Self::from_native(socket)}
 })*};}
 public_socket!(TcpStream,TcpListener,UdpSocket);
}
'''
NATIVE_TESTS=r'''
#[cfg(test)]mod socket_tests {
 use super::*;use crate::{Socket,SockAddr,SockRef,Domain,Type,Protocol,TcpKeepalive};
 use std::{io::{Read,Write},mem::MaybeUninit,net::Shutdown};
 fn sockets()->(Socket,Socket,Socket){let listener=Socket::new(Domain::IPV4,Type::STREAM,Some(Protocol::TCP)).unwrap();listener.bind(&"127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap().into()).unwrap();listener.listen(8).unwrap();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();client.connect_timeout(&listener.local_addr().unwrap(),Duration::from_millis(20)).unwrap();let(peer,_)=listener.accept().unwrap();(listener,client,peer)}
 #[test]fn native_tcp_real_partial_io_peek_and_addresses(){let(listener,client,mut peer)=sockets();assert_eq!(client.peer_addr().unwrap(),listener.local_addr().unwrap());let mut writer=&client;writer.write_all(b"native socket2 payload").unwrap();let mut peek=[MaybeUninit::uninit();4];let n=peer.peek(&mut peek).unwrap();assert!(n>0);assert_eq!(unsafe{peek[0].assume_init()},b'n');let mut bytes=[0;22];peer.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"native socket2 payload");assert_eq!(client.domain().unwrap(),Domain::IPV4);assert_eq!(client.r#type().unwrap(),Type::STREAM);assert_eq!(client.protocol().unwrap(),Some(Protocol::TCP));}
 #[test]fn owned_and_borrowed_native_stream_conversions_retain_same_capability(){let before=service::live_sockets();let(listener,client,peer)=sockets();let stream=native_test_net::TcpStream::from(client);let borrowed=SockRef::from(&stream);borrowed.set_tcp_nodelay(true).unwrap();assert!(stream.nodelay().unwrap());drop(borrowed);let mut client=Socket::from(stream);let alias=client.try_clone().unwrap();let reference=SockRef::from(&client);assert!(reference.tcp_nodelay().unwrap());drop(reference);drop(alias);assert_eq!(service::live_sockets(),before+3);client.write_all(b"ok").unwrap();let mut bytes=[0;2];let mut peer=&peer;peer.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"ok");drop((listener,client,peer));}
 #[test]fn clone_shares_real_flags_timeouts_and_options(){let(_listener,client,_peer)=sockets();let alias=client.try_clone().unwrap();client.set_nonblocking(true).unwrap();assert!(alias.nonblocking().unwrap());assert_eq!((&alias).read(&mut[0]).unwrap_err().kind(),ErrorKind::WouldBlock);client.set_read_timeout(Some(Duration::from_millis(9))).unwrap();assert_eq!(alias.read_timeout().unwrap(),Some(Duration::from_millis(9)));client.set_tcp_nodelay(true).unwrap();assert!(alias.tcp_nodelay().unwrap());client.set_ttl_v4(39).unwrap();assert_eq!(alias.ttl_v4().unwrap(),39);assert_eq!(client.set_write_timeout(Some(Duration::ZERO)).unwrap_err().kind(),ErrorKind::InvalidInput);}
 #[test]fn unsupported_creation_contract_rejects_before_allocating(){let before=service::live_sockets();for(domain,kind,protocol)in[(Domain::UNIX,Type::STREAM,None),(Domain::IPV4,Type::DGRAM,Some(Protocol::UDP)),(Domain::IPV4,Type::RAW,None),(Domain::IPV4,Type::STREAM,Some(Protocol::UDP))]{assert_eq!(Socket::new(domain,kind,protocol).unwrap_err().kind(),ErrorKind::Unsupported);}assert_eq!(service::live_sockets(),before);assert_eq!(SockAddr::unix("must-not-create").unwrap_err().kind(),ErrorKind::Unsupported);}
 #[test]fn unavailable_options_do_not_claim_success_or_change_supported_state(){let(_listener,client,_peer)=sockets();let ttl=client.ttl_v4().unwrap();assert_eq!(client.set_reuse_address(true).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(client.set_tcp_keepalive(&TcpKeepalive::new().with_time(Duration::from_secs(10))).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(client.set_only_v6(true).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(client.set_linger(Some(Duration::ZERO)).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(client.ttl_v4().unwrap(),ttl);assert!(!client.tcp_nodelay().unwrap());}
 #[test]fn unsupported_message_flags_do_not_consume_or_send(){let(_listener,client,peer)=sockets();(&client).write_all(b"retained").unwrap();let mut bytes=[MaybeUninit::new(b'q');8];assert_eq!(peer.recv_with_flags(&mut bytes,1).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(unsafe{bytes[0].assume_init()},b'q');assert_eq!(client.send_with_flags(b"not sent",1).unwrap_err().kind(),ErrorKind::Unsupported);let mut bytes=[0;8];(&peer).read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"retained");assert_eq!(client.send_to(b"not sent",&peer.local_addr().unwrap()).unwrap_err().kind(),ErrorKind::Unsupported);}
 #[test]fn udp_socket_read_traits_dispatch_datagrams_and_preserve_peek(){let a=native_test_net::UdpSocket::bind("127.0.0.1:0").unwrap();let b=native_test_net::UdpSocket::bind("127.0.0.1:0").unwrap();let a=Socket::from(a);let mut b=Socket::from(b);a.connect(&b.local_addr().unwrap()).unwrap();b.connect_timeout(&a.local_addr().unwrap(),Duration::from_millis(10)).unwrap();assert_eq!(a.protocol().unwrap(),Some(Protocol::UDP));assert_eq!(a.send(b"udp").unwrap(),3);let mut peek=[MaybeUninit::uninit();5];assert_eq!(b.peek(&mut peek).unwrap(),3);let mut bytes=[0;5];assert_eq!(b.read(&mut bytes).unwrap(),3);assert_eq!(&bytes[..3],b"udp");a.send(b"next").unwrap();assert_eq!((&b).read(&mut bytes).unwrap(),4);assert_eq!(&bytes[..4],b"next");}
 #[test]fn udp_send_to_and_closed_empty_datagram_are_real(){let a=Socket::from(native_test_net::UdpSocket::bind("127.0.0.1:0").unwrap());let b=Socket::from(native_test_net::UdpSocket::bind("127.0.0.1:0").unwrap());assert_eq!(a.send_to(b"",&b.local_addr().unwrap()).unwrap(),0);let mut bytes=[0;1];assert_eq!((&b).read(&mut bytes).unwrap(),0);assert_eq!(a.send_to(b"x",&b.local_addr().unwrap()).unwrap(),1);assert_eq!((&b).read(&mut bytes).unwrap(),1);assert_eq!(bytes,[b'x']);}
 #[test]fn datagram_unsupported_tcp_operations_preserve_endpoint(){let a=Socket::from(native_test_net::UdpSocket::bind("127.0.0.1:0").unwrap());let address=a.local_addr().unwrap();let before=service::live_sockets();assert_eq!(a.bind(&address).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(a.listen(4).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(a.accept().unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(a.local_addr().unwrap(),address);assert_eq!(service::live_sockets(),before);}
 #[test]fn zero_connect_timeout_is_rejected_before_connection_effects(){let listener=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();listener.bind(&"127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap().into()).unwrap();listener.listen(4).unwrap();listener.set_nonblocking(true).unwrap();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();assert_eq!(client.connect_timeout(&listener.local_addr().unwrap(),Duration::ZERO).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(listener.accept().unwrap_err().kind(),ErrorKind::WouldBlock);assert!(!client.nonblocking().unwrap());}
 fn listener()->Socket{let listener=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();listener.bind(&"127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap().into()).unwrap();listener.listen(4).unwrap();listener}
 #[test]fn nonblocking_connect_reports_pending_before_writable_readiness(){let listener=listener();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();client.set_nonblocking(true).unwrap();service::POLL_CALLS.store(0,Ordering::Relaxed);service::POLL_DELAY.store(4,Ordering::Relaxed);assert_eq!(client.connect(&listener.local_addr().unwrap()).unwrap_err().kind(),ErrorKind::WouldBlock);assert_eq!(service::POLL_CALLS.load(Ordering::Relaxed),0);assert_eq!(client.peer_addr().unwrap(),listener.local_addr().unwrap());let stream=native_test_net::TcpStream::from(client.try_clone().unwrap());stream.native_socket().wait_connected(Duration::from_millis(50)).unwrap();assert!(service::POLL_CALLS.load(Ordering::Relaxed)>=5);let _peer=listener.accept().unwrap();assert!(client.nonblocking().unwrap());}
 #[test]fn connect_timeout_accepts_pending_waits_and_restores_both_modes(){let listener=listener();for previous in[false,true]{let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();client.set_nonblocking(previous).unwrap();service::POLL_CALLS.store(0,Ordering::Relaxed);service::POLL_DELAY.store(4,Ordering::Relaxed);client.connect_timeout(&listener.local_addr().unwrap(),Duration::from_millis(50)).unwrap();assert!(service::POLL_CALLS.load(Ordering::Relaxed)>=5);assert_eq!(client.nonblocking().unwrap(),previous);let _peer=listener.accept().unwrap();}}
 #[test]fn connect_error_precedes_writable_and_restores_mode_without_leaking(){let listener=listener();let baseline=service::live_sockets();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();service::READY_ERROR.store(true,Ordering::Relaxed);assert_eq!(client.connect_timeout(&listener.local_addr().unwrap(),Duration::from_millis(50)).unwrap_err().kind(),ErrorKind::NotConnected);service::READY_ERROR.store(false,Ordering::Relaxed);assert!(!client.nonblocking().unwrap());drop(client);assert_eq!(service::live_sockets(),baseline+1);drop(listener.accept().unwrap());assert_eq!(service::live_sockets(),baseline);}
 #[test]fn connect_deadline_restores_mode_and_drop_releases_owned_capability(){let listener=listener();let baseline=service::live_sockets();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();client.set_nonblocking(true).unwrap();service::POLL_DELAY.store(100,Ordering::Relaxed);assert_eq!(client.connect_timeout(&listener.local_addr().unwrap(),Duration::from_millis(3)).unwrap_err().kind(),ErrorKind::TimedOut);service::POLL_DELAY.store(0,Ordering::Relaxed);assert!(client.nonblocking().unwrap());drop(client);assert_eq!(service::live_sockets(),baseline+1);drop(listener.accept().unwrap());assert_eq!(service::live_sockets(),baseline);}
 #[test]fn blocking_connect_confirms_immediate_readiness_and_submission_error_is_preserved(){let listener=listener();let address=listener.local_addr().unwrap();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();service::POLL_CALLS.store(0,Ordering::Relaxed);client.connect(&address).unwrap();assert!(service::POLL_CALLS.load(Ordering::Relaxed)>0);assert!(!client.nonblocking().unwrap());let _peer=listener.accept().unwrap();drop(listener);let failed=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();assert_eq!(failed.connect_timeout(&address,Duration::from_millis(10)).unwrap_err().kind(),ErrorKind::NotConnected);assert!(!failed.nonblocking().unwrap());}
 #[test]fn tcp_half_close_is_not_final_capability_close(){let(_listener,client,peer)=sockets();client.shutdown(Shutdown::Write).unwrap();assert_eq!(client.send(b"late").unwrap_err().kind(),ErrorKind::BrokenPipe);assert_eq!((&peer).read(&mut[0]).unwrap(),0);(&peer).write_all(b"reply").unwrap();let mut bytes=[0;5];(&client).read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"reply");}
}
'''
HOST_TESTS=r'''
#[cfg(all(test,not(target_os="trueos")))]mod native_host_controls{
 use super::*;use std::io::{Read,Write};
 #[test]fn host_stage_keeps_real_tcp_loopback_and_options(){let listener=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();listener.set_reuse_address(true).unwrap();assert!(listener.reuse_address().unwrap());listener.bind(&"127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap().into()).unwrap();listener.listen(8).unwrap();let client=Socket::new(Domain::IPV4,Type::STREAM,None).unwrap();client.connect(&listener.local_addr().unwrap()).unwrap();let(mut peer,_)=listener.accept().unwrap();(&client).write_all(b"real host").unwrap();let mut bytes=[0;9];peer.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"real host");let stream:std::net::TcpStream=client.into();SockRef::from(&stream).set_nodelay(true).unwrap();assert!(stream.nodelay().unwrap());}
 #[test]fn host_stage_keeps_real_udp_creation_and_datagrams(){let a=Socket::new(Domain::IPV4,Type::DGRAM,Some(Protocol::UDP)).unwrap();let b=std::net::UdpSocket::bind("127.0.0.1:0").unwrap();a.send_to(b"host udp",&b.local_addr().unwrap().into()).unwrap();let mut bytes=[0;8];assert_eq!(b.recv_from(&mut bytes).unwrap().0,8);assert_eq!(&bytes,b"host udp");}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--custom-target',action='store_true');args=p.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[];counts={};native_sources={}
 module=SDK/'src/codex_native_socket_overlay.rs';facade=SDK/'src/socket2_trueos.rs'
 with tempfile.TemporaryDirectory(prefix='trueos-socket2-')as tmp:
  work=Path(tmp)
  def run(label,command,env=None):
   r=subprocess.run(command,cwd=work,env=env,text=True,capture_output=True,timeout=240);log=out/(label+'.log');log.write_text(r.stdout+r.stderr);checks.append({'label':label,'command':command,'exit':r.returncode,'log':str(log)})
   if r.returncode:raise RuntimeError(log.read_text()[-18000:])
   return r.stdout
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(module))).replace('overlay::stage(','overlay::stage_socket2(');(work/'stage.rs').write_text(driver)
  run('compile-production-stage',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
  runtime=HARNESS.split('#[test]fn actual_native_pair',1)[0];runtime=re.sub(r'^#![^\n]*\n','',runtime,flags=re.M)
  boundary=BOUNDARY.read_text().replace('//!','//')
  poll='let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let bits=match &s.kind{'
  assert boundary.count(poll)==1
  boundary=boundary.replace(poll,'POLL_CALLS.fetch_add(1,AtomicOrdering::Relaxed);if POLL_DELAY.try_update(AtomicOrdering::Relaxed,AtomicOrdering::Relaxed,|n|n.checked_sub(1)).is_ok(){return 0;}if READY_ERROR.load(AtomicOrdering::Relaxed){return 6;}'+poll)
  error='fn trueos_mio_socket_take_error(id:u32)->i32{if '
  assert boundary.count(error)==1
  boundary=boundary.replace(error,'fn trueos_mio_socket_take_error(id:u32)->i32{if READY_ERROR.load(AtomicOrdering::Relaxed){return -3;}if ')
  runtime=runtime.replace('BOUNDARY',boundary).replace('NATIVE',SOURCE.read_text().replace('//!','//')).replace('crate::','crate::native_fixture::')
  for version in ['0.6.3','0.5.10']:
   sources=list((Path.home()/'.cargo/registry/src').glob('*/socket2-'+version));assert len(sources)==1;before=info(sources[0]/'src/lib.rs')
   staged=Path(run('stage-'+version,[str(work/'stage'),str(work/'staged'),str(sources[0])]).strip());assert before==info(sources[0]/'src/lib.rs')
   native_sources[version]=Path(run('stage-native-source-'+version,[str(work/'stage'),str(out/'native-source'),str(sources[0])]).strip())
   # The standalone upstream development lock is unrelated to Codex's graph;
   # resolve the fixture against its cached host toolchain dependencies.
   (staged/'Cargo.lock').unlink(missing_ok=True)
   path=staged/'src/trueos_native.rs';text=path.read_text();before_import='self, Ipv4Addr, Ipv6Addr, NativeSocket, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6,';assert text.count(before_import)==1;text=text.replace(before_import,'Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6,');text+='\nuse crate::native_fixture::native_test_net::{self as net,NativeSocket};\n';path.write_text(text)
   host_tests=HOST_TESTS.replace('.set_nodelay(','.set_tcp_nodelay(')if version=='0.6.3'else HOST_TESTS
   root=staged/'src/lib.rs';root.write_text('#![cfg_attr(target_os="trueos",feature(core_io_borrowed_buf))]\n'+root.read_text()+'\n#[cfg(target_os="trueos")]\n#[allow(dead_code,unused_imports,missing_docs)]\nmod native_fixture {\n'+runtime+WRAPPERS+NATIVE_TESTS+'\n}\n'+host_tests)
   wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\nif "--crate-name"in a and a[a.index("--crate-name")+1]=="socket2":a += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]\nos.execvp(a[0],a)\n''');wrapper.chmod(0o755)
   for target in ['native','host']:
    env=environment(out/(version+'-'+target+'-target'))
    if target=='native':env['RUSTC_WRAPPER']=str(wrapper)
    for profile,flags in [('debug',[]),('release',['--release'])]:
     cmd=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(staged/'Cargo.toml'),'--lib','--','--test-threads=1'];result=run(version+'-'+target+'-'+profile,cmd,env);count=int(re.search(r'test result: ok\. (\d+) passed',result)[1]);assert count== (16 if target=='native'else 12);counts[version+'-'+target+'-'+profile]=count
  if args.custom_target:
   probe=out/'custom-target';probe.mkdir(exist_ok=True)
   (probe/'Cargo.toml').write_text(f'''[package]\nname="socket2-real-native-std-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nsocket2_6={{package="socket2",path={json.dumps(str(native_sources['0.6.3']))}}}\nsocket2_5={{package="socket2",path={json.dumps(str(native_sources['0.5.10']))}}}\n[patch.crates-io]\nlibc={{path={json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/libc-0.2.186'))}}}\n''')
   (probe/'lib.rs').write_text('''use std::net::{TcpStream,TcpListener,UdpSocket};\npub fn selected_native_api()->std::io::Result<()>{let socket=socket2_6::Socket::new(socket2_6::Domain::IPV4,socket2_6::Type::STREAM,None)?;let alias=socket.try_clone()?;socket2_6::SockRef::from(&socket).set_tcp_nodelay(true)?;let stream:TcpStream=alias.into();let _=socket2_6::Socket::from(stream);let listener:TcpListener=socket.into();let _=socket2_6::SockRef::from(&listener);let udp=UdpSocket::bind("127.0.0.1:0")?;let _=socket2_6::Socket::from(udp);let old=socket2_5::Socket::new(socket2_5::Domain::IPV4,socket2_5::Type::STREAM,None)?;let stream:TcpStream=old.into();let _=socket2_5::SockRef::from(&stream);Ok(())}\n''')
   rustc=run('installed-rustc',['rustup','run',TOOLCHAIN,'rustc','--print','sysroot']).strip()+'/bin/rustc'
   run('prepare-real-native-std',['python3',str(ROOT.parent/'TRUEOS-Buildins/tools/prepare_sysroot.py'),'--output',str(out/'std'),'--kernel',str(ROOT),'--rustc',rustc,'--result',str(out/'std-selection.json')])
   selected=json.loads((out/'std-selection.json').read_text());env=environment(out/'custom-target-cache');env['RUSTC']=selected['rustc_wrapper'];env['RUSTC_BOOTSTRAP_SYNTHETIC_TARGET']='1';env['CARGO_ENCODED_RUSTFLAGS']='\x1f'.join(['-Cpanic=abort','--check-cfg=cfg(target_os,values("trueos","zkvm"))'])
   run('actual-native-std-check',['rustup','run',TOOLCHAIN,'cargo','check','--offline','--manifest-path',str(probe/'Cargo.toml'),'--target',str(ROOT.parent/'TRUEOS-Buildins/tools/trueos-app.json'),'-Zbuild-std=core,compiler_builtins,alloc,std,panic_abort','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec','-j','2','--config','patch.crates-io.libc.path='+json.dumps(str(ROOT.parent/'TRUEOS-Buildins/vendor/libc-0.2.186'))],env)
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'cases':counts,'checks':checks,'sources':[info(p)for p in[module,facade,SOURCE,BOUNDARY,Path(__file__)]]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
