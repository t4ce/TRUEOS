#![allow(dead_code)]

use std::io;
use std::net::{self, SocketAddr};

#[cfg(not(target_os = "wasi"))]
pub(crate) fn new_for_addr(_: SocketAddr) -> io::Result<i32> {
    unsupported_io!("mio zkvm TCP socket creation backend is not wired yet");
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn bind(_: &net::TcpListener, _: SocketAddr) -> io::Result<()> {
    unsupported_io!("mio zkvm TCP listener bind backend is not wired yet");
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn connect(_: &net::TcpStream, _: SocketAddr) -> io::Result<()> {
    unsupported_io!("mio zkvm TCP connect backend is not wired yet");
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn listen(_: &net::TcpListener, _: i32) -> io::Result<()> {
    unsupported_io!("mio zkvm TCP listen backend is not wired yet");
}

#[cfg(any(unix, target_os = "hermit", any(target_os = "trueos", target_os = "zkvm")))]
pub(crate) fn set_reuseaddr(_: &net::TcpListener, _: bool) -> io::Result<()> {
    unsupported_io!("mio zkvm TCP socket option backend is not wired yet");
}

pub(crate) fn accept(_: &net::TcpListener) -> io::Result<(net::TcpStream, SocketAddr)> {
    unsupported_io!("mio zkvm TCP accept backend is not wired yet");
}
