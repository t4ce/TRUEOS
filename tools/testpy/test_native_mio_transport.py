#!/usr/bin/env python3
"""Run actual native socket-option and datagram bodies with real smoltcp sockets.

This is a host contract test: service discovery, guest execution and memory copy
boundaries are instrumented. Transport option storage is the production smoltcp
implementation. Guest UDP marshalling and host dispatch execute source bodies
unchanged. No NIC, VMX, builtin publication or rig reset is exercised.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

from test_native_mio_readiness import item, constant
from test_native_mio import environment

ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']


def function(source, name):
    return item(source, rf'(?:pub(?:\(crate\))? )?(?:unsafe )?(?:extern "C" )?fn {name}\(')


HARNESS = r'''#![allow(dead_code, unsafe_op_in_unsafe_fn)]
extern crate alloc;
extern crate self as trueos_vm;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use core::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
#[path=@VNET@] mod api;
thread_local! {
 static OWNER: Cell<Option<u8>> = const { Cell::new(None) };
 static HULL: Cell<Option<u8>> = const { Cell::new(None) };
 static CALL_FAIL: Cell<bool> = const { Cell::new(false) };
 static RESPONSE: Cell<(u64,u64,u32)> = const { Cell::new((0,0,0)) };
 static PAGE: std::cell::UnsafeCell<Page> = const { std::cell::UnsafeCell::new(Page{payload:[0;128]}) };
 static RESPONSE_HISTORY: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}
struct Page { payload:[u8;128] }
#[derive(Debug)] enum DispatchOutcome { Resume }
fn host_ptr(_:u8)->Option<*mut Page>{Some(PAGE.with(|p|p.get()))}
fn write_response(_:u8,_:u32,status:u64,data:u64,len:u32){RESPONSE.with(|r|r.set((status,data,len)));RESPONSE_HISTORY.with(|r|r.borrow_mut().push(len));}
mod hv {
 pub fn current_hull_guest_context_vm_id()->Option<u8>{super::HULL.with(|v|v.get())}
 pub fn current_guest_execution_context_vm_id()->Option<u8>{super::OWNER.with(|v|v.get())}
 pub fn with_guest_broker_context<T>(vm:u8,f:impl FnOnce()->T)->T{
  let owner=super::OWNER.with(|v|v.replace(Some(vm)));let hull=super::HULL.with(|v|v.replace(None));
  let value=f();super::OWNER.with(|v|v.set(owner));super::HULL.with(|v|v.set(hull));value
 }
}
pub mod vmcall {
 use super::*;
 pub const STATUS_OK:u64=0; pub const STATUS_BAD_ARG:u64=1; pub const PAYLOAD_CAP:usize=128;
 @VM_CONSTANTS@
 pub fn call(op:u32,a:u64,b:u64)->(u64,u64){let mut out=[];call_with_payload(op,a,b,&[],&mut out)}
 pub fn call_with_payload(op:u32,a:u64,b:u64,input:&[u8],out:&mut[u8])->(u64,u64){
  if CALL_FAIL.with(|v|v.get()){return (STATUS_BAD_ARG,0);}
  let vm=HULL.with(|v|v.get()).expect("guest call without Hull identity");
  PAGE.with(|p|unsafe{(*p.get()).payload.fill(0);(&mut (*p.get()).payload)[..input.len()].copy_from_slice(input)});
  dispatch(vm,op,a,b);let (status,data,len)=RESPONSE.with(|v|v.get());
  PAGE.with(|p|unsafe{let n=out.len().min(len as usize);out[..n].copy_from_slice(&(&(*p.get()).payload)[..n]);});(status,data)
 }
}
use vmcall::*;
const MIO_ADDR_BYTES:usize=core::mem::size_of::<mio_compat::TrueosMioSocketAddr>();
@WRITE_MIO_ADDR@
fn dispatch(vm_id:u8,op:u32,arg0:u64,arg1:u64)->DispatchOutcome{
 let seq=1;
 match op {
  @OPTION_DISPATCH@
  @UDP_DISPATCH@
  _=>panic!("unexpected native operation")
 }
}
mod net { pub mod adapter {
 use smoltcp::socket::{tcp,udp};
 use smoltcp::iface::{SocketHandle,SocketSet};
 #[derive(Clone,Copy,PartialEq)] pub enum SocketKind{Tcp,Udp,Tun}
 #[derive(Clone,Copy,PartialEq)] pub struct NetHandle(pub u32);
 struct SocketRecord{handle:NetHandle,kind:SocketKind,socket:SocketHandle}
 struct NetService{records:Vec<SocketRecord>,sockets:SocketSet<'static>}
 static SERVICE:std::sync::OnceLock<spin::Mutex<NetService>>=std::sync::OnceLock::new();
 fn service_for_device(device:usize)->Option<&'static spin::Mutex<NetService>>{if device==0{SERVICE.get()}else{None}}
 pub fn reset(){
  let mut sockets=SocketSet::new(Vec::new());
  let tcp=sockets.add(tcp::Socket::new(tcp::SocketBuffer::new(vec![0;64]),tcp::SocketBuffer::new(vec![0;64])));
  let udp=sockets.add(udp::Socket::new(udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY;4],vec![0;64]),udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY;4],vec![0;64])));
  let state=NetService{records:vec![SocketRecord{handle:NetHandle(100),kind:SocketKind::Tcp,socket:tcp},SocketRecord{handle:NetHandle(200),kind:SocketKind::Udp,socket:udp}],sockets};
  let service=SERVICE.get_or_init(||spin::Mutex::new(NetService{records:Vec::new(),sockets:SocketSet::new(Vec::new())}));*service.lock()=state;
 }
 @ADAPTER_OPTION@
 pub fn transport_values()->(bool,u8,u8){let s=SERVICE.get().unwrap().lock();(!s.sockets.get::<tcp::Socket>(s.records[0].socket).nagle_enabled(),s.sockets.get::<tcp::Socket>(s.records[0].socket).hop_limit().unwrap_or(64),s.sockets.get::<udp::Socket>(s.records[1].socket).hop_limit().unwrap_or(64))}
} }
mod mio_compat {
 use super::*;
 @MIO_CONSTANTS@
 static MIO_RX_DEVICE_INDEX:AtomicUsize=AtomicUsize::new(0);
 #[derive(Clone,Copy,Default,Debug,PartialEq)] #[repr(C)] @RAW_ADDR@
 #[derive(Clone,Copy)] @COMPAT_ADDR@
 impl CompatAddr {@FROM_RAW@ @TO_RAW@}
 #[derive(Clone,Copy,PartialEq)] @SOCKET_KIND@
 #[derive(Clone,Copy)] @SOCKET_SETUP@
 @SOCKET_STATE@
 struct MioCompat{sockets:Vec<MioSocketState>}
 impl MioCompat {
  fn pump(&mut self){} fn kick_net(&mut self){}
  @SOCKET_LOOKUP@
  @SOCKET_MUT@
  @SOCKET_MUT_OWNER@
 }
 thread_local!{static COMPAT:RefCell<MioCompat>=const{RefCell::new(MioCompat{sockets:Vec::new()})};}
 fn with_compat<T>(f:impl FnOnce(&mut MioCompat)->T)->T{COMPAT.with(|c|f(&mut c.borrow_mut()))}
 fn copy_mio_addr_to_active_guest(_:u8,out:*mut TrueosMioSocketAddr,addr:TrueosMioSocketAddr)->bool{unsafe{out.write(addr)};true}
 fn copy_to_active_guest(_:u8,out:*mut u8,bytes:&[u8])->bool{if !bytes.is_empty(){unsafe{core::ptr::copy_nonoverlapping(bytes.as_ptr(),out,bytes.len())}};true}
 @MIO_FUNCTIONS@
 pub fn reset(){
  OWNER.with(|v|v.set(None));HULL.with(|v|v.set(None));CALL_FAIL.with(|v|v.set(false));RESPONSE_HISTORY.with(|v|v.borrow_mut().clear());
  net::adapter::reset();MIO_RX_DEVICE_INDEX.store(0,AtomicOrdering::Relaxed);
  with_compat(|c|{c.sockets.clear();for (id,owner,kind,handle) in [(10,None,MioSocketKind::TcpStream,100),(20,Some(1),MioSocketKind::Udp,200),(21,Some(2),MioSocketKind::Udp,200),(30,Some(1),MioSocketKind::TcpListener,100)]{
   c.sockets.push(MioSocketState{id,owner_vm:owner,progress_generation:1,kind,setup:None,handle:Some(api::NetHandle(handle)),listen_handles:Vec::new(),local:None,peer:None,listen_port:None,connected:true,closed:false,read_shutdown:false,write_shutdown:false,peer_eof:false,error:STATUS_OK,tx_in_flight:0,rx_stream:VecDeque::new(),rx_dgrams:VecDeque::new(),accept_queue:VecDeque::new()});
  }});
 }
 pub fn datagram(bytes:&[u8],v6:bool){with_compat(|c|c.sockets[1].rx_dgrams.push_back((if v6{CompatAddr::V6{addr:[7;16],port:8080}}else{CompatAddr::V4{addr:[10,1,2,3],port:8080}},bytes.to_vec())));}
 pub fn queued()->usize{with_compat(|c|c.sockets[1].rx_dgrams.len())}
 pub fn test_peer_predicate(){
  let v4=CompatAddr::V4{addr:[10,1,2,3],port:8080};let v6=CompatAddr::V6{addr:[7;16],port:8080};
  for from in [v4,v6]{assert!(udp_peer_matches(None,from));assert!(udp_peer_matches(Some(from),from));}
  assert!(!udp_peer_matches(Some(v4),v6));assert!(!udp_peer_matches(Some(v6),v4));
  assert!(!udp_peer_matches(Some(v4),CompatAddr::V4{addr:[10,1,2,4],port:8080}));assert!(!udp_peer_matches(Some(v4),CompatAddr::V4{addr:[10,1,2,3],port:8081}));
  assert!(!udp_peer_matches(Some(v6),CompatAddr::V6{addr:[8;16],port:8080}));assert!(!udp_peer_matches(Some(v6),CompatAddr::V6{addr:[7;16],port:8081}));
 }

 pub fn change(id:u32,which:u8){with_compat(|c|{let s=c.sockets.iter_mut().find(|s|s.id==id).unwrap();match which{0=>s.closed=true,1=>s.error=-6,2=>s.handle=None,_=>unreachable!()}});}
}
use mio_compat::*;
#[test] fn options_change_real_smoltcp_transport_and_read_it_back(){
 reset();assert_eq!(trueos_mio_socket_option(10,1,-1),0);assert_eq!(trueos_mio_socket_option(10,1,1),1);assert_eq!(trueos_mio_socket_option(10,2,42),42);
 OWNER.with(|v|v.set(Some(1)));assert_eq!(trueos_mio_socket_option(20,2,17),17);assert_eq!(net::adapter::transport_values(),(true,42,17));
 HULL.with(|v|v.set(Some(1)));assert_eq!(trueos_mio_socket_option(20,2,-1),17);assert_eq!(trueos_mio_socket_option(20,2,255),255);assert_eq!(net::adapter::transport_values().2,255);
}
#[test] fn option_ranges_and_owner_namespaces_are_enforced(){
 reset();for value in [i32::MIN,-2,0,256,i32::MAX]{assert_eq!(trueos_mio_socket_option(10,2,value),-4);}assert_eq!(trueos_mio_socket_option(10,1,2),-4);assert_eq!(trueos_mio_socket_option(10,99,0),-1);
 assert_eq!(trueos_mio_socket_option(20,2,99),-5);OWNER.with(|v|v.set(Some(1)));assert_eq!(trueos_mio_socket_option(21,2,99),-5);assert_eq!(trueos_mio_socket_option(10,2,99),-5);assert_eq!(trueos_mio_socket_option(30,2,99),-1);
 HULL.with(|v|v.set(Some(1)));assert_eq!(trueos_mio_socket_option(21,2,99),-5);assert_eq!(net::adapter::transport_values(),(false,64,64));
}
#[test] fn pending_closed_failed_and_missing_sockets_never_succeed(){
 for (which,status) in [(0,-3),(1,-6),(2,-2)]{reset();change(10,which);assert_eq!(trueos_mio_socket_option(10,1,1),status);assert_eq!(net::adapter::transport_values(),(false,64,64));}
 reset();assert_eq!(trueos_mio_socket_option(999,1,1),-5);
}
#[test] fn vm_option_dispatch_validates_socket_width_and_transport_failure(){
 reset();OWNER.with(|v|v.set(Some(1)));HULL.with(|v|v.set(Some(1)));CALL_FAIL.with(|v|v.set(true));assert_eq!(trueos_mio_socket_option(20,2,88),-4);CALL_FAIL.with(|v|v.set(false));
 hv::with_guest_broker_context(1,||{dispatch(1,OP_BP_MIO_SOCKET_OPTION,1u64<<32,2u64<<32|88);});assert_eq!(RESPONSE.with(|v|v.get()).1 as i64,-4);assert_eq!(net::adapter::transport_values().2,64);
}
#[test] fn udp_peek_is_bounded_preserves_source_and_does_not_consume(){
 reset();OWNER.with(|v|v.set(Some(1)));datagram(b"abcdef",false);let mut addr=TrueosMioSocketAddr::default();let mut buf=[0x55;8];
 for _ in 0..2{assert_eq!(unsafe{trueos_mio_udp_socket_peek_from(20,&mut addr,buf.as_mut_ptr(),3)},3);assert_eq!(queued(),1);assert_eq!(addr.family,4);assert_eq!(addr.port,8080);assert_eq!(&addr.addr[..4],&[10,1,2,3]);assert_eq!(&buf[..3],b"abc");assert_eq!(&buf[3..],&[0x55;5]);}
 assert_eq!(unsafe{trueos_mio_udp_socket_recv_from(20,&mut addr,buf.as_mut_ptr(),8)},6);assert_eq!(queued(),0);assert_eq!(&buf[..6],b"abcdef");assert_eq!(unsafe{trueos_mio_udp_socket_peek_from(20,&mut addr,buf.as_mut_ptr(),8)},-2);
}
#[test] fn empty_datagrams_and_zero_capacity_preserve_guest_source_address(){
 for (data,cap) in [(&b""[..],8usize),(&b"payload"[..],0)]{reset();OWNER.with(|v|v.set(Some(1)));HULL.with(|v|v.set(Some(1)));datagram(data,true);let mut addr=TrueosMioSocketAddr::default();let mut buf=[0x55;8];
  for peek in [true,false]{let n=unsafe{if peek{trueos_mio_udp_socket_peek_from(20,&mut addr,buf.as_mut_ptr(),cap)}else{trueos_mio_udp_socket_recv_from(20,&mut addr,buf.as_mut_ptr(),cap)}};assert_eq!(n,0);assert_eq!(addr.family,6);assert_eq!(addr.addr,[7;16]);assert_eq!(addr.port,8080);assert_eq!(buf,[0x55;8]);assert_eq!(queued(),usize::from(peek));}
  assert_eq!(RESPONSE_HISTORY.with(|v|v.borrow().clone()),[20,20]);
 }
}
#[test] fn guest_udp_peek_marshalling_preserves_queue_and_bounds(){
 reset();OWNER.with(|v|v.set(Some(1)));HULL.with(|v|v.set(Some(1)));datagram(b"wirepacket",true);let mut addr=TrueosMioSocketAddr::default();let mut buf=[0x55;16];
 assert_eq!(unsafe{trueos_mio_udp_socket_peek_from(20,&mut addr,buf.as_mut_ptr(),4)},4);assert_eq!(&buf[..4],b"wire");assert_eq!(&buf[4..],&[0x55;12]);assert_eq!(queued(),1);assert_eq!(addr.addr,[7;16]);
 assert_eq!(unsafe{trueos_mio_udp_socket_recv_from(20,&mut addr,buf.as_mut_ptr(),16)},10);assert_eq!(&buf[..10],b"wirepacket");assert_eq!(queued(),0);assert_eq!(RESPONSE_HISTORY.with(|v|v.borrow().clone()),[24,30]);
}
#[test] fn udp_invalid_pointers_kind_and_owner_do_not_consume(){
 reset();OWNER.with(|v|v.set(Some(1)));datagram(b"keep",false);let mut addr=TrueosMioSocketAddr::default();let mut buf=[0x55;8];
 for (id,out,cap) in [(20,core::ptr::null_mut(),8),(21,buf.as_mut_ptr(),8),(30,buf.as_mut_ptr(),8)]{assert!(unsafe{trueos_mio_udp_socket_peek_from(id,&mut addr,out,cap)}<0);assert_eq!(queued(),1);}
 assert_eq!(unsafe{trueos_mio_udp_socket_recv_from(20,&mut addr,core::ptr::null_mut(),0)},0);assert_eq!(queued(),0);
}
#[test] fn connected_udp_filters_full_peer_and_purges_old_queue_on_reconnect(){
 reset();OWNER.with(|v|v.set(Some(1)));datagram(b"match",false);datagram(b"other family",true);
 let mut peer=TrueosMioSocketAddr{family:4,reserved:0,port:8080,addr:[0;16]};peer.addr[..4].copy_from_slice(&[10,1,2,3]);
 assert_eq!(unsafe{mio_udp_socket_connect_host(20,peer)},0);assert_eq!(queued(),1);
 let mut out=TrueosMioSocketAddr::default();let mut buf=[0;16];assert_eq!(unsafe{trueos_mio_udp_socket_peek_from(20,&mut out,buf.as_mut_ptr(),16)},5);assert_eq!(&buf[..5],b"match");
 assert_eq!(out,peer);
 peer.family=6;peer.addr=[7;16];assert_eq!(unsafe{mio_udp_socket_connect_host(20,peer)},0);assert_eq!(queued(),0);
 mio_compat::test_peer_predicate();change(20,0);assert_eq!(unsafe{mio_udp_socket_connect_host(20,peer)},-3);
}
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    output=parser.parse_args().output.resolve();output.mkdir(parents=True,exist_ok=True)
    paths={name:ROOT/path for name,path in {'mio':'src/mio_compat.rs','adapter':'src/net/adapter.rs','dispatch':'src/hv/vmcall.rs','protocol':'crates/trueos-vm/src/vmcall.rs'}.items()}
    sources={name:path.read_text() for name,path in paths.items()}; fragments={}
    def keep(name,source):fragments[name]=source;return source
    subs={'@VNET@':json.dumps(str(ROOT/'crates/trueos-v/src/vnet.rs')),
      '@VM_CONSTANTS@':'\n'.join(keep(name,constant(sources['protocol'],name)) for name in ['OP_BP_MIO_SOCKET_OPTION','OP_BP_MIO_UDP_SOCKET_RECV_FROM','MIO_UDP_READ_PEEK']),
      '@ADAPTER_OPTION@':keep('adapter::socket_option',function(sources['adapter'],'socket_option')),
      '@OPTION_DISPATCH@':keep('option dispatch',item(sources['dispatch'],'OP_BP_MIO_SOCKET_OPTION => \\{')),
      '@UDP_DISPATCH@':keep('UDP dispatch',item(sources['dispatch'],'OP_BP_MIO_UDP_SOCKET_RECV_FROM => \\{')),
      '@WRITE_MIO_ADDR@':keep('write_mio_addr',function(sources['dispatch'],'write_mio_addr')),
      '@MIO_CONSTANTS@':'\n'.join(keep(name,constant(sources['mio'],name)) for name in ['STATUS_OK','STATUS_INVALID_INPUT','STATUS_NOT_FOUND','STATUS_NOT_CONNECTED','STATUS_WOULD_BLOCK','STATUS_UNSUPPORTED']),
      '@RAW_ADDR@':keep('TrueosMioSocketAddr',item(sources['mio'],'pub struct TrueosMioSocketAddr')),
      '@COMPAT_ADDR@':keep('CompatAddr',item(sources['mio'],'enum CompatAddr')),
      '@FROM_RAW@':keep('CompatAddr::from_raw',function(sources['mio'],'from_raw')),
      '@TO_RAW@':keep('CompatAddr::to_raw',function(sources['mio'],'to_raw')),
      '@SOCKET_KIND@':keep('MioSocketKind',item(sources['mio'],'enum MioSocketKind')),
      '@SOCKET_STATE@':keep('MioSocketState',item(sources['mio'],'struct MioSocketState')),
      '@SOCKET_SETUP@':keep('TcpSocketSetup',item(sources['mio'],'struct TcpSocketSetup')),
      '@SOCKET_LOOKUP@':keep('socket',function(sources['mio'],'socket')),
      '@SOCKET_MUT@':keep('socket_mut',function(sources['mio'],'socket_mut')),
      '@SOCKET_MUT_OWNER@':keep('socket_mut_for_owner',function(sources['mio'],'socket_mut_for_owner')),
      '@MIO_FUNCTIONS@':'\n'.join(keep(name,function(sources['mio'],name)) for name in ['current_owner_vm','owner_matches','vm_guest_vmcall_active','vmcall_i32','vmcall_isize','read_addr','udp_peer_matches','mio_udp_socket_connect_host','mio_socket_option_host','trueos_mio_socket_option','mio_udp_socket_receive_host','mio_udp_socket_recv_from_host','mio_udp_socket_receive','trueos_mio_udp_socket_recv_from','trueos_mio_udp_socket_peek_from'])}
    harness=HARNESS
    for key,value in subs.items():harness=harness.replace(key,value)
    assert not re.findall(r'@[A-Z_]+@',harness)
    (output/'production.rs').write_text(harness)
    env=environment(output/'target')
    checks=[]
    with tempfile.TemporaryDirectory(prefix='trueos-mio-transport-') as work:
      work=Path(work);(work/'src').mkdir();(work/'src/lib.rs').write_text(harness)
      (work/'Cargo.toml').write_text(f'''[package]\nname="trueos-native-transport-contract"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nspin="0.10"\nsmoltcp={{path={json.dumps(str(ROOT/'vendor/smoltcp-0.13.1'))},default-features=false,features=["alloc","proto-ipv4","proto-ipv6","socket-tcp","socket-udp","medium-ethernet"]}}\n''')
      for mode in ['debug','release']:
       command=['rustup','run',TOOLCHAIN,'cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--manifest-path',str(work/'Cargo.toml')]
       if mode=='release':command+=['--release']
       command+=['--','--test-threads=1'];log=output/f'{mode}.log'
       with log.open('w') as stream:result=subprocess.run(command,env=env,cwd=work,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
       if result.returncode:raise RuntimeError(log.read_text()[-12000:])
       count=int(re.search(r'test result: ok\. (\d+) passed',log.read_text())[1]);assert count==9,count
       checks.append({'mode':mode,'tests':count,'exit':result.returncode,'log':str(log)})
    receipt={'scope':__doc__,'checks':checks,'source_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in paths.items()},'production_fragments':{name:hashlib.sha256(fragment.encode()).hexdigest() for name,fragment in fragments.items()},'harness_sha256':hashlib.sha256(harness.encode()).hexdigest()}
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print('Native Mio kernel transport: 9 production-body tests in debug and release, with real smoltcp option storage and guest UDP marshalling.')


if __name__=='__main__':main()
