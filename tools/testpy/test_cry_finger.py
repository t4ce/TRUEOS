#!/usr/bin/env python3
"""Test the actual Cry Finger USB diagnostics without accessing hardware."""
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
thread_local! {
    static SNAPSHOT: std::cell::RefCell<usb2::TlbUsbSnapshot> = Default::default();
    static LOGS: std::cell::RefCell<Vec<String>> = Default::default();
}
#[macro_export]
macro_rules! log_info {
    (target: $target:literal; $($args:tt)*) => {
        crate::LOGS.with(|logs| logs.borrow_mut().push(format!($($args)*)))
    };
}
mod usb2 {
    #[derive(Clone,Debug,Default)]
    pub struct TlbUsbEndpoint { pub address:u8, pub transfer_type:&'static str, pub max_packet_size:u16, pub interval:u8 }
    #[derive(Clone,Debug,Default)]
    pub struct TlbUsbInterface { pub interface_number:u8, pub alternate_setting:u8, pub class:u8, pub endpoints:Vec<TlbUsbEndpoint> }
    #[derive(Clone,Debug,Default)]
    pub struct TlbUsbConfiguration { pub configuration_value:u8, pub interfaces:Vec<TlbUsbInterface> }
    #[derive(Clone,Debug,Default)]
    pub struct TlbUsbDevice {
        pub vendor_id:u16, pub product_id:u16, pub controller_index:usize, pub root_port_id:u8, pub slot_id:u8,
        pub serial:Option<String>, pub product:Option<String>, pub configurations:Vec<TlbUsbConfiguration>
    }
    #[derive(Clone,Debug,Default)]
    pub struct TlbUsbSnapshot { pub devices:Vec<TlbUsbDevice>, pub controllers:Vec<()>, pub probe_error:Option<&'static str> }
    pub fn tlb_usb_snapshot()->TlbUsbSnapshot {crate::SNAPSHOT.with(|snapshot|snapshot.borrow().clone())}
}
#[path=@MODULE@] mod cry_finger;
#[test]
fn painting_is_silent_and_explicit_scenarios_log_metadata_only() {
    LOGS.with(|logs|logs.borrow_mut().clear());
    SNAPSHOT.with(|snapshot|snapshot.borrow_mut().devices.push(usb2::TlbUsbDevice {
        vendor_id:0x06cb, product_id:0x00e9, serial:Some("private-serial".into()), ..Default::default()
    }));
    for _ in 0..5 { assert_eq!(cry_finger::summary().len(),4); }
    LOGS.with(|logs|assert!(logs.borrow().is_empty()));
    for scenario in cry_finger::SCENARIOS { assert_eq!(scenario.run().len(),4); }
    LOGS.with(|logs| {
        let logs=logs.borrow();assert_eq!(logs.len(),2);
        assert!(logs.iter().all(|line|line.contains("readers=1") && line.contains("source=cached-usb") && !line.contains("private-serial")));
    });
}
'''.replace("@MODULE@", json.dumps(str(ROOT / "src/shell3/system/cry_finger.rs")))
    with tempfile.TemporaryDirectory(prefix="trueos-cry-finger-") as directory:
        directory = Path(directory)
        source = directory / "tests.rs"
        binary = directory / "tests"
        source.write_text(harness)
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
