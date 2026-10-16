#!/usr/bin/env python3
"""Compile production TCP receive helpers and native recv's Mio dispatch.

The socket registry and hardware pump are mocked; byte copying, consuming versus
peeking, EOF, zero-capacity handling, and MSG_PEEK selection are production code.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def extract(source, name):
    match = re.search(rf'^(?:pub(?:\([^)]*\))? )?unsafe (?:extern "C" )?fn {name}\(', source, re.M)
    assert match, name
    end = re.search(r'^}\n', source[match.end():], re.M)
    return source[match.start():match.end() + end.end()]


def main():
    mio = (ROOT / 'src/mio_compat.rs').read_text()
    shim = (ROOT / 'src/std_abi_shim.rs').read_text()
    production = '\n'.join(extract(mio, n) for n in (
        'mio_tcp_stream_receive_host', 'mio_tcp_stream_read_host', 'mio_tcp_stream_peek_host',
        'mio_tcp_stream_receive', 'trueos_mio_tcp_stream_read', 'trueos_mio_tcp_stream_peek',
    ))
    # Exercise the exact native recv dispatch, including its blocking retry rule.
    recv = extract(shim, 'recv')
    dispatch = recv.split('    let raw = if is_mio {', 1)[1].split('\n    } else {', 1)[0]
    dispatch = dispatch.replace('crate::mio_compat::trueos_mio_tcp_stream_peek', 'mio_tcp_stream_peek_host')
    dispatch = dispatch.replace('crate::mio_compat::trueos_mio_tcp_stream_read', 'mio_tcp_stream_read_host')
    vmcall_source = (ROOT / 'src/hv/vmcall.rs').read_text()
    dispatcher = vmcall_source.split('        OP_BP_MIO_TCP_STREAM_READ => {', 1)[1].split('        OP_BP_MIO_TCP_STREAM_WRITE => {', 1)[0].rsplit('}', 1)[0]
    flag = re.search(r'^pub const MIO_TCP_READ_PEEK:.*?;', vmcall_source, re.M).group()
    guest_flag = re.search(r'^pub const MIO_TCP_READ_PEEK:.*?;', (ROOT / 'crates/trueos-vm/src/vmcall.rs').read_text(), re.M).group()
    assert flag == guest_flag, 'host and guest peek flag ABI must agree'
    constants = flag + '\n' + '\n'.join(re.search(rf'^(?:pub\(crate\) )?const {n}:.*?;', shim, re.M).group() for n in (
        'TRUEOS_MSG_PEEK', 'TRUEOS_MSG_DONTWAIT', 'TRUEOS_EINVAL',
    ))
    progress_helper = mio.split('fn compat_progress(', 1)[1].split('\n}\n', 1)[0]
    progress_helper = 'fn compat_progress(' + progress_helper + '\n}\n'
    harness = r'''
#![allow(dead_code, unused_variables, non_camel_case_types, unsafe_op_in_unsafe_fn)]
extern crate alloc;
#[path = "@PROGRESS@"] mod progress;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
type c_int = i32;
const STATUS_INVALID_INPUT: i32 = -1;
const STATUS_NOT_FOUND: i32 = -3;
const STATUS_WOULD_BLOCK: i32 = -2;
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
#[macro_export] macro_rules! log { ($($args:tt)*) => {} }
#[derive(PartialEq)] enum MioSocketKind { TcpStream, UdpSocket }
struct Socket { id:u32, owner_vm:Option<u8>, progress_generation:u64, kind:MioSocketKind, closed:bool, read_shutdown:bool, peer_eof:bool, rx_stream:VecDeque<u8> }
struct Compat { sockets:Vec<Socket>, progress:[progress::Counters;1] }
impl Compat {
    fn pump(&mut self) {}
    fn socket_mut_for_owner(&mut self, id:u32, _:Option<u8>) -> Option<&mut Socket> {
        self.sockets.iter_mut().find(|socket|socket.id==id)
    }
}
std::thread_local! {
    static STATE:std::cell::RefCell<Compat> = std::cell::RefCell::new(Compat {
        sockets:vec![Socket { id:1, owner_vm:Some(0), progress_generation:1, kind:MioSocketKind::TcpStream, closed:false,
                        read_shutdown:false, peer_eof:false, rx_stream:VecDeque::new() }],
        progress:[progress::Counters::new();1]
    });
}
fn with_compat<R>(f:impl FnOnce(&mut Compat)->R)->R { STATE.with(|s| f(&mut s.borrow_mut())) }
std::thread_local! {
    static MODE:std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static PAYLOAD:std::cell::UnsafeCell<Page> = const { std::cell::UnsafeCell::new(Page { payload:[0;PAYLOAD_CAP] }) };
    static RESPONSE:std::cell::Cell<(u64,u64,u32)> = const { std::cell::Cell::new((0,0,0)) };
}
const PAYLOAD_CAP:usize=16;
const STATUS_OK:u64=0;
const STATUS_BAD_ARG:u64=1;
struct Page { payload:[u8;PAYLOAD_CAP] }
enum DispatchOutcome { Resume }
fn host_ptr(_:u8)->Option<*mut Page> { Some(PAYLOAD.with(|p| p.get())) }
fn write_response(_:u8,_:u64,status:u64,data:u64,len:u32) { RESPONSE.with(|r|r.set((status,data,len))); }
fn dispatch(vm_id:u8, seq:u64, arg0:u64, arg1:u64)->DispatchOutcome { @DISPATCHER@ }
mod trueos_vm { pub mod vmcall {
    pub const MIO_TCP_READ_PEEK:u64=super::super::MIO_TCP_READ_PEEK;
    pub const OP_BP_MIO_TCP_STREAM_READ:u32=0x57;
    pub const PAYLOAD_CAP:usize=super::super::PAYLOAD_CAP;
    pub const STATUS_OK:u64=0;
    pub fn call_with_payload(op:u32,id:u64,cap:u64,_:&[u8],out:&mut [u8])->(u64,u64) {
        assert_eq!(op,OP_BP_MIO_TCP_STREAM_READ);
        super::super::dispatch(1,0,id,cap);
        let (status,data,len)=super::super::RESPONSE.with(|r|r.get());
        super::super::PAYLOAD.with(|p| unsafe { out[..len as usize].copy_from_slice(&(&(*p.get()).payload)[..len as usize]); });
        (status,data)
    }
} }
mod mio_compat { pub(crate) use super::{mio_tcp_stream_read_host,mio_tcp_stream_peek_host}; }
mod hv {
    pub fn current_hull_guest_context_vm_id()->Option<u8> { Some(1) }
    pub fn with_guest_broker_context<R>(_:u8,f:impl FnOnce()->R)->R { f() }
}
fn vmcall_isize(data:u64)->isize { data as i64 as isize }
fn vm_guest_vmcall_active()->bool { MODE.with(|m| m.get()==1) }
fn owner_matches(socket_owner:Option<u8>, requested:Option<u8>)->bool { requested.is_none()||socket_owner==requested }
mod allcaps { pub mod hv { pub const CODEX_PROGRESS_PROBES_ENABLED:bool=true; } }
fn current_owner_vm()->Option<u8> { MODE.with(|m| if m.get()==2 { Some(1) } else { None }) }
fn socket_owner_vm(_:u32)->Option<u8> { MODE.with(|m| if m.get()==3 { Some(1) } else { None }) }
fn copy_to_active_guest(_:u8,out:*mut u8,bytes:&[u8])->bool {
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(),out,bytes.len()); } true
}
fn copy_to_guest_stack(vm:u8,out:*mut u8,bytes:&[u8])->bool { copy_to_active_guest(vm,out,bytes) }
fn tcp_flow_logging_enabled(_: &Socket)->bool { false }
fn should_log_tcp_read_would_block()->bool { false }
unsafe fn abi_write_bytes<'a>(p:*mut u8, len:usize)->Option<&'a mut [u8]> {
    if len==0 { Some(&mut []) } else if p.is_null() { None }
    else { Some(unsafe { std::slice::from_raw_parts_mut(p,len) }) }
}
fn wait_for_socket_io(_:i32, _:u32)->bool { panic!("nonblocking and zero-length receive must not wait") }
unsafe fn native_recv(buf:*mut u8, len:usize, flags:i32)->isize {
    let backend=1; let socket_id=1; let nonblocking=true;
    @DISPATCH@
}
fn prepare(bytes:&[u8], closed:bool) {
    with_compat(|c| { c.sockets[0].owner_vm=current_owner_vm(); c.sockets[0].rx_stream=bytes.iter().copied().collect();
        c.sockets[0].closed=closed; c.sockets[0].read_shutdown=false; c.sockets[0].peer_eof=false; });
}
#[test] fn repeated_native_peeks_preserve_the_witness_for_read() {
    prepare(b"peek-witness",true);
    let mut out=[0;32];
    for _ in 0..3 {
        assert_eq!(unsafe { native_recv(out.as_mut_ptr(),out.len(),TRUEOS_MSG_PEEK) },12);
        assert_eq!(&out[..12],b"peek-witness");
    }
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),out.len(),0) },12);
    assert_eq!(&out[..12],b"peek-witness");
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),out.len(),0) },0);
}
#[test] fn short_peek_then_short_read_retains_stream_order() {
    prepare(b"abcdefgh",false);
    let mut out=[0;3];
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),3,TRUEOS_MSG_PEEK|TRUEOS_MSG_DONTWAIT) },3);
    assert_eq!(&out,b"abc");
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),2,0) },2);
    assert_eq!(&out[..2],b"ab");
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),3,TRUEOS_MSG_PEEK) },3);
    assert_eq!(&out,b"cde");
    with_compat(|c| assert_eq!(c.sockets[0].rx_stream.iter().copied().collect::<Vec<_>>(),b"cdefgh"));
}
#[test] fn empty_open_is_would_block_and_empty_closed_is_eof() {
    let mut out=[0;1];
    for flags in [0,TRUEOS_MSG_PEEK,TRUEOS_MSG_PEEK|TRUEOS_MSG_DONTWAIT] {
        prepare(b"",false);
        assert_eq!(unsafe { native_recv(out.as_mut_ptr(),1,flags) },-2);
        prepare(b"",true);
        assert_eq!(unsafe { native_recv(out.as_mut_ptr(),1,flags) },0);
    }
}
#[test] fn zero_capacity_succeeds_without_consuming_or_waiting() {
    for bytes in [&b""[..],&b"pending"[..]] {
        prepare(bytes,false);
        for flags in [0,TRUEOS_MSG_PEEK] {
            assert_eq!(unsafe { native_recv(std::ptr::null_mut(),0,flags) },0);
            with_compat(|c| assert_eq!(c.sockets[0].rx_stream.len(),bytes.len()));
        }
    }
}
#[test] fn invalid_pointer_and_socket_are_rejected_without_consumption() {
    prepare(b"data",false);
    let mut out=[0;4];
    assert_eq!(unsafe { mio_tcp_stream_peek_host(1,std::ptr::null_mut(),1) },-1);
    assert_eq!(unsafe { mio_tcp_stream_peek_host(999,out.as_mut_ptr(),4) },-3);
    assert_eq!(unsafe { mio_tcp_stream_read_host(1,out.as_mut_ptr(),4) },4);
    assert_eq!(&out,b"data");
}
#[test] fn peer_half_close_drains_buffer_before_eof_and_keeps_peek_non_consuming() {
    prepare(b"last",false);
    with_compat(|c|c.sockets[0].peer_eof=true);
    let mut out=[0;4];
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),4,TRUEOS_MSG_PEEK) },4);
    assert_eq!(&out,b"last");
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),4,0) },4);
    assert_eq!(&out,b"last");
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),4,TRUEOS_MSG_PEEK) },0);
    assert_eq!(unsafe { native_recv(out.as_mut_ptr(),4,0) },0);
}
#[test] fn read_shutdown_is_eof_for_both_operations() {
    prepare(b"buffered",false);
    with_compat(|c| c.sockets[0].read_shutdown=true);
    let mut out=[0;8];
    assert_eq!(unsafe { mio_tcp_stream_peek_host(1,out.as_mut_ptr(),8) },0);
    assert_eq!(unsafe { mio_tcp_stream_read_host(1,out.as_mut_ptr(),8) },0);
}
#[test] fn production_receive_records_consumption_peeking_and_would_block() {
    prepare(b"four",false);
    with_compat(|c| { c.sockets[0].owner_vm=Some(0); c.progress=[progress::Counters::new();1]; });
    let mut out=[0;8];
    assert_eq!(unsafe { mio_tcp_stream_peek_host(1,out.as_mut_ptr(),8) },4);
    assert_eq!(unsafe { mio_tcp_stream_read_host(1,out.as_mut_ptr(),8) },4);
    assert_eq!(unsafe { mio_tcp_stream_read_host(1,out.as_mut_ptr(),8) },-2);
    with_compat(|c| {
        let p=c.progress[0];
        assert_eq!((p.generation,p.read_bytes,p.peek_bytes),(1,4,4));
        assert_eq!((p.read_calls,p.peek_calls,p.would_block),(2,1,1));
    });
}
'''
    harness += r"""
