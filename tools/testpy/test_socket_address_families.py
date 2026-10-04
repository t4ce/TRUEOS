#!/usr/bin/env python3
"""Test family collision rules, VM wire encoding, and real TCP SYN dispatch.

Uses the production Mio reservation method and wire codec, and the vendored
smoltcp stack with an in-memory NIC. No hardware or kernel libc interposition.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    mio = (ROOT / 'src/mio_compat.rs').read_text()
    method = re.search(r'    fn tcp_listener_address_in_use\(.*?\n    }', mio, re.S).group()
    source = r'''
#![allow(dead_code, unused_imports)]
extern crate self as v;
#[path="VNET"] pub mod vnet;
#[path="WIRE"] mod wire;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken, ChecksumCapabilities};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::*;

#[derive(Clone,Copy)] enum CompatAddr { V4 { addr:[u8;4],port:u16 }, V6 { addr:[u8;16],port:u16 } }
#[derive(PartialEq)] enum MioSocketKind { TcpListener, TcpStream }
struct Reservation { kind:MioSocketKind, closed:bool, local:Option<CompatAddr> }
struct MioCompat { sockets:Vec<Reservation> }
impl MioCompat { METHOD }

struct Nic { packets:std::collections::VecDeque<Vec<u8>> }
struct Rx(Vec<u8>);
struct Tx;
impl RxToken for Rx { fn consume<R,F:FnOnce(&[u8])->R>(self,f:F)->R { f(&self.0) } }
impl TxToken for Tx { fn consume<R,F:FnOnce(&mut [u8])->R>(self,len:usize,f:F)->R { f(&mut vec![0;len]) } }
impl Device for Nic {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx;
    fn receive(&mut self,_:Instant)->Option<(Rx,Tx)> { self.packets.pop_front().map(|p|(Rx(p),Tx)) }
    fn transmit(&mut self,_:Instant)->Option<Tx> { Some(Tx) }
    fn capabilities(&self)->DeviceCapabilities {
        let mut c=DeviceCapabilities::default(); c.medium=Medium::Ip;
        c.max_transmission_unit=1500; c.checksum=ChecksumCapabilities::ignored(); c
    }
}
fn syn(src:IpAddress,dst:IpAddress,port:u16)->Vec<u8> {
    let tcp=TcpRepr { src_port:49152,dst_port:port,control:TcpControl::Syn,
        seq_number:TcpSeqNumber(1),ack_number:None,window_len:8192,
        window_scale:None,max_seg_size:None,sack_permitted:false,
        sack_ranges:[None;3],timestamp:None,payload:&[] };
    let header=if dst.version()==IpVersion::Ipv4 {20} else {40};
    let mut out=vec![0;header+tcp.buffer_len()];
    match (src,dst) {
        (IpAddress::Ipv4(src_addr),IpAddress::Ipv4(dst_addr)) => {
            let repr=Ipv4Repr { src_addr,dst_addr,next_header:IpProtocol::Tcp,payload_len:tcp.buffer_len(),hop_limit:64 };
            repr.emit(&mut Ipv4Packet::new_unchecked(&mut out[..]),&ChecksumCapabilities::ignored());
        }
        (IpAddress::Ipv6(src_addr),IpAddress::Ipv6(dst_addr)) => {
            let repr=Ipv6Repr { src_addr,dst_addr,next_header:IpProtocol::Tcp,payload_len:tcp.buffer_len(),hop_limit:64 };
            repr.emit(&mut Ipv6Packet::new_unchecked(&mut out[..]));
        }
        _ => unreachable!(),
    }
    tcp.emit(&mut TcpPacket::new_unchecked(&mut out[header..]),&src,&dst,&ChecksumCapabilities::ignored());
    out
}
fn listener(local:Option<SocketAddr>,port:u16)->tcp::Socket<'static> {
    let mut s=tcp::Socket::new(tcp::SocketBuffer::new(vec![0;1024]),tcp::SocketBuffer::new(vec![0;1024]));
    match local { Some(local)=>s.listen(local).unwrap(), None=>s.listen(port).unwrap() }; s
}
#[test] fn ipv4_ipv6_same_port_dispatch_in_either_creation_order() {
    for v6_first in [true,false] {
        let mut nic=Nic { packets:Default::default() };
        let mut iface=Interface::new(Config::new(HardwareAddress::Ip),&mut nic,Instant::ZERO);
        let v4:Ipv4Address="10.0.0.1".parse().unwrap();
        let v6:Ipv6Address="fd00::1".parse().unwrap();
        iface.update_ip_addrs(|a| { a.push(IpCidr::new(v4.into(),24)).unwrap();a.push(IpCidr::new(v6.into(),64)).unwrap(); });
        let mut sockets=SocketSet::new(vec![]);
        let a=SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(),14004);
        let b=SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(),14004);
        let (h4,h6)=if v6_first {
            let h6=sockets.add(listener(Some(b),14004));let h4=sockets.add(listener(Some(a),14004));(h4,h6)
        } else { (sockets.add(listener(Some(a),14004)),sockets.add(listener(Some(b),14004))) };
        nic.packets.push_back(syn("10.0.0.2".parse::<Ipv4Address>().unwrap().into(),v4.into(),14004));
        iface.poll(Instant::from_millis(1),&mut nic,&mut sockets);
        assert_eq!(sockets.get::<tcp::Socket>(h4).state(),tcp::State::SynReceived);
        assert_eq!(sockets.get::<tcp::Socket>(h6).state(),tcp::State::Listen);
        nic.packets.push_back(syn("fd00::2".parse::<Ipv6Address>().unwrap().into(),v6.into(),14004));
        iface.poll(Instant::from_millis(2),&mut nic,&mut sockets);
        assert_eq!(sockets.get::<tcp::Socket>(h6).state(),tcp::State::SynReceived);
    }
}
#[test] fn legacy_port_only_listener_still_accepts_both_families() {
    for v6 in [false,true] {
        let mut nic=Nic { packets:Default::default() };
        let mut iface=Interface::new(Config::new(HardwareAddress::Ip),&mut nic,Instant::ZERO);
        let dst:IpAddress=if v6 { "fd00::1".parse::<Ipv6Address>().unwrap().into() } else { "10.0.0.1".parse::<Ipv4Address>().unwrap().into() };
        let src:IpAddress=if v6 { "fd00::2".parse::<Ipv6Address>().unwrap().into() } else { "10.0.0.2".parse::<Ipv4Address>().unwrap().into() };
        iface.update_ip_addrs(|a| { a.push(IpCidr::new(dst,if v6 {64} else {24})).unwrap(); });
        let mut sockets=SocketSet::new(vec![]); let h=sockets.add(listener(None,14004));
        nic.packets.push_back(syn(src,dst,14004));iface.poll(Instant::from_millis(1),&mut nic,&mut sockets);
        assert_eq!(sockets.get::<tcp::Socket>(h).state(),tcp::State::SynReceived);
    }
}
#[test] fn vm_wire_roundtrip_and_truncation() {
    for local in ["0.0.0.0:14004","[::]:14004","[fd00::1]:50001"] {
        let command=vnet::Command::OpenTcpListenAt { local:local.parse().unwrap() };
        let mut bytes=[0;128];let n=wire::encode_command(command,&mut bytes).unwrap();
        assert_eq!(wire::decode_command(&bytes[..n]).unwrap(),command);
        for end in 0..n { assert!(wire::decode_command(&bytes[..end]).is_err()); }
    }
    let mut bytes=[0;128];let n=wire::encode_command(vnet::Command::OpenTcpListen {port:14004},&mut bytes).unwrap();
    assert_eq!(n,3);assert_eq!(&bytes[..n],&[2,0xb4,0x36]); // existing little-endian port ABI
}
#[test] fn same_family_conflicts_and_closed_listener_release() {
    let mut compat=MioCompat { sockets:vec![Reservation { kind:MioSocketKind::TcpListener, closed:false, local:Some(CompatAddr::V6 {addr:[0;16],port:14004}) }] };
    assert!(!compat.tcp_listener_address_in_use(CompatAddr::V4 {addr:[0;4],port:14004}));
    assert!(compat.tcp_listener_address_in_use(CompatAddr::V6 {addr:[1;16],port:14004}));
    assert!(!compat.tcp_listener_address_in_use(CompatAddr::V6 {addr:[0;16],port:14005}));
    compat.sockets[0].closed=true;
    assert!(!compat.tcp_listener_address_in_use(CompatAddr::V6 {addr:[0;16],port:14004}));
    compat.sockets[0].closed=false;compat.sockets[0].local=Some(CompatAddr::V4 {addr:[127,0,0,1],port:14004});
    assert!(compat.tcp_listener_address_in_use(CompatAddr::V4 {addr:[0;4],port:14004}));
    assert!(compat.tcp_listener_address_in_use(CompatAddr::V4 {addr:[127,0,0,1],port:14004}));
    assert!(!compat.tcp_listener_address_in_use(CompatAddr::V4 {addr:[127,0,0,2],port:14004}));
}
'''
    source = source.replace('VNET', str(ROOT / 'crates/trueos-v/src/vnet.rs')).replace('WIRE', str(ROOT / 'src/hv/blueprint/blueprint_net_wire.rs')).replace('METHOD', method)
    with tempfile.TemporaryDirectory(prefix='trueos-socket-families-') as directory:
        folder = Path(directory)
        (folder / 'lib.rs').write_text(source)
        (folder / 'Cargo.toml').write_text(f'''[package]
name="trueos-socket-family-tests"
version="0.1.0"
edition="2024"
[lib]
path="lib.rs"
[dependencies]
smoltcp={{path="{ROOT / 'vendor/smoltcp-0.13.1'}",default-features=false,features=["alloc","proto-ipv4","proto-ipv6","socket-tcp","medium-ip"]}}
''')
        subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(folder / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu', '--target-dir', '/tmp/trueos-socket-family-target', '-q'], cwd=folder, check=True)


if __name__ == '__main__':
    main()
