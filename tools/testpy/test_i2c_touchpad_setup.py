#!/usr/bin/env python3
"""Exercise actual descriptor-driven touchpad setup through the I2C HID protocol.

The captured descriptor is reused from the production decoder's tests. Firmware
and controller access are replaced by an explicit bus script; no hardware is
accessed and no UI4 or Link model is substituted into the setup function.
"""
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

from test_sdk_metadata_v2 import item


ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{vec, vec::Vec};
use core::fmt::Debug;
thread_local! {static LOGS: std::cell::RefCell<Vec<String>> = const {std::cell::RefCell::new(Vec::new())};}
#[macro_export] macro_rules! log_info { (target:$target:literal;$($args:tt)*) => {{crate::LOGS.with(|logs|logs.borrow_mut().push(format!($($args)*)));}}; }
#[macro_export] macro_rules! log_warn { (target:$target:literal;$($args:tt)*) => {{crate::LOGS.with(|logs|logs.borrow_mut().push(format!($($args)*)));}}; }
#[macro_export] macro_rules! log_important { (target:$target:literal;$($args:tt)*) => {{crate::LOGS.with(|logs|logs.borrow_mut().push(format!($($args)*)));}}; }
#[path=@DECODER@] mod hid_touchpad;
#[path=@PROTOCOL@] mod i2c_hid;
use hid_touchpad::Touchpad;
use i2c_hid::{Descriptor,Transport};
@CAPTURED@
@SETUP@

const DESC: Descriptor = Descriptor {
    report_descriptor_length: CAPTURED.len() as u16,
    report_descriptor_register: 0x21, input_register: 0x22, max_input_length: 66,
    command_register: 0x24, data_register: 0x25,
    vendor_id: 0x06cb, product_id: 0xce1a, version_id: 0,
};

fn complete<T>(future: impl core::future::Future<Output=T>) -> T {
    let mut future=core::pin::pin!(future);
    let mut context=core::task::Context::from_waker(core::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        core::task::Poll::Ready(value)=>value,
        _=>panic!("setup unexpectedly waits outside its transport"),
    }
}

#[derive(Default)]
struct Bus {
    transfers: Vec<(Vec<u8>,usize)>,
    get_response: Option<Vec<u8>>,
    fail_set: Option<u8>,
}
impl Transport for Bus {
    type Error=u8;
    async fn transfer(&mut self,write:&[u8],read:&mut[u8])->Result<(),u8> {
        self.transfers.push((write.to_vec(),read.len()));
        assert_eq!(&write[..2],[0x24,0]);
        if read.is_empty() {
            assert_eq!(write[3],3,"only SET_REPORT is expected for a write");
            let report_id=write[8];
            if self.fail_set==Some(report_id) {return Err(0xb0);}
        } else {
            assert_eq!(write,[0x24,0,0x34,2,0x25,0],"GET must preserve the modified report4 only");
            let Some(response)=&self.get_response else {return Err(0xa0);};
            assert_eq!(response.len(),read.len());
            read.copy_from_slice(response);
        }
        Ok(())
    }
}
impl Bus {
    fn set_reports(&self)->Vec<Vec<u8>> {
        self.transfers.iter().filter(|(_,read)|*read==0).map(|(write,_)| {
            assert_eq!(&write[4..6],[0x25,0]);
            assert_eq!(u16::from_le_bytes([write[6],write[7]]) as usize,write.len()-6);
            write[8..].to_vec()
        }).collect()
    }
    fn get_count(&self)->usize {self.transfers.iter().filter(|(_,read)|*read!=0).count()}
}

fn with_unknown_feature_data()->Vec<u8> {
    let mut descriptor=CAPTURED.to_vec();
    // Append one independent vendor byte to Input Mode's report. Existing
    // sensor settings in that byte must survive changing the known control.
    let needle=[0x09,0x52,0x15,0x00,0x25,0x0a,0x75,0x08,0x95,0x01,0xb1,0x02];
    let offset=descriptor.windows(needle.len()).position(|bytes|bytes==needle).unwrap()+needle.len();
    descriptor.splice(offset..offset,[0x06,0x00,0xff,0x09,0x01,0x75,0x08,0x95,0x01,0xb1,0x02,0x05,0x0d]);
    descriptor
}

