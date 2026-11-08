//! Production socket lifecycle bodies over instrumented service/VM boundaries.
#![allow(dead_code, unused_variables, unsafe_op_in_unsafe_fn)]
extern crate alloc;
extern crate self as trueos_vm;
extern crate self as v;
pub mod vnet{pub const MAX_MSG:usize=128;}
use std::{cell::{Cell, RefCell}, collections::{BTreeMap, VecDeque}};
use core::{net::SocketAddr, sync::atomic::{AtomicUsize, Ordering as AtomicOrdering}};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum NetworkActor{Public,Vm(u8,u64)}
mod r{pub mod fs{pub mod trueosfs{pub mod authorized{pub(crate) use crate::NetworkActor as Actor;pub fn actor_for_vm(vm:u8)->Result<Actor,()>{Ok(Actor::Vm(vm,crate::EPOCHS.with(|e|e.get()[vm as usize])))}}}}}
mod access_key_db{
 use crate::NetworkActor;
 pub fn authorize_tcp_listen(actor:NetworkActor,local:core::net::SocketAddr)->Result<(),()>{if local.port()!=42116||!crate::RESERVED.with(|v|v.get()){return Ok(())}if local=="127.0.0.1:42116".parse().unwrap()&&actor==NetworkActor::Vm(2,crate::EPOCHS.with(|e|e.get()[2]))&&crate::ALLOWED.with(|v|v.get()){Ok(())}else{Err(())}}
 pub fn authorize_tcp_connect(client:NetworkActor,server:NetworkActor,port:u16)->Result<(),()>{if port!=42116||!crate::RESERVED.with(|v|v.get()){return Ok(())}if client==NetworkActor::Vm(1,crate::EPOCHS.with(|e|e.get()[1]))&&server==NetworkActor::Vm(2,crate::EPOCHS.with(|e|e.get()[2]))&&crate::ALLOWED.with(|v|v.get()){Ok(())}else{Err(())}}
}
#[path=@VNET@] mod api;
#[macro_export] macro_rules! log { ($($tt:tt)*) => {{}}; }
#[macro_export] macro_rules! log_info { ($($tt:tt)*) => {{}}; }
mod log_os { pub mod flags { pub const NET_LOG_TCP_FLOW:bool=false; pub const NET_LOG_TCP_CONNECT_STATES:bool=false; } }
mod allcaps { pub mod io { pub const DESCRIPTOR_SOFT_CAP:usize=64; } }
thread_local! {
 static OWNER:Cell<Option<u8>>=const{Cell::new(None)};
 static HULL:Cell<Option<u8>>=const{Cell::new(None)};
 static COPY_FAIL:Cell<bool>=const{Cell::new(false)};
 static NET_FAIL:Cell<bool>=const{Cell::new(false)};
 static SUBMIT_FAIL:Cell<bool>=const{Cell::new(false)};
 static OPTION_FAIL:Cell<bool>=const{Cell::new(false)};
 static OPTIONS:RefCell<BTreeMap<u32,(u32,u32)>>=const{RefCell::new(BTreeMap::new())};
 static RX_OWNERS:RefCell<BTreeMap<u32,Option<u8>>>=const{RefCell::new(BTreeMap::new())};
 static COMMANDS:RefCell<Vec<api::Command>>=const{RefCell::new(Vec::new())};
 static CONNECTS:RefCell<Vec<(SocketAddr,SocketAddr)>>=const{RefCell::new(Vec::new())};
 static ACTORS:RefCell<Vec<NetworkActor>>=const{RefCell::new(Vec::new())};
 static RESERVED:Cell<bool>=const{Cell::new(true)};
 static ALLOWED:Cell<bool>=const{Cell::new(true)};
 static EPOCHS:Cell<[u64;8]>=const{Cell::new([1;8])};
 static PAUSED:Cell<bool>=const{Cell::new(false)};
}
mod hv {
 pub fn current_hull_guest_context_vm_id()->Option<u8>{crate::HULL.with(Cell::get)}
 pub fn current_guest_execution_context_vm_id()->Option<u8>{crate::OWNER.with(Cell::get)}
 pub fn vm_run_generation(_:u8)->Option<u64>{Some(9)}
 pub fn guest_pause_requested(_:u8)->bool{false}
 pub fn with_guest_broker_context<T>(owner:u8,f:impl FnOnce()->T)->T{
  let old=crate::OWNER.with(|v|v.replace(Some(owner)));let hull=crate::HULL.with(|v|v.replace(None));let result=f();crate::OWNER.with(|v|v.set(old));crate::HULL.with(|v|v.set(hull));result
 }
 use std::cell::Cell;
}
pub mod vmcall {
 pub const STATUS_OK:u64=0;
 @OP_CONSTANTS@
}
mod net { pub mod adapter {
 #[derive(Clone,Copy)]pub struct NetHandle(pub u32);
 pub fn socket_option(_:usize,handle:NetHandle,option:u32,value:Option<u32>)->Result<u32,i32>{
  if crate::OPTION_FAIL.with(|v|v.get()){return Err(-6);}
  crate::OPTIONS.with(|m|{let mut m=m.borrow_mut();let state=m.entry(handle.0).or_insert((0,64));let slot=match option{1=>&mut state.0,2=>&mut state.1,_=>return Err(-1)};if let Some(value)=value{*slot=value;}Ok(*slot)})
 }
} }
#[derive(Debug)]enum NetCommand{OpenTcpConnectFrom{local:SocketAddr,remote:SocketAddr},OpenTcpConnectForActor{local:Option<SocketAddr>,remote:SocketAddr,actor:NetworkActor}}
struct FakeNet;
impl FakeNet {
 fn submit(&self,cmd:api::Command)->Result<(),()>{if SUBMIT_FAIL.with(Cell::get){Err(())}else{COMMANDS.with(|c|c.borrow_mut().push(cmd));Ok(())}}
 fn submit_for_actor(&self,cmd:api::Command,actor:NetworkActor)->Result<(),()>{ACTORS.with(|v|v.borrow_mut().push(actor));self.submit(cmd)}
 @BRIDGE_CONNECT@
 @BRIDGE_ACTOR_CONNECT@
 fn submit_kernel(&self,cmd:NetCommand)->Result<(),()>{
  if SUBMIT_FAIL.with(Cell::get){return Err(());}let(local,remote)=match cmd{NetCommand::OpenTcpConnectFrom{local,remote}=>(local,remote),NetCommand::OpenTcpConnectForActor{local:Some(local),remote,..}=>(local,remote),_=>panic!("bound connect expected")};CONNECTS.with(|c|c.borrow_mut().push((local,remote)));Ok(())
 }
}
mod mio_compat {
 use super::*;
 #[path=@PROGRESS@] mod progress;
 @MIO_CONSTANTS@
 static MIO_RX_DEVICE_INDEX:AtomicUsize=AtomicUsize::new(0);
 const MIO_ADDR_BYTES:usize=20;
 #[repr(C)]#[derive(Clone,Copy,Default,Debug)] @RAW_ADDR@
 #[derive(Clone,Copy)] @ADDR@
 @ADDR_IMPL@
 #[derive(Clone,Copy,PartialEq,Eq)] @KIND@
 #[derive(Clone,Copy)] @SETUP@
 @STATE@
 @PENDING@
 struct MioCompat{net:Option<FakeNet>,sockets:Vec<MioSocketState>,pending_opens:VecDeque<PendingOpen>,next_socket_id:u32,tcp_listen_next_ephemeral:u16,progress:[progress::Counters;4]}
 impl MioCompat {
  fn new()->Self{Self{net:None,sockets:Vec::new(),pending_opens:VecDeque::new(),next_socket_id:1,tcp_listen_next_ephemeral:50_000,progress:[progress::Counters::new();4]}}
  fn ensure_net(&mut self)->Result<(),i32>{if NET_FAIL.with(Cell::get){return Err(STATUS_NO_DEVICE);}self.net.get_or_insert(FakeNet);Ok(())}
  fn pump(&mut self){}fn kick_net(&mut self){}
  @METHODS@
 }
 thread_local!{static COMPAT:RefCell<MioCompat>=RefCell::new(MioCompat::new());}
 fn with_compat<T>(f:impl FnOnce(&mut MioCompat)->T)->T{COMPAT.with(|c|f(&mut c.borrow_mut()))}
 fn current_owner_vm()->Option<u8>{OWNER.with(Cell::get)}
 pub fn tcp_receive_paused(_:usize,_:u32)->bool{PAUSED.with(Cell::get)}
 @CAPTURE_NETWORK_ACTOR@
 @OWNER_MATCHES@
 @SOCKET_HAS_HANDLE@
 @UDP_PEER_MATCHES@
 fn vm_guest_vmcall_active()->bool{HULL.with(Cell::get).is_some()}
 fn publish_rx_owner(handle:u32,owner:Option<u8>)->bool{RX_OWNERS.with(|m|m.borrow_mut().insert(handle,owner));true}
 fn compat_progress(_: &mut [progress::Counters],_: (Option<u8>,u64))->Option<&mut progress::Counters>{None}
 fn should_log_tcp_rx_queue_growth(_:usize,_:usize)->bool{false}
 fn tcp_flow_logging_enabled(_: &MioSocketState)->bool{false}
 fn log_tcp_endpoint(_: &str,_:u32,_:u32,_:CompatAddr){}
 fn log_mio_pending_endpoint(_: &str,_:&str,_:usize,_:&MioSocketState){}
 fn copy_u32_to_guest_stack(_:u8,out:*mut u32,value:u32)->bool{if COPY_FAIL.with(Cell::get){false}else{unsafe{out.write(value)};true}}
 fn addr_bytes(addr:&TrueosMioSocketAddr)->&[u8]{unsafe{core::slice::from_raw_parts((addr as *const TrueosMioSocketAddr).cast(),20)}}
 fn guest_mio_socket_out(op:u32,addr:TrueosMioSocketAddr,out:*mut u32)->i32{
  let owner=HULL.with(Cell::get).unwrap();assert_eq!(op,vmcall::OP_BP_MIO_TCP_SOCKET_CREATE);let mut id=0;
  let status=hv::with_guest_broker_context(owner,||unsafe{mio_tcp_socket_create_host(u32::from(addr.family),&mut id)});if status==0{unsafe{out.write(id)}}status
 }
 fn guest_mio_signed_payload(op:u32,id:u64,backlog:u64,payload:&[u8])->isize{
  let owner=HULL.with(Cell::get).unwrap();hv::with_guest_broker_context(owner,||{let Ok(id)=u32::try_from(id)else{return -4};match op{
   vmcall::OP_BP_MIO_TCP_SOCKET_BIND|vmcall::OP_BP_MIO_TCP_SOCKET_CONNECT=>{let Some(addr)=read_addr(payload)else{return -4};if op==vmcall::OP_BP_MIO_TCP_SOCKET_BIND{mio_tcp_socket_bind_host(id,addr)as isize}else{mio_tcp_socket_connect_host(id,addr)as isize}},
   vmcall::OP_BP_MIO_TCP_SOCKET_LISTEN=>match u32::try_from(backlog){Ok(backlog)=>mio_tcp_socket_listen_host(id,backlog)as isize,Err(_)=>-4},_=>panic!("unexpected guest op")}})
 }
 @READ_ADDR@
 @CLOSE@
 @OPTION@
 @ACCEPT@
 #[path=@TCP_SOCKET@] mod tcp_socket;
 pub(crate) use tcp_socket::*;

