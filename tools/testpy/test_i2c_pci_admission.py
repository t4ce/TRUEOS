#!/usr/bin/env python3
"""Test actual Intel I2C PCI admission against a modeled firmware handoff.

The production adapter, BAR codec, DesignWare master and I2C-HID protocol are
compiled unchanged. PCI configuration, time and boot modules are deterministic
host boundaries. MMIO mapping always fails before any register access.
"""
from pathlib import Path
import json
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
#[macro_export] macro_rules! log_info {
    (target:$target:literal;$($args:tt)*) => {{ let _ = format_args!($($args)*); }};
}
pub struct Duration(u64);
impl Duration { pub fn from_millis(value:u64)->Self { Self(value) } }
pub struct Instant(u64);
impl Instant {
    pub fn now()->Self { Self(fixture::with(|model| model.now_ms)) }
    pub fn as_millis(&self)->u64 { self.0 }
}
pub struct Timer;
impl Timer {
    pub async fn after(duration:Duration) { fixture::with(|model| model.now_ms += duration.0); }
}
fn complete<T>(future:impl core::future::Future<Output=T>)->T {
    let mut future=core::pin::pin!(future);
    let mut context=core::task::Context::from_waker(core::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        core::task::Poll::Ready(value)=>value,
        _=>panic!("fixture timers must finish synchronously"),
    }
}
mod pci {
    #[path=@WIRE@] pub mod firmware_bar_wire;
    #[derive(Clone,Copy,Debug)]
    pub struct PciDevice {
        pub bus:u8, pub slot:u8, pub function:u8,
        pub vendor_id:u16, pub device_id:u16, pub class:u8, pub subclass:u8,
    }
    fn check(bus:u8,slot:u8,function:u8) {
        assert_eq!((bus,slot,function),(0,0x15,0));
    }
    pub fn config_read_u32(bus:u8,slot:u8,function:u8,offset:u16)->u32 {
        check(bus,slot,function);
        crate::fixture::with(|model| u32::from_le_bytes(model.config[usize::from(offset)..usize::from(offset)+4].try_into().unwrap()))
    }
    pub fn config_read_u16(bus:u8,slot:u8,function:u8,offset:u16)->u16 {
        check(bus,slot,function);
        crate::fixture::with(|model| model.word(offset))
    }
    pub fn config_read_u8(bus:u8,slot:u8,function:u8,offset:u16)->u8 {
        check(bus,slot,function);
        crate::fixture::with(|model| model.config[usize::from(offset)])
    }
    pub fn config_write_u32(bus:u8,slot:u8,function:u8,offset:u16,value:u32) {
        check(bus,slot,function);
        assert!([0x10,0x14].contains(&offset), "unexpected 32-bit config write");
        crate::fixture::with(|model| {
            let command=model.word(4);
            model.bar_writes.push((offset,value,command));
            if !model.reject_bar_writes { model.dword_set(offset,value); }
        });
    }
    pub fn config_write_u16(bus:u8,slot:u8,function:u8,offset:u16,value:u16) {
        check(bus,slot,function);
        crate::fixture::with(|model| match offset {
            4=>{ model.command_writes.push(value); model.word_set(4,value); },
            0x74=>{
                let previous=model.word(0x74);
                model.pmcsr_writes.push(value);
                // PME status is W1C; a zero preserves the previously set bit.
                let next=(value & !0x8000) | ((previous & 0x8000) & !(value & 0x8000));
                model.word_set(0x74,next);
                if previous & 3 == 3 && next & 3 == 0 {
                    if model.d0_clears_bar { model.dword_set(0x10,4); model.dword_set(0x14,0); }
                    if model.d0_resets_command { model.word_set(4,6); }
                }
            },
            _=>panic!("unexpected 16-bit config write {offset:#x}"),
        });
    }
    pub fn read_bar_raw(bus:u8,slot:u8,function:u8,index:u8)->(u32,Option<u32>) {
        assert_eq!(index,0);
        let low=config_read_u32(bus,slot,function,0x10);
        let high=((low>>1)&3==2).then(||config_read_u32(bus,slot,function,0x14));
        (low,high)
    }
    pub fn bar_size_bytes(bus:u8,slot:u8,function:u8,index:u8)->Option<u64> {
        check(bus,slot,function); assert_eq!(index,0);
        crate::fixture::with(|model| {
            model.size_probe_commands.push(model.word(4));
            model.bar_size
        })
    }
    pub fn claim_device(_: &PciDevice,owner:&str)->Result<(),()> {
        assert_eq!(owner,"input/i2c-hid");
        crate::fixture::with(|model| {
            if model.claimed {return Err(());}
            model.claimed=true; model.claims+=1; Ok(())
        })
    }
    pub fn release_device_claim(bus:u8,slot:u8,function:u8,owner:&str)->bool {
        check(bus,slot,function); assert_eq!(owner,"input/i2c-hid");
        crate::fixture::with(|model| {
            assert!(model.claimed,"claim released twice");
            model.claimed=false; model.releases+=1; true
        })
    }
    pub mod mmio {
        pub fn map_mmio_region_exact(base:u64,length:usize)->Result<core::ptr::NonNull<u8>,()> {
            crate::fixture::with(|model| model.maps.push((base,length)));
            Err(())
        }
    }
}
mod limine {
    pub fn module_bytes_by_string(name:&[u8])->Option<&'static [u8]> {
        assert_eq!(name,crate::pci::firmware_bar_wire::MODULE_CMDLINE);
        crate::fixture::with(|model| { model.module_reads+=1; model.module })
    }
}
mod fixture {
    use super::pci::{PciDevice,firmware_bar_wire::{self,Reservation}};
    pub const DEVICE:PciDevice=PciDevice {
        bus:0,slot:0x15,function:0,vendor_id:0x8086,device_id:0xa0e8,class:0x0c,subclass:0x80,
    };
    pub const ADDRESS:u64=0x4017_000000;
    pub const RESERVATION:Reservation=Reservation {
        segment:0,bus:0,slot:0x15,function:0,bar_index:0,
        vendor_id:0x8086,device_id:0xa0e8,base:ADDRESS,length:4096,
    };
    pub struct Model {
        pub config:[u8;256], pub now_ms:u64, pub bar_size:Option<u64>,
        pub module:Option<&'static [u8]>, pub module_reads:usize,
        pub d0_clears_bar:bool, pub d0_resets_command:bool, pub reject_bar_writes:bool,
        pub bar_writes:Vec<(u16,u32,u16)>, pub command_writes:Vec<u16>,
        pub pmcsr_writes:Vec<u16>, pub size_probe_commands:Vec<u16>,
        pub maps:Vec<(u64,usize)>, pub claimed:bool, pub claims:usize, pub releases:usize,
    }
    impl Default for Model {
        fn default()->Self {
            let mut model=Self {
                config:[0;256],now_ms:0,bar_size:Some(4096),module:None,module_reads:0,
                d0_clears_bar:false,d0_resets_command:false,reject_bar_writes:false,
                bar_writes:Vec::new(),command_writes:Vec::new(),pmcsr_writes:Vec::new(),
                size_probe_commands:Vec::new(),maps:Vec::new(),claimed:false,claims:0,releases:0,
            };
            model.dword_set(0,0xa0e8_8086);
            model.word_set(4,0x0107); // IO/MEM/DMA enabled plus an unrelated control.
            model.word_set(6,0x10); // Standard capability list.
            model.config[0x34]=0x70;
            model.config[0x70]=1;
            model.word_set(0x74,0x8103); // D3hot, PME enable and pending W1C status.
            model.set_bar(0);
            model
        }
    }
    impl Model {
        pub fn word(&self,offset:u16)->u16 {
            u16::from_le_bytes(self.config[usize::from(offset)..usize::from(offset)+2].try_into().unwrap())
        }
        pub fn word_set(&mut self,offset:u16,value:u16) {
            self.config[usize::from(offset)..usize::from(offset)+2].copy_from_slice(&value.to_le_bytes());
        }
        pub fn dword_set(&mut self,offset:u16,value:u32) {
            self.config[usize::from(offset)..usize::from(offset)+4].copy_from_slice(&value.to_le_bytes());
        }
        pub fn set_bar(&mut self,address:u64) {
            self.dword_set(0x10,(address as u32 & !15)|4);
            self.dword_set(0x14,(address>>32) as u32);
        }
    }
    thread_local! {static MODEL:std::cell::RefCell<Model>=Default::default();}
    pub fn with<T>(f:impl FnOnce(&mut Model)->T)->T { MODEL.with(|model|f(&mut model.borrow_mut())) }
    pub fn reset(){with(|model|*model=Model::default());}
    pub fn install(records:&[Reservation]) {
        let mut bytes=vec![0;firmware_bar_wire::HEADER_BYTES+records.len()*firmware_bar_wire::RECORD_BYTES];
        firmware_bar_wire::encode_header(&mut bytes,records.len()).unwrap();
        for (index,record) in records.iter().enumerate() {
            record.encode(&mut bytes[firmware_bar_wire::HEADER_BYTES+index*firmware_bar_wire::RECORD_BYTES..]).unwrap();
        }
        install_raw(bytes);
    }
    pub fn install_raw(bytes:Vec<u8>) {
        let bytes:&'static [u8]=Box::leak(bytes.into_boxed_slice());
        with(|model|model.module=Some(bytes));
    }
    pub fn safe_writes() {
        with(|model| {
            assert!(model.bar_writes.iter().all(|write|write.2 & 6==0),"BAR written with MEM or DMA enabled");
            assert!(model.size_probe_commands.iter().all(|command|command & 6==0),"BAR sized with MEM or DMA enabled");
        });
    }
}
mod io {
    #[path=@MASTER@] pub mod designware_i2c;
    #[path=@PROTOCOL@] pub mod i2c_hid;
    #[path=@PLATFORM@] pub mod intel_i2c;
}
'''

TESTS = r'''