#[test] fn all_public_read_routes_preserve_peek_and_then_consume() {
    for mode in 0..=3 {
        MODE.with(|m|m.set(mode));
        prepare(b"peek-witness",true);
        let mut out=[0;32];
        for _ in 0..2 {
            assert_eq!(unsafe { trueos_mio_tcp_stream_peek(1,out.as_mut_ptr(),32) },12,"route {mode}");
            assert_eq!(&out[..12],b"peek-witness");
        }
        assert_eq!(unsafe { trueos_mio_tcp_stream_read(1,out.as_mut_ptr(),32) },12,"route {mode}");
        assert_eq!(&out[..12],b"peek-witness");
        assert_eq!(unsafe { trueos_mio_tcp_stream_read(1,out.as_mut_ptr(),32) },0);
    }
}
#[test] fn public_guest_peek_caps_copy_and_preserves_zero_length() {
    for mode in 1..=3 {
        MODE.with(|m|m.set(mode));
        prepare(b"abcdefghijklmnopqrstuvwxyz",false);
        let mut out=[0x55;32];
        assert_eq!(unsafe { trueos_mio_tcp_stream_peek(1,out.as_mut_ptr(),32) },PAYLOAD_CAP as isize);
        assert_eq!(&out[..PAYLOAD_CAP],b"abcdefghijklmnop");
        assert!(out[PAYLOAD_CAP..].iter().all(|&b|b==0x55));
        assert_eq!(unsafe { trueos_mio_tcp_stream_peek(1,std::ptr::null_mut(),0) },0);
        with_compat(|c|assert_eq!(c.sockets[0].rx_stream.len(),26));
    }
}
"""
    harness = harness.replace('@DISPATCH@', dispatch).replace('@DISPATCHER@', dispatcher).replace('@PROGRESS@', str(ROOT / 'src/mio_compat/progress.rs'))
    production += progress_helper
    with tempfile.TemporaryDirectory(prefix='trueos-mio-peek-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness + constants + production)
        subprocess.run(['rustc','--edition=2024','--test','--target','x86_64-unknown-linux-gnu',str(rust),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
