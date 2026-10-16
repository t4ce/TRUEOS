#!/usr/bin/env python3
"""Join the real HTTP completion state with extracted TCP finish/flush code.

A bounded fake TCP send buffer separates local admission from peer wire/ACK
progress. It deliberately retains the last admitted bytes after TcpSent.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def method(source, name):
    start = source.index(f'    fn {name}(')
    end = source.index('\n    }', start) + len('\n    }')
    return source[start:end]


def main():
    adapter = (ROOT / 'src/net/adapter.rs').read_text()
    arms = adapter.split('            NetCommand::Close { handle } => {', 1)[1].split('\n        }\n    }', 1)[0]
    arms = '            NetCommand::Close { handle } => {' + arms
    harness = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
#[path="@VNET@"] mod vnet;
use vnet as api;
use api::NetHandle;
#[path="@HTTP@"] mod vhttp_srv;
#[path="@QUEUE@"] mod tcp_tx_queue;
use tcp_tx_queue::TcpTxQueue;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64,Ordering};
#[macro_export] macro_rules! log_info { ($($arg:tt)*)=>{}; }
mod log_os { pub mod flags { pub const NET_LOG_TCP_SEND_FLUSH:bool=false; } }
static LOGTOTCP_SEND_FLUSH_SUPPRESSED:AtomicU64=AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_BYTES:AtomicU64=AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_LAST_LOG_NS:AtomicU64=AtomicU64::new(0);
fn net_log_once_per_second(_: &AtomicU64)->bool { false }
#[derive(Clone, Copy, PartialEq)] enum SocketKind { Tcp, Udp }
enum NetCommand { Close {handle:NetHandle}, FinishTcp {handle:NetHandle} }
enum NetEvent { TcpSent {handle:NetHandle,len:usize}, Closed {handle:NetHandle}, Error {msg:&'static str} }
std::thread_local! { static EVENTS:std::cell::RefCell<VecDeque<NetEvent>>=const { std::cell::RefCell::new(VecDeque::new()) }; }
fn push_event(_: &'static str,event:NetEvent)->bool { EVENTS.with(|q|q.borrow_mut().push_back(event));true }
mod tcp {
    use super::*;
    pub struct Socket { pub bytes:VecDeque<u8>, pub wire:Vec<u8>, pub capacity:usize, pub fin_requested:bool, pub fin_on_wire:bool }
    impl Socket {
        pub fn can_send(&self)->bool {self.bytes.len()<self.capacity&&!self.fin_requested}
        pub fn may_send(&self)->bool {!self.fin_requested}
        pub fn send_slice(&mut self,data:&[u8])->Result<usize,()> {
            let n=data.len().min(self.capacity-self.bytes.len());
            self.bytes.extend(data[..n].iter().copied());Ok(n)
        }
        pub fn close(&mut self) { self.fin_requested=true; }
        // Peer progress, not application send admission. FIN follows all bytes.
        pub fn peer_ack(&mut self,budget:usize) {
            let n=budget.min(self.bytes.len());
            self.wire.extend(self.bytes.drain(..n));
            if self.fin_requested&&self.bytes.is_empty() {self.fin_on_wire=true;}
        }
    }
}
struct Sockets { socket:Option<tcp::Socket> }
impl Sockets {
    fn get_mut<T>(&mut self,_:usize)->&mut tcp::Socket {self.socket.as_mut().unwrap()}
    fn remove(&mut self,_:usize)->Option<tcp::Socket> {self.socket.take()}
}
struct Record { owner:&'static str,handle:NetHandle,kind:SocketKind,socket:usize,tcp_tx:TcpTxQueue,tcp_finish_pending:bool,tcp_write_closed:bool,tcp_loopback_peer:Option<NetHandle> }
struct Service {records:Vec<Record>,sockets:Sockets}
impl Service {
    @FLUSH@
    @REMOVE@
    fn flush_loopback_tcp_tx(&mut self,_:usize) {panic!("not a loopback fixture")}
    fn remove_tun_record(&mut self,_:NetHandle)->bool {false}
    fn close_loopback_tcp(&mut self,_:NetHandle)->bool {false}
    fn command(&mut self,owner:&'static str,command:NetCommand) {
        match command { @ARMS@ }
    }
}
fn setup(keepalive:bool)->(vhttp_srv::HttpServer,Service,Vec<u8>) {
    EVENTS.with(|q|q.borrow_mut().clear());
    let handle=NetHandle(1);
    let mut server=vhttp_srv::HttpServer::with_session_limit(80,1024,1);
    server.on_event(api::Event::Opened {handle,kind:api::SocketKind::Tcp});
    server.on_event(api::Event::TcpEstablished {handle,peer:None,peer6:None});
    let body:Vec<u8>=(0..=255).cycle().take(512).collect();
    let mut commands=Vec::new();
    let pending=vhttp_srv::queue_response_head(&mut commands,handle,"HTTP/1.1 200 OK\r\n","application/octet-stream","",body.len() as u64,keepalive);
    server.mark_response(handle,pending,keepalive);
    vhttp_srv::queue_send_bytes(&mut commands,handle,&body);
    let mut expected=Vec::new();let mut tx=TcpTxQueue::new();
    for command in commands {
        match command {api::Command::SendTcp {data,..}=>{expected.extend_from_slice(data.as_slice());tx.push(data.as_slice().to_vec());},_=>panic!("unexpectedresponsecommand")}
    }
    let service=Service {records:vec![Record {owner:"fixture",handle,kind:SocketKind::Tcp,socket:0,tcp_tx:tx,tcp_finish_pending:false,tcp_write_closed:false,tcp_loopback_peer:None}],sockets:Sockets {socket:Some(tcp::Socket {bytes:VecDeque::new(),wire:Vec::new(),capacity:16,fin_requested:false,fin_on_wire:false})}};
    (server,service,expected)
}
fn events(server:&mut vhttp_srv::HttpServer,service:&mut Service)->usize {
    let mut finishes=0;
    while let Some(event)=EVENTS.with(|q|q.borrow_mut().pop_front()) {
        let event=match event {NetEvent::TcpSent {handle,len}=>api::Event::TcpSent {handle,len:len.try_into().unwrap()},NetEvent::Closed {handle}=>api::Event::Closed {handle},NetEvent::Error {msg}=>panic!("unexpected {msg}")};
        match server.on_event(event) {
            vhttp_srv::HttpServerEvent::Submit(api::Command::FinishTcp {handle})=>{finishes+=1;service.command("fixture",NetCommand::FinishTcp {handle});},
            vhttp_srv::HttpServerEvent::Submit(api::Command::Close {..})=>panic!("successfulresponseaborteditsTCPbuffer"),
            vhttp_srv::HttpServerEvent::None=>{},
            _=>panic!("unexpectedservertransition"),
        }
    }
    finishes
}
#[test] fn response_admission_finishes_without_discarding_unacknowledged_bytes() {
    let (mut server,mut service,expected)=setup(false);
    let mut finishes=0;
    for _ in 0..200 {
        service.flush_tcp_tx(0);finishes+=events(&mut server,&mut service);
        if finishes!=0 {break;}
        service.sockets.socket.as_mut().unwrap().peer_ack(7);
    }
    assert_eq!(finishes,1);
    assert_eq!(service.records.len(),1);
    let socket=service.sockets.socket.as_mut().unwrap();
    assert!(socket.fin_requested);
    assert!(!socket.fin_on_wire);
    assert!(!socket.bytes.is_empty(),"the local send-completion edge precedes wire completion");
    assert!(socket.wire.len()<expected.len());
    while !socket.fin_on_wire {socket.peer_ack(3);}
    assert_eq!(socket.wire,expected);
    assert!(socket.bytes.is_empty());
    // The disconnect event, rather than local admission, releases HTTP capacity.
    service.remove_record(NetHandle(1));
    assert!(matches!(server.on_event(api::Event::Closed {handle:NetHandle(1)}),vhttp_srv::HttpServerEvent::Submit(api::Command::OpenTcpListen {port:80})));
    assert!(service.records.is_empty());
}
#[test] fn keepalive_never_requests_fin_after_identical_send_admission() {
    let (mut server,mut service,expected)=setup(true);
    for _ in 0..200 {
        service.flush_tcp_tx(0);
        assert_eq!(events(&mut server,&mut service),0);
        service.sockets.socket.as_mut().unwrap().peer_ack(7);
        if service.sockets.socket.as_ref().unwrap().wire.len()==expected.len() {break;}
    }
    let socket=service.sockets.socket.as_ref().unwrap();
    assert_eq!(socket.wire,expected);
    assert!(!socket.fin_requested&&!socket.fin_on_wire);
    assert_eq!(service.records.len(),1);
}
#[test] fn abort_command_still_discards_buffer_and_reports_disconnect() {
    let (mut server,mut service,_)=setup(false);
    service.flush_tcp_tx(0);
    assert!(!service.sockets.socket.as_ref().unwrap().bytes.is_empty());
    service.command("fixture",NetCommand::Close {handle:NetHandle(1)});
    assert!(service.sockets.socket.is_none());
    assert!(service.records.is_empty());
    while let Some(event)=EVENTS.with(|q|q.borrow_mut().pop_front()) {
        if let NetEvent::Closed {handle}=event {
            assert!(matches!(server.on_event(api::Event::Closed {handle}),vhttp_srv::HttpServerEvent::Submit(api::Command::OpenTcpListen {port:80})));
            return;
        }
    }
    panic!("abortmustreportdisconnect");
}
#[test] fn finish_with_adapter_backlog_waits_for_tcp_buffer_capacity() {
    let (_,mut service,expected)=setup(false);
    service.flush_tcp_tx(0);
    service.command("fixture",NetCommand::FinishTcp {handle:NetHandle(1)});
    assert!(service.records[0].tcp_finish_pending);
    assert!(!service.sockets.socket.as_ref().unwrap().fin_requested);
    for _ in 0..250 {
        service.sockets.socket.as_mut().unwrap().peer_ack(5);
        service.flush_tcp_tx(0);
        if service.sockets.socket.as_ref().unwrap().fin_on_wire {break;}
    }
    let socket=service.sockets.socket.as_ref().unwrap();
    assert!(socket.fin_on_wire);
    assert_eq!(socket.wire,expected);
    assert!(service.records[0].tcp_tx.is_empty());
}
'''
    for key, value in {
        '@VNET@': str(ROOT / 'crates/trueos-v/src/vnet.rs'),
        '@HTTP@': str(ROOT / 'crates/trueos-v/src/vhttp_srv.rs'),
        '@QUEUE@': str(ROOT / 'src/net/tcp_tx_queue.rs'),
        '@FLUSH@': method(adapter, 'flush_tcp_tx'),
        '@REMOVE@': method(adapter, 'remove_record'),
        '@ARMS@': arms,
    }.items():
        harness = harness.replace(key, value)
    with tempfile.TemporaryDirectory(prefix='trueos-http-finish-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', '--target', 'x86_64-unknown-linux-gnu', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