 fn reset(){OWNER.with(|v|v.set(None));HULL.with(|v|v.set(None));for flag in [&COPY_FAIL,&NET_FAIL,&SUBMIT_FAIL,&OPTION_FAIL]{flag.with(|v|v.set(false));}OPTIONS.with(|v|v.borrow_mut().clear());RX_OWNERS.with(|v|v.borrow_mut().clear());COMMANDS.with(|v|v.borrow_mut().clear());CONNECTS.with(|v|v.borrow_mut().clear());ACTORS.with(|v|v.borrow_mut().clear());RESERVED.with(|v|v.set(true));ALLOWED.with(|v|v.set(true));EPOCHS.with(|v|v.set([1;8]));with_compat(|c|*c=MioCompat::new());}
 fn create(family:u32)->u32{let mut id=0;assert_eq!(unsafe{mio_tcp_socket_create_host(family,&mut id)},0);id}
 fn v4(ip:[u8;4],port:u16)->TrueosMioSocketAddr{let mut raw=TrueosMioSocketAddr{family:4,port,..Default::default()};raw.addr[..4].copy_from_slice(&ip);raw}
 fn v6(port:u16)->TrueosMioSocketAddr{TrueosMioSocketAddr{family:6,addr:[1;16],port,..Default::default()}}
 fn local(id:u32)->TrueosMioSocketAddr{with_compat(|c|c.socket(id).unwrap().local.unwrap().to_raw())}
 fn opened(handle:u32){with_compat(|c|c.handle_event(api::Event::Opened{handle:api::NetHandle(handle),kind:api::SocketKind::Tcp}));}
 fn establish(handle:u32){with_compat(|c|c.handle_event(api::Event::TcpEstablished{handle:api::NetHandle(handle),peer:Some(api::EndpointV4{addr:[10,0,0,2],port:80}),peer6:None}));}
 fn pending(id:u32)->usize{with_compat(|c|c.pending_opens_for_socket(id,api::SocketKind::Tcp))}
 fn live()->usize{with_compat(|c|c.sockets.len())}

