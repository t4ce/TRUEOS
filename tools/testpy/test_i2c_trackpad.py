#!/usr/bin/env python3
"""Exercise I2C transport, firmware resources, HID decoding and pointer publish."""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
pub struct Duration; impl Duration { pub fn from_millis(_:u64)->Self {Self} }
pub struct Instant; impl Instant {pub fn now()->Self{Self} pub fn as_millis(&self)->u64{0}}
pub struct Timer; impl Timer {pub async fn after(_:Duration) {}}
mod pci {
 #[derive(Clone,Copy,Debug)] pub struct PciDevice{pub bus:u8,pub slot:u8,pub function:u8,pub vendor_id:u16,pub device_id:u16,pub class:u8,pub subclass:u8}
 impl PciDevice {pub fn bar_address(&self,_:usize)->Option<u64>{Some(0x100000)}}
 pub fn release_device_claim(_:u8,_:u8,_:u8,_:&str)->bool{true}
 pub fn claim_device(_: &PciDevice,_:&str)->Result<(),()>{Ok(())}
 pub fn config_read_u32(_:u8,_:u8,_:u8,_:u16)->u32{0}
 pub fn config_read_u16(_:u8,_:u8,_:u8,_:u16)->u16{0}
 pub fn config_read_u8(_:u8,_:u8,_:u8,_:u16)->u8{0}
 pub fn config_write_u16(_:u8,_:u8,_:u8,_:u16,_:u16){}
 pub fn read_bar_raw(_:u8,_:u8,_:u8,_:u8)->(u32,Option<u32>){(0x100000,None)}
 pub fn bar_size_bytes(_:u8,_:u8,_:u8,_:u8)->Option<u64>{Some(0x1000)}
 pub mod mmio {pub fn map_mmio_region_exact(_:u64,_:usize)->Result<core::ptr::NonNull<u8>,()>{Err(())}}
}
mod efi { pub mod acpi {
 #[path=@RESOURCES@] pub mod i2c_hid;
 #[path=@FIRMWARE@] pub mod i2c_hid_firmware;
}}
mod io {
 #[path=@MASTER@] pub mod designware_i2c;
 #[path=@PROTOCOL@] pub mod i2c_hid;
 #[path=@PLATFORM@] pub mod intel_i2c;
 #[path=@GPIO@] pub mod intel_gpio;
 #[path=@DECODER@] pub mod hid_touchpad;
}
use io::{i2c_hid::{self,Descriptor,Input,Transport},hid_touchpad::{Touchpad,Pointer,Feature}};
thread_local! {static EVENTS: std::cell::RefCell<Vec<(u32,u32,u32,Pointer,bool)>> = const {std::cell::RefCell::new(Vec::new())};}
mod usb2 {pub mod hid {
 pub fn handle_mouse_report_values(c:u32,s:u32,e:u32,buttons:u8,dx:i32,dy:i32,wheel:i32,has_wheel:bool) {
  crate::EVENTS.with(|events|events.borrow_mut().push((c,s,e,crate::Pointer{buttons,dx,dy,wheel},has_wheel)));
 }
}}
@IDENTITY@
@PUBLISH@
@FEATURE_VALUE@

fn complete<T>(future:impl core::future::Future<Output=T>)->T {
 let mut future=core::pin::pin!(future);
 let mut context=core::task::Context::from_waker(core::task::Waker::noop());
 match future.as_mut().poll(&mut context) {core::task::Poll::Ready(value)=>value,_=>panic!("unexpected wait")}
}

