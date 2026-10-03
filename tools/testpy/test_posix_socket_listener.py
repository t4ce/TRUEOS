#!/usr/bin/env python3
"""Exercise the production POSIX bind/listen/getsockname/connect flow on a host.

Only the kernel Mio/vnet boundary is mocked: it assigns an ephemeral listener
port and rejects a connect to another port. Actual shim functions, FD variants,
sockaddr conversion, errno conversion and the fixed FD registry are compiled.
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
        "TRUEOS_AF_INET", "TRUEOS_EAGAIN", "TRUEOS_EBADF", "TRUEOS_EINVAL",
        "TRUEOS_EIO", "TRUEOS_EADDRINUSE", "TRUEOS_EINPROGRESS",
    )]
    definitions += [item(SOURCE, name) for name in (
        "TrueosInAddr", "TrueosSockAddrIn", "SocketAddrV4", "SocketFd",
        "mio_status_to_errno", "posix_mio_i32", "posix_rc_i32",
        "parse_sockaddr_v4", "write_sockaddr_v4", "socket_v4_to_mio", "socket_v4_from_mio",
    )]
    definitions += [cabi_item(name) for name in ("bind", "listen", "getsockname", "connect")]
    support = f'''#![allow(dead_code)]
extern crate alloc;
use core::ffi::{{c_int, c_void}};
use core::sync::atomic::{{AtomicI32, Ordering}};
#[path="{ROOT / 'src/r/static_map.rs'}"] mod static_map;
use static_map::FixedKeyMap;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {{
    const fn new(value: T) -> Self {{ Self(std::sync::Mutex::new(value)) }}
    fn lock(&self) -> std::sync::MutexGuard<'_, T> {{ self.0.lock().unwrap() }}
}}
static SOCKET_FDS: Mutex<FixedKeyMap<c_int, SocketFd, 32>> = Mutex::new(FixedKeyMap::new());
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
fn abi_read_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {{
    if ptr.is_null() {{ None }} else {{ Some(unsafe {{ core::slice::from_raw_parts(ptr, len) }}) }}
}}
fn copy_to_abi_out(ptr: *mut u8, bytes: &[u8]) -> bool {{
    if ptr.is_null() {{ return false; }}
    unsafe {{ core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()); }}
    true
}}
mod hv {{ pub fn hvlogf(_args: core::fmt::Arguments<'_>) {{}} }}
mod mio_compat {{
    #[repr(C)] #[derive(Clone, Copy, Default)]
    pub struct TrueosMioSocketAddr {{ pub family:u8, pub reserved:u8, pub port:u16, pub addr:[u8;16] }}
    pub struct State {{
        pub local: Option<TrueosMioSocketAddr>, pub connected: Option<TrueosMioSocketAddr>,
        pub bind_status: i32, pub address_status: i32, pub invalid_family: bool,
        pub mio_closed: Vec<u32>, pub cabi_closed: Vec<u32>,
    }}
    impl State {{ pub const fn new() -> Self {{ Self {{
        local:None, connected:None, bind_status:0, address_status:0, invalid_family:false,
        mio_closed:Vec::new(), cabi_closed:Vec::new(),
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
        if state.invalid_family {{ addr.family = 6; }}
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
}}
mod r {{ pub mod net {{ pub mod socket_cabi {{
    pub fn trueos_cabi_socket_tcp_close(backend:u32)->i32 {{
        crate::mio_compat::STATE.lock().cabi_closed.push(backend); 0
    }}
    pub fn trueos_cabi_socket_tcp_connect_v4(_backend:u32,_addr:u32,_port:u16,_nonblocking:u32)->i32 {{ unreachable!() }}
}} }} }}
#[cfg(test)] mod tests {{
    use super::*;
    fn prepare(port:u16) {{
        *SOCKET_FDS.lock() = FixedKeyMap::new();
        *mio_compat::STATE.lock() = mio_compat::State::new();
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        SOCKET_FDS.lock().insert(7, SocketFd::PendingListener {{
            backend:37, local:None, nonblocking:true,
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
        let addr = parse_sockaddr_v4(raw.as_ptr().cast(),len).unwrap();
        assert_eq!(addr.addr,[127,0,0,1]); assert_eq!(addr.port,50001);
        SOCKET_FDS.lock().insert(8, SocketFd::PendingListener {{
            backend:38,local:None,nonblocking:true,
        }}).unwrap_or_else(|_| panic!("client FD fixture capacity"));
        assert_eq!(unsafe {{ connect(8,raw.as_ptr().cast(),len) }},-1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINPROGRESS);
        assert_eq!(mio_compat::STATE.lock().connected.unwrap().port,50001);
        assert!(matches!(SOCKET_FDS.lock().get(8),Some(SocketFd::MioStream {{ backend:202 }})));
        assert_eq!(mio_compat::STATE.lock().cabi_closed,[37,38]);
    }}
    #[test] fn explicit_listener_port_is_preserved() {{
        prepare(12345); assert_eq!(unsafe {{ listen(7,128) }},0);
        let (raw,len)=reported_local();
        assert_eq!(parse_sockaddr_v4(raw.as_ptr().cast(),len).unwrap().port,12345);
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