 #[test]fn create_validates_output_family_capacity_and_execution_owner(){reset();let mut out=123;for family in [0,1,5,7,u32::MAX]{assert_eq!(unsafe{mio_tcp_socket_create_host(family,&mut out)},-4);assert_eq!(out,123);}assert_eq!(unsafe{mio_tcp_socket_create_host(4,core::ptr::null_mut())},-4);OWNER.with(|o|o.set(Some(2)));let a=create(4);let b=create(6);with_compat(|c|{for id in [a,b]{let s=c.socket(id).unwrap();assert_eq!(s.owner_vm,Some(2));assert_eq!(s.progress_generation,9);assert!(s.kind==MioSocketKind::TcpSocket);}});for _ in 2..64{create(4);}assert_eq!(unsafe{mio_tcp_socket_create_host(4,&mut out)},-9);OWNER.with(|o|o.set(Some(1)));create(4);}
 #[test]fn reserve_is_immediate_wildcard_aware_family_specific_and_released_on_close(){reset();let a=create(4);let b=create(4);let c=create(6);assert_eq!(mio_tcp_socket_bind_host(a,v4([0;4],9000)),0);assert_eq!(mio_tcp_socket_bind_host(b,v4([10,0,0,1],9000)),-6);assert_eq!(mio_tcp_socket_bind_host(c,v6(9000)),0);assert_eq!(mio_tcp_socket_bind_host(a,v4([0;4],9001)),-4);assert_eq!(unsafe{mio_socket_close_host(a)},0);assert_eq!(mio_tcp_socket_bind_host(b,v4([10,0,0,1],9000)),0);}
 #[test]fn ephemeral_reservations_are_distinct_and_family_mismatch_is_not_a_bind(){reset();let a=create(4);let b=create(4);assert_eq!(mio_tcp_socket_bind_host(a,v6(0)),-4);assert_eq!(mio_tcp_socket_bind_host(a,v4([0;4],0)),0);assert_eq!(mio_tcp_socket_bind_host(b,v4([0;4],0)),0);assert!(local(a).port>=50_000);assert_ne!(local(a).port,local(b).port);}
 #[test]fn namespaces_block_bind_connect_listen_options_and_close(){reset();OWNER.with(|o|o.set(Some(1)));let a=create(4);OWNER.with(|o|o.set(Some(2)));assert_eq!(mio_tcp_socket_bind_host(a,v4([0;4],80)),-5);assert_eq!(mio_tcp_socket_connect_host(a,v4([10,0,0,2],80)),-5);assert_eq!(mio_tcp_socket_listen_host(a,1),-5);assert_eq!(mio_socket_option_host(a,1,1),-5);assert_eq!(unsafe{mio_socket_close_host(a)},-5);assert_eq!(live(),1);}
 #[test]fn connect_retains_id_local_endpoint_nodelay_and_private_native_command(){reset();let id=create(4);let from=v4([10,0,0,1],55001);let to=v4([10,0,0,2],80);assert_eq!(mio_tcp_socket_bind_host(id,from),0);assert_eq!(mio_socket_option_host(id,1,1),1);assert_eq!(mio_tcp_socket_connect_host(id,to),0);assert_eq!(live(),1);assert_eq!(pending(id),1);CONNECTS.with(|c|assert_eq!(c.borrow().as_slice(),&[("10.0.0.1:55001".parse().unwrap(),"10.0.0.2:80".parse().unwrap())]));opened(700);assert_eq!(OPTIONS.with(|o|o.borrow()[&700].0),1);establish(700);assert_eq!(local(id).port,55001);with_compat(|c|{let s=c.socket(id).unwrap();assert!(s.connected);assert!(s.kind==MioSocketKind::TcpStream);assert_eq!(s.handle,Some(api::NetHandle(700)));});assert_eq!(mio_socket_option_host(id,2,37),37);assert_eq!(mio_socket_option_host(id,2,-1),37);}
 #[test]fn connect_automatically_reserves_but_rejects_zero_port_and_wrong_family(){reset();let id=create(4);assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],0)),-4);assert_eq!(mio_tcp_socket_connect_host(id,v6(80)),-4);assert_eq!(local(id).port,0);assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),0);assert_ne!(local(id).port,0);}
 #[test]fn enqueue_failure_restores_unconnected_ownership_and_keeps_reservation(){reset();let id=create(4);assert_eq!(mio_tcp_socket_bind_host(id,v4([0;4],55002)),0);SUBMIT_FAIL.with(|v|v.set(true));assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),-2);assert_eq!(pending(id),0);assert_eq!(local(id).port,55002);with_compat(|c|assert!(c.socket(id).unwrap().kind==MioSocketKind::TcpSocket));SUBMIT_FAIL.with(|v|v.set(false));assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),0);}
 #[test]fn network_absence_does_not_claim_a_connected_or_listening_transport(){reset();let id=create(4);NET_FAIL.with(|v|v.set(true));assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),-8);assert_eq!(mio_tcp_socket_listen_host(id,1),-8);with_compat(|c|assert!(c.socket(id).unwrap().kind==MioSocketKind::TcpSocket));}
 #[test]fn backlog_zero_means_one_and_excess_capacity_is_rejected(){reset();let id=create(4);assert_eq!(mio_tcp_socket_listen_host(id,33),-4);assert_eq!(local(id).port,0);assert_eq!(mio_tcp_socket_listen_host(id,0),0);assert_eq!(pending(id),1);with_compat(|c|assert_eq!(c.socket(id).unwrap().setup.unwrap().backlog,1));}
 #[test]fn pending_listens_and_queued_accepts_jointly_bound_backlog_and_accept_replenishes(){reset();let id=create(4);assert_eq!(mio_socket_option_host(id,1,1),1);assert_eq!(mio_tcp_socket_listen_host(id,2),0);assert_eq!(pending(id),2);opened(701);opened(702);assert_eq!(pending(id),0);assert_eq!(OPTIONS.with(|o|o.borrow()[&701].0),1);establish(701);establish(702);with_compat(|c|{let s=c.socket(id).unwrap();assert_eq!(s.accept_queue.len(),2);assert!(s.listen_handles.is_empty());});assert_eq!(pending(id),0);let mut child=0;let mut addr=TrueosMioSocketAddr::default();assert_eq!(unsafe{mio_tcp_listener_accept_host(id,&mut child,&mut addr)},0);assert_eq!(pending(id),1);assert_eq!(addr.port,80);assert_eq!(mio_socket_option_host(child,1,-1),1);}
 #[test]fn close_listener_discards_unaccepted_children_and_all_transports(){reset();let id=create(4);assert_eq!(mio_tcp_socket_listen_host(id,2),0);opened(701);opened(702);establish(701);assert_eq!(live(),2);assert_eq!(unsafe{mio_socket_close_host(id)},0);assert_eq!(live(),0);let mut closes=COMMANDS.with(|q|q.borrow().iter().filter_map(|c|match c{api::Command::Close{handle}=>Some(handle.0),_=>None}).collect::<Vec<_>>());closes.sort_unstable();assert_eq!(closes,[701,702]);}
 #[test]fn cancelled_pending_open_cannot_steal_a_later_sockets_transport(){reset();let a=create(4);let b=create(4);assert_eq!(mio_tcp_socket_connect_host(a,v4([10,0,0,2],80)),0);assert_eq!(unsafe{mio_socket_close_host(a)},0);assert_eq!(mio_tcp_socket_connect_host(b,v4([10,0,0,3],80)),0);opened(800);with_compat(|c|assert!(c.socket(b).unwrap().handle.is_none()));COMMANDS.with(|q|assert!(q.borrow().iter().any(|c|matches!(c,api::Command::Close{handle}if handle.0==800))));opened(801);with_compat(|c|assert_eq!(c.socket(b).unwrap().handle,Some(api::NetHandle(801))));}
 #[test]fn cancelled_pending_error_does_not_fail_a_later_connect(){reset();let a=create(4);let b=create(4);assert_eq!(mio_tcp_socket_connect_host(a,v4([10,0,0,2],80)),0);assert_eq!(unsafe{mio_socket_close_host(a)},0);assert_eq!(mio_tcp_socket_connect_host(b,v4([10,0,0,3],80)),0);with_compat(|c|c.handle_event(api::Event::Error{msg:"connect failed"}));with_compat(|c|assert_eq!(c.socket(b).unwrap().error,0));opened(801);with_compat(|c|assert_eq!(c.socket(b).unwrap().handle,Some(api::NetHandle(801))));}
 #[test]fn failing_later_enqueue_does_not_retire_an_earlier_open(){reset();let a=create(4);let b=create(4);assert_eq!(mio_tcp_socket_connect_host(a,v4([10,0,0,2],80)),0);SUBMIT_FAIL.with(|v|v.set(true));assert_eq!(mio_tcp_socket_connect_host(b,v4([10,0,0,3],80)),-2);SUBMIT_FAIL.with(|v|v.set(false));opened(801);with_compat(|c|{assert_eq!(c.socket(a).unwrap().handle,Some(api::NetHandle(801)));assert!(c.socket(b).unwrap().handle.is_none());});}
 #[test]fn rejected_local_address_becomes_terminal_error_instead_of_hanging_connect(){reset();let id=create(4);assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),0);with_compat(|c|c.handle_event(api::Event::Error{msg:"invalid local address"}));with_compat(|c|{let s=c.socket(id).unwrap();assert!(s.closed);assert_ne!(s.error,0);});assert_eq!(pending(id),0);}
 #[test]fn send_link_failure_does_not_retire_an_unrelated_pending_connect(){reset();let id=create(4);assert_eq!(mio_tcp_socket_connect_host(id,v4([10,0,0,2],80)),0);with_compat(|c|c.handle_event(api::Event::Error{msg:"link down"}));assert_eq!(pending(id),1);with_compat(|c|{let s=c.socket(id).unwrap();assert!(!s.closed);assert_eq!(s.error,0);});with_compat(|c|c.handle_event(api::Event::Error{msg:"tcp connect link down"}));assert_eq!(pending(id),0);with_compat(|c|assert!(c.socket(id).unwrap().closed));}
 #[test]fn failed_listener_pool_reports_error_only_when_all_opens_fail(){reset();let id=create(4);assert_eq!(mio_tcp_socket_listen_host(id,2),0);with_compat(|c|c.handle_event(api::Event::Error{msg:"tcp listen link down"}));with_compat(|c|assert!(!c.socket(id).unwrap().closed));with_compat(|c|c.handle_event(api::Event::Error{msg:"tcp listen link down"}));with_compat(|c|{let s=c.socket(id).unwrap();assert!(s.closed);assert_ne!(s.error,0);});}
 #[test]fn nodelay_change_reaches_current_listeners_and_future_replenished_transports(){reset();let id=create(4);assert_eq!(mio_tcp_socket_listen_host(id,2),0);opened(701);assert_eq!(mio_socket_option_host(id,1,1),1);assert_eq!(OPTIONS.with(|o|o.borrow()[&701].0),1);opened(702);assert_eq!(OPTIONS.with(|o|o.borrow()[&702].0),1);establish(701);let mut child=0;let mut addr=TrueosMioSocketAddr::default();assert_eq!(unsafe{mio_tcp_listener_accept_host(id,&mut child,&mut addr)},0);opened(703);assert_eq!(OPTIONS.with(|o|o.borrow()[&703].0),1);}
 #[test]fn failed_guest_output_copy_releases_new_native_owner(){reset();OWNER.with(|v|v.set(Some(1)));COPY_FAIL.with(|v|v.set(true));let mut out=123;assert_eq!(unsafe{trueos_mio_tcp_socket_create(4,&mut out)},-4);assert_eq!(out,123);assert_eq!(live(),0);}
 #[test]fn hull_wrappers_use_guest_namespace_and_preserve_reserved_endpoint(){reset();OWNER.with(|v|v.set(Some(2)));HULL.with(|v|v.set(Some(1)));let mut id=0;assert_eq!(unsafe{trueos_mio_tcp_socket_create(4,&mut id)},0);assert_eq!(trueos_mio_tcp_socket_bind(id,v4([0;4],55123)),0);assert_eq!(trueos_mio_tcp_socket_connect(id,v4([10,0,0,2],80)),0);with_compat(|c|{assert_eq!(c.socket(id).unwrap().owner_vm,Some(1));assert_eq!(local_raw(c,id).port,55123);});}
 fn local_raw(c:&MioCompat,id:u32)->TrueosMioSocketAddr{c.socket(id).unwrap().local.unwrap().to_raw()}
 #[test]fn shared_native_carrier_rearms_with_captured_listener_actor(){reset();OWNER.with(|v|v.set(Some(1)));let a=create(4);assert_eq!(mio_tcp_socket_listen_host(a,1),0);opened(701);OWNER.with(|v|v.set(Some(2)));let b=create(4);assert_eq!(mio_tcp_socket_listen_host(b,1),0);opened(702);establish(701);let mut child=0;let mut peer=TrueosMioSocketAddr::default();OWNER.with(|v|v.set(Some(1)));assert_eq!(unsafe{mio_tcp_listener_accept_host(a,&mut child,&mut peer)},0);ACTORS.with(|v|assert_eq!(v.borrow().as_slice(),&[NetworkActor::Vm(1,1),NetworkActor::Vm(2,1),NetworkActor::Vm(1,1)]));with_compat(|c|assert_eq!(c.socket(child).unwrap().actor,NetworkActor::Vm(1,1)));}
 #[test]fn native_private_bind_denies_before_reserving_any_endpoint(){reset();for who in [None,Some(1),Some(3),Some(4)]{OWNER.with(|v|v.set(who));let id=create(4);assert_eq!(mio_tcp_socket_bind_host(id,v4([127,0,0,1],42116)),STATUS_IO);assert_eq!(local(id).port,0);}OWNER.with(|v|v.set(Some(2)));let id=create(4);assert_eq!(mio_tcp_socket_bind_host(id,v4([0,0,0,0],42116)),STATUS_IO);assert_eq!(mio_tcp_socket_bind_host(id,v4([127,0,0,1],42116)),STATUS_OK);}
 #[test]fn preexisting_unauthorized_native_reservation_cannot_block_delegate(){reset();RESERVED.with(|v|v.set(false));let old=create(4);assert_eq!(mio_tcp_socket_bind_host(old,v4([0;4],42116)),STATUS_OK);RESERVED.with(|v|v.set(true));OWNER.with(|v|v.set(Some(2)));let correct=create(4);assert_eq!(mio_tcp_socket_bind_host(correct,v4([127,0,0,1],42116)),STATUS_OK);}
}