#[cfg(test)]
mod admission_integration_tests {
    use super::*;
    use crate::fixture::{self,ADDRESS,DEVICE,RESERVATION};

    fn prepare()->Result<(u64,u64),Error> { crate::complete(prepare_pci(DEVICE)) }
    fn open_error()->Error {
        match crate::complete(Controller::open(DEVICE,400_000)) {
            Err(error)=>error,
            Ok(_)=>panic!("fixture must reject mapping before any MMIO"),
        }
    }
    fn no_mapping_or_unsafe_writes() {
        fixture::with(|model|assert!(model.maps.is_empty()));
        fixture::safe_writes();
    }

    #[test]
    fn unassigned_64_bit_bar_uses_the_exact_firmware_allocation_above_four_gib() {
        fixture::reset(); fixture::install(&[RESERVATION]);
        assert_eq!(prepare(),Ok((ADDRESS,4096)));
        assert_eq!(Bar0::read(DEVICE).address(),Some(ADDRESS));
        fixture::with(|model| {
            assert_eq!(model.module_reads,1);
            assert_eq!(model.bar_writes.iter().map(|w|(w.0,w.1)).collect::<Vec<_>>(),[(0x14,0x40),(0x10,0x1700_0004)]);
            assert_eq!(model.word(4)&0x406,0x402);
            assert_eq!(model.word(4)&0x101,0x101); // Unrelated command controls survive.
            assert_eq!(model.now_ms,10);
        });
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn d0_reset_restores_the_preexisting_bar_with_decode_off_again() {
        fixture::reset();
        fixture::with(|model| {
            model.set_bar(ADDRESS); model.d0_clears_bar=true; model.d0_resets_command=true;
        });
        assert_eq!(prepare(),Ok((ADDRESS,4096)));
        assert_eq!(Bar0::read(DEVICE).address(),Some(ADDRESS));
        fixture::with(|model| {
            assert_eq!(model.module_reads,0);
            assert_eq!(model.bar_writes.len(),2);
            assert_eq!(model.word(4)&0x406,0x402);
        });
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn valid_existing_bar_is_preserved_without_consulting_or_rewriting_a_hint() {
        fixture::reset(); fixture::install_raw(vec![0xff;16]);
        fixture::with(|model|model.set_bar(ADDRESS));
        assert_eq!(prepare(),Ok((ADDRESS,4096)));
        fixture::with(|model| {assert_eq!(model.module_reads,0);assert!(model.bar_writes.is_empty());});
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn missing_or_zero_record_handoff_fails_without_guessing_an_address() {
        for empty in [false,true] {
            fixture::reset(); if empty {fixture::install(&[]);}
            let error=prepare().unwrap_err();
            assert_eq!(error.stage,"bar0");
            assert_eq!(error.reason,if empty {
                "BAR0 unassigned; firmware reservation unavailable"
            } else {"BAR0 unassigned; bootloader PCI reservations missing"});
            fixture::with(|model| {assert!(model.bar_writes.is_empty());assert_eq!(model.word(4)&6,0);});
            assert_eq!(Bar0::read(DEVICE).address(),None);
            no_mapping_or_unsafe_writes();
        }
    }

    #[test]
    fn wrong_pci_identity_size_and_malformed_hints_never_write_an_assignment() {
        for hint in [
            Reservation{bus:1,..RESERVATION}, Reservation{slot:0x16,..RESERVATION},
            Reservation{function:1,..RESERVATION}, Reservation{segment:1,..RESERVATION},
            Reservation{vendor_id:0x8087,..RESERVATION}, Reservation{device_id:0xa0e9,..RESERVATION},
            Reservation{bar_index:2,..RESERVATION}, Reservation{length:8192,..RESERVATION},
        ] {
            fixture::reset(); fixture::install(&[hint]);
            let error=prepare().unwrap_err(); assert_eq!(error.stage,"bar0");
            fixture::with(|model|assert!(model.bar_writes.is_empty()));
            no_mapping_or_unsafe_writes();
        }
        fixture::reset(); fixture::install_raw(vec![0;16]);
        assert_eq!(prepare().unwrap_err(),error("bar0","bootloader PCI reservations invalid"));
        fixture::with(|model|assert!(model.bar_writes.is_empty()));
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn invalid_size_and_nonzero_misaligned_assignment_do_not_use_a_hint() {
        for size in [None,Some(0),Some(2048),Some(12288)] {
            fixture::reset(); fixture::install(&[RESERVATION]);
            fixture::with(|model|model.bar_size=size);
            assert!(prepare().is_err());
            fixture::with(|model| {assert_eq!(model.module_reads,0);assert!(model.bar_writes.is_empty());});
            no_mapping_or_unsafe_writes();
        }
        fixture::reset(); fixture::install(&[RESERVATION]);
        fixture::with(|model|model.set_bar(ADDRESS+16));
        assert!(prepare().is_err());
        fixture::with(|model| {assert_eq!(model.module_reads,0);assert!(model.bar_writes.is_empty());});
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn actual_open_releases_the_claim_when_bar_restore_readback_fails() {
        fixture::reset(); fixture::install(&[RESERVATION]);
        fixture::with(|model|model.reject_bar_writes=true);
        assert_eq!(open_error(),error("bar0","firmware BAR0 restore readback failed"));
        fixture::with(|model| {
            assert_eq!((model.claims,model.releases,model.claimed),(1,1,false));
            assert_eq!(model.word(4)&6,0); assert_eq!(model.bar_writes.len(),2);
        });
        no_mapping_or_unsafe_writes();
    }

    #[test]
    fn actual_open_reaches_only_the_expected_mapping_and_releases_its_claim() {
        fixture::reset(); fixture::install(&[RESERVATION]);
        assert_eq!(open_error(),error("map","BAR0 MMIO mapping failed"));
        fixture::with(|model| {
            assert_eq!((model.claims,model.releases,model.claimed),(1,1,false));
            assert_eq!(model.maps,[(ADDRESS,4096)]);
        });
        fixture::safe_writes();
    }

    #[test]
    fn actual_power_transition_preserves_pending_pme_status_without_acknowledging_it() {
        fixture::reset(); fixture::with(|model|model.set_bar(ADDRESS));
        assert_eq!(prepare(),Ok((ADDRESS,4096)));
        fixture::with(|model| {
            assert_eq!(model.pmcsr_writes,[0x0100]);
            assert_eq!(model.word(0x74),0x8100);
        });
        no_mapping_or_unsafe_writes();
    }
}
'''


def main():
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="trueos-i2c-pci-admission-") as directory:
        directory = Path(directory)
        adapter = directory / "intel_i2c.rs"
        adapter.write_text((ROOT / "src/r/io/intel_i2c.rs").read_text() + TESTS)
        source = HARNESS
        for marker, path in {
            "WIRE": ROOT / "src/pci/firmware_bar_wire.rs",
            "MASTER": ROOT / "src/r/io/designware_i2c.rs",
            "PROTOCOL": ROOT / "src/r/io/i2c_hid.rs",
            "PLATFORM": adapter,
        }.items():
            source = source.replace("@" + marker + "@", json.dumps(str(path)))
        code = directory / "tests.rs"
        binary = directory / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(code), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=directory, check=True)


if __name__ == "__main__":
    main()
