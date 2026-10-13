#!/usr/bin/env python3
"""Exercise the production POSIX bind/listen/getsockname/connect flow on a host.

Only kernel Mio/vnet operations and readiness waits are mocked. Actual shim
functions, FD variants, sockaddr/errno conversion and the fixed FD registry are
compiled, including blocking callback I/O and TCP stream descriptor clones.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[2]
SOURCE = str(ROOT / "src/std_abi_shim.rs")


def cabi_item(name):
    source = Path(SOURCE).read_text()
    declaration = re.search(rf'^pub unsafe extern "C" fn {name}\(', source, re.MULTILINE)
    if declaration is None:
        raise ValueError(f"missing production function {name}")
    ending = re.search(r"^}\s*\n", source[declaration.end():], re.MULTILINE)
    if ending is None:
        raise ValueError(f"missing function boundary {name}")
    # Exclude no_mangle: the host harness must not interpose on host libc.
    return source[declaration.start():declaration.end() + ending.end()]


def main():
    definitions = [constant(SOURCE, name) for name in (
        "TRUEOS_AF_INET", "TRUEOS_AF_INET6", "TRUEOS_EOPNOTSUPP", "TRUEOS_EAGAIN", "TRUEOS_EBADF", "TRUEOS_EINVAL",
        "TRUEOS_EIO", "TRUEOS_EADDRINUSE", "TRUEOS_EINPROGRESS",
        "TRUEOS_SOCK_NONBLOCK", "TRUEOS_SOCK_CLOEXEC", "TRUEOS_FD_CLOEXEC",
        "TRUEOS_MSG_DONTWAIT", "TRUEOS_O_NONBLOCK",
    )]
    definitions += [item(SOURCE, name) for name in (
        "TrueosInAddr", "TrueosSockAddrIn", "TrueosSockAddrIn6", "SocketFd",
        "mio_status_to_errno", "posix_mio_i32", "posix_rc_i32", "posix_mio_isize", "posix_rc_isize",
        "parse_sockaddr", "write_sockaddr", "socket_to_mio", "socket_from_mio", "unspecified_socket_addr",
        "wait_for_socket_io", "socket_set_nonblocking",
        "duplicate_native_tcp_fd", "remove_socket_fd_for_close",
    )]
    definitions += [cabi_item(name) for name in ("bind", "listen", "getsockname", "connect", "accept4", "getpeername", "setsockopt", "send", "recv")]
    support = f'''#![allow(dead_code)]
extern crate alloc;
use core::ffi::{{c_int, c_void}};
use core::net::{{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6}};
use core::sync::atomic::{{AtomicI32, Ordering}};
use std::sync::Arc;
#[path="{ROOT / 'src/r/static_map.rs'}"] mod static_map;
use static_map::FixedKeyMap;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {{
    const fn new(value: T) -> Self {{ Self(std::sync::Mutex::new(value)) }}
    fn lock(&self) -> std::sync::MutexGuard<'_, T> {{ self.0.lock().unwrap() }}
}}
static SOCKET_FDS: Mutex<FixedKeyMap<c_int, SocketFd, 32>> = Mutex::new(FixedKeyMap::new());
static FD_FLAGS: Mutex<FixedKeyMap<c_int, c_int, 32>> = Mutex::new(FixedKeyMap::new());
struct BytePipe {{ bytes:Vec<u8>, read_open:bool, write_open:bool }}
enum OpenFile {{ UnixSocket {{ rx:Arc<Mutex<BytePipe>>, tx:Arc<Mutex<BytePipe>>, flags:i32 }} }}
static OPEN_FILES: Mutex<FixedKeyMap<c_int, OpenFile, 32>> = Mutex::new(FixedKeyMap::new());
fn wake_current_blueprint_io() {{}}
fn next_file_fd() -> c_int {{ 9 }}
fn next_file_fd_at_least(minimum:c_int)->Option<c_int> {{ Some(minimum.max(10)) }}
unsafe fn close(fd:c_int)->c_int {{
    FD_FLAGS.lock().remove(fd);
    let (socket,last)=remove_socket_fd_for_close(fd).unwrap();
    if last {{ unsafe {{ mio_compat::trueos_mio_socket_close(socket.backend()) }} }} else {{0}}
}}
impl SocketFd {{ fn backend(&self) -> u32 {{ match self {{ Self::PendingListener {{ backend, .. }} | Self::Cabi {{ backend, .. }} | Self::MioStream {{backend,..}} => *backend, _ => 0 }} }} }}
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
fn abi_read_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {{
    if ptr.is_null() {{ None }} else {{ Some(unsafe {{ core::slice::from_raw_parts(ptr, len) }}) }}
}}
fn abi_write_bytes<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {{
    if ptr.is_null() {{ None }} else {{ Some(unsafe {{ core::slice::from_raw_parts_mut(ptr, len) }}) }}
}}
fn copy_to_abi_out(ptr: *mut u8, bytes: &[u8]) -> bool {{
    if ptr.is_null() {{ return false; }}
    unsafe {{ core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()); }}
    true
}}
mod hv {{ pub fn hvlogf(_args: core::fmt::Arguments<'_>) {{}} }}
mod unix_abi_shim {{
    pub struct PollFd {{ pub fd:i32, pub events:i16, pub revents:i16 }}
    pub unsafe fn poll(fd:*mut PollFd, count:usize, timeout:i32)->i32 {{
        assert_eq!(count,1); assert_eq!(timeout,-1);
        // Waiting must release the process descriptor lock.
        let descriptors=super::SOCKET_FDS.lock();
        assert!(descriptors.get(unsafe {{ (*fd).fd }}).is_some());
        drop(descriptors);
        let mut state=super::mio_compat::STATE.lock();
        state.waits.push(unsafe {{ (*fd).events }});
        if state.spurious_wakes != 0 {{ state.spurious_wakes-=1; return 0; }}
        state.accept_status=0;
        state.read_status=0; state.write_status=0;
        1
    }}
}}
mod mio_compat {{
    #[repr(C)] #[derive(Clone, Copy, Default)]
    pub struct TrueosMioSocketAddr {{ pub family:u8, pub reserved:u8, pub port:u16, pub addr:[u8;16] }}
    pub struct State {{
        pub local: Option<TrueosMioSocketAddr>, pub connected: Option<TrueosMioSocketAddr>,
        pub bind_status: i32, pub address_status: i32, pub invalid_family: bool,
        pub mio_closed: Vec<u32>, pub cabi_closed: Vec<u32>,
        pub accept_status:i32, pub accept_attempts:usize, pub waits:Vec<i16>, pub spurious_wakes:usize,
        pub read_status:isize, pub write_status:isize, pub sent:Vec<u8>,
    }}
    impl State {{ pub const fn new() -> Self {{ Self {{
        local:None, connected:None, bind_status:0, address_status:0, invalid_family:false,
        mio_closed:Vec::new(), cabi_closed:Vec::new(),
        accept_status:0, accept_attempts:0, waits:Vec::new(), spurious_wakes:0,
        read_status:0, write_status:0, sent:Vec::new(),
    }} }} }}
    pub static STATE: super::Mutex<State> = super::Mutex::new(State::new());
    pub unsafe fn trueos_mio_tcp_listener_bind(mut addr:TrueosMioSocketAddr, out:*mut u32) -> i32 {{
        let mut state = STATE.lock();
        if state.bind_status != 0 {{ return state.bind_status; }}
        if addr.port == 0 {{ addr.port = 50001; }}
        state.local = Some(addr);
        unsafe {{ *out = 101; }}
        0
    }}
    pub unsafe fn trueos_mio_socket_local_addr(_backend:u32, out:*mut TrueosMioSocketAddr) -> i32 {{
        let state = STATE.lock();
        if state.address_status != 0 {{ return state.address_status; }}
        let mut addr = state.local.unwrap();
        if state.invalid_family {{ addr.family = 99; }}
        unsafe {{ *out = addr; }}
        0
    }}
    pub unsafe fn trueos_mio_socket_close(backend:u32) -> i32 {{ STATE.lock().mio_closed.push(backend); 0 }}
    pub unsafe fn trueos_mio_tcp_stream_connect(addr:TrueosMioSocketAddr, out:*mut u32) -> i32 {{
        let mut state = STATE.lock();
        if state.local.is_none_or(|local| local.port != addr.port || local.addr != addr.addr) {{ return -6; }}
        state.connected = Some(addr);
        unsafe {{ *out = 202; }}
        0
    }}
    pub unsafe fn trueos_mio_udp_socket_bind(_addr:TrueosMioSocketAddr,_out:*mut u32)->i32 {{ unreachable!() }}
    pub unsafe fn trueos_mio_udp_socket_connect(_backend:u32,_addr:TrueosMioSocketAddr)->i32 {{ unreachable!() }}
    pub unsafe fn trueos_mio_tcp_listener_accept(_backend:u32, out:*mut u32, addr:*mut TrueosMioSocketAddr) -> i32 {{
        let mut state = STATE.lock();
        state.accept_attempts+=1;
        if state.accept_status != 0 {{ return state.accept_status; }}
        unsafe {{ *out = 303; *addr = state.connected.or(state.local).unwrap(); }} 0
    }}
    pub unsafe fn trueos_mio_socket_peer_addr(_backend:u32, out:*mut TrueosMioSocketAddr) -> i32 {{
        unsafe {{ *out = STATE.lock().connected.unwrap(); }} 0
    }}
    pub unsafe fn trueos_mio_tcp_stream_read(_backend:u32, out:*mut u8, len:usize)->isize {{
        let state=STATE.lock();
        if state.read_status != 0 {{ return state.read_status; }}
        let n=len.min(4); unsafe {{ core::ptr::copy_nonoverlapping(b"HTTP".as_ptr(),out,n); }} n as isize
    }}
    pub unsafe fn trueos_mio_tcp_stream_write(_backend:u32, data:*const u8, len:usize)->isize {{
        let mut state=STATE.lock();
        if state.write_status != 0 {{ return state.write_status; }}
        state.sent.extend_from_slice(unsafe {{ core::slice::from_raw_parts(data,len) }}); len as isize
    }}
    pub unsafe fn trueos_mio_udp_socket_send_to(_backend:u32,_peer:TrueosMioSocketAddr,_data:*const u8,_len:usize)->isize {{ unreachable!() }}
    pub unsafe fn trueos_mio_udp_socket_recv_from(_backend:u32,_peer:*mut TrueosMioSocketAddr,_data:*mut u8,_len:usize)->isize {{ unreachable!() }}
}}
mod r {{ pub mod net {{ pub mod socket_cabi {{
    pub fn trueos_cabi_socket_tcp_close(backend:u32)->i32 {{
        crate::mio_compat::STATE.lock().cabi_closed.push(backend); 0
    }}
    pub fn trueos_cabi_socket_tcp_connect_v6(_backend:u32,_addr:*const u8,_port:u16,_nonblocking:u32)->i32 {{ unreachable!() }}
    pub fn trueos_cabi_socket_tcp_peer_v4(_backend:u32,_addr:*mut u32,_port:*mut u16)->i32 {{ unreachable!() }}
    pub fn trueos_cabi_socket_tcp_peer_v6(_backend:u32,_addr:*mut u8,_port:*mut u16)->i32 {{ unreachable!() }}
    pub fn trueos_cabi_socket_tcp_set_nonblocking(_backend:u32,_nonblocking:u32)->i32 {{ 0 }}
    pub fn trueos_cabi_socket_tcp_connect_v4(_backend:u32,_addr:u32,_port:u16,_nonblocking:u32)->i32 {{ unreachable!() }}
    pub fn trueos_cabi_socket_tcp_send(_backend:u32,_data:*const u8,_len:usize)->isize {{ unreachable!() }}
    pub fn trueos_cabi_socket_tcp_recv(_backend:u32,_data:*mut u8,_len:usize,_flags:i32,_nonblocking:u32,_timeout:u64)->isize {{ unreachable!() }}
}} }} }}
#[cfg(test)] mod tests {{
    use super::*;
    fn prepare(port:u16) {{
        *SOCKET_FDS.lock() = FixedKeyMap::new();
        *FD_FLAGS.lock() = FixedKeyMap::new();
        *mio_compat::STATE.lock() = mio_compat::State::new();
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        SOCKET_FDS.lock().insert(7, SocketFd::PendingListener {{
            backend:37, local:None, domain:TRUEOS_AF_INET, nonblocking:true,
        }}).unwrap_or_else(|_| panic!("FD fixture capacity"));
        let raw = TrueosSockAddrIn {{
            sin_family:TRUEOS_AF_INET as u16, sin_port:port.to_be(),
            sin_addr:TrueosInAddr {{ s_addr:u32::from_ne_bytes([127,0,0,1]) }}, sin_zero:[0;8],
        }};
        assert_eq!(unsafe {{ bind(7, (&raw as *const TrueosSockAddrIn).cast(), 16) }}, 0);
    }}
    fn reported_local() -> ([u8;16], u32) {{
        let mut raw = [0u8;16]; let mut len = raw.len() as u32;
        assert_eq!(unsafe {{ getsockname(7, raw.as_mut_ptr().cast(), &mut len) }}, 0);
        (raw, len)
    }}
    #[test] fn bind_zero_then_listen_reports_the_allocated_port_for_connect() {{
        prepare(0);
        assert_eq!(unsafe {{ listen(7,128) }}, 0);
        let (raw,len) = reported_local();
        let addr = parse_sockaddr(raw.as_ptr().cast(),len).unwrap();
        assert_eq!(addr.ip(),IpAddr::V4(Ipv4Addr::LOCALHOST)); assert_eq!(addr.port(),50001);
        SOCKET_FDS.lock().insert(8, SocketFd::PendingListener {{
            backend:38,local:None,domain:TRUEOS_AF_INET,nonblocking:true,
        }}).unwrap_or_else(|_| panic!("client FD fixture capacity"));
        assert_eq!(unsafe {{ connect(8,raw.as_ptr().cast(),len) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINPROGRESS);
        assert_eq!(mio_compat::STATE.lock().connected.unwrap().port,50001);
        assert!(matches!(SOCKET_FDS.lock().get(8),Some(SocketFd::MioStream {{ backend:202, nonblocking:true }})));
        assert_eq!(mio_compat::STATE.lock().cabi_closed,[37,38]);
    }}
    fn prepare_v6() -> [u8;28] {{
        *SOCKET_FDS.lock() = FixedKeyMap::new();
        *mio_compat::STATE.lock() = mio_compat::State::new();
        SOCKET_FDS.lock().insert(7, SocketFd::PendingListener {{
            backend:37, local:None, domain:TRUEOS_AF_INET6, nonblocking:true,
        }}).unwrap_or_else(|_| panic!("fixture"));
        let mut raw=[0;28]; let mut len=28;
        assert!(write_sockaddr(raw.as_mut_ptr().cast(), &mut len, SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 14004)));
        raw
    }}
    #[test] fn ipv6_bind_connect_accept_and_names_preserve_family_and_port() {{
        let raw = prepare_v6();
        let one=1i32;
        assert_eq!(unsafe {{ setsockopt(7,41,26,(&one as *const i32).cast(),4) }},0);
        assert_eq!(unsafe {{ bind(7,raw.as_ptr().cast(),28) }},0);
        assert_eq!(unsafe {{ listen(7,128) }},0);
        let mut reported=[0;28]; let mut len=28;
        assert_eq!(unsafe {{ getsockname(7,reported.as_mut_ptr().cast(),&mut len) }},0);
        assert_eq!(reported,raw); assert_eq!(len,28);
        SOCKET_FDS.lock().insert(8, SocketFd::PendingListener {{
            backend:38,local:None,domain:TRUEOS_AF_INET6,nonblocking:true,
        }}).unwrap_or_else(|_| panic!("fixture"));
        assert_eq!(unsafe {{ connect(8,raw.as_ptr().cast(),28) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINPROGRESS);
        assert_eq!(unsafe {{ accept4(7,reported.as_mut_ptr().cast(),&mut len,0) }},9);
        assert_eq!(reported,raw);
        assert_eq!(unsafe {{ getpeername(9,reported.as_mut_ptr().cast(),&mut len) }},0);
        assert_eq!(reported,raw);
    }}
    #[test] fn ipv6_unbound_listener_gets_ipv6_ephemeral_port() {{
        prepare_v6();
        assert_eq!(unsafe {{ listen(7,128) }},0);
        let mut raw=[0;28]; let mut len=28;
        assert_eq!(unsafe {{ getsockname(7,raw.as_mut_ptr().cast(),&mut len) }},0);
        assert_eq!(parse_sockaddr(raw.as_ptr().cast(),28),Some(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(),50001)));
    }}
    #[test] fn sockaddr_truncation_reports_full_size_without_overwrite() {{
        let raw=prepare_v6();
        let value=parse_sockaddr(raw.as_ptr().cast(),28).unwrap();
        let mut out=[0xcc;32]; let mut len=8;
        assert!(write_sockaddr(out.as_mut_ptr().cast(), &mut len, value));
        assert_eq!(len,28); assert_eq!(&out[..8],&raw[..8]); assert_eq!(&out[8..],&[0xcc;24]);
    }}
    #[test] fn invalid_ipv6_lengths_scope_and_family_are_rejected() {{
        let mut raw=prepare_v6();
        for size in [0,1,2,16,27] {{ assert!(parse_sockaddr(raw.as_ptr().cast(),size).is_none()); }}
        raw[24]=1; assert!(parse_sockaddr(raw.as_ptr().cast(),28).is_none()); raw[24]=0;
        raw[4]=1; assert!(parse_sockaddr(raw.as_ptr().cast(),28).is_none()); raw[4]=0;
        raw[0]=99; assert!(parse_sockaddr(raw.as_ptr().cast(),28).is_none());
    }}
    #[test] fn mismatched_bind_family_and_unsupported_dual_stack_fail() {{
        let raw=prepare_v6();
        SOCKET_FDS.lock().get_mut(7).map(|socket| if let SocketFd::PendingListener {{domain,..}}=socket {{*domain=TRUEOS_AF_INET;}});
        assert_eq!(unsafe {{ bind(7,raw.as_ptr().cast(),28) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);
        prepare_v6(); let zero=0i32;
        assert_eq!(unsafe {{ setsockopt(7,41,26,(&zero as *const i32).cast(),4) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EOPNOTSUPP);
    }}
    #[test] fn explicit_listener_port_is_preserved() {{
        prepare(12345); assert_eq!(unsafe {{ listen(7,128) }},0);
        let (raw,len)=reported_local();
        assert_eq!(parse_sockaddr(raw.as_ptr().cast(),len).unwrap().port(),12345);
    }}
    fn accept_child(flags:i32)->i32 {{
        let mut raw=[0u8;28]; let mut len=28;
        unsafe {{ accept4(7,raw.as_mut_ptr().cast(),&mut len,flags) }}
    }}
    #[test] fn blocking_listener_waits_for_client_through_spurious_wakes() {{
        prepare(1455);
        socket_set_nonblocking(7,false).unwrap();
        assert_eq!(unsafe {{ listen(7,128) }},0);
        assert!(matches!(SOCKET_FDS.lock().get(7),Some(SocketFd::MioListener {{nonblocking:false,..}})));
        {{ let mut state=mio_compat::STATE.lock(); state.accept_status=-2; state.spurious_wakes=1; }}
        assert_eq!(accept_child(0),9);
        let state=mio_compat::STATE.lock();
        assert_eq!(state.accept_attempts,3); assert_eq!(state.waits,[1,1]);
        drop(state);
        assert!(matches!(SOCKET_FDS.lock().get(9),Some(SocketFd::MioStream {{nonblocking:false,..}})));
    }}
    #[test] fn nonblocking_listener_returns_eagain_without_waiting() {{
        prepare(1455); assert_eq!(unsafe {{ listen(7,128) }},0);
        mio_compat::STATE.lock().accept_status=-2;
        assert_eq!(accept_child(0),-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EAGAIN);
        assert!(mio_compat::STATE.lock().waits.is_empty());
        socket_set_nonblocking(7,false).unwrap();
        assert_eq!(accept_child(0),9);
    }}
    #[test] fn accepted_stream_mode_comes_from_accept_flags_and_can_change() {{
        prepare(1455); assert_eq!(unsafe {{ listen(7,128) }},0);
        assert_eq!(accept_child(0),9);
        assert!(matches!(SOCKET_FDS.lock().get(9),Some(SocketFd::MioStream {{nonblocking:false,..}})));
        socket_set_nonblocking(9,true).unwrap();
        assert!(matches!(SOCKET_FDS.lock().get(9),Some(SocketFd::MioStream {{nonblocking:true,..}})));
        SOCKET_FDS.lock().remove(9);
        assert_eq!(accept_child(TRUEOS_SOCK_NONBLOCK|TRUEOS_SOCK_CLOEXEC),9);
        assert!(matches!(SOCKET_FDS.lock().get(9),Some(SocketFd::MioStream {{nonblocking:true,..}})));
        assert_eq!(FD_FLAGS.lock().get(9),Some(&TRUEOS_FD_CLOEXEC));
    }}
    #[test] fn hard_accept_errors_are_not_retried() {{
        prepare(1455); socket_set_nonblocking(7,false).unwrap();
        assert_eq!(unsafe {{ listen(7,128) }},0);
        mio_compat::STATE.lock().accept_status=-5;
        assert_eq!(accept_child(0),-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);
        assert!(mio_compat::STATE.lock().waits.is_empty());
    }}
    #[test] fn blocking_callback_stream_waits_for_input_and_output_capacity() {{
        prepare(1455); assert_eq!(unsafe {{ listen(7,128) }},0);
        assert_eq!(accept_child(0),9);
        mio_compat::STATE.lock().read_status=-2;
        let mut raw=[0u8;4];
        assert_eq!(unsafe {{ recv(9,raw.as_mut_ptr().cast(),raw.len(),0) }},4);
        assert_eq!(&raw,b"HTTP");
        mio_compat::STATE.lock().write_status=-2;
        assert_eq!(unsafe {{ send(9,b"OK".as_ptr().cast(),2,0) }},2);
        let state=mio_compat::STATE.lock();
        assert_eq!(state.waits,[1,4]); assert_eq!(state.sent,b"OK");
    }}
    #[test] fn nonblocking_and_dontwait_stream_io_return_eagain_without_waiting() {{
        for nonblocking in [true,false] {{
            prepare(1455); assert_eq!(unsafe {{ listen(7,128) }},0);
            assert_eq!(accept_child(if nonblocking {{TRUEOS_SOCK_NONBLOCK}} else {{0}}),9);
            {{ let mut state=mio_compat::STATE.lock(); state.read_status=-2; state.write_status=-2; }}
            let flags=if nonblocking {{0}} else {{TRUEOS_MSG_DONTWAIT}};
            let mut byte=0u8;
            assert_eq!(unsafe {{ recv(9,(&mut byte as *mut u8).cast(),1,flags) }},-1);
            assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EAGAIN);
            assert_eq!(unsafe {{ send(9,b"x".as_ptr().cast(),1,flags) }},-1);
            assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EAGAIN);
            assert!(mio_compat::STATE.lock().waits.is_empty());
        }}
    }}
    #[test] fn callback_stream_clones_share_mode_and_keep_backend_until_last_close() {{
        prepare(1455); assert_eq!(unsafe {{ listen(7,128) }},0);
        assert_eq!(accept_child(0),9);
        let clone=duplicate_native_tcp_fd(9,10,true).unwrap();
        assert_eq!(FD_FLAGS.lock().get(clone),Some(&TRUEOS_FD_CLOEXEC));
        socket_set_nonblocking(clone,true).unwrap();
        assert!(matches!(SOCKET_FDS.lock().get(9),Some(SocketFd::MioStream {{nonblocking:true,..}})));
        socket_set_nonblocking(9,false).unwrap();
        assert!(matches!(SOCKET_FDS.lock().get(clone),Some(SocketFd::MioStream {{nonblocking:false,..}})));
        assert_eq!(unsafe {{ close(9) }},0);
        assert!(mio_compat::STATE.lock().mio_closed.is_empty());
        let mut raw=[0u8;4];
        assert_eq!(unsafe {{ recv(clone,raw.as_mut_ptr().cast(),4,0) }},4);
        assert_eq!(&raw,b"HTTP");
        assert_eq!(unsafe {{ close(clone) }},0);
        assert_eq!(mio_compat::STATE.lock().mio_closed,[303]);
    }}
    #[test] fn local_address_failure_closes_new_backend_and_keeps_pending_fd() {{
        prepare(0); mio_compat::STATE.lock().address_status=-5;
        assert_eq!(unsafe {{ listen(7,128) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);
        assert_eq!(mio_compat::STATE.lock().mio_closed,[101]);
        assert!(mio_compat::STATE.lock().cabi_closed.is_empty());
        assert!(matches!(SOCKET_FDS.lock().get(7),Some(SocketFd::PendingListener {{ backend:37,.. }})));
    }}
    #[test] fn incompatible_native_address_closes_new_backend_and_keeps_pending_fd() {{
        prepare(0); mio_compat::STATE.lock().invalid_family=true;
        assert_eq!(unsafe {{ listen(7,128) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);
        assert_eq!(mio_compat::STATE.lock().mio_closed,[101]);
        assert!(mio_compat::STATE.lock().cabi_closed.is_empty());
        assert!(matches!(SOCKET_FDS.lock().get(7),Some(SocketFd::PendingListener {{ backend:37,.. }})));
    }}
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-posix-listener-") as directory:
        folder = Path(directory)
        rust_source = folder / "tests.rs"
        binary = folder / "tests"
        rust_source.write_text(support + "\n".join(definitions))
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
