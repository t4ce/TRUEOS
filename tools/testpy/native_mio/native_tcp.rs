//! Full production Mio native sources; host test process supplies only kernel ABI stubs.
use mio::net::{Shutdown, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering};
use trueos_io::{ErrorKind, Read, Write};

#[derive(Default)]
struct State {
    next_write_error: Option<isize>, error: i32, readiness: u8, rx: VecDeque<u8>, tx: Vec<u8>, read_shutdown: bool, write_shutdown: bool,
    peer_eof: bool, closed: bool, closes: usize, reads: usize, peeks: usize,
    shutdowns: Vec<u32>, next_shutdown_error: Option<i32>, next_peek_error: Option<isize>,
}
static SOCKETS: OnceLock<Mutex<HashMap<u32, State>>> = OnceLock::new();
static NEXT: AtomicU32 = AtomicU32::new(100);
fn state<R>(id: u32, f: impl FnOnce(&mut State) -> R) -> R {
    let mut sockets = SOCKETS.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(sockets.get_mut(&id).expect("owned test socket"))
}
fn connect() -> (u32, TcpStream) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    SOCKETS.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id, State::default());
    let addr = core::net::SocketAddr::from(([127,0,0,1],id as u16));
    (id, TcpStream::connect(addr).unwrap())
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SocketAddrRaw { family:u8, reserved:u8, port:u16, addr:[u8;16] }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_connect(addr:SocketAddrRaw, out:*mut u32)->i32 {
    assert_eq!(addr.family,4); assert_eq!(&addr.addr[..4], &[127,0,0,1]);
    let id=u32::from(addr.port); state(id, |_| ());
    unsafe {out.write(id)}; 0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_socket_close(id:u32)->i32 {
    state(id,|s|{s.closed=true;s.closes+=1;});0
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_socket_shutdown(id:u32,how:u32)->i32 {
    state(id,|s|{
        s.shutdowns.push(how);
        if let Some(error)=s.next_shutdown_error.take(){return error;}
        if s.closed{return -3;}
        if how>2{return -4;}
        if how!=1 {s.read_shutdown=true;s.rx.clear();s.readiness|=1|8;}
        if how!=0 {s.write_shutdown=true;s.readiness=(s.readiness & !2)|16;}
        0
    })
}
unsafe fn receive(id:u32,out:*mut u8,cap:usize,peek:bool)->isize {
    state(id,|s|{
        if peek{s.peeks+=1;}else{s.reads+=1;}
        if peek {if let Some(error)=s.next_peek_error.take(){return error;}}
        if cap==0||s.read_shutdown{return 0;}
        if s.closed{return -3;}
        if s.rx.is_empty(){return if s.peer_eof{0}else{-2};}
        let n=cap.min(s.rx.len());
        for i in 0..n {let byte=if peek{s.rx[i]}else{s.rx.pop_front().unwrap()};unsafe{out.add(i).write(byte)}}
        n as isize
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_read(id:u32,out:*mut u8,cap:usize)->isize {
    unsafe{receive(id,out,cap,false)}
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_peek(id:u32,out:*mut u8,cap:usize)->isize {
    unsafe{receive(id,out,cap,true)}
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_write(id:u32,input:*const u8,len:usize)->isize {
    state(id,|s|{
        if s.closed{return -3;}
        if s.write_shutdown{return -10;}
        if let Some(status)=s.next_write_error.take(){return status;}
        s.tx.extend_from_slice(unsafe{std::slice::from_raw_parts(input,len)}); len as isize
    })
}

#[test]
fn write_half_close_retains_reads_and_closes_owned_socket_only_on_drop(){
    let(id,mut stream)=connect();state(id,|s|s.rx.extend(b"reply"));
    assert_eq!(stream.write(b"request").unwrap(),7);
    stream.shutdown(Shutdown::Write).unwrap();
    state(id,|s|{assert_eq!(s.shutdowns,[1]);assert!(s.write_shutdown);assert!(!s.read_shutdown);assert_eq!(s.closes,0);assert_eq!(s.tx,b"request");});
    assert_eq!(stream.write(b"late").unwrap_err().kind(),ErrorKind::BrokenPipe);
    let mut reply=[0;5];assert_eq!(stream.read(&mut reply).unwrap(),5);assert_eq!(&reply,b"reply");
    drop(stream);state(id,|s|assert_eq!(s.closes,1));
}
#[test]
fn read_half_close_retains_writes_and_does_not_close_handle(){
    let(id,mut stream)=connect();state(id,|s|s.rx.extend(b"discard"));
    stream.shutdown(Shutdown::Read).unwrap();
    let mut buf=[0;8];assert_eq!(stream.read(&mut buf).unwrap(),0);assert_eq!(stream.peek(&mut buf).unwrap(),0);
    assert_eq!(stream.write(b"still writable").unwrap(),14);
    state(id,|s|{assert_eq!(s.shutdowns,[0]);assert!(s.read_shutdown);assert!(!s.write_shutdown);assert_eq!(s.closes,0);assert_eq!(s.tx,b"still writable");});
    drop(stream);state(id,|s|assert_eq!(s.closes,1));
}
#[test]
fn both_directions_shutdown_is_distinct_from_resource_close(){
    let(id,mut stream)=connect();stream.shutdown(Shutdown::Both).unwrap();
    let mut buf=[0;1];assert_eq!(stream.read(&mut buf).unwrap(),0);
    assert_eq!(stream.write(b"x").unwrap_err().kind(),ErrorKind::BrokenPipe);
    state(id,|s|{assert_eq!(s.shutdowns,[2]);assert!(s.read_shutdown&&s.write_shutdown);assert_eq!(s.closes,0);});
    drop(stream);state(id,|s|assert_eq!(s.closes,1));
}
#[test]
fn failed_shutdown_propagates_status_without_closing_or_consuming_socket(){
    let(id,mut stream)=connect();state(id,|s|s.next_shutdown_error=Some(-3));
    assert_eq!(stream.shutdown(Shutdown::Write).unwrap_err().kind(),ErrorKind::NotConnected);
    assert_eq!(stream.write(b"usable").unwrap(),6);
    state(id,|s|{assert!(!s.write_shutdown);assert_eq!(s.closes,0);});
    stream.shutdown(Shutdown::Write).unwrap();stream.shutdown(Shutdown::Write).unwrap();
    state(id,|s|{assert_eq!(s.shutdowns,[1,1,1]);assert_eq!(s.closes,0);});
    drop(stream);state(id,|s|assert_eq!(s.closes,1));
}
#[test]
fn peek_is_nondestructive_bounded_and_uses_peek_transport(){
    let(id,mut stream)=connect();state(id,|s|s.rx.extend(b"abcdef"));
    let mut buf=[0x55;8];assert_eq!(stream.peek(&mut buf[..3]).unwrap(),3);
    assert_eq!(&buf[..3],b"abc");assert_eq!(&buf[3..],&[0x55;5]);
    assert_eq!(stream.peek(&mut buf[..3]).unwrap(),3);assert_eq!(&buf[..3],b"abc");
    state(id,|s|{assert_eq!(s.peeks,2);assert_eq!(s.reads,0);assert_eq!(s.rx.len(),6);});
    assert_eq!(stream.read(&mut buf[..2]).unwrap(),2);assert_eq!(&buf[..2],b"ab");
    assert_eq!(stream.peek(&mut buf).unwrap(),4);assert_eq!(&buf[..4],b"cdef");
    state(id,|s|assert_eq!(s.rx.iter().copied().collect::<Vec<_>>(),b"cdef"));
}
#[test]
fn peek_preserves_would_block_zero_length_and_eof(){
    let(id,stream)=connect();let mut buf=[0x55;4];
    assert_eq!(stream.peek(&mut buf).unwrap_err().kind(),ErrorKind::WouldBlock);assert_eq!(buf,[0x55;4]);
    assert_eq!(stream.peek(&mut []).unwrap(),0);
    state(id,|s|s.peer_eof=true);assert_eq!(stream.peek(&mut buf).unwrap(),0);assert_eq!(buf,[0x55;4]);
}
#[test]
fn peek_failure_preserves_error_kind_and_caller_buffer(){
    let(id,stream)=connect();state(id,|s|{s.rx.extend(b"remain");s.next_peek_error=Some(-4);});
    let mut buf=[0x55;4];assert_eq!(stream.peek(&mut buf).unwrap_err().kind(),ErrorKind::InvalidInput);
    assert_eq!(buf,[0x55;4]);assert_eq!(stream.peek(&mut buf).unwrap(),4);assert_eq!(&buf,b"rema");
    state(id,|s|assert_eq!(s.rx.len(),6));
}


const IO_KEY:u64=0x4250_494f_0000_0001;
static GENERATION:AtomicU32=AtomicU32::new(0);
static NOW:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static WAKES:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static WAITS:OnceLock<Mutex<Vec<(u32,u64)>>>=OnceLock::new();
type QueryHook=(u32,Box<dyn FnOnce()+Send>);
static QUERY_HOOK:OnceLock<Mutex<Option<QueryHook>>>=OnceLock::new();
type WaitHook=Box<dyn FnOnce(u32,u64)->bool+Send>;
static WAIT_HOOKS:OnceLock<Mutex<VecDeque<WaitHook>>>=OnceLock::new();
fn waits()->Vec<(u32,u64)>{WAITS.get_or_init(Default::default).lock().unwrap().clone()}
fn reset_waits(){
 NOW.store(0,Ordering::Relaxed);WAITS.get_or_init(Default::default).lock().unwrap().clear();
 *QUERY_HOOK.get_or_init(Default::default).lock().unwrap()=None;
 WAIT_HOOKS.get_or_init(Default::default).lock().unwrap().clear();
}
fn on_query(id:u32,f:impl FnOnce()+Send+'static){*QUERY_HOOK.get_or_init(Default::default).lock().unwrap()=Some((id,Box::new(f)));}
fn on_wait(f:impl FnOnce(u32,u64)->bool+Send+'static){WAIT_HOOKS.get_or_init(Default::default).lock().unwrap().push_back(Box::new(f));}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_monotonic_nanos()->u64{NOW.load(Ordering::Acquire)}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_observe(key:u64)->u32{assert_eq!(key,IO_KEY);GENERATION.load(Ordering::Acquire)}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wake_all(key:u64)->usize{
 assert_eq!(key,IO_KEY);GENERATION.fetch_add(1,Ordering::Release);WAKES.fetch_add(1,Ordering::Relaxed);0
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_after(key:u64,observed:u32,timeout_ms:u64)->bool{
 assert_eq!(key,IO_KEY);WAITS.get_or_init(Default::default).lock().unwrap().push((observed,timeout_ms));
 if GENERATION.load(Ordering::Acquire)!=observed{return true;}
 let hook=WAIT_HOOKS.get_or_init(Default::default).lock().unwrap().pop_front();
 if let Some(hook)=hook{return hook(observed,timeout_ms);}
 assert_ne!(timeout_ms,u64::MAX,"infinite test wait needs a scheduled wake");
 NOW.try_update(Ordering::AcqRel,Ordering::Acquire,|v|Some(v.saturating_add(timeout_ms.saturating_mul(1_000_000)))).unwrap();false
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_socket_poll_ready(id:u32,interests:u8)->i32{
 let result=state(id,|s|if s.closed{-5}else{i32::from(s.readiness & (interests|0b11100))});
 let hook={let mut hook=QUERY_HOOK.get_or_init(Default::default).lock().unwrap();if hook.as_ref().is_some_and(|(socket,_)|*socket==id){hook.take()}else{None}};
 if let Some((_,hook))=hook{hook();}
 result
}
fn poll_tokens(poll:&mut Poll,events:&mut Events)->Vec<usize>{poll.poll(events,Some(std::time::Duration::ZERO)).unwrap();events.iter().map(|e|e.token().0).collect()}
#[test]
fn selector_rotates_ready_sockets_when_capacity_is_one(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(1);
 let(a,mut first)=connect();let(b,mut second)=connect();state(a,|s|s.readiness=2);state(b,|s|s.readiness=2);
 poll.registry().register(&mut first,Token(10),Interest::WRITABLE).unwrap();
 poll.registry().register(&mut second,Token(20),Interest::WRITABLE).unwrap();
 let one=poll_tokens(&mut poll,&mut events);let two=poll_tokens(&mut poll,&mut events);assert_ne!(one,two);assert_eq!([one[0],two[0]].into_iter().collect::<std::collections::BTreeSet<_>>(),[10,20].into());
 poll.registry().reregister(&mut second,Token(99),Interest::WRITABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[99]);assert!(poll_tokens(&mut poll,&mut events).is_empty());
 poll.registry().deregister(&mut first).unwrap();poll.registry().reregister(&mut second,Token(99),Interest::WRITABLE).unwrap();assert_eq!(poll_tokens(&mut poll,&mut events),[99]);assert!(waits().is_empty());
}
#[test]
fn socket_drop_removes_registration_before_kernel_id_reuse(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(4);let(id,mut stream)=connect();state(id,|s|s.readiness=2);
 poll.registry().register(&mut stream,Token(7),Interest::WRITABLE).unwrap();drop(stream);
 state(id,|s|{assert_eq!(s.closes,1);*s=State::default();s.readiness=2;});
 let mut reused=TcpStream::connect(core::net::SocketAddr::from(([127,0,0,1],id as u16))).unwrap();
 assert!(poll_tokens(&mut poll,&mut events).is_empty());poll.registry().register(&mut reused,Token(8),Interest::WRITABLE).unwrap();assert_eq!(poll_tokens(&mut poll,&mut events),[8]);
}
#[test]
fn a_socket_has_one_registration_owner_until_deregistered(){
 reset_waits();let mut a=Poll::new().unwrap();let mut b=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=2);
 a.registry().register(&mut stream,Token(1),Interest::WRITABLE).unwrap();
 assert_eq!(b.registry().register(&mut stream,Token(2),Interest::WRITABLE).unwrap_err().kind(),ErrorKind::AlreadyExists);
 assert_eq!(b.registry().deregister(&mut stream).unwrap_err().kind(),ErrorKind::NotFound);
 assert_eq!(poll_tokens(&mut a,&mut events),[1]);a.registry().deregister(&mut stream).unwrap();
 b.registry().register(&mut stream,Token(2),Interest::WRITABLE).unwrap();assert!(poll_tokens(&mut a,&mut events).is_empty());assert_eq!(poll_tokens(&mut b,&mut events),[2]);
 drop(stream);assert!(poll_tokens(&mut b,&mut events).is_empty());
}
#[test]
fn inflight_old_registration_cannot_deliver_after_same_token_aba(){
 reset_waits();let mut poll=Poll::new().unwrap();let registry=poll.registry().try_clone().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=1);
 registry.register(&mut stream,Token(7),Interest::READABLE).unwrap();let stream=std::sync::Arc::new(Mutex::new(stream));let changed=stream.clone();
 on_query(id,move||{let mut stream=changed.lock().unwrap();registry.deregister(&mut *stream).unwrap();registry.register(&mut *stream,Token(7),Interest::WRITABLE).unwrap();});
 assert!(poll_tokens(&mut poll,&mut events).is_empty(),"old readable result crossed a deregister/register boundary with the same token");
 state(id,|s|s.readiness=2);assert_eq!(poll_tokens(&mut poll,&mut events),[7]);assert!(events.iter().next().unwrap().is_writable());
}
#[test]
fn wake_between_probe_and_wait_is_not_lost(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();poll.registry().register(&mut stream,Token(9),Interest::READABLE).unwrap();
 on_query(id,move||{state(id,|s|s.readiness=1);trueos_tokio_platform_wake_all(IO_KEY);});
 poll.poll(&mut events,Some(std::time::Duration::from_secs(1))).unwrap();assert_eq!(events.iter().next().unwrap().token(),Token(9));assert_eq!(NOW.load(Ordering::Relaxed),0);assert_eq!(waits().len(),1);assert_eq!(waits()[0].1,1000);
}
#[test]
fn registration_during_infinite_wait_wakes_and_is_reprobed(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let registry=poll.registry().try_clone().unwrap();let(id,stream)=connect();let stream=std::sync::Arc::new(Mutex::new(stream));let register=stream.clone();state(id,|s|s.readiness=2);
 on_wait(move|_,timeout|{assert_eq!(timeout,u64::MAX);registry.register(&mut *register.lock().unwrap(),Token(23),Interest::WRITABLE).unwrap();true});
 poll.poll(&mut events,None).unwrap();assert_eq!(events.iter().next().unwrap().token(),Token(23));assert_eq!(waits().len(),1);
}
#[test]
fn waker_publishes_before_waking_and_coalesces_a_storm(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(1);let waker=Waker::new(poll.registry(),Token(42)).unwrap();let before=WAKES.load(Ordering::Relaxed);
 for _ in 0..10_000{waker.wake().unwrap();}assert_eq!(WAKES.load(Ordering::Relaxed)-before,1);
 assert_eq!(poll_tokens(&mut poll,&mut events),[42]);assert!(poll_tokens(&mut poll,&mut events).is_empty());
 let waker=std::sync::Arc::new(waker);on_wait(move|_,_|{waker.wake().unwrap();true});poll.poll(&mut events,None).unwrap();assert_eq!(events.iter().next().unwrap().token(),Token(42));
}
#[test]
fn finite_timeouts_round_up_and_spurious_wakes_keep_the_original_deadline(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(1);
 poll.poll(&mut events,Some(std::time::Duration::from_nanos(1))).unwrap();assert_eq!(waits().iter().map(|x|x.1).collect::<Vec<_>>(),[1]);assert!(events.is_empty());
 reset_waits();on_wait(|_,ms|{assert_eq!(ms,5);NOW.store(1_000_000,Ordering::Relaxed);trueos_tokio_platform_wake_all(IO_KEY);true});
 poll.poll(&mut events,Some(std::time::Duration::from_millis(5))).unwrap();assert_eq!(waits().iter().map(|x|x.1).collect::<Vec<_>>(),[5,4]);assert_eq!(NOW.load(Ordering::Relaxed),5_000_000);
}
#[test]
fn finite_max_duration_uses_a_finite_chunk_instead_of_infinity(){
 reset_waits();NOW.store(u64::MAX-10,Ordering::Relaxed);let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(1);let waker=Waker::new(poll.registry(),Token(5)).unwrap();
 on_wait(move|_,ms|{assert_eq!(ms,u64::MAX-1);waker.wake().unwrap();true});poll.poll(&mut events,Some(std::time::Duration::MAX)).unwrap();assert_eq!(events.iter().next().unwrap().token(),Token(5));
}
#[test]
fn invalid_closed_socket_is_retired_instead_of_becoming_readiness_bits(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();poll.registry().register(&mut stream,Token(7),Interest::READABLE).unwrap();state(id,|s|s.closed=true);
 assert!(poll_tokens(&mut poll,&mut events).is_empty());state(id,|s|{s.closed=false;s.readiness=1;});assert!(poll_tokens(&mut poll,&mut events).is_empty());
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_socket_take_error(id:u32)->i32{
 state(id,|s|{let error=s.error;s.error=0;s.readiness&=!4;error})
}
#[test]
fn permanent_writable_level_is_silent_after_one_edge_until_rearmed(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=2);poll.registry().register(&mut stream,Token(8),Interest::WRITABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[8]);for _ in 0..32{assert!(poll_tokens(&mut poll,&mut events).is_empty());}
 state(id,|s|s.next_write_error=Some(-2));assert_eq!(stream.write(b"blocked").unwrap_err().kind(),ErrorKind::WouldBlock);
 assert_eq!(poll_tokens(&mut poll,&mut events),[8]);assert!(poll_tokens(&mut poll,&mut events).is_empty());
}
#[test]
fn readable_would_block_rearms_even_if_new_data_arrives_before_next_poll(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=1);poll.registry().register(&mut stream,Token(9),Interest::READABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[9]);assert!(poll_tokens(&mut poll,&mut events).is_empty());let mut buf=[0;4];
 assert_eq!(stream.read(&mut buf).unwrap_err().kind(),ErrorKind::WouldBlock);state(id,|s|s.rx.extend(b"data"));
 assert_eq!(poll_tokens(&mut poll,&mut events),[9]);assert_eq!(stream.read(&mut buf).unwrap(),4);assert_eq!(&buf,b"data");assert!(poll_tokens(&mut poll,&mut events).is_empty());
}
#[test]
fn rearm_during_native_probe_uses_current_delivered_mask(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=1);poll.registry().register(&mut stream,Token(11),Interest::READABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[11]);let stream=std::sync::Arc::new(Mutex::new(stream));let rearm=stream.clone();
 on_query(id,move||{let mut buf=[0;1];assert_eq!(rearm.lock().unwrap().read(&mut buf).unwrap_err().kind(),ErrorKind::WouldBlock);state(id,|s|s.rx.push_back(b'n'));});
 assert_eq!(poll_tokens(&mut poll,&mut events),[11],"a concurrent WouldBlock rearm was overwritten by the pre-probe delivered mask");
 assert!(poll_tokens(&mut poll,&mut events).is_empty());
}
#[test]
fn level_zero_then_new_level_creates_a_new_edge_without_io(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=1);poll.registry().register(&mut stream,Token(12),Interest::READABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[12]);state(id,|s|s.readiness=0);assert!(poll_tokens(&mut poll,&mut events).is_empty());state(id,|s|s.readiness=1);assert_eq!(poll_tokens(&mut poll,&mut events),[12]);
}
#[test]
fn consumed_error_rearms_error_without_repeating_closed_edges(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|{s.readiness=4|8;s.error=-6;});poll.registry().register(&mut stream,Token(13),Interest::READABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[13]);let event=events.iter().next().unwrap();assert!(event.is_error()&&event.is_read_closed());assert!(poll_tokens(&mut poll,&mut events).is_empty());
 assert!(stream.take_error().unwrap().is_some());state(id,|s|{s.readiness|=4;s.error=-6;});assert_eq!(poll_tokens(&mut poll,&mut events),[13]);let event=events.iter().next().unwrap();assert!(event.is_error());assert!(!event.is_read_closed());
 assert!(poll_tokens(&mut poll,&mut events).is_empty());
}
#[test]
fn shutdown_delivers_closed_edge_once_without_repeating_writable(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let(id,mut stream)=connect();state(id,|s|s.readiness=2);poll.registry().register(&mut stream,Token(14),Interest::READABLE|Interest::WRITABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[14]);assert!(events.iter().next().unwrap().is_writable());stream.shutdown(Shutdown::Write).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[14]);let event=events.iter().next().unwrap();assert!(event.is_write_closed());assert!(!event.is_writable());assert!(poll_tokens(&mut poll,&mut events).is_empty());
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_bind(addr:SocketAddrRaw,out:*mut u32)->i32{unsafe{trueos_mio_tcp_stream_connect(addr,out)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_listener_bind(addr:SocketAddrRaw,out:*mut u32)->i32{unsafe{trueos_mio_tcp_stream_connect(addr,out)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_listener_accept(_id:u32,_out:*mut u32,_addr:*mut SocketAddrRaw)->i32{-2}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_send_to(id:u32,_addr:SocketAddrRaw,input:*const u8,len:usize)->isize{unsafe{trueos_mio_tcp_stream_write(id,input,len)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_recv_from(id:u32,addr:*mut SocketAddrRaw,out:*mut u8,cap:usize)->isize{
 unsafe{addr.write(SocketAddrRaw{family:4,reserved:0,port:99,addr:[127,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0]});trueos_mio_tcp_stream_read(id,out,cap)}
}
fn new_socket_id()->u32{let id=NEXT.fetch_add(1,Ordering::Relaxed);SOCKETS.get_or_init(Default::default).lock().unwrap().insert(id,State::default());id}
#[test]
fn accept_would_block_rearms_listener_readiness(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let id=new_socket_id();let mut listener=mio::net::TcpListener::bind(core::net::SocketAddr::from(([127,0,0,1],id as u16))).unwrap();state(id,|s|s.readiness=1);poll.registry().register(&mut listener,Token(15),Interest::READABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[15]);assert!(poll_tokens(&mut poll,&mut events).is_empty());assert_eq!(listener.accept().unwrap_err().kind(),ErrorKind::WouldBlock);assert_eq!(poll_tokens(&mut poll,&mut events),[15]);
}
#[test]
fn udp_would_block_rearms_only_the_affected_direction(){
 reset_waits();let mut poll=Poll::new().unwrap();let mut events=Events::with_capacity(2);let id=new_socket_id();let address=core::net::SocketAddr::from(([127,0,0,1],id as u16));let mut udp=mio::net::UdpSocket::bind(address).unwrap();state(id,|s|s.readiness=3);poll.registry().register(&mut udp,Token(16),Interest::READABLE|Interest::WRITABLE).unwrap();
 assert_eq!(poll_tokens(&mut poll,&mut events),[16]);let event=events.iter().next().unwrap();assert!(event.is_readable()&&event.is_writable());assert!(poll_tokens(&mut poll,&mut events).is_empty());
 let mut buf=[0;3];assert_eq!(udp.recv_from(&mut buf).unwrap_err().kind(),ErrorKind::WouldBlock);state(id,|s|s.rx.extend(b"new"));assert_eq!(poll_tokens(&mut poll,&mut events),[16]);let event=events.iter().next().unwrap();assert!(event.is_readable());assert!(!event.is_writable());assert_eq!(udp.recv_from(&mut buf).unwrap().0,3);
 state(id,|s|s.next_write_error=Some(-2));assert_eq!(udp.send_to(b"out",address).unwrap_err().kind(),ErrorKind::WouldBlock);assert_eq!(poll_tokens(&mut poll,&mut events),[16]);let event=events.iter().next().unwrap();assert!(event.is_writable());assert!(!event.is_readable());
}
