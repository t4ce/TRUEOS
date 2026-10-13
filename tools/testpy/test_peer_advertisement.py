#!/usr/bin/env python3
"""Discovery remains independent of checkpoints and advertises a live service."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = r'''#![allow(dead_code)]
extern crate alloc;
use alloc::{format,vec::Vec};
use core::sync::atomic::{AtomicBool,AtomicU64,Ordering};
#[macro_export]macro_rules! log {($($a:tt)*)=>{{let _=format_args!($($a)*);}}}
mod v {pub mod vnet {#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct EndpointV4{pub addr:[u8;4],pub port:u16}impl EndpointV4{pub fn new(addr:[u8;4],port:u16)->Self{Self{addr,port}}}}}
mod net {
    pub fn primary_device_index()->usize{0}
    pub fn mac_address_at(_:usize)->Option<[u8;6]>{Some([0,1,2,3,4,5])}
    pub mod adapter{pub fn ipv4_at(_:usize)->Option<[u8;4]>{Some([192,168,178,1])}}
}
mod r {pub mod net {pub mod ports {
    pub const VM_STORE_REPL_PORT:u16=4250;
    pub const HTTP_TRUEOSFS_TCP_PORT:u16=80;
    pub const TRUEOS_DISCOVERY_UDP_PORT:u16=4251;
}}pub mod readiness {
    pub const HTTP_TRUEOSFS_LISTENING:u32=1;
    pub static READY:crate::AtomicBool=crate::AtomicBool::new(false);
    pub fn mask()->u32{if READY.load(crate::Ordering::SeqCst){HTTP_TRUEOSFS_LISTENING}else{0}}
}}
#[cfg(feature="legacy_vm_checkpoint")]
mod hv {pub mod store {pub fn replication_online()->bool{crate::r::readiness::READY.load(crate::Ordering::SeqCst)}}}
static NOW:AtomicU64=AtomicU64::new(1000);
fn monotonic_ms()->u64{NOW.load(Ordering::SeqCst)}
@ITEMS@
fn reset(){LAST_ADVERTISE_MS.store(0,Ordering::SeqCst);NOW.store(1000,Ordering::SeqCst);r::readiness::READY.store(false,Ordering::SeqCst);}
#[test]fn does_not_advertise_an_unserved_endpoint(){reset();assert!(take_peer_advertisements().is_empty());}
#[test]fn advertised_endpoint_and_caps_match_the_enabled_service(){
    reset();r::readiness::READY.store(true,Ordering::SeqCst);let ads=take_peer_advertisements();assert_eq!(ads.len(),2);
    for ad in ads{let parsed=parse_peer_advertisement(v::vnet::EndpointV4::new([192,168,178,1],4251),&ad.data).unwrap();
        #[cfg(not(feature="legacy_vm_checkpoint"))]{assert_eq!(parsed.peer_tcp_port,80);assert_eq!(parsed.caps,peer_caps::FS);}
        #[cfg(feature="legacy_vm_checkpoint")]{assert_eq!(parsed.peer_tcp_port,4250);assert_eq!(parsed.caps,peer_caps::TRUEOS_HOST_DEFAULT);}
        assert_ne!(parsed.node_id,0);
    }
}
#[test]fn rate_limit_and_remote_legacy_advertisements_remain_supported(){
    reset();r::readiness::READY.store(true,Ordering::SeqCst);assert!(!take_peer_advertisements().is_empty());assert!(take_peer_advertisements().is_empty());NOW.store(2000,Ordering::SeqCst);assert!(!take_peer_advertisements().is_empty());
    let remote=parse_peer_advertisement(v::vnet::EndpointV4::new([1,2,3,4],4251),b"C0DEC0DE v=1 tcp=4250 node=0x123 caps=registry,status,fs").unwrap();assert_eq!(remote.peer_tcp_port,4250);assert_eq!(remote.node_id,0x123);assert_eq!(remote.caps,7);
    assert!(parse_peer_advertisement(remote.from,b"C0DEC0DE").is_none());
}
'''
    path = 'src/r/net/trueos_peer.rs'
    peer = (ROOT / path).read_text()
    items = []
    for name in ('PEER_ADVERTISE_INTERVAL_MS', 'PEER_ADVERTISE_UNICAST_TARGETS', 'PEER_ADVERTISEMENT_MAGIC',
                 'LOCAL_NODE_ID', 'LAST_ADVERTISE_MS', 'ADVERTISE_TARGETS_LOGGED'):
        items.append(re.search(rf'^(?:const|static) {name}:.*?;$', peer, re.M | re.S).group())
    for name in ('peer_caps', 'PeerAdvertisement', 'PeerHostAdvertisement', 'local_node_id',
                 'parse_u64_token', 'parse_caps_token', 'parse_peer_advertisement', 'take_peer_advertisements'):
        item = extract.item(path, name)
        items.append(item)
    source = source.replace('@ITEMS@', '\n'.join(items))
    with tempfile.TemporaryDirectory(prefix='trueos-peer-advertisement-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        for name, cfg in [('default', []), ('legacy', ['--cfg', 'feature="legacy_vm_checkpoint"'])]:
            subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / name)] + cfg, check=True)
            subprocess.run([str(path / name), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