#[test]
fn captured_setup_sets_precision_surface_buttons_and_latency_without_get_readback() {
    let decoder=Touchpad::from_descriptor(CAPTURED).unwrap();
    let mut bus=Bus::default();
    assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_ok());
    let mut reports=bus.set_reports();reports.sort();
    assert_eq!(reports,[vec![4,3],vec![6,3],vec![13,0]]);
    assert_eq!(bus.get_count(),0,"unsupported GET_REPORT cannot block this known descriptor");
    assert_eq!(bus.transfers.len(),3,"both report6 controls must share one merged SET");
}

#[test]
fn unknown_feature_data_requires_one_get_and_survives_known_control_updates() {
    let decoder=Touchpad::from_descriptor(&with_unknown_feature_data()).unwrap();
    let mut bus=Bus{get_response:Some(vec![5,0,4,9,0xa5]),..Default::default()};
    assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_ok());
    let mut reports=bus.set_reports();reports.sort();
    assert_eq!(reports,[vec![4,3,0xa5],vec![6,3],vec![13,0]]);
    assert_eq!(bus.get_count(),1);
}

#[test]
fn failed_get_cannot_zero_unrelated_feature_data() {
    let decoder=Touchpad::from_descriptor(&with_unknown_feature_data()).unwrap();
    let mut bus=Bus::default();
    assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_err());
    assert_eq!(bus.get_count(),1);
    assert!(bus.set_reports().is_empty());
}

#[test]
fn incomplete_or_wrong_id_feature_get_never_becomes_a_set() {
    for response in [vec![4,0,4,9,0xa5],vec![5,0,7,9,0xa5]] {
        let decoder=Touchpad::from_descriptor(&with_unknown_feature_data()).unwrap();
        let mut bus=Bus{get_response:Some(response),..Default::default()};
        assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_err());
        assert_eq!(bus.get_count(),1);
        assert!(bus.set_reports().is_empty());
    }
}

#[test]
fn set_transport_failure_stops_setup_before_later_controls() {
    let decoder=Touchpad::from_descriptor(CAPTURED).unwrap();
    let mut bus=Bus{fail_set:Some(6),..Default::default()};
    assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_err());
    assert_eq!(bus.get_count(),0);
    assert_eq!(bus.set_reports(),[vec![4,3],vec![6,3]]);
}

#[test]
fn descriptor_without_configuration_features_needs_no_feature_commands() {
    // First mouse collection from the same capture, through both EndCollection
    // items. It remains usable without any digitizer configuration features.
    let decoder=Touchpad::from_descriptor(&CAPTURED[..50]).unwrap();
    let mut bus=Bus::default();
    assert!(complete(select_input_mode(&mut bus,DESC,&decoder)).is_ok());
    assert!(bus.transfers.is_empty());
}
'''


def main():
    decoder = ROOT / "src/r/io/hid_touchpad.rs"
    service = (ROOT / "src/r/io/i2c_touchpad.rs").read_text()
    captured = re.search(r"const SYNAPTICS: &\[u8\] = &(\[.*?\]);", decoder.read_text(), re.DOTALL)
    assert captured, "captured Synaptics descriptor fixture missing"
    source = HARNESS.replace("@DECODER@", json.dumps(str(decoder)))
    source = source.replace("@PROTOCOL@", json.dumps(str(ROOT / "src/r/io/i2c_hid.rs")))
    source = source.replace("@CAPTURED@", "const CAPTURED: &[u8] = &" + captured[1] + ";")
    source = source.replace("@SETUP@", item(service, "async fn select_input_mode"))
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="trueos-i2c-touchpad-setup-") as directory:
        work = Path(directory)
        path = work / "tests.rs"
        binary = work / "tests"
        path.write_text(source)
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(path), "-o", str(binary)], cwd=work, check=True)
        subprocess.run([str(binary)], cwd=work, check=True)


if __name__ == "__main__":
    main()
