#!/usr/bin/env python3
"""Round-trip the production TCP half-close wire ABI with its appended tags."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    harness = r'''
#![allow(dead_code)]
extern crate self as v;
#[path = "@API@"] pub mod vnet;
#[path = "@WIRE@"] mod wire;
#[test]
fn finish_and_receive_eof_round_trip_without_changing_legacy_close() {
    use vnet::{Command, Event, NetHandle};
    let handle = NetHandle(0x12345678);
    let mut out = [0; 64];
    let n = wire::encode_command(Command::FinishTcp { handle }, &mut out).unwrap();
    assert_eq!(&out[..n], &[14, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(wire::decode_command(&out[..n]).unwrap(), Command::FinishTcp { handle });
    let n = wire::encode_event(Event::TcpReadClosed { handle }, &mut out).unwrap();
    assert_eq!(&out[..n], &[12, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(wire::decode_event(&out[..n]).unwrap(), Event::TcpReadClosed { handle });
    let n = wire::encode_command(Command::Close { handle }, &mut out).unwrap();
    assert_eq!(out[0], 8);
    assert_eq!(wire::decode_command(&out[..n]).unwrap(), Command::Close { handle });
    let n = wire::encode_event(Event::Closed { handle }, &mut out).unwrap();
    assert_eq!(out[0], 2);
    assert_eq!(wire::decode_event(&out[..n]).unwrap(), Event::Closed { handle });
}
#[test]
fn incomplete_half_close_messages_are_rejected() {
    for n in 1..5 {
        let command = [14, 1, 2, 3, 4];
        let event = [12, 1, 2, 3, 4];
        assert!(wire::decode_command(&command[..n]).is_err());
        assert!(wire::decode_event(&event[..n]).is_err());
    }
}
'''
    harness = harness.replace("@API@", str(ROOT / "crates/trueos-v/src/vnet.rs"))
    harness = harness.replace("@WIRE@", str(ROOT / "src/hv/blueprint/blueprint_net_wire.rs"))
    with tempfile.TemporaryDirectory(prefix="trueos-tcp-finish-wire-") as directory:
        path = Path(directory)
        source = path / "wire.rs"
        source.write_text(harness)
        binary = path / "wire"
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(source), "-o", str(binary)],
                       check=True, cwd=ROOT)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
