#!/usr/bin/env python3
"""Compile actual vnet types and HTTP transport state, without filesystem mocks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


OWNER_FIXTURE = r''' 
use vnet as api;
#[macro_export] macro_rules! log { ($($arg:tt)*) => {}; }
#[derive(Default)]
struct VNet {
    events: std::cell::RefCell<std::collections::VecDeque<api::Event>>,
    commands: std::cell::RefCell<Vec<api::Command>>,
}
impl VNet {
    fn pop_event(&self) -> Option<api::Event> { self.events.borrow_mut().pop_front() }
    fn submit(&self, command: api::Command) -> Result<(), ()> {
        self.commands.borrow_mut().push(command); Ok(())
    }
}
fn http_submit_error_response(_: &VNet, _: &mut vhttp_srv::HttpServer, _: api::NetHandle, _: &'static str, _: &'static [u8]) { panic!("unexpected error response") }
fn http_submit_busy_response(_: &VNet, _: &mut vhttp_srv::HttpServer, _: api::NetHandle) { panic!("unexpected busy response") }
#[test]
fn cancelled_active_stream_forwards_capacity_rearm_before_returning() {
    let mut server = vhttp_srv::HttpServer::with_session_limit(80, 1024, 1);
    let handle = api::NetHandle(1);
    server.on_event(api::Event::Opened { handle, kind: api::SocketKind::Tcp });
    assert!(matches!(server.on_event(api::Event::TcpEstablished { handle, peer: None, peer6: None }), vhttp_srv::HttpServerEvent::None));
    let vnet = VNet::default();
    vnet.events.borrow_mut().push_back(api::Event::Closed { handle });
    assert!(!http_stream_drain_events(&vnet, &mut server, handle));
    assert!(matches!(vnet.commands.borrow().as_slice(), [api::Command::OpenTcpListen { port: 80 }]));
    assert!(http_stream_drain_events(&vnet, &mut server, handle));
    assert_eq!(vnet.commands.borrow().len(), 1);
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix="trueos-vhttp-rearm-") as directory:
        source = Path(directory) / "test.rs"
        binary = Path(directory) / "tests"
        owner_source = (ROOT / "src/r/fs/http_trueosfs.rs").read_text()
        owner_fn = owner_source.split("fn http_stream_drain_events(", 1)[1].split(
            "async fn http_submit_body_bytes", 1
        )[0]
        source.write_text(
            '#![allow(dead_code, unused_variables)]\nextern crate alloc;\n'
            f'#[path="{ROOT / "crates/trueos-v/src/vnet.rs"}"] mod vnet;\n'
            f'#[path="{ROOT / "crates/trueos-v/src/vhttp_srv.rs"}"] mod vhttp_srv;\n'
            + OWNER_FIXTURE + "\nfn http_stream_drain_events(" + owner_fn
        )
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