mod transport {
 use super::*;
 use smoltcp::{iface::{Config,Interface,SocketHandle,SocketSet},phy::{Loopback,Medium},socket::tcp,time::{Instant,Duration as SmolDuration},wire::{EthernetAddress,HardwareAddress,IpAddress,IpEndpoint,Ipv4Address,Ipv6Address}};
 const SOCKET_SOFT_CAP:usize=64;const TCP_RX_BUF_BYTES:usize=128;const TCP_TX_BUF_BYTES:usize=128;
 #[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct NetHandle(pub u32);
 #[derive(Clone,Copy,Debug,PartialEq,Eq)] @NET_ENDPOINT@
 #[derive(Clone,Copy,Debug,PartialEq,Eq)] @NET_ENDPOINT6@
 #[derive(Clone,Copy,Debug,PartialEq,Eq)] @SOCKET_KIND@
 #[path=@TCP_TX_QUEUE@]mod tcp_tx_queue;use tcp_tx_queue::TcpTxQueue;
 @SOCKET_RECORD@
 #[derive(Debug)]enum NetEvent{Opened{handle:NetHandle,kind:SocketKind},Error{msg:&'static str},Closed{handle:NetHandle},TcpData{handle:NetHandle,data:Vec<u8>},TcpSent{handle:NetHandle,len:usize},TcpReadClosed{handle:NetHandle}}
 thread_local!{static EVENTS:RefCell<Vec<(&'static str,NetEvent)>>=const{RefCell::new(Vec::new())};}
 fn push_event(owner:&'static str,event:NetEvent)->bool{EVENTS.with(|v|v.borrow_mut().push((owner,event)));true}
 enum TcpRoute{Network(Vec<u8>),Loopback(Result<(),()>)}
 fn is_ipv4_loopback(addr:[u8;4])->bool{addr[0]==127}
 struct NetService{device_index:usize,iface:Interface,sockets:SocketSet<'static>,records:Vec<SocketRecord>,local_ipv4:Option<Ipv4Address>,local_ipv6_ll:Ipv6Address,local_ipv6_global:Option<Ipv6Address>,tcp_next_ephemeral:u16,next:u32}
 impl NetService {
  fn new()->Self{RESERVED.with(|v|v.set(true));ALLOWED.with(|v|v.set(true));EPOCHS.with(|v|v.set([1;8]));PAUSED.with(|v|v.set(false));EVENTS.with(|v|v.borrow_mut().clear());let mut device=Loopback::new(Medium::Ethernet);let iface=Interface::new(Config::new(HardwareAddress::Ethernet(EthernetAddress([2,0,0,0,0,1]))),&mut device,Instant::from_millis(0));Self{device_index:0,iface,sockets:SocketSet::new(Vec::new()),records:Vec::new(),local_ipv4:Some(Ipv4Address::new(10,0,0,1)),local_ipv6_ll:Ipv6Address::new(0xfe80,0,0,0,0,0,0,1),local_ipv6_global:Some(Ipv6Address::new(0x2001,0xdb8,0,0,0,0,0,1)),tcp_next_ephemeral:49152,next:1}}
  fn require_link_up(&self)->Result<(),&'static str>{Ok(())}fn alloc_handle(&mut self)->NetHandle{let id=self.next;self.next+=1;NetHandle(id)}
  @OPEN_CONNECT@
  @OPEN_CONNECT6@
  @PRIVATE_ADAPTER_METHODS@
  fn process_tcp_connect_v4(&mut self,owner:&'static str,remote:NetEndpoint,bound:Option<core::net::SocketAddrV4>){self.open_tcp_connect(owner,remote,bound).unwrap();}
  fn route_command(&mut self,cmd:NetCommand){let owner="test";match cmd{@CONNECT_DISPATCH@ _=>panic!("unexpected fixture command")}}
 }
 fn endpoint(service:&NetService,handle:NetHandle)->(IpEndpoint,IpEndpoint){let r=service.records.iter().find(|r|r.handle==handle).unwrap();let s=service.sockets.get::<tcp::Socket>(r.socket);(s.local_endpoint().unwrap(),s.remote_endpoint().unwrap())}
 #[test]fn real_smoltcp_ipv4_connect_uses_reserved_port_and_validates_source_ip(){let mut s=NetService::new();let remote=NetEndpoint{addr:[10,0,0,2],port:80};let h=s.open_tcp_connect("test",remote,Some("10.0.0.1:55001".parse().unwrap())).unwrap();let(local,peer)=endpoint(&s,h);assert_eq!(local.port,55001);assert_eq!(local.addr,IpAddress::Ipv4(Ipv4Address::new(10,0,0,1)));assert_eq!(peer.port,80);assert_eq!(s.tcp_next_ephemeral,49152);assert_eq!(s.open_tcp_connect("test",remote,Some("10.0.0.9:55002".parse().unwrap())),Err("invalid local address"));}
 #[test]fn real_smoltcp_wildcard_bind_selects_configured_ipv4_and_keeps_port(){let mut s=NetService::new();let h=s.open_tcp_connect("test",NetEndpoint{addr:[10,0,0,2],port:80},Some("0.0.0.0:55003".parse().unwrap())).unwrap();let(local,_)=endpoint(&s,h);assert_eq!(local.port,55003);assert_eq!(local.addr,IpAddress::Ipv4(Ipv4Address::new(10,0,0,1)));}
 #[test]fn real_smoltcp_ipv6_connect_preserves_global_and_link_local_endpoint(){let mut s=NetService::new();let remote=NetEndpointV6{addr:core::net::Ipv6Addr::new(0x2001,0xdb8,0,0,0,0,0,2).octets(),port:80};let h=s.open_tcp_connect_v6("test",remote,Some("[2001:db8::1]:55004".parse().unwrap())).unwrap();let(local,_)=endpoint(&s,h);assert_eq!(local.port,55004);assert_eq!(local.addr,IpAddress::Ipv6(s.local_ipv6_global.unwrap()));let h=s.open_tcp_connect_v6("test",remote,Some("[fe80::1]:55005".parse().unwrap())).unwrap();assert_eq!(endpoint(&s,h).0.addr,IpAddress::Ipv6(s.local_ipv6_ll));assert_eq!(s.open_tcp_connect_v6("test",remote,Some("[2001:db8::9]:55006".parse().unwrap())),Err("invalid local address"));}
 #[test]fn actual_private_command_dispatch_keeps_reserved_ipv4_and_ipv6_transport_ports(){for(local,remote)in[("10.0.0.1:55007","10.0.0.2:80"),("[2001:db8::1]:55008","[2001:db8::2]:80")]{let mut s=NetService::new();s.route_command(NetCommand::OpenTcpConnectFrom{local:local.parse().unwrap(),remote:remote.parse().unwrap()});let local:SocketAddr=local.parse().unwrap();assert_eq!(endpoint(&s,NetHandle(1)).0.port,local.port());}}
 fn actor(vm:u8)->NetworkActor{NetworkActor::Vm(vm,1)}
 fn private(s:&mut NetService)->NetHandle{s.open_tcp_listener_for_actor("server",actor(2),"127.0.0.1:42116".parse().unwrap()).unwrap()}
 fn connect(s:&mut NetService,who:NetworkActor)->Result<(NetHandle,NetHandle,&'static str),&'static str>{s.open_loopback_tcp_connect("client",who,NetEndpoint{addr:[127,0,0,1],port:42116},None)}
 #[test]fn private_listen_rejects_public_other_account_other_app_and_wildcards(){let mut s=NetService::new();for who in [NetworkActor::Public,actor(1),actor(3),actor(4)]{assert_eq!(s.open_tcp_listener_for_actor("bad",who,"127.0.0.1:42116".parse().unwrap()),Err("tcp listen permission denied"));}for endpoint in ["0.0.0.0:42116","10.0.0.1:42116","[::1]:42116"]{assert_eq!(s.open_tcp_listener_for_actor("server",actor(2),endpoint.parse().unwrap()),Err("tcp listen permission denied"));}assert!(s.records.is_empty());private(&mut s);}
 #[test]fn private_connect_checks_actual_peer_before_any_listener_mutation(){let mut s=NetService::new();let listener=private(&mut s);for who in [NetworkActor::Public,actor(2),actor(3),actor(4)]{assert_eq!(connect(&mut s,who),Err("tcp connect permission denied"));assert_eq!(s.records.len(),1);assert_eq!(s.records[0].handle,listener);assert!(s.records[0].tcp_loopback_peer.is_none());assert!(s.sockets.get::<tcp::Socket>(s.records[0].socket).is_listening());}let(client,server,_)=connect(&mut s,actor(1)).unwrap();assert!(matches!(s.route_tcp_send(client,b"request".to_vec()),TcpRoute::Loopback(Ok(()))));assert!(matches!(s.route_tcp_send(server,b"reply".to_vec()),TcpRoute::Loopback(Ok(()))));EVENTS.with(|v|{let events=v.borrow();assert!(events.iter().any(|(_,e)|matches!(e,NetEvent::TcpData{handle,data}if *handle==server&&data==b"request")));assert!(events.iter().any(|(_,e)|matches!(e,NetEvent::TcpData{handle,data}if *handle==client&&data==b"reply")));});}
 #[test]fn prior_impostor_listener_cannot_receive_after_reservation_and_is_reaped(){let mut s=NetService::new();RESERVED.with(|v|v.set(false));s.open_tcp_listener_for_actor("impostor",NetworkActor::Public,"0.0.0.0:42116".parse().unwrap()).unwrap();RESERVED.with(|v|v.set(true));assert_eq!(connect(&mut s,actor(1)),Err("tcp connect permission denied"));assert!(s.revoke_tcp_access(0));assert!(s.records.is_empty());private(&mut s);connect(&mut s,actor(1)).unwrap();}
 #[test]fn grant_revocation_discards_buffered_private_bytes_and_closes_both_peers(){let mut s=NetService::new();private(&mut s);let(client,server,_)=connect(&mut s,actor(1)).unwrap();PAUSED.with(|v|v.set(true));assert!(matches!(s.route_tcp_send(client,b"queued-private".to_vec()),TcpRoute::Loopback(Ok(()))));assert!(EVENTS.with(|v|v.borrow().iter().all(|(_,e)|!matches!(e,NetEvent::TcpData{..}))));ALLOWED.with(|v|v.set(false));let index=s.records.iter().position(|r|r.handle==client).unwrap();assert!(s.flush_loopback_tcp_tx(index));assert!(s.records.is_empty());EVENTS.with(|v|{let events=v.borrow();assert!(events.iter().all(|(_,e)|!matches!(e,NetEvent::TcpData{..})));for expected in [client,server]{assert!(events.iter().any(|(_,e)|matches!(e,NetEvent::Closed{handle}if *handle==expected)));}});}
 #[test]fn retired_service_epoch_blocks_existing_byte_transfers(){let mut s=NetService::new();private(&mut s);let(client,_,_)=connect(&mut s,actor(1)).unwrap();EPOCHS.with(|v|{let mut epochs=v.get();epochs[2]+=1;v.set(epochs);});assert!(matches!(s.route_tcp_send(client,b"retired".to_vec()),TcpRoute::Loopback(Err(()))));assert!(s.records.is_empty());assert!(EVENTS.with(|v|v.borrow().iter().all(|(_,e)|!matches!(e,NetEvent::TcpData{..}))));}
 #[test]fn explicitly_public_other_instance_keeps_ordinary_loopback_behavior(){let mut s=NetService::new();s.open_tcp_listener_for_actor("public-server",NetworkActor::Public,"127.0.0.1:42117".parse().unwrap()).unwrap();let(client,_,_)=s.open_loopback_tcp_connect("public-client",NetworkActor::Public,NetEndpoint{addr:[127,0,0,1],port:42117},None).unwrap();assert!(matches!(s.route_tcp_send(client,b"public".to_vec()),TcpRoute::Loopback(Ok(()))));}
}
