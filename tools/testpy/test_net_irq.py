#!/usr/bin/env python3
"""Host-check the IRQ owner, register races and actual stack deadline policy."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![feature(abi_x86_interrupt)]
#![allow(dead_code)]
extern crate alloc;
use core::sync::atomic::{AtomicBool, Ordering};
use std::{collections::BTreeMap, sync::Mutex};
#[macro_export] macro_rules! log_info { ($($tt:tt)*) => {{}}; }
#[macro_export] macro_rules! log_warn { ($($tt:tt)*) => {{}}; }
mod chronos { pub const CHRONOS_TIMER_VECTOR:u8=0x40; }
mod remote_work_wake { pub const REMOTE_WORK_WAKE_VECTOR:u8=0x41;pub fn local_eoi(){} }
mod hv { pub mod control_kick {pub const LIFECYCLE_KICK_VECTOR:u8=0x42;} }
mod live_update { pub const RENDEZVOUS_VECTOR:u8=0x43; }
mod intel { pub mod interrupts { pub const GRAPHICS_VECTOR:u8=0x44; } }
mod percpu {
    pub struct Cpu {pub slot:u32,pub lapic_id:u32}
    pub fn cpu_slots()->&'static [Cpu]{&[Cpu{slot:0,lapic_id:0}]}
}
mod pci {
    use super::*;
    pub static CONFIG:Mutex<BTreeMap<u16,u32>>=Mutex::new(BTreeMap::new());
    pub static SUCCESS:AtomicBool=AtomicBool::new(true);
    pub fn config_read_u32(_:u8,_:u8,_:u8,offset:u16)->u32{*CONFIG.lock().unwrap().get(&offset).unwrap_or(&0)}
    pub fn config_read_u16(b:u8,s:u8,f:u8,o:u16)->u16{config_read_u32(b,s,f,o) as u16}
    pub fn config_read_u8(b:u8,s:u8,f:u8,o:u16)->u8{config_read_u32(b,s,f,o) as u8}
    pub fn config_write_u32(_:u8,_:u8,_:u8,offset:u16,value:u32){CONFIG.lock().unwrap().insert(offset,value);}
    pub fn config_write_u16(b:u8,s:u8,f:u8,o:u16,value:u16){config_write_u32(b,s,f,o,u32::from(value));}
    pub fn enable_single_msi(b:u8,s:u8,f:u8,vector:u8,_:u32)->bool{
        config_write_u32(b,s,f,0x54,0xfee00000);config_write_u16(b,s,f,0x5c,u16::from(vector));
        config_write_u16(b,s,f,0x52,0x81);config_write_u16(b,s,f,4,0x406);
        SUCCESS.load(Ordering::SeqCst)
    }
    pub fn reset(){
        let mut config=CONFIG.lock().unwrap();config.clear();
        config.extend([(0,0x812510ec),(4,6),(6,0x10),(8,5),(0x34,0x50),
            (0x50,5),(0x51,0),(0x52,0x80),(0x54,0x12340000),(0x58,0x5678),(0x5c,0x39)]);
        SUCCESS.store(true,Ordering::SeqCst);
    }
}
mod net {
    pub mod adapter {pub const MAX_NET_DEVICES:usize=8;}
    #[path="@IRQ@"] pub mod interrupts;
}
#[path="@STALL@"] mod stall_deadline;
use smoltcp::time::{Instant, Duration as SmolDuration};
mod embassy_time_driver {pub const TICK_HZ:u64=1000;}
const IRQ_HEALTH_INTERVAL_MS:u64=1000;
const IPV6_RS_RETRY_MS:i64=5000;
const ICMP_VNET_TIMEOUT_MS:i64=2000;
const DHCP6_EAGER_DNS:bool=true;
#[derive(Clone,Copy,PartialEq,Eq)] @DHCP_STAGE@
struct Interface{next:Option<Instant>}
impl Interface {fn poll_at(&mut self,_:Instant,_:&Sockets)->Option<Instant>{self.next}}
mod raw {pub struct Socket;}
mod udp {pub struct Socket;}
mod icmp {pub struct Socket;}
struct Buffer(bool);
impl Buffer {fn can_send(&self)->bool{self.0}}
struct Sockets([Buffer;3]);
impl Sockets {fn get<T>(&self,handle:usize)->&Buffer{&self.0[handle]}}
struct Inflight{sent_at:Instant}
struct Bench{last_log_tick:u64}
struct NetService {
    iface:Interface,sockets:Sockets,raw_icmpv6:usize,dhcp6_udp:usize,icmp:usize,
    router_ipv6:Option<()>,local_ipv6_global:Option<()>,rs_last_sent:Option<Instant>,
    ra_managed:bool,ipv6_global_is_dhcp:bool,ra_dns6_count:u8,dhcp6_dns6_count:u8,ra_other:bool,
    dhcp6_stage:Dhcp6Stage,dhcp6_cooldown_until:Option<Instant>,dhcp6_last_sent:Option<Instant>,dhcp6_retries:u8,
    link:bool,icmp_ping_pongs:u8,router_ipv4:Option<()>,icmp_ping_inflight:Option<(u16,Instant)>,icmp_ping_last_sent:Option<Instant>,
    icmp6_ping_pongs:u8,icmp6_ping_inflight:Option<(u16,Instant)>,icmp6_ping_last_sent:Option<Instant>,
    icmp_vnet_inflight:Vec<Inflight>,internal_netbench:Vec<Bench>,
}
impl NetService {
    fn new()->Self{Self{iface:Interface{next:None},sockets:Sockets([Buffer(true),Buffer(true),Buffer(true)]),
        raw_icmpv6:0,dhcp6_udp:1,icmp:2,router_ipv6:Some(()),local_ipv6_global:Some(()),rs_last_sent:None,
        ra_managed:false,ipv6_global_is_dhcp:false,ra_dns6_count:1,dhcp6_dns6_count:0,ra_other:false,
        dhcp6_stage:Dhcp6Stage::Bound,dhcp6_cooldown_until:None,dhcp6_last_sent:None,dhcp6_retries:0,link:true,
        icmp_ping_pongs:1,router_ipv4:Some(()),icmp_ping_inflight:None,icmp_ping_last_sent:None,
        icmp6_ping_pongs:1,icmp6_ping_inflight:None,icmp6_ping_last_sent:None,icmp_vnet_inflight:vec![],internal_netbench:vec![]}}
    fn link_up(&self)->bool{self.link}
    @DEADLINE@
}
fn at(ms:i64)->Instant{Instant::from_millis(ms)}
#[test] fn stack_deadline_precedes_health_and_pending_egress_runs_immediately(){
    let mut service=NetService::new();assert_eq!(service.next_deadline(at(10_000),true),at(11_000));
    service.iface.next=Some(at(10_012));assert_eq!(service.next_deadline(at(10_000),true),at(10_012));
    service.iface.next=Some(at(0));assert_eq!(service.next_deadline(at(10_000),true),at(0));
}
#[test] fn solicitation_and_dhcp6_retries_keep_their_real_deadlines(){
    let mut service=NetService::new();service.router_ipv6=None;service.local_ipv6_global=None;
    service.rs_last_sent=Some(at(5_200));assert_eq!(service.next_deadline(at(10_000),true),at(10_200));
    service.router_ipv6=Some(());service.dhcp6_stage=Dhcp6Stage::Info;service.dhcp6_last_sent=Some(at(7_500));service.dhcp6_retries=1;
    assert_eq!(service.next_deadline(at(10_000),true),at(10_500));
    service.dhcp6_stage=Dhcp6Stage::Idle;service.ra_dns6_count=0;service.dhcp6_cooldown_until=Some(at(10_750));
    assert_eq!(service.next_deadline(at(10_000),true),at(10_750));
    service.dhcp6_cooldown_until=None;assert_eq!(service.next_deadline(at(10_000),true),at(10_000));
}
#[test] fn ping_timeouts_and_pruning_do_not_depend_on_periodic_scans(){
    let mut service=NetService::new();service.icmp_ping_pongs=0;service.icmp_ping_inflight=Some((1,at(8_100)));
    assert_eq!(service.next_deadline(at(10_000),true),at(10_100));
    service.link=false;assert_eq!(service.next_deadline(at(10_000),true),at(11_000),"link-down diagnostic must not busy-loop");
    service.icmp_vnet_inflight.push(Inflight{sent_at:at(8_050)});assert_eq!(service.next_deadline(at(10_000),true),at(10_050));
}
#[test] fn benchmark_reporting_and_dhcp_address_transition_remain_scheduled(){
    let mut service=NetService::new();service.internal_netbench.push(Bench{last_log_tick:9_100});
    assert_eq!(service.next_deadline(at(10_000),true),at(10_100));
    service.internal_netbench.clear();service.ra_managed=true;assert_eq!(service.next_deadline(at(10_000),true),at(10_000));
}
#[test] fn link_down_with_overdue_rs_dhcp6_and_stack_egress_parks_instead_of_yielding_forever(){
    let mut service=NetService::new();service.link=false;service.router_ipv6=None;service.local_ipv6_global=None;
    service.rs_last_sent=Some(at(0));service.ra_dns6_count=0;service.dhcp6_stage=Dhcp6Stage::Idle;
    service.dhcp6_cooldown_until=Some(at(0));service.iface.next=Some(at(0));
    for timestamp in [10_000,11_000,15_000,50_000] {
        assert_eq!(service.next_deadline(at(timestamp),false),at(timestamp+1000));
    }
    service.iface.next=Some(at(10_200));
    assert_eq!(service.next_deadline(at(10_000),false),at(10_200),"future TCP deadline lost behind link backpressure");
}
#[test] fn full_tx_ring_suppresses_only_already_due_work_and_resumes_on_readiness(){
    let mut service=NetService::new();service.iface.next=Some(at(0));
    assert_eq!(service.next_deadline(at(10_000),false),at(11_000));
    assert_eq!(service.next_deadline(at(10_000),true),at(0));
    service.iface.next=Some(at(10_300));
    assert_eq!(service.next_deadline(at(10_000),false),at(10_300));
}
#[test] fn full_local_control_buffers_do_not_rearm_expired_deadlines(){
    let mut service=NetService::new();service.router_ipv6=None;service.local_ipv6_global=None;
    service.rs_last_sent=Some(at(0));service.dhcp6_stage=Dhcp6Stage::Info;
    service.dhcp6_last_sent=Some(at(0));service.icmp_ping_pongs=0;service.icmp_ping_last_sent=Some(at(0));
    service.sockets= Sockets([Buffer(false),Buffer(false),Buffer(false)]);
    assert_eq!(service.next_deadline(at(10_000),true),at(11_000));
    service.dhcp6_last_sent=Some(at(9_000));
    assert_eq!(service.next_deadline(at(10_000),true),at(10_500),"future retry must remain a deadline");
    service.sockets.0[0].0=true;
    assert_eq!(service.next_deadline(at(10_000),true),at(5_000));
}
'''

IRQ_TESTS = r'''
#[cfg(test)] mod host_tests {
    use super::*;
    use core::future::Future;
    use std::{sync::Arc, task::{Wake,Waker}};
    fn reset()->Box<[u32;32]>{
        READY.store(false,Ordering::SeqCst);FAULTED.store(false,Ordering::SeqCst);DEVICE.store(usize::MAX,Ordering::SeqCst);
        MMIO.store(0,Ordering::SeqCst);CONFIGURING.store(false,Ordering::SeqCst);HANDLERS.store(0,Ordering::SeqCst);
        crate::pci::reset();Box::new([0;32])
    }
    #[test] fn physical_gate_and_existing_msi_are_rejected_without_taking_ownership(){
        let mut mmio=reset();let base=mmio.as_mut_ptr() as usize;
        crate::pci::config_write_u32(2,0,0,0,0x812610ec);assert!(!configure_r8125(1,2,0,0,base));
        crate::pci::reset();crate::pci::config_write_u16(2,0,0,0x52,0x81);assert!(!configure_r8125(1,2,0,0,base));
        assert!(!is_ready(1));assert_eq!(mmio[registers::MASK/4],0);
    }
    #[test] fn msi_programming_failure_restores_disabled_capability_and_command(){
        let mut mmio=reset();crate::pci::SUCCESS.store(false,Ordering::SeqCst);
        assert!(!configure_r8125(1,2,0,0,mmio.as_mut_ptr() as usize));
        assert_eq!(crate::pci::config_read_u16(2,0,0,0x52),0x80);
        assert_eq!(crate::pci::config_read_u32(2,0,0,0x54),0x12340000);
        assert_eq!(crate::pci::config_read_u32(2,0,0,0x58),0x5678);
        assert_eq!(crate::pci::config_read_u16(2,0,0,0x5c),0x39);
        assert_eq!(crate::pci::config_read_u16(2,0,0,4),6);assert!(!is_ready(1));
    }
    #[derive(Default)] struct Count(AtomicUsize);
    impl Wake for Count{fn wake(self:Arc<Self>){self.0.fetch_add(1,Ordering::SeqCst);}}
    #[test] fn publication_before_and_during_registration_wakes_without_consuming_a_permit(){
        let mut mmio=reset();assert!(configure_r8125(1,2,0,0,mmio.as_mut_ptr() as usize));
        assert_eq!(mmio[registers::MASK/4],0,"initial drain must precede unmask");
        let observed=generation(1);notify();let mut before=Box::pin(wait_after(1,observed));
        assert!(before.as_mut().poll(&mut core::task::Context::from_waker(Waker::noop())).is_ready());drop(before);
        let current=generation(1);let count=Arc::new(Count::default());let waker=Waker::from(count.clone());
        let mut waiting=Box::pin(wait_after(1,current));
        assert!(waiting.as_mut().poll(&mut core::task::Context::from_waker(&waker)).is_pending());
        notify();assert_eq!(count.0.load(Ordering::SeqCst),1);
        assert!(waiting.as_mut().poll(&mut core::task::Context::from_waker(&waker)).is_ready());
    }
}
'''


def main():
    source = (ROOT / "src/net/adapter.rs").read_text()
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-net-irq-") as directory:
        directory = Path(directory)
        (directory / "interrupts").mkdir()
        irq = directory / "interrupts.rs"
        irq.write_text((ROOT / "src/net/interrupts.rs").read_text() + IRQ_TESTS)
        shutil.copyfile(ROOT / "src/net/interrupts/registers.rs", directory / "registers.rs")
        harness = HARNESS.replace("@IRQ@", str(irq)).replace("@DHCP_STAGE@", item(source, "enum Dhcp6Stage"))
        harness = harness.replace("@STALL@", str(ROOT / "src/net/r8125/stall_deadline.rs"))
        harness = harness.replace("@DEADLINE@", item(source, "fn next_deadline(&mut self"))
        (directory / "lib.rs").write_text(harness)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="net-irq-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n'
            '[dependencies]\natomic-waker="=1.1.2"\n'
            'x86_64={git="https://github.com/rust-osdev/x86_64",rev="78b81025d65a3e899f48a6fded8b27889a09d70b",default-features=false,features=["instructions","nightly","memory_encryption"]}\n'
            f'smoltcp={{path="{ROOT / "vendor/smoltcp-0.13.1"}",default-features=false,features=["proto-ipv4","socket-tcp","medium-ethernet","alloc"]}}\n')
        subprocess.run(["cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/net-irq"),
            "--", "--test-threads=1"], cwd=directory, check=True, timeout=120)


if __name__ == "__main__":
    main()
