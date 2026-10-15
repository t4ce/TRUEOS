#!/usr/bin/env python3
"""Compile actual Mio half-close, readiness, writes, and POSIX shutdown dispatch."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[2]


def function(path, name):
    source = (ROOT / path).read_text()
    match = re.search(rf'^(?:pub(?:\([^)]*\))? )?(?:unsafe )?(?:extern "C" )?fn {name}\(', source, re.M)
    assert match, name
    end = re.search(r'^}\n', source[match.end():], re.M)
    return source[match.start():match.end() + end.end()]


def main():
    mio = 'src/mio_compat.rs'
    shim = 'src/std_abi_shim.rs'
    production = '\n'.join(function(mio, n) for n in (
        'mio_socket_shutdown_host', 'mio_tcp_stream_write_host', 'owner_matches',
    ))
    production += '\n' + item(mio, 'MioSocketKind') + '\n' + item(mio, 'MioSocketState')
    production += '\n' + '\n'.join(constant(mio, n) for n in (
        'STATUS_OK', 'STATUS_NOT_CONNECTED', 'STATUS_INVALID_INPUT', 'STATUS_NOT_FOUND',
        'STATUS_BROKEN_PIPE', 'STATUS_WOULD_BLOCK', 'MIO_TCP_TX_WINDOW_BYTES',
        'READY_READABLE', 'READY_WRITABLE', 'READY_ERROR', 'READY_READ_CLOSED', 'READY_WRITE_CLOSED',
    ))
    source = (ROOT / mio).read_text()
    start = source.index('    fn ready_mask(')
    end = source.index('\n    }', start) + len('\n    }')
    readiness = source[start:end]
    abi = function(shim, 'shutdown').replace('fn shutdown(', 'fn native_shutdown(')
    abi += '\n' + '\n'.join(function(shim, n) for n in (
        'posix_rc_i32', 'posix_mio_i32', 'mio_status_to_errno',
    ))
    abi += '\n' + '\n'.join(constant(shim, n) for n in (
        'TRUEOS_EINVAL', 'TRUEOS_ENOTCONN', 'TRUEOS_EBADF', 'TRUEOS_EAGAIN', 'TRUEOS_EIO', 'TRUEOS_EPIPE',
    ))
    harness = r'''
#![allow(dead_code, unused_variables, non_camel_case_types, unsafe_op_in_unsafe_fn)]
extern crate self as v;
#[path = "@API@"] pub mod vnet;
use vnet as api;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
#[macro_export] macro_rules! log { ($($args:tt)*) => {} }
type CompatAddr = ();
type c_int = i32;
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
std::thread_local! {
    static OWNER:std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    static STATE:std::cell::RefCell<MioCompat> = std::cell::RefCell::new(MioCompat {
        socket: new_socket(), submitted:Vec::new(), submit_error:None,
    });
    static CABI_CALL:std::cell::Cell<Option<(u32,u32)>> = const { std::cell::Cell::new(None) };
    static WAKES:std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn new_socket()->MioSocketState {
    MioSocketState { id:1, owner_vm:Some(7), kind:MioSocketKind::TcpStream,
        handle:Some(api::NetHandle(42)), listen_handles:Vec::new(),local:None,peer:None,
        listen_port:None,connected:true,closed:false,read_shutdown:false,write_shutdown:false,
        peer_eof:false,error:STATUS_OK,tx_in_flight:0,rx_stream:b"witness".iter().copied().collect(),
        rx_dgrams:VecDeque::new(),accept_queue:VecDeque::new() }
}
struct MioCompat { socket:MioSocketState, submitted:Vec<api::Command>, submit_error:Option<i32> }
impl MioCompat {
    fn pump(&mut self) {}
    fn socket_for_owner(&self,id:u32,owner:Option<u8>)->Option<&MioSocketState> {
        (id==self.socket.id && owner_matches(self.socket.owner_vm,owner)).then_some(&self.socket)
    }
    fn socket_mut_for_owner(&mut self,id:u32,owner:Option<u8>)->Option<&mut MioSocketState> {
        if id==self.socket.id && owner_matches(self.socket.owner_vm,owner) { Some(&mut self.socket) } else { None }
    }
    fn submit(&mut self,cmd:api::Command)->Result<(),i32> {
        if let Some(error)=self.submit_error { Err(error) } else { self.submitted.push(cmd); Ok(()) }
    }
    @READINESS@
}
mod wait {
    pub fn platform_wake_all_blueprint_io_waiters()->usize {
        super::STATE.with(|state| assert!(state.try_borrow_mut().is_ok(),
            "Mio state lock must be released before waking waiters"));
        super::WAKES.with(|w|w.set(w.get()+1));
        1
    }
}
fn wake_count()->usize { WAKES.with(|w|w.get()) }
fn current_owner_vm()->Option<u8> { OWNER.with(|o|o.get()) }
fn with_compat<R>(f:impl FnOnce(&mut MioCompat)->R)->R { STATE.with(|s|f(&mut s.borrow_mut())) }
fn tcp_flow_logging_enabled(_: &MioSocketState)->bool { false }
fn ready()->u8 { with_compat(|c|c.ready_mask(&c.socket,READY_READABLE|READY_WRITABLE)) }
fn write()->isize { unsafe { mio_tcp_stream_write_host(1,b"reply".as_ptr(),5) } }
fn reset()->std::sync::MutexGuard<'static,()> {
    let guard=SERIAL.lock().unwrap();
    OWNER.with(|o|o.set(None));
    with_compat(|c| { c.socket=new_socket(); c.submitted.clear(); c.submit_error=None; });
    SOCKET_FDS.lock().entry=Some(SocketFd::MioStream { backend:1 });
    CABI_CALL.with(|c|c.set(None));
    WAKES.with(|w|w.set(0));
    TRUEOS_ERRNO.store(0,Ordering::Relaxed);
    guard
}
enum SocketFd { Cabi { backend:u32 }, PendingListener { backend:u32 }, MioStream { backend:u32 }, Other }
struct Fds { entry:Option<SocketFd> }
impl Fds { fn get(&self,id:i32)->Option<&SocketFd> { if id==9 { self.entry.as_ref() } else { None } } }
struct FdLock(std::sync::Mutex<Fds>);
impl FdLock { fn lock(&self)->std::sync::MutexGuard<'_,Fds> { self.0.lock().unwrap() } }
static SOCKET_FDS:FdLock=FdLock(std::sync::Mutex::new(Fds { entry:None }));
mod mio_compat {
    pub fn mio_socket_shutdown(id:u32,how:u32)->i32 {
        assert!(super::SOCKET_FDS.0.try_lock().is_ok(),"FD lock held during Mio shutdown");
        super::mio_socket_shutdown_host(id,how)
    }
}
mod r { pub mod net { pub mod socket_cabi {
    pub unsafe fn trueos_cabi_socket_tcp_shutdown(id:u32,how:u32)->i32 {
        assert!(super::super::super::SOCKET_FDS.0.try_lock().is_ok(),"FD lock held during CABI shutdown");
        super::super::super::CABI_CALL.with(|c|c.set(Some((id,how))));
        -32
    }
} } }
@PRODUCTION@
@ABI@
#[test] fn write_shutdown_is_idempotent_preserves_receive_and_blocks_new_writes() {
    let _serial=reset();
    assert_eq!(write(),5);
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_OK);
    assert_eq!(wake_count(),1);
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_OK);
    assert_eq!(wake_count(),2);
    with_compat(|c| {
        assert_eq!(c.submitted.len(),2);
        assert!(matches!(c.submitted[0],api::Command::SendTcp { .. }));
        assert_eq!(c.submitted[1],api::Command::FinishTcp { handle:api::NetHandle(42) });
        assert_eq!(c.socket.rx_stream.iter().copied().collect::<Vec<_>>(),b"witness");
        assert_eq!(c.socket.handle,Some(api::NetHandle(42)));
        assert!(!c.socket.closed && !c.socket.read_shutdown && c.socket.write_shutdown);
    });
    assert_eq!(ready(),READY_READABLE|READY_WRITE_CLOSED);
    assert_eq!(write(),STATUS_BROKEN_PIPE as isize);
    assert_eq!(mio_status_to_errno(STATUS_BROKEN_PIPE),TRUEOS_EPIPE);
}
#[test] fn read_shutdown_discards_receive_sends_no_fin_and_preserves_write() {
    let _serial=reset();
    assert_eq!(mio_socket_shutdown_host(1,0),STATUS_OK);
    assert_eq!(wake_count(),1);
    with_compat(|c| { assert!(c.socket.rx_stream.is_empty()); assert!(c.submitted.is_empty());
        assert!(c.socket.read_shutdown && !c.socket.write_shutdown); });
    assert_eq!(ready(),READY_READABLE|READY_WRITABLE|READY_READ_CLOSED);
    assert_eq!(write(),5);
}
#[test] fn both_closes_both_halves_once_without_removing_handle() {
    let _serial=reset();
    for _ in 0..2 { assert_eq!(mio_socket_shutdown_host(1,2),STATUS_OK); }
    assert_eq!(wake_count(),2);
    with_compat(|c| { assert!(c.socket.rx_stream.is_empty()); assert_eq!(c.submitted.len(),1);
        assert!(c.socket.read_shutdown && c.socket.write_shutdown);
        assert_eq!(c.socket.handle,Some(api::NetHandle(42))); });
    assert_eq!(ready(),READY_READABLE|READY_READ_CLOSED|READY_WRITE_CLOSED);
}
#[test] fn pressure_does_not_commit_shutdown_or_discard_buffered_receive() {
    let _serial=reset();
    for how in [1,2] {
        with_compat(|c|c.submit_error=Some(STATUS_WOULD_BLOCK));
        assert_eq!(mio_socket_shutdown_host(1,how),STATUS_WOULD_BLOCK);
        assert_eq!(wake_count(),0);
        with_compat(|c| { assert!(!c.socket.read_shutdown && !c.socket.write_shutdown);
            assert_eq!(c.socket.rx_stream.len(),7); assert!(c.submitted.is_empty()); });
    }
    with_compat(|c|c.submit_error=None);
    assert_eq!(mio_socket_shutdown_host(1,2),STATUS_OK);
    assert_eq!(wake_count(),1);
}
#[test] fn invalid_how_owner_handle_kind_and_connection_preserve_state() {
    let _serial=reset();
    assert_eq!(mio_socket_shutdown_host(1,3),STATUS_INVALID_INPUT);
    assert_eq!(mio_socket_shutdown_host(99,1),STATUS_NOT_FOUND);
    OWNER.with(|o|o.set(Some(8)));
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_NOT_FOUND);
    OWNER.with(|o|o.set(Some(7)));
    with_compat(|c|c.socket.kind=MioSocketKind::Udp);
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_NOT_CONNECTED);
    with_compat(|c| { c.socket.kind=MioSocketKind::TcpStream; c.socket.connected=false; });
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_NOT_CONNECTED);
    with_compat(|c| { c.socket.connected=true; c.socket.closed=true; });
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_NOT_CONNECTED);
    with_compat(|c| { c.socket.closed=false; c.socket.handle=None; });
    assert_eq!(mio_socket_shutdown_host(1,1),STATUS_NOT_CONNECTED);
    assert_eq!(wake_count(),0);
    with_compat(|c| { assert!(!c.socket.write_shutdown && !c.socket.read_shutdown);
        assert_eq!(c.socket.rx_stream.len(),7); assert!(c.submitted.is_empty()); });
}
#[test] fn peer_eof_is_readable_after_buffer_drains_and_still_allows_reply() {
    let _serial=reset();
    with_compat(|c|c.socket.peer_eof=true);
    assert_eq!(ready(),READY_READABLE|READY_WRITABLE|READY_READ_CLOSED);
    with_compat(|c|c.socket.rx_stream.clear());
    assert_eq!(ready(),READY_READABLE|READY_WRITABLE|READY_READ_CLOSED);
    assert_eq!(write(),5);
}
#[test] fn native_shutdown_routes_exact_backend_with_lock_released_and_maps_errors() {
    let _serial=reset();
    assert_eq!(unsafe { native_shutdown(9,1) },0);
    assert_eq!(write(),STATUS_BROKEN_PIPE as isize);
    SOCKET_FDS.lock().entry=Some(SocketFd::Cabi { backend:77 });
    assert_eq!(unsafe { native_shutdown(9,2) },-1);
    assert_eq!(CABI_CALL.with(|c|c.get()),Some((77,2)));
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EPIPE);
    SOCKET_FDS.lock().entry=Some(SocketFd::PendingListener { backend:88 });
    assert_eq!(unsafe { native_shutdown(9,0) },-1);
    assert_eq!(CABI_CALL.with(|c|c.get()),Some((88,0)));
    SOCKET_FDS.lock().entry=Some(SocketFd::Other);
    assert_eq!(unsafe { native_shutdown(9,1) },-1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOTCONN);
    assert_eq!(unsafe { native_shutdown(-1,1) },-1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);
    assert_eq!(unsafe { native_shutdown(9,-1) },-1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);
    assert_eq!(mio_status_to_errno(-3),TRUEOS_EIO);
}
'''
    harness = harness.replace('@API@', str(ROOT / 'crates/trueos-v/src/vnet.rs'))
    harness = harness.replace('@READINESS@', readiness).replace('@PRODUCTION@', production).replace('@ABI@', abi)
    with tempfile.TemporaryDirectory(prefix='trueos-mio-shutdown-') as directory:
        source = Path(directory) / 'shutdown.rs'
        binary = Path(directory) / 'shutdown'
        source.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', '--target', 'x86_64-unknown-linux-gnu',
                        str(source), '-o', str(binary)], check=True, cwd=ROOT)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
