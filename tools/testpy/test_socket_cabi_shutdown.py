#!/usr/bin/env python3
"""Host-test production CABI socket state, event pump and half-close functions."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import item
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    path = ROOT / 'src/r/net/socket_cabi.rs'
    source = path.read_text()
    production = '\n'.join(re.findall(r'^const .*?;', source, re.M))
    production += '\n' + '\n'.join(item(str(path), n) for n in ('RemoteAddr','SocketState'))
    state_impl = source.split('impl SocketState {', 1)[1].split('\n}\n', 1)[0]
    production += '\nimpl SocketState {' + state_impl + '\n}\n'
    production += '\n' + '\n'.join(function(path,n) for n in (
        'pump_socket','map_net_error','connect_inner','recv_inner','socket_tcp_send_host',
        'socket_tcp_recv_host','socket_tcp_shutdown_host',
    ))
    harness = r'''
#![allow(dead_code)]
use std::collections::VecDeque;
mod api {
    #[derive(Clone,Copy,Debug,PartialEq)] pub struct NetHandle(pub u32);
    #[derive(PartialEq)] pub enum SocketKind { Tcp, Tun }
    pub struct EndpointV4 { pub addr:[u8;4], pub port:u16 }
    pub struct EndpointV6 { pub addr:[u8;16], pub port:u16 }
    pub const MAX_MSG:usize=8;
    pub struct ByteBuf(Vec<u8>);
    impl ByteBuf {
        pub fn from_slice_trunc(bytes:&[u8])->Self { Self(bytes.to_vec()) }
        pub fn as_slice(&self)->&[u8] { &self.0 }
    }
    pub enum Command {
        OpenTcpConnect { remote:EndpointV4 }, OpenTcpConnectV6 { remote:EndpointV6 },
        SendTcp { handle:NetHandle,data:ByteBuf }, FinishTcp { handle:NetHandle },
    }
    pub enum Event {
        Opened { handle:NetHandle,kind:SocketKind }, TcpEstablished { handle:NetHandle },
        TcpData { handle:NetHandle,data:ByteBuf }, TcpReadClosed { handle:NetHandle },
        Closed { handle:NetHandle }, Error { msg:&'static str },
    }
}
struct VNetBridge;
struct BridgeState { events:VecDeque<api::Event>, commands:Vec<api::Command>, blocked:bool }
std::thread_local! {
    static WAKES:std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SOCKET:std::cell::RefCell<SocketState> = std::cell::RefCell::new(SocketState::new(None,2,1,6));
    static BRIDGE:std::cell::RefCell<BridgeState> = const { std::cell::RefCell::new(BridgeState { events:VecDeque::new(),commands:Vec::new(),blocked:false }) };
}
impl VNetBridge {
    fn open_primary()->Option<Self> { Some(Self) }
    fn pop_event(&self)->Option<api::Event> { BRIDGE.with(|b| b.borrow_mut().events.pop_front()) }
    fn submit(&self,cmd:api::Command)->Result<(),()> {
        BRIDGE.with(|b| { let mut b=b.borrow_mut(); if b.blocked { Err(()) } else { b.commands.push(cmd); Ok(()) } })
    }
}
mod net { pub fn device_count()->usize { 1 } }
mod wait {
    pub fn platform_wake_all_blueprint_io_waiters()->usize {
        // A mutable borrow stands in for the registry lock: waking while
        // with_socket_mut still holds it must fail this assertion.
        super::SOCKET.with(|s|assert!(s.try_borrow_mut().is_ok(), "wake under socket lock"));
        super::WAKES.with(|w|w.set(w.get()+1));
        1
    }
}
fn with_socket_mut<T>(id:u32,f:impl FnOnce(&mut SocketState)->Result<T,i32>)->Result<T,i32> {
    if id!=1 { return Err(-EBADF); }
    SOCKET.with(|s|f(&mut s.borrow_mut()))
}
fn finish_connect(_:u32,_:Option<u64>)->Result<(),i32> { Ok(()) }
fn wait_until(_:Option<u64>,mut ready:impl FnMut()->bool)->bool { ready() }
fn prepare() {
    WAKES.with(|w|w.set(0));
    BRIDGE.with(|b|*b.borrow_mut()=BridgeState { events:VecDeque::new(),commands:Vec::new(),blocked:false });
    SOCKET.with(|s| { let mut s=s.borrow_mut(); *s=SocketState::new(None,2,1,6);
        s.vnet=Some(VNetBridge); s.handle=Some(api::NetHandle(7)); s.connected=true; });
}
fn event(e:api::Event) { BRIDGE.with(|b|b.borrow_mut().events.push_back(e)); }
fn data(bytes:&[u8]) { event(api::Event::TcpData { handle:api::NetHandle(7),data:api::ByteBuf::from_slice_trunc(bytes) }); }
fn eof() { event(api::Event::TcpReadClosed { handle:api::NetHandle(7) }); }
fn read(out:&mut [u8],flags:i32)->isize { socket_tcp_recv_host(1,out,flags,1,0) }
#[test] fn write_shutdown_queues_finish_after_data_and_preserves_reply() {
    prepare();
    assert_eq!(socket_tcp_send_host(1,b"request"),7);
    assert_eq!(socket_tcp_shutdown_host(1,1),0);
    assert_eq!(socket_tcp_send_host(1,b"more"),-32);
    BRIDGE.with(|b| { let b=b.borrow();
        assert!(matches!(b.commands[0],api::Command::SendTcp { .. }));
        assert!(matches!(b.commands[1],api::Command::FinishTcp { handle:api::NetHandle(7) }));
        assert_eq!(b.commands.len(),2);
    });
    data(b"reply"); eof();
    let mut out=[0;16];
    assert_eq!(read(&mut out,MSG_PEEK),5); assert_eq!(&out[..5],b"reply");
    assert_eq!(read(&mut out,0),5); assert_eq!(&out[..5],b"reply");
    assert_eq!(read(&mut out,0),0);
    SOCKET.with(|s| { let s=s.borrow(); assert!(s.connected); assert!(!s.closed); assert!(s.handle.is_some()); });
}
#[test] fn read_shutdown_discards_queued_and_future_bytes_but_allows_send() {
    prepare(); data(b"old");
    assert_eq!(socket_tcp_shutdown_host(1,0),0);
    data(b"new"); let mut out=[0;16];
    assert_eq!(read(&mut out,0),0);
    SOCKET.with(|s|assert!(s.borrow().recv_buf.is_empty()));
    assert_eq!(socket_tcp_send_host(1,b"reply"),5);
    BRIDGE.with(|b|assert_eq!(b.borrow().commands.len(),1));
}
#[test] fn peer_eof_drains_buffer_and_still_allows_writes() {
    prepare(); data(b"abcdef"); eof();
    let mut out=[0;3];
    assert_eq!(read(&mut out,0),3); assert_eq!(&out,b"abc");
    assert_eq!(read(&mut out,MSG_PEEK),3); assert_eq!(&out,b"def");
    assert_eq!(read(&mut out,0),3); assert_eq!(&out,b"def");
    assert_eq!(read(&mut out,0),0);
    assert_eq!(socket_tcp_send_host(1,b"response"),8);
}
#[test] fn both_shutdown_and_repeat_calls_are_idempotent() {
    prepare(); data(b"queued");
    assert_eq!(socket_tcp_shutdown_host(1,2),0);
    for how in [0,1,2] { assert_eq!(socket_tcp_shutdown_host(1,how),0); }
    WAKES.with(|w|assert_eq!(w.get(),4,"every successful shutdown wakes"));
    let mut out=[0;8]; assert_eq!(read(&mut out,0),0);
    assert_eq!(socket_tcp_send_host(1,b"no"),-EPIPE as isize);
    BRIDGE.with(|b|assert_eq!(b.borrow().commands.len(),1));
}
#[test] fn backpressure_keeps_both_shutdown_retryable_without_dropping_data() {
    prepare(); data(b"retained");
    BRIDGE.with(|b|b.borrow_mut().blocked=true);
    assert_eq!(socket_tcp_shutdown_host(1,2),-EAGAIN);
    WAKES.with(|w|assert_eq!(w.get(),0,"failed shutdown must not wake"));
    SOCKET.with(|s| { let s=s.borrow(); assert!(!s.read_shutdown); assert!(!s.write_shutdown); assert_eq!(s.recv_buf.len(),8); });
    BRIDGE.with(|b|b.borrow_mut().blocked=false);
    assert_eq!(socket_tcp_send_host(1,b"still open"),10);
    assert_eq!(socket_tcp_shutdown_host(1,2),0);
    WAKES.with(|w|assert_eq!(w.get(),1,"successful retry must wake"));
    SOCKET.with(|s|assert!(s.borrow().recv_buf.is_empty()));
}
#[test] fn invalid_how_does_not_pump_or_modify_socket() {
    prepare(); data(b"queued");
    for how in [3,u32::MAX] { assert_eq!(socket_tcp_shutdown_host(1,how),-EINVAL); }
    SOCKET.with(|s| { let s=s.borrow(); assert!(s.connected); assert!(!s.read_shutdown); assert!(!s.write_shutdown); assert!(s.recv_buf.is_empty()); });
    BRIDGE.with(|b| { let b=b.borrow(); assert_eq!(b.events.len(),1); assert!(b.commands.is_empty()); });
    assert_eq!(socket_tcp_shutdown_host(99,1),-EBADF);
    SOCKET.with(|s|s.borrow_mut().connected=false);
    assert_eq!(socket_tcp_shutdown_host(1,1),-ENOTCONN);
    WAKES.with(|w|assert_eq!(w.get(),0,"invalid requests must not wake"));
}
#[test] fn reconnect_submission_resets_previous_shutdown_and_eof() {
    prepare();
    SOCKET.with(|s| { let mut s=s.borrow_mut(); s.connected=false; s.read_shutdown=true; s.write_shutdown=true;
        s.peer_eof=true; s.closed=true; s.recv_buf.extend(b"stale"); });
    assert_eq!(connect_inner(1,RemoteAddr::V4([127,0,0,1],80),true),Err(-EINPROGRESS));
    SOCKET.with(|s| { let s=s.borrow(); assert!(!s.read_shutdown); assert!(!s.write_shutdown); assert!(!s.peer_eof); assert!(!s.closed); assert!(s.recv_buf.is_empty()); });
}
#[test] fn unrelated_handle_eof_is_ignored_and_empty_open_is_would_block() {
    prepare(); event(api::Event::TcpReadClosed { handle:api::NetHandle(99) });
    let mut out=[0;1]; assert_eq!(read(&mut out,0),-EAGAIN as isize);
    SOCKET.with(|s|assert!(!s.borrow().peer_eof));
    assert_eq!(socket_tcp_recv_host(1,&mut [],0,1,0),0);
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-cabi-shutdown-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness + production)
        subprocess.run(['rustc','--edition=2024','--test','--target','x86_64-unknown-linux-gnu',str(rust),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
