#!/usr/bin/env python3
"""Exercise production Mio close-event handling, readiness and take_error.

Only event delivery, socket lookup, ownership and logging are mocked. Compile
the real closed-event arm to check that failed connects cannot look successful
to Tokio, while established connections retain orderly EOF behavior.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    path = ROOT / "src/mio_compat.rs"
    source = path.read_text()
    closed = source.split("api::Event::Closed { handle } => {", 1)[1]
    closed = closed.split("\n            api::Event::Error", 1)[0].rsplit("}", 1)[0]
    definitions = "\n".join(constant(str(path), name) for name in (
        "STATUS_OK", "STATUS_NOT_CONNECTED", "STATUS_NOT_FOUND", "STATUS_IO",
        "READY_READABLE", "READY_WRITABLE", "READY_ERROR", "READY_READ_CLOSED",
        "READY_WRITE_CLOSED", "MIO_TCP_TX_WINDOW_BYTES",
    ))
    definitions += "\n" + "\n".join(item(str(path), name) for name in (
        "MioSocketKind", "MioSocketState",
    ))
    definitions += "\n" + re.search(
        r"^pub\(crate\) unsafe fn mio_socket_take_error_host\(.*?^}",
        source, re.MULTILINE | re.DOTALL,
    ).group()
    harness = r'''
#![allow(dead_code)]
use std::collections::VecDeque;
#[macro_export] macro_rules! log { ($($arg:tt)*) => {} }
mod api { #[derive(Clone,Copy,PartialEq)] pub struct NetHandle(pub u32); }
#[derive(Clone,Copy)] struct CompatAddr;
struct MioCompat { sockets: Vec<MioSocketState>, refills: usize }
impl MioCompat {
    fn socket_by_handle_mut(&mut self, handle: api::NetHandle) -> Option<&mut MioSocketState> {
        self.sockets.iter_mut().find(|s| s.handle == Some(handle) || s.listen_handles.contains(&handle))
    }
    fn socket_mut_for_owner(&mut self, id:u32, _:Option<u8>) -> Option<&mut MioSocketState> {
        self.sockets.iter_mut().find(|s| s.id == id)
    }
    fn pump(&mut self) {}
    fn ensure_tcp_listener_depth(&mut self, _:u32) { self.refills += 1; }
    fn close_event(&mut self, handle:api::NetHandle) { @CLOSED@ }
    @READY@
}
std::thread_local! {
    static STATE:std::cell::RefCell<MioCompat> = std::cell::RefCell::new(MioCompat { sockets:vec![], refills:0 });
}
fn with_compat<R>(f:impl FnOnce(&mut MioCompat)->R)->R { STATE.with(|s| f(&mut s.borrow_mut())) }
fn current_owner_vm()->Option<u8> { None }
fn tcp_flow_logging_enabled(_: &MioSocketState)->bool { false }
fn log_tcp_endpoint(_: &str, _:u32, _:u32, _:CompatAddr) {}
fn prepare(kind:MioSocketKind, connected:bool, error:i32) {
    with_compat(|c| {
        c.refills=0;
        c.sockets=vec![MioSocketState {
            id:1, owner_vm:None, kind, handle:Some(api::NetHandle(2)),
            listen_handles:vec![api::NetHandle(2)], local:None, peer:Some(CompatAddr),
            listen_port:None, connected, closed:false, error, tx_in_flight:0,
            rx_stream:VecDeque::new(), rx_dgrams:VecDeque::new(), accept_queue:VecDeque::new(),
        }];
        c.close_event(api::NetHandle(2));
    });
}
#[test] fn close_before_establishment_reports_a_connect_error() {
    prepare(MioSocketKind::TcpStream,false,STATUS_OK);
    with_compat(|c| {
        let ready=c.ready_mask(&c.sockets[0],READY_WRITABLE);
        assert_ne!(ready & READY_ERROR,0,"Tokio must see failure, not a successful connect");
        assert_ne!(ready & READY_WRITE_CLOSED,0);
        assert_eq!(ready & READY_WRITABLE,0);
    });
    assert_eq!(unsafe { mio_socket_take_error_host(1) },STATUS_NOT_CONNECTED);
    assert_eq!(unsafe { mio_socket_take_error_host(1) },STATUS_OK);
}
#[test] fn established_close_keeps_buffered_data_and_orderly_eof() {
    prepare(MioSocketKind::TcpStream,true,STATUS_OK);
    with_compat(|c| {
        c.sockets[0].rx_stream.extend(b"last bytes");
        let ready=c.ready_mask(&c.sockets[0],READY_READABLE);
        assert_ne!(ready & READY_READABLE,0);
        assert_ne!(ready & READY_READ_CLOSED,0);
        assert_eq!(ready & READY_ERROR,0);
        assert_eq!(c.sockets[0].rx_stream.len(),10);
    });
    assert_eq!(unsafe { mio_socket_take_error_host(1) },STATUS_OK);
}
#[test] fn close_preserves_a_more_specific_existing_error() {
    prepare(MioSocketKind::TcpStream,false,STATUS_IO);
    assert_eq!(unsafe { mio_socket_take_error_host(1) },STATUS_IO);
}
#[test] fn listener_close_refills_without_failing_the_listener() {
    prepare(MioSocketKind::TcpListener,false,STATUS_OK);
    with_compat(|c| {
        assert_eq!(c.refills,1);
        assert!(c.sockets[0].listen_handles.is_empty());
        assert!(!c.sockets[0].closed);
    });
    assert_eq!(unsafe { mio_socket_take_error_host(1) },STATUS_OK);
}
'''
    harness = harness.replace("@CLOSED@", closed).replace("@READY@", function(path, "ready_mask"))
    with tempfile.TemporaryDirectory(prefix="trueos-mio-connect-close-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(harness + definitions)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
