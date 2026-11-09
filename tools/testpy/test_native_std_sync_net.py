#!/usr/bin/env python3
"""Execute production synchronous std networking over the existing native ABI simulator.

Only standard-library internal imports/vector helper visibility are adapted for
host execution. The separate custom-target probe compiles unchanged installed
std. Native packet transport, VM scheduling, and grants are not simulated proof
of a live rig; existing actual adapter tests cover those boundaries separately.
"""
from pathlib import Path
import argparse,subprocess,json,tempfile,re,hashlib,os
from test_native_mio import ROOT, TOOLCHAIN, info
SOURCE=ROOT/'tools/rust-std/trueos_net.rs'
BOUNDARY=ROOT/'tools/testpy/native_tokio/network_boundary.rs'
HARNESS=r'''
#![feature(core_io_borrowed_buf,read_buf)]
#![allow(dead_code,unused_imports)]
pub use std::{net,sync,time,thread,vec,mem};
mod io{
 pub use std::io::*;
 pub fn default_read_vectored<F:FnOnce(&mut[u8])->Result<usize>>(f:F,b:&mut[IoSliceMut<'_>])->Result<usize>{for s in b{if !s.is_empty(){return f(s);}}f(&mut[])}
 pub fn default_write_vectored<F:FnOnce(&[u8])->Result<usize>>(f:F,b:&[IoSlice<'_>])->Result<usize>{for s in b{if !s.is_empty(){return f(s);}}f(&[])}
}
mod boundary{pub fn trueos_tokio_platform_wake_all(_:u64)->usize{0}pub fn trueos_cabi_thread_current_id()->usize{0}}
mod service{
 use std::sync::atomic::{AtomicBool,Ordering as AtomicOrdering};
 pub static POLL_DELAY:AtomicUsize=AtomicUsize::new(0);
 pub static POLL_CALLS:AtomicUsize=AtomicUsize::new(0);
 pub static READY_ERROR:AtomicBool=AtomicBool::new(false);
 pub static REVOKE_READ:AtomicBool=AtomicBool::new(false);
 pub static READS:AtomicUsize=AtomicUsize::new(0);
 pub static HUGE_READ:AtomicBool=AtomicBool::new(false);
 BOUNDARY
}
mod connection{
 pub fn each_addr<A:std::net::ToSocketAddrs,F,T>(addr:A,mut f:F)->std::io::Result<T>where F:FnMut(&std::net::SocketAddr)->std::io::Result<T>{let mut err=None;for a in addr.to_socket_addrs()?{match f(&a){Ok(v)=>return Ok(v),Err(e)=>err=Some(e)}}Err(err.unwrap_or(std::io::ErrorKind::InvalidInput.into()))}
 pub mod native {NATIVE}
}
use connection::native::*;
use std::io::ErrorKind;
use std::sync::atomic::Ordering;
use std::time::{Duration,Instant};
#[unsafe(no_mangle)]unsafe extern "C" fn trueos_cabi_dns_resolve_ipv4(name:*const u8,len:usize,out:*mut u8)->i32{service::DNS_CALLS.fetch_add(1,Ordering::Relaxed);match unsafe{std::str::from_utf8(std::slice::from_raw_parts(name,len)).unwrap()}{"native.test"=>{unsafe{std::ptr::copy_nonoverlapping([127,0,0,1].as_ptr(),out,4)};0},"timeout.test"=>110,""=>22,_=>5}}
fn pair()->(TcpListener,TcpStream,TcpStream){let l=TcpListener::bind("127.0.0.1:0").unwrap();let a=TcpStream::connect(l.socket_addr().unwrap()).unwrap();let(b,_)=l.accept().unwrap();(l,a,b)}
fn write_all(s:&TcpStream,mut bytes:&[u8]){while !bytes.is_empty(){let n=s.write(bytes).unwrap();assert!(n>0);bytes=&bytes[n..];}}
fn read_all(s:&TcpStream,mut bytes:&mut[u8]){while !bytes.is_empty(){let n=s.read(bytes).unwrap();assert!(n>0);bytes=&mut bytes[n..];}}
#[test]fn actual_native_pair_partial_io_and_addresses(){let(l,a,b)=pair();assert_eq!(a.peer_addr().unwrap(),l.socket_addr().unwrap());assert_eq!(a.socket_addr().unwrap(),b.peer_addr().unwrap());write_all(&a,b"partial native request");let mut bytes=[0;22];read_all(&b,&mut bytes);assert_eq!(&bytes,b"partial native request");write_all(&b,b"reply");let mut bytes=[0;5];read_all(&a,&mut bytes);assert_eq!(&bytes,b"reply");}
#[test]fn clone_shares_options_and_closes_only_last_reference(){let baseline=service::live_sockets();let(l,a,b)=pair();let c=a.duplicate().unwrap();assert_eq!(a.native().registration_id(),c.native().registration_id());a.set_read_timeout(Some(Duration::from_millis(17))).unwrap();a.set_nonblocking(true).unwrap();assert_eq!(c.read_timeout().unwrap(),Some(Duration::from_millis(17)));assert!(c.native().nonblocking());drop(a);assert_eq!(service::live_sockets(),baseline+3);write_all(&c,b"alive");drop(c);assert_eq!(service::live_sockets(),baseline+2);let mut bytes=[0;5];read_all(&b,&mut bytes);assert_eq!(&bytes,b"alive");assert_eq!(b.read(&mut bytes).unwrap(),0);drop((l,b));assert_eq!(service::live_sockets(),baseline);}
#[test]fn blocking_read_waits_for_later_writer(){let(_l,a,b)=pair();let t=std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(12));write_all(&a,b"later");});let mut bytes=[0;5];read_all(&b,&mut bytes);assert_eq!(&bytes,b"later");t.join().unwrap();}
#[test]fn nonblocking_and_finite_timeouts_are_distinct(){let(_l,a,_b)=pair();assert_eq!(a.set_read_timeout(Some(Duration::ZERO)).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(a.set_write_timeout(Some(Duration::ZERO)).unwrap_err().kind(),ErrorKind::InvalidInput);a.set_read_timeout(Some(Duration::from_millis(6))).unwrap();assert_eq!(a.read(&mut[0]).unwrap_err().kind(),ErrorKind::TimedOut);a.set_nonblocking(true).unwrap();assert_eq!(a.read(&mut[0]).unwrap_err().kind(),ErrorKind::WouldBlock);a.set_read_timeout(None).unwrap();assert_eq!(a.read_timeout().unwrap(),None);}
#[test]fn accept_waits_and_listener_clone_retains_capability(){let l=TcpListener::bind("127.0.0.1:0").unwrap();let copy=l.duplicate().unwrap();let addr=l.socket_addr().unwrap();drop(l);copy.set_nonblocking(true).unwrap();assert_eq!(copy.accept().unwrap_err().kind(),ErrorKind::WouldBlock);copy.set_nonblocking(false).unwrap();let t=std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(8));TcpStream::connect(addr).unwrap()});let(peer,_)=copy.accept().unwrap();drop(peer);drop(t.join().unwrap());}
#[test]fn half_close_is_not_socket_close(){let(_l,a,b)=pair();a.shutdown(std::net::Shutdown::Write).unwrap();assert_eq!(a.write(b"late").unwrap_err().kind(),ErrorKind::BrokenPipe);assert_eq!(b.read(&mut[0]).unwrap(),0);write_all(&b,b"reply");let mut out=[0;5];read_all(&a,&mut out);assert_eq!(&out,b"reply");}
#[test]fn nodelay_ttl_are_real_and_unavailable_options_fail(){let(_l,a,_b)=pair();assert!(!a.nodelay().unwrap());a.set_nodelay(true).unwrap();assert!(a.nodelay().unwrap());a.set_ttl(37).unwrap();assert_eq!(a.ttl().unwrap(),37);assert_eq!(a.set_ttl(0).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(a.set_keepalive(true).unwrap_err().kind(),ErrorKind::Unsupported);assert_eq!(a.linger().unwrap_err().kind(),ErrorKind::Unsupported);}
#[test]fn raw_tcp_factory_uses_real_binding_and_ownership(){let socket=NativeSocket::new_tcp(4).unwrap();socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();socket.listen(8).unwrap();let addr=socket.local_addr().unwrap();let a=NativeSocket::new_tcp(4).unwrap();a.connect(addr).unwrap();let(b,_)=socket.accept().unwrap();a.write(b"raw").unwrap();let mut out=[0;3];assert_eq!(b.read(&mut out,false).unwrap(),3);assert_eq!(&out,b"raw");}
#[test]fn udp_peek_empty_datagram_connected_peer_and_unsupported_options(){let a=UdpSocket::bind("127.0.0.1:0").unwrap();let b=UdpSocket::bind("127.0.0.1:0").unwrap();a.connect(b.socket_addr().unwrap()).unwrap();b.connect(a.socket_addr().unwrap()).unwrap();assert_eq!(a.send(b"").unwrap(),0);assert_eq!(b.peek_from(&mut[1]).unwrap().0,0);assert_eq!(b.recv(&mut[1]).unwrap(),0);a.send(b"udp").unwrap();let mut out=[0;5];assert_eq!(b.peek(&mut out).unwrap(),3);assert_eq!(b.recv(&mut out).unwrap(),3);assert_eq!(&out[..3],b"udp");assert_eq!(a.set_broadcast(true).unwrap_err().kind(),ErrorKind::Unsupported);}
#[test]fn ipv6_scope_is_rejected_before_creating_capability(){let baseline=service::live_sockets();let addr=std::net::SocketAddr::V6(std::net::SocketAddrV6::new(std::net::Ipv6Addr::LOCALHOST,80,0,3));assert_eq!(TcpStream::connect(addr).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(UdpSocket::bind(addr).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(service::live_sockets(),baseline);}
#[test]fn dns_positive_errors_never_become_successful_zero_address(){let before=service::DNS_CALLS.load(Ordering::Relaxed);assert_eq!(lookup_host("native.test",73).unwrap().collect::<Vec<_>>(),vec!["127.0.0.1:73".parse().unwrap()]);assert_eq!(lookup_host("timeout.test",73).unwrap_err().kind(),ErrorKind::TimedOut);assert_eq!(lookup_host("",73).unwrap_err().kind(),ErrorKind::InvalidInput);assert_eq!(lookup_host("missing.test",73).unwrap_err().kind(),ErrorKind::Other);assert_eq!(service::DNS_CALLS.load(Ordering::Relaxed),before+4);assert_eq!(lookup_host("::1",73).unwrap().next().unwrap(),"[::1]:73".parse().unwrap());assert_eq!(lookup_host("localhost",73).unwrap().next().unwrap(),"127.0.0.1:73".parse().unwrap());assert_eq!(service::DNS_CALLS.load(Ordering::Relaxed),before+4);}
#[test]fn pending_peer_address_does_not_certify_connect_completion(){let l=TcpListener::bind("127.0.0.1:0").unwrap();service::POLL_CALLS.store(0,Ordering::Relaxed);service::POLL_DELAY.store(4,Ordering::Relaxed);let a=TcpStream::connect_timeout(&l.socket_addr().unwrap(),Duration::from_millis(50)).unwrap();assert!(service::POLL_CALLS.load(Ordering::Relaxed)>=5);drop(a);let _=l.accept().unwrap();}
#[test]fn ready_error_takes_precedence_over_writable_and_failed_connect_closes(){let l=TcpListener::bind("127.0.0.1:0").unwrap();service::READY_ERROR.store(true,Ordering::Relaxed);assert_eq!(TcpStream::connect(l.socket_addr().unwrap()).unwrap_err().kind(),ErrorKind::NotConnected);service::READY_ERROR.store(false,Ordering::Relaxed);drop(l.accept().unwrap());}
#[test]fn pending_connect_deadline_cleans_up_native_capability(){let l=TcpListener::bind("127.0.0.1:0").unwrap();let before=service::live_sockets();service::POLL_DELAY.store(100,Ordering::Relaxed);assert_eq!(TcpStream::connect_timeout(&l.socket_addr().unwrap(),Duration::from_millis(3)).unwrap_err().kind(),ErrorKind::TimedOut);service::POLL_DELAY.store(0,Ordering::Relaxed);assert_eq!(service::live_sockets(),before+1);drop(l.accept().unwrap());assert_eq!(service::live_sockets(),before);}
#[test]fn revocation_while_waiting_is_propagated_without_reading(){let(_l,a,_b)=pair();service::READS.store(0,Ordering::Relaxed);service::REVOKE_READ.store(true,Ordering::Relaxed);assert_eq!(a.read(&mut[0]).unwrap_err().kind(),ErrorKind::NotConnected);assert!(service::READS.load(Ordering::Relaxed)>=2);service::REVOKE_READ.store(false,Ordering::Relaxed);}
#[test]fn invalid_native_count_is_rejected(){let(_l,a,_b)=pair();service::HUGE_READ.store(true,Ordering::Relaxed);assert_eq!(a.read(&mut[0]).unwrap_err().kind(),ErrorKind::InvalidData);service::HUGE_READ.store(false,Ordering::Relaxed);}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True)
 native=SOURCE.read_text().replace('//!','//')
 boundary=BOUNDARY.read_text().replace('//!','//')
 boundary=boundary.replace('let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let bits=match &s.kind{','POLL_CALLS.fetch_add(1,AtomicOrdering::Relaxed);if POLL_DELAY.fetch_update(AtomicOrdering::Relaxed,AtomicOrdering::Relaxed,|n|n.checked_sub(1)).is_ok(){return 0;}if READY_ERROR.load(AtomicOrdering::Relaxed){return 6;}let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let bits=match &s.kind{')
 boundary=boundary.replace('fn trueos_mio_socket_take_error(id:u32)->i32{if ', 'fn trueos_mio_socket_take_error(id:u32)->i32{if READY_ERROR.load(AtomicOrdering::Relaxed){return -3;}if ')
 boundary=boundary.replace('unsafe{read(id,out,cap,false)}','if HUGE_READ.load(AtomicOrdering::Relaxed){return cap as isize+1;}let count=READS.fetch_add(1,AtomicOrdering::Relaxed);if REVOKE_READ.load(AtomicOrdering::Relaxed)&&count>0{return -3;}unsafe{read(id,out,cap,false)}')
 source=HARNESS.replace('BOUNDARY',boundary).replace('NATIVE',native)
 fixture=out/'fixture.rs';fixture.write_text(source);checks=[]
 rustc=['rustup','run',TOOLCHAIN,'rustc'];env=os.environ.copy();env['RUSTC_BOOTSTRAP']='1'
 for mode,opts in [('debug',[]),('release',['-O'])]:
  exe=out/f'tests-{mode}';command=rustc+['--edition=2024','--test',str(fixture),'-o',str(exe),*opts]
  log=out/f'{mode}.log'
  with log.open('w')as f:
   r=subprocess.run(command,env=env,stdout=f,stderr=subprocess.STDOUT);assert r.returncode==0,log.read_text()[-14000:]
   r=subprocess.run([str(exe),'--test-threads=1'],env=env,stdout=f,stderr=subprocess.STDOUT,timeout=30);assert r.returncode==0,log.read_text()[-14000:]
  count=int(re.search(r'test result: ok\. (\d+) passed',log.read_text())[1]);assert count==16
  checks.append({'mode':mode,'cases':count,'command':command,'log':str(log),'binary':info(exe)})
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SOURCE),info(BOUNDARY),info(Path(__file__))],'fixture':info(fixture),'limits':['Uses existing native ABI simulator; no VM or real packet proof.','Actual custom-target std compilation is recorded separately.','Fault statuses/readiness are explicitly injected at simulator boundary; kernel policy enforcement has separate actual adapter proofs.']},indent=2)+'\n')
 print('16 debug + 16 release cases passed');print(out/'receipt.json')
if __name__=='__main__':main()
