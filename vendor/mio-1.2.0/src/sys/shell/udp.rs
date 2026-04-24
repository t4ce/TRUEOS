#![cfg(not(target_os = "wasi"))]
#![allow(dead_code)]

use std::io;
use std::net::{self, SocketAddr};

pub fn bind(_: SocketAddr) -> io::Result<net::UdpSocket> {
    unsupported_io!("mio zkvm UDP socket bind backend is not wired yet")
}

pub(crate) fn only_v6(_: &net::UdpSocket) -> io::Result<bool> {
    unsupported_io!("mio zkvm UDP socket option backend is not wired yet")
}
