#!/usr/bin/env python3
"""Production NET menu and network operations with deterministic host services."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code,unreachable_patterns)]
extern crate alloc;
extern crate self as trueos_time;
extern crate self as trueos_executor;
extern crate self as v;
use alloc::{string::String,vec::Vec,collections::VecDeque};
use spin::Mutex;
use std::sync::atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering};
static NOW:AtomicU64=AtomicU64::new(0);
static LIVE:AtomicBool=AtomicBool::new(true);
static VISIBLE:AtomicBool=AtomicBool::new(true);
static SLOT:AtomicBool=AtomicBool::new(false);
static SPAWNS:AtomicUsize=AtomicUsize::new(0);
static REQUESTS:AtomicUsize=AtomicUsize::new(0);
static DNS_FAIL:AtomicBool=AtomicBool::new(false);
static AUTO_REPLY:AtomicBool=AtomicBool::new(true);
static DNS:Mutex<Vec<Option<usize>>>=Mutex::new(Vec::new());
static SENT:Mutex<Vec<(usize,u16)>>=Mutex::new(Vec::new());
static REPLIES:Mutex<VecDeque<vnet::Event>>=Mutex::new(VecDeque::new());
static IRC:Mutex<Vec<String>>=Mutex::new(Vec::new());
static HOST:Mutex<Option<String>>=Mutex::new(None);
#[derive(Clone,Copy,PartialEq,PartialOrd)]pub struct Instant(u64);
impl Instant {pub fn now()->Self{Self(NOW.load(Ordering::SeqCst))}}
pub struct Duration(u64);impl Duration {pub fn from_millis(v:u64)->Self{Self(v)}pub fn from_secs(v:u64)->Self{Self(v*1000)}}
impl core::ops::Add<Duration> for Instant {type Output=Self;fn add(self,d:Duration)->Self{Self(self.0+d.0)}}
pub struct Timer;impl Timer {pub async fn after(d:Duration){NOW.fetch_add(d.0,Ordering::SeqCst);}}
fn run<T>(f:impl std::future::Future<Output=T>)->T {let mut f=std::pin::pin!(f);loop {if let std::task::Poll::Ready(v)=f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())){return v;}}}
#[derive(Clone,Copy)]pub struct Spawner;impl Spawner {pub fn spawn<T>(&self,_:T){SPAWNS.fetch_add(1,Ordering::SeqCst);}}
mod chronos {pub fn monotonic_nanos()->u64{crate::NOW.load(crate::Ordering::SeqCst)*1_000_000}}
mod workers {pub type WorkerSpawner=crate::Spawner;pub fn pick_background_spawner()->Option<WorkerSpawner>{Some(crate::Spawner)}}
mod wait {
    use super::*;thread_local!{static QUEUE:std::cell::RefCell<Vec<std::pin::Pin<Box<dyn std::future::Future<Output=()>>>>>=std::cell::RefCell::new(Vec::new());}
    pub fn spawn_local_detached(f:impl std::future::Future<Output=()>+'static){QUEUE.with(|q|q.borrow_mut().push(Box::pin(f)));}
    pub fn drain(){loop {let f=QUEUE.with(|q|q.borrow_mut().pop());if let Some(f)=f{crate::run(f);}else{break;}}}
}
pub mod vnet {
    pub struct ByteBuf;impl ByteBuf {pub fn from_slice_trunc(v:&[u8])->Self{assert_eq!(v.len(),56);Self}}
    pub enum Command {IcmpEcho {target:[u8;4],seq:u16,data:ByteBuf}}
    pub enum Event {IcmpReply {from:[u8;4],seq:u16,rtt_ms:u32},Other}
}
mod net {
    pub fn device_count()->usize{2}
    pub fn find_device_by_bdf(b:u8,s:u8,f:u8)->Option<usize>{(b==0&&s==2&&f==0).then_some(1)}
    pub fn find_device_by_vidpid(v:u16,p:u16)->Option<usize>{(v==0x10ec&&p==0x8125).then_some(1)}
    pub fn bdf_at(i:usize)->Option<(u8,u8,u8)>{Some((0,i as u8+1,0))}
    pub fn device_id_at(_:usize)->Option<(u16,u16)>{Some((0x10ec,0x8125))}
    pub fn device_name_at(_:usize)->Option<&'static str>{Some("RTL8125")}
    pub fn mac_address_at(_:usize)->Option<[u8;6]>{Some([1,2,3,4,5,6])}
    pub mod adapter {
        pub fn get_hostname()->String{crate::HOST.lock().clone().unwrap_or("TRUEOS".into())}
        pub fn set_hostname(v:&str){*crate::HOST.lock()=Some(v.into());}
        pub fn ipv4_at(_:usize)->Option<[u8;4]>{Some([192,168,1,10])}
        pub fn dhcp_has_lease_at(_:usize)->Option<bool>{Some(true)}
        pub fn ipv6_global_at(_:usize)->Option<[u8;16]>{None}
        pub fn ipv6_link_local_at(_:usize)->Option<[u8;16]>{None}
    }
    pub mod r8125 {
        pub struct Snapshot {pub bus:u8,pub slot:u8,pub function:u8,pub family:&'static str,pub xid:u32,pub revision:u8,pub subsystem_vendor:u16,pub subsystem_device:u16,pub firmware_hint:&'static str,pub initial_tcr:u32,pub rcr:u32,pub multicast_hash:u64,pub cplus:u16,pub mcu_before:u8,pub mcu_after:u8,pub config3:u8,pub config5:u8}
        impl Snapshot {pub fn accepts_own_mac(&self)->bool{true}pub fn accepts_broadcast(&self)->bool{true}pub fn accepts_multicast(&self)->bool{false}pub fn promiscuous(&self)->bool{false}}
        pub fn snapshots()->Vec<Snapshot>{Vec::new()}
    }
}
mod r {pub mod net {
    pub struct NetProfile;impl NetProfile {pub fn default()->Self{Self}}
    pub struct VNet(usize);impl VNet {
        pub fn open(i:usize)->Option<Self>{(i<2).then_some(Self(i))}pub fn open_primary()->Option<Self>{Self::open(0)}
        pub fn submit(&self,c:crate::vnet::Command)->Result<(),()> {let crate::vnet::Command::IcmpEcho {target,seq,..}=c;
            crate::SENT.lock().push((self.0,seq));if crate::AUTO_REPLY.load(crate::Ordering::SeqCst){crate::REPLIES.lock().push_back(crate::vnet::Event::IcmpReply {from:target,seq,rtt_ms:5});}Ok(())}
        pub fn pop_event(&self)->Option<crate::vnet::Event>{crate::REPLIES.lock().pop_front()}
    }
    pub mod dns {
        pub struct DnsConfig;impl DnsConfig {pub fn default()->Self{Self}pub fn for_device(_:usize)->Self{Self}}
        pub async fn resolve_ipv4_primary(_:&str,_:DnsConfig)->Result<[u8;4],&'static str>{resolve(None)}
        pub async fn resolve_ipv4_for_device(i:usize,_:&str,_:DnsConfig)->Result<[u8;4],&'static str>{resolve(Some(i))}
        fn resolve(i:Option<usize>)->Result<[u8;4],&'static str>{crate::DNS.lock().push(i);if crate::DNS_FAIL.load(crate::Ordering::SeqCst){Err("DNS failed")}else{Ok([1,2,3,4])}}
    }
    pub mod cli {pub mod irc {
        pub const IRC_DEFAULT_PORT:u16=6667;pub struct IrcSession;
        impl IrcSession {
            pub async fn connect(h:&str,_:u16,_:super::super::NetProfile,_:u64)->Result<Self,()> {crate::IRC.lock().push(format!("connect {h}"));Ok(Self)}
            pub async fn register(&mut self,_:&str,_:&str,_:&str,_:u64)->Result<(),()> {crate::IRC.lock().push("register".into());Ok(())}
            pub fn join(&mut self,c:&str)->Result<(),()> {crate::IRC.lock().push(format!("join {c}"));Ok(())}
            pub async fn recv_line(&mut self,_:u64)->Result<Option<String>,()> {crate::NOW.fetch_add(1000,crate::Ordering::SeqCst);Ok(Some("server line".into()))}
            pub async fn quit(&mut self,_:&str,_:u64)->Result<(),()> {crate::IRC.lock().push("quit".into());Ok(())}
        }
    }}
}}
mod shell3 {
    #[derive(Clone)]pub struct MatrixSlotLease;
    #[derive(Clone)]pub struct MatrixTarget;
    impl MatrixSlotLease {pub fn from_identity(_:String,_:u64)->Self{Self}}
    impl MatrixTarget {pub fn from_lease(_:MatrixSlotLease)->Self{Self}}
    pub fn matrix_target_slot_lease(_: &MatrixTarget)->MatrixSlotLease{MatrixSlotLease}
    pub mod matrix_target {pub fn matrix_slot_is_live(_: &super::MatrixSlotLease)->bool{crate::LIVE.load(crate::Ordering::SeqCst)}}
    struct Slots {ids:Vec<String>}static SLOTS:spin::Mutex<Slots>=spin::Mutex::new(Slots{ids:Vec::new()});
    fn matrix_slots()->&'static spin::Mutex<Slots>{&SLOTS}
    struct MatrixSlots;impl MatrixSlots {fn ensure_named(n:&str)->u64{SLOTS.lock().ids.push(n.into());1}fn drop_slot(n:Option<&str>){SLOTS.lock().ids.retain(|v|Some(v.as_str())!=n);}}
    pub mod tui {
        #[derive(Clone,Copy)]pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}
        pub struct Surface {pub cols:u32,pub rows:u32}
        pub fn native_slot(_:&str)->bool{crate::SLOT.load(crate::Ordering::SeqCst)}
        pub fn request(_:Frontend,_:&str)->Result<(),&'static str>{crate::REQUESTS.fetch_add(1,crate::Ordering::SeqCst);Ok(())}
        pub fn attach_native(_:Frontend,_:&super::MatrixTarget)->Result<(),String>{crate::SLOT.store(true,crate::Ordering::SeqCst);Ok(())}
        pub fn cancel_native_attach(_:&super::MatrixTarget){}
        pub fn native_visible(_:&super::MatrixTarget)->bool{crate::VISIBLE.load(crate::Ordering::SeqCst)}
        pub fn native_return(_:&super::MatrixTarget){crate::VISIBLE.store(false,crate::Ordering::SeqCst);}
        pub fn native_read(_:&super::MatrixTarget)->Option<(Vec<u8>,Vec<String>)>{None}
        pub fn native_write(_:&super::MatrixTarget,_:&[u8]){}
        pub fn surface(_:&super::MatrixTarget)->Option<Surface>{Some(Surface{cols:100,rows:25})}
    }
    #[path="@ROOT@/src/shell3/helper.rs"]mod helper;
    mod table {
        use alloc::{string::String,vec::Vec};use unicode_width::UnicodeWidthStr;
        const CELL_SEPARATOR:&str=" | ";const FRAME_SIDE_WIDTH:usize=4;const MIN_COL_WIDTH:usize=3;
        @TABLE@
    }
    mod system_net {
        @NET@
        fn menu_task(_:super::MatrixTarget)->Result<(),()>{Ok(())}
        #[cfg(test)]mod tests {
            use super::*;use crate::*;
            fn reset(){wait::drain();NOW.store(0,Ordering::SeqCst);LIVE.store(true,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);SLOT.store(false,Ordering::SeqCst);SPAWNS.store(0,Ordering::SeqCst);REQUESTS.store(0,Ordering::SeqCst);DNS_FAIL.store(false,Ordering::SeqCst);AUTO_REPLY.store(true,Ordering::SeqCst);DNS.lock().clear();SENT.lock().clear();REPLIES.lock().clear();IRC.lock().clear();*HOST.lock()=None;super::super::SLOTS.lock().ids.clear();}
            #[test]fn nic_selectors_and_snapshot_preserve_existing_table(){reset();assert_eq!(parse_device_selector("1"),Some(1));assert_eq!(parse_device_selector("10ec:8125"),Some(1));assert_eq!(parse_device_selector("00:02.0"),Some(1));assert_eq!(parse_device_selector("bad"),None);let rows=nic_snapshot(180,Some("1"));assert!(rows.iter().any(|s|s.contains("RTL8125")&&s.contains("dhcp")));assert_eq!(rows.iter().filter(|s|s.contains("RTL8125")).count(),1);}
            #[test]fn ping_uses_selected_nic_and_matches_replies(){reset();let job=Job::new();run(ping("example.test".into(),Some(1),job.clone(),shell3::MatrixSlotLease));assert_eq!(*DNS.lock(),vec![Some(1)]);assert_eq!(*SENT.lock(),vec![(1,1),(1,2),(1,3),(1,4)]);assert_eq!(job.lines.lock().iter().filter(|s|s.contains("time=5ms")).count(),4);}
            #[test]fn ping_reports_dns_failure_and_bounded_timeouts(){reset();DNS_FAIL.store(true,Ordering::SeqCst);let job=Job::new();run(ping("bad.test".into(),None,job.clone(),shell3::MatrixSlotLease));assert!(SENT.lock().is_empty());assert!(job.lines.lock()[0].contains("resolve failed"));DNS_FAIL.store(false,Ordering::SeqCst);AUTO_REPLY.store(false,Ordering::SeqCst);let job=Job::new();run(ping("slow.test".into(),None,job.clone(),shell3::MatrixSlotLease));assert_eq!(job.lines.lock().iter().filter(|s|s.contains("timeout")).count(),4);assert!(NOW.load(Ordering::SeqCst)<15_000);}
            #[test]fn irc_registers_joins_receives_and_quits(){reset();let job=Job::new();run(irc("irc.test".into(),Some("#trueos".into()),job.clone(),shell3::MatrixSlotLease));assert_eq!(*IRC.lock(),vec!["connect irc.test","register","join #trueos","quit"]);assert!(job.lines.lock().iter().any(|s|s=="server line"));assert_eq!(job.lines.lock().last().unwrap(),"irc: done");}
            #[test]fn forms_validate_before_start_and_accept_literal_navigation_letters(){reset();let mut view=View::new();view.page=Page::Field(Field::Hostname);view.input(&shell3::MatrixTarget,b"hjq-host\r\n",100,25,0);assert_eq!(net::adapter::get_hostname(),"hjq-host");assert!(matches!(view.page,Page::Home));view.page=Page::Field(Field::Ping);view.value="host 99".into();view.submit(Field::Ping,&shell3::MatrixTarget,100);assert!(view.job.is_none());assert!(view.message.contains("No matching"));view.page=Page::Field(Field::Irc);view.value="irc.test bad".into();view.submit(Field::Irc,&shell3::MatrixTarget,100);assert!(view.job.is_none());view.page=Page::Field(Field::Hostname);view.value="bad name".into();view.submit(Field::Hostname,&shell3::MatrixTarget,100);assert_eq!(net::adapter::get_hostname(),"hjq-host");}
            #[test]fn home_help_small_geometry_and_mouse_share_layout(){reset();let mut view=View::new();for rows in [5,8,10,25]{view.selected=4;let frame=view.frame(100,rows,0);assert!(frame[1].contains("Enter"));assert!(frame.iter().any(|s|s.contains("› Hostname")));let (first,visible,offset)=view.layout(rows);let row=first+4-offset;assert!(row<first+visible);let bytes=format!("\x1b[<0;1;{}M",row+1);view.input(&shell3::MatrixTarget,bytes.as_bytes(),100,rows,0);assert!(matches!(view.page,Page::Field(Field::Hostname)));view.home();}let frame=view.frame(100,25,0);assert!(frame[10..].iter().all(String::is_empty));view.input(&shell3::MatrixTarget,b"q",100,25,0);assert!(!VISIBLE.load(Ordering::SeqCst));}
            #[test]fn output_is_bounded_and_cancellation_prevents_requests(){reset();let job=Job::new();for n in 0..300{job.line(&format!("line{n}\n"));}assert_eq!(job.lines.lock().len(),MAX_OUTPUT_LINES);assert_eq!(job.lines.lock()[0],"line44 ");let mut view=View::new();view.page=Page::Field(Field::Ping);view.value="example.test".into();view.submit(Field::Ping,&shell3::MatrixTarget,100);let job=view.job.clone().unwrap();view.home();assert!(job.cancelled.load(Ordering::Acquire));wait::drain();assert!(DNS.lock().is_empty());assert!(!job.running.load(Ordering::Acquire));}
            #[test]fn dropped_lease_prevents_network_job_and_parked_slot_reenters_once(){reset();let f=shell3::tui::Frontend{id:1,cols:100,rows:25};start(f).unwrap();start(f).unwrap();assert_eq!(SPAWNS.load(Ordering::SeqCst),1);assert_eq!(REQUESTS.load(Ordering::SeqCst),1);let mut view=View::new();view.value="example.test".into();view.submit(Field::Ping,&shell3::MatrixTarget,100);LIVE.store(false,Ordering::SeqCst);wait::drain();assert!(DNS.lock().is_empty());}
        }
    }
}
'''

def main():
    net=(ROOT/'src/shell3/system/net.rs').read_text()
    net=re.sub(r'^//!.*\n|^#\[trueos_executor::task[^\n]*\n','',net,flags=re.M).replace('async fn menu_task(', 'async fn menu_task_run(')
    table=(ROOT/'src/shell3/table.rs').read_text()
    table=table[table.index("pub(crate) struct TlbTable"):table.index('pub(crate) fn parse_disc_id_raw')]+table[table.index('fn push_cell'):]
    with tempfile.TemporaryDirectory(prefix='shell3-system-net-') as directory:
        path=Path(directory);(path/'src').mkdir()
        (path/'src/lib.rs').write_text(HARNESS.replace('@ROOT@',str(ROOT)).replace('@NET@',net).replace('@TABLE@',table))
        (path/'Cargo.toml').write_text('''[package]
name="shell3-system-net-host"
version="0.1.0"
edition="2024"
[dependencies]
spin="0.10"
unicode-width="0.2.2"
''')
        env=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'tgt/system-net-host-tests'))
        subprocess.run(['cargo','test','--offline','--quiet','--manifest-path',str(path/'Cargo.toml'),'--','--test-threads=1'],env=env,cwd=path,check=True)

if __name__=='__main__':main()
