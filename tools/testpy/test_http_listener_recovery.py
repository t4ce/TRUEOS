#!/usr/bin/env python3
"""Run real HTTP listener state and service retry hooks with no NIC or storage."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def function(source, name):
    start = source.index(f"fn {name}(")
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def main():
    source = (ROOT / "src/r/fs/http_trueosfs.rs").read_text()
    hooks = "\n".join(function(source, name) for name in [
        "http_submit_server_command", "http_poll_listener", "http_wifi_observe_body"])
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::string::String;
use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
#[path="@ROOT@/crates/trueos-v/src/vnet.rs"] mod vnet;
#[path="@ROOT@/crates/trueos-v/src/vhttp_srv.rs"] mod vhttp_srv;
use vnet as api;
#[macro_export] macro_rules! log_warn {
    (target: $target:literal; $($args:tt)*) => { let _ = format_args!($($args)*); };
}
mod trueos_time {
    pub static NOW: super::AtomicU64 = super::AtomicU64::new(0);
    pub struct Instant(u64);
    impl Instant {
        pub fn now() -> Self { Self(NOW.load(super::Ordering::Acquire)) }
        pub fn as_millis(&self) -> u64 { self.0 }
    }
}
mod net { pub mod wifi {
    pub static CALLS: super::super::AtomicUsize = super::super::AtomicUsize::new(0);
    pub fn diagnostic_snapshot_text() -> alloc::string::String {
        CALLS.fetch_add(1, super::super::Ordering::Relaxed);
        alloc::string::String::from("wifi-state=Connected\n")
    }
} }
struct VNet { accept: Cell<bool>, accepted: RefCell<Vec<api::Command>> }
impl VNet {
    fn submit(&self, cmd: api::Command) -> Result<(), ()> {
        if !self.accept.get() { return Err(()); }
        self.accepted.borrow_mut().push(cmd);
        Ok(())
    }
}
@HOOKS@
#[test]
fn service_rejects_then_retries_without_socket_events_and_retains_pending_ack() {
    let mut server = vhttp_srv::HttpServer::new(80, 1024);
    server.on_event(api::Event::Opened { handle: api::NetHandle(1), kind: api::SocketKind::Tcp });
    let vnet = VNet { accept: Cell::new(false), accepted: RefCell::new(Vec::new()) };
    if let vhttp_srv::HttpServerEvent::Submit(cmd) = server.on_event(api::Event::TcpEstablished {
        handle: api::NetHandle(1), peer: None, peer6: None,
    }) { http_submit_server_command(&vnet, &mut server, cmd); } else { panic!("expected listen"); }
    trueos_time::NOW.store(10, Ordering::Release);
    http_poll_listener(&vnet, &mut server);
    vnet.accept.set(true);
    trueos_time::NOW.store(1_009, Ordering::Release);
    http_poll_listener(&vnet, &mut server);
    assert!(vnet.accepted.borrow().is_empty());
    trueos_time::NOW.store(1_010, Ordering::Release);
    http_poll_listener(&vnet, &mut server);
    assert!(matches!(vnet.accepted.borrow().as_slice(), [api::Command::OpenTcpListen { port: 80 }]));
    trueos_time::NOW.store(u64::MAX, Ordering::Release);
    http_poll_listener(&vnet, &mut server);
    assert_eq!(vnet.accepted.borrow().len(), 1, "accepted listen keeps its token until acknowledgement");
}
#[test]
fn wifi_observe_is_an_exact_read_only_get_route_without_storage_services() {
    assert!(http_wifi_observe_body("POST", "/wifi/observe").is_none());
    assert!(http_wifi_observe_body("GET", "/wifi/observe/other").is_none());
    assert_eq!(net::wifi::CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(http_wifi_observe_body("GET", vhttp_srv::path_only("/wifi/observe?fresh=1")).as_deref(), Some("wifi-state=Connected\n"));
    assert_eq!(net::wifi::CALLS.load(Ordering::Relaxed), 1);
}
'''.replace("@ROOT@", str(ROOT)).replace("@HOOKS@", hooks)
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text()).group(1)
    with tempfile.TemporaryDirectory(prefix="http-listener-recovery-") as directory:
        work = Path(directory)
        (work / "test.rs").write_text(harness)
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(work / "test.rs"),
                        "-o", str(work / "tests")], cwd=work, check=True)
        subprocess.run([str(work / "tests")], cwd=work, check=True)


if __name__ == "__main__":
    main()
