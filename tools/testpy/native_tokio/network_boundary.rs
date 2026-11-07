//! In-memory native socket service simulator. Exercises actual Tokio+Mio source;
//! this is not a smoltcp, VM ownership or physical packet execution proof.
use std::{collections::{HashMap,VecDeque},sync::{Mutex,OnceLock},sync::atomic::{AtomicU32,AtomicUsize,Ordering}};
use core::net::{IpAddr,Ipv4Addr,SocketAddr};
const IO_KEY:u64=0x4250_494f_0000_0001;
#[derive(Clone,Copy,Debug)]#[repr(C)]pub struct Raw{family:u8,reserved:u8,port:u16,addr:[u8;16]}
fn address(raw:Raw)->SocketAddr{assert_eq!(raw.family,4);SocketAddr::new(IpAddr::V4(Ipv4Addr::new(raw.addr[0],raw.addr[1],raw.addr[2],raw.addr[3])),raw.port)}
fn raw(addr:SocketAddr)->Raw{let IpAddr::V4(ip)=addr.ip()else{panic!("simulator IPv4 transport only")};let mut bytes=[0;16];bytes[..4].copy_from_slice(&ip.octets());Raw{family:4,reserved:0,port:addr.port(),addr:bytes}}
enum Kind{Unconnected{bound:bool},Listener{pending:VecDeque<u32>,backlog:u32},Tcp{peer_id:u32,rx:VecDeque<u8>,peer_eof:bool,read_closed:bool,write_closed:bool},Udp{packets:VecDeque<(SocketAddr,Vec<u8>)>}}
struct Socket{local:SocketAddr,peer:Option<SocketAddr>,kind:Kind,nodelay:bool,ttl:u32}
static SOCKETS:OnceLock<Mutex<HashMap<u32,Socket>>>=OnceLock::new();
static NEXT:AtomicU32=AtomicU32::new(1);
static PORT:AtomicU32=AtomicU32::new(40000);
fn sockets()->&'static Mutex<HashMap<u32,Socket>>{SOCKETS.get_or_init(Default::default)}
fn id()->u32{NEXT.fetch_add(1,Ordering::Relaxed)}
fn local(mut addr:SocketAddr)->SocketAddr{if addr.port()==0{addr.set_port(PORT.fetch_add(1,Ordering::Relaxed)as u16);}addr}
fn socket(local:SocketAddr,peer:Option<SocketAddr>,kind:Kind)->Socket{Socket{local,peer,kind,nodelay:false,ttl:64}}
fn wake(){crate::boundary::trueos_tokio_platform_wake_all(IO_KEY);}
pub fn live_sockets()->usize{sockets().lock().unwrap().len()}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_socket_create(family:u32,out:*mut u32)->i32{
 if family!=4{return -1;}let id=id();sockets().lock().unwrap().insert(id,socket(SocketAddr::from(([0,0,0,0],0)),None,Kind::Unconnected{bound:false}));unsafe{out.write(id);}0
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_tcp_socket_bind(id:u32,addr:Raw)->i32{
 if addr.family!=4{return -4;}let mut map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};if !matches!(s.kind,Kind::Unconnected{bound:false}){return -4;}
 let addr=local(address(addr));if map.iter().any(|(&other,s)|other!=id&&s.local==addr&&!matches!(s.kind,Kind::Udp{..})){return -6;}
 let s=map.get_mut(&id).unwrap();s.local=addr;s.kind=Kind::Unconnected{bound:true};0
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_tcp_socket_connect(client:u32,addr:Raw)->i32{
 if addr.family!=4{return -4;}let peer=address(addr);let mut map=sockets().lock().unwrap();let Some(s)=map.get(&client)else{return -5;};let Kind::Unconnected{bound}=s.kind else{return -4;};
 let mine=if bound{s.local}else{local(SocketAddr::from(([127,0,0,1],0)))};
 let listener=map.iter().find(|(_,s)|matches!(s.kind,Kind::Listener{..})&&s.local==peer).map(|(&id,_)|id);let Some(listener)=listener else{return -3;};
 if let Kind::Listener{pending,backlog}=&map[&listener].kind{if pending.len()>=*backlog as usize{return -9;}}
 let server=id();let mut accepted=socket(peer,Some(mine),Kind::Tcp{peer_id:client,rx:VecDeque::new(),peer_eof:false,read_closed:false,write_closed:false});accepted.nodelay=map[&listener].nodelay;map.insert(server,accepted);
 let s=map.get_mut(&client).unwrap();s.local=mine;s.peer=Some(peer);s.kind=Kind::Tcp{peer_id:server,rx:VecDeque::new(),peer_eof:false,read_closed:false,write_closed:false};
 if let Kind::Listener{pending,..}=&mut map.get_mut(&listener).unwrap().kind{pending.push_back(server);}drop(map);wake();0
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_tcp_socket_listen(id:u32,backlog:u32)->i32{
 if backlog>32{return -4;}let backlog=backlog.max(1);let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};if !matches!(s.kind,Kind::Unconnected{bound:true}){return -4;}s.kind=Kind::Listener{pending:VecDeque::new(),backlog};drop(map);wake();0
}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_listener_bind(addr:Raw,out:*mut u32)->i32{
 if addr.family!=4{return -4;}let addr=local(address(addr));let mut map=sockets().lock().unwrap();
 if map.values().any(|s|s.local==addr&&matches!(s.kind,Kind::Listener{..})){return -6;}
 let id=id();map.insert(id,socket(addr,None,Kind::Listener{pending:VecDeque::new(),backlog:128}));unsafe{out.write(id);}drop(map);wake();0
}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_stream_connect(addr:Raw,out:*mut u32)->i32{
 if addr.family!=4{return -4;}let peer=address(addr);let mut map=sockets().lock().unwrap();
 let listener=map.iter().find(|(_,s)|matches!(s.kind,Kind::Listener{..})&&s.local==peer).map(|(&id,_)|id);
 let Some(listener)=listener else{return -3;};
 if let Kind::Listener{pending,backlog}=&map[&listener].kind{if pending.len()>=*backlog as usize{return -9;}}
 let client=id();let server=id();let mine=local(SocketAddr::from(([127,0,0,1],0)));
 map.insert(client,socket(mine,Some(peer),Kind::Tcp{peer_id:server,rx:VecDeque::new(),peer_eof:false,read_closed:false,write_closed:false}));
 let mut accepted=socket(peer,Some(mine),Kind::Tcp{peer_id:client,rx:VecDeque::new(),peer_eof:false,read_closed:false,write_closed:false});accepted.nodelay=map[&listener].nodelay;map.insert(server,accepted);
 if let Kind::Listener{pending,..}= &mut map.get_mut(&listener).unwrap().kind{pending.push_back(server);}
 unsafe{out.write(client);}drop(map);wake();0
}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_listener_accept(id:u32,out:*mut u32,addr:*mut Raw)->i32{
 let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};let Kind::Listener{pending,..}=&mut s.kind else{return -4;};let Some(child)=pending.pop_front()else{return -2;};let peer=map[&child].peer.unwrap();unsafe{out.write(child);addr.write(raw(peer));}0
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_socket_close(id:u32)->i32{
 let mut map=sockets().lock().unwrap();let Some(s)=map.remove(&id)else{return -5;};
 if let Kind::Tcp{peer_id,..}=s.kind{if let Some(Socket{kind:Kind::Tcp{peer_eof,..},..})=map.get_mut(&peer_id){*peer_eof=true;}}
 else if let Kind::Listener{pending,..}=s.kind{for child in pending{map.remove(&child);}}
 drop(map);wake();0
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_socket_shutdown(id:u32,how:u32)->i32{
 let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};let Kind::Tcp{peer_id,rx,read_closed,write_closed,..}=&mut s.kind else{return -4;};if how>2{return -4;}
 if how!=1{*read_closed=true;rx.clear();}let peer_id=*peer_id;if how!=0{*write_closed=true;if let Some(Socket{kind:Kind::Tcp{peer_eof,..},..})=map.get_mut(&peer_id){*peer_eof=true;}}
 drop(map);wake();0
}
unsafe fn read(id:u32,out:*mut u8,cap:usize,peek:bool)->isize{
 let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};let Kind::Tcp{rx,read_closed,peer_eof,..}=&mut s.kind else{return -4;};if cap==0||*read_closed{return 0;}if rx.is_empty(){return if *peer_eof{0}else{-2};}
 let n=cap.min(rx.len());for i in 0..n{let b=if peek{rx[i]}else{rx.pop_front().unwrap()};unsafe{out.add(i).write(b);}}n as isize
}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_stream_read(id:u32,out:*mut u8,cap:usize)->isize{unsafe{read(id,out,cap,false)}}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_stream_peek(id:u32,out:*mut u8,cap:usize)->isize{unsafe{read(id,out,cap,true)}}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_tcp_stream_write(id:u32,data:*const u8,len:usize)->isize{
 let mut map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let Kind::Tcp{peer_id,write_closed,..}=s.kind else{return -4;};if write_closed{return -10;}
 let Some(Socket{kind:Kind::Tcp{rx,read_closed,..},..})=map.get_mut(&peer_id)else{return -10;};if *read_closed{return -10;}
 let n=len.min(3);rx.extend(unsafe{std::slice::from_raw_parts(data,n)});drop(map);wake();n as isize
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_socket_poll_ready(id:u32,interests:u8)->i32{
 let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let bits=match &s.kind{
 Kind::Unconnected{..}=>0,
 Kind::Listener{pending,..}=>u8::from(!pending.is_empty()),
 Kind::Tcp{rx,peer_eof,read_closed,write_closed,..}=>u8::from(!rx.is_empty()||*peer_eof||*read_closed)|if !*write_closed{2}else{16}|if *peer_eof||*read_closed{8}else{0},
 Kind::Udp{packets}=>2|u8::from(!packets.is_empty()),};i32::from(bits&(interests|28))
}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_socket_local_addr(id:u32,out:*mut Raw)->i32{let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};unsafe{out.write(raw(s.local));}0}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_socket_peer_addr(id:u32,out:*mut Raw)->i32{let map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let Some(peer)=s.peer else{return -3;};unsafe{out.write(raw(peer));}0}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_socket_take_error(id:u32)->i32{if sockets().lock().unwrap().contains_key(&id){0}else{-5}}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_socket_option(id:u32,option:u32,value:i32)->i32{let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};match option{1=>{if value>=0{s.nodelay=value!=0;}i32::from(s.nodelay)},2=>{if value==0||value>255{return -4;}if value>=0{s.ttl=value as u32;}s.ttl as i32},_=>-1}}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_udp_socket_bind(addr:Raw,out:*mut u32)->i32{if addr.family!=4{return -4;}let addr=local(address(addr));let mut map=sockets().lock().unwrap();if map.values().any(|s|s.local==addr&&matches!(s.kind,Kind::Udp{..})){return -6;}let id=id();map.insert(id,socket(addr,None,Kind::Udp{packets:VecDeque::new()}));unsafe{out.write(id);}drop(map);wake();0}
#[unsafe(no_mangle)]pub extern "C" fn trueos_mio_udp_socket_connect(id:u32,addr:Raw)->i32{let peer=address(addr);let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};s.peer=Some(peer);if let Kind::Udp{packets}=&mut s.kind{packets.retain(|(from,_)|*from==peer);}0}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_udp_socket_send_to(id:u32,addr:Raw,data:*const u8,len:usize)->isize{let mut map=sockets().lock().unwrap();let Some(s)=map.get(&id)else{return -5;};let from=s.local;let to=address(addr);let target=map.iter().find(|(_,s)|s.local==to&&matches!(s.kind,Kind::Udp{..})).map(|(&id,_)|id);if let Some(target)=target{let s=map.get_mut(&target).unwrap();if s.peer.is_none_or(|p|p==from){if let Kind::Udp{packets}=&mut s.kind{packets.push_back((from,unsafe{std::slice::from_raw_parts(data,len)}.to_vec()));}}}drop(map);wake();len as isize}
unsafe fn recv_udp(id:u32,addr:*mut Raw,out:*mut u8,cap:usize,peek:bool)->isize{let mut map=sockets().lock().unwrap();let Some(s)=map.get_mut(&id)else{return -5;};let Kind::Udp{packets}=&mut s.kind else{return -4;};let Some((from,data))=packets.front()else{return -2;};let n=cap.min(data.len());unsafe{addr.write(raw(*from));std::ptr::copy_nonoverlapping(data.as_ptr(),out,n);}if !peek{packets.pop_front();}n as isize}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_udp_socket_recv_from(id:u32,addr:*mut Raw,out:*mut u8,cap:usize)->isize{unsafe{recv_udp(id,addr,out,cap,false)}}
#[unsafe(no_mangle)]pub unsafe extern "C" fn trueos_mio_udp_socket_peek_from(id:u32,addr:*mut Raw,out:*mut u8,cap:usize)->isize{unsafe{recv_udp(id,addr,out,cap,true)}}
pub static DNS_CALLS:AtomicUsize=AtomicUsize::new(0);
#[unsafe(no_mangle)]pub extern "Rust" fn trueos_platform_dns_resolve_ipv4(host:&str)->Result<[u8;4],u32>{DNS_CALLS.fetch_add(1,Ordering::Relaxed);assert!(crate::boundary::trueos_cabi_thread_current_id()<u32::MAX as usize,"DNS ran on foreground instead of native blocking thread");match host{"native.test"=>Ok([127,0,0,1]),"timeout.test"=>Err(4),_=>Err(5)}}