#[test]
fn framed_relative_input_reaches_the_shared_pointer_sink_with_physical_identity() {
 // The mouse-compatible collection from the captured Synaptics descriptor.
 let raw:Vec<u8>="05 01 09 02 a1 01 85 02 09 01 a1 00 05 09 19 01 29 02 15 00 25 01 75 01 95 02 81 02 95 06 81 01 05 01 09 30 09 31 15 81 25 7f 75 08 95 02 81 06 c0 c0"
  .split_whitespace().map(|byte|u8::from_str_radix(byte,16).unwrap()).collect();
 struct Fake {step:usize,raw:Vec<u8>}
 impl Transport for Fake {
  type Error=();
  async fn transfer(&mut self,write:&[u8],read:&mut[u8])->Result<(),()> {
   match self.step {
    0=>{assert_eq!(write,[0x20,0]);assert_eq!(read.len(),30);
     for (offset,value) in [(0,30u16),(2,0x100),(4,self.raw.len() as u16),(6,0x21),(10,16),(16,0x24),(18,0x25),(20,0x6cb),(22,0xce1a)] {
      read[offset..offset+2].copy_from_slice(&value.to_le_bytes());
     }
    },
    1=>{assert_eq!(write,[0x21,0]);read.copy_from_slice(&self.raw);},
    2=>{assert!(write.is_empty());assert_eq!(read.len(),16);read[..6].copy_from_slice(&[6,0,2,1,3,0xfd]);},
    _=>panic!("unexpected transaction"),
   }
   self.step+=1;Ok(())
  }
 }
 let mut bus=Fake{step:0,raw};
 let descriptor=complete(Descriptor::read(&mut bus,0x20)).unwrap();
 let mut bytes=vec![0;usize::from(descriptor.report_descriptor_length)];
 complete(descriptor.report_descriptor(&mut bus,&mut bytes)).unwrap();
 let mut decoder=Touchpad::from_descriptor(&bytes).unwrap();
 let mut input=[0;16];
 let Input::Report{length}=complete(descriptor.read_input(&mut bus,&mut input)).unwrap() else {panic!("no input")};
 let pointer=decoder.decode_i2c(&input[..length]).unwrap();
 assert_eq!(pointer,Pointer{buttons:1,dx:3,dy:-3,wheel:0});
 publish(pointer);
 EVENTS.with(|events| {
  let events=events.borrow();assert_eq!(events.len(),1);
  assert_eq!(events[0],(CONTROLLER_ID,SLOT_ID,ENDPOINT,pointer,false));
  assert_ne!(CONTROLLER_ID,0);assert!(SLOT_ID>65535);
 });
}

#[test]
fn input_mode_readback_requires_the_actual_field_and_correct_report_id() {
 let feature=Feature{report_id:4,report_len:2,bit_offset:0,bit_size:8};
 assert_eq!(feature_value(feature,&[4,3]),Some(3));
 assert_eq!(feature_value(feature,&[4]),None);
 assert_eq!(feature_value(feature,&[9,3]),None);
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dsdt", type=Path, help="optional private raw DSDT capture")
    args = parser.parse_args()
    driver = (ROOT / "src/r/io/i2c_touchpad.rs").read_text()
    source = HARNESS
    for marker, path in {
        "RESOURCES": "src/efi/acpi/i2c_hid.rs",
        "FIRMWARE": "src/efi/acpi/i2c_hid_firmware.rs",
        "MASTER": "src/r/io/designware_i2c.rs",
        "PROTOCOL": "src/r/io/i2c_hid.rs",
        "PLATFORM": "src/r/io/intel_i2c.rs",
        "GPIO": "src/r/io/intel_gpio.rs",
        "DECODER": "src/r/io/hid_touchpad.rs",
    }.items():
        source = source.replace("@" + marker + "@", json.dumps(str(ROOT / path)))
    source = source.replace("@IDENTITY@", "\n".join(
        re.search(rf"const {name}:.*?;", driver)[0]
        for name in ("CONTROLLER_ID", "SLOT_ID", "ENDPOINT")))
    source = source.replace("@PUBLISH@", item(driver, "fn publish("))
    source = source.replace("@FEATURE_VALUE@", item(driver, "fn feature_value("))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    environment = os.environ.copy()
    if args.dsdt:
        environment["TRUEOS_I2C_TEST_DSDT"] = str(args.dsdt.resolve())
    with tempfile.TemporaryDirectory(prefix="trueos-i2c-trackpad-") as directory:
        directory = Path(directory)
        path = directory / "tests.rs"
        path.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(path), "-o", str(executable)], cwd=directory, check=True)
        subprocess.run([str(executable), "--test-threads=1"], cwd=directory, env=environment, check=True)


if __name__ == "__main__":
    main()
