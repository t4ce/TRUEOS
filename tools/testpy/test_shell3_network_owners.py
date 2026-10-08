#!/usr/bin/env python3
"""Exercise production AP terminal routing and lifecycle with host adapter queues."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
ROOT=Path(__file__).resolve().parents[2]
extract.ROOT=ROOT


def main():
    source='''#![allow(dead_code,unreachable_patterns)]
extern crate alloc;
use alloc::{boxed::Box,collections::VecDeque,vec::Vec};
use std::sync::atomic::{AtomicU32,Ordering};
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {pub const fn new(t:T)->Self {Self(std::sync::Mutex::new(t))} pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {pub const fn new()->Self {Self(std::sync::OnceLock::new())} pub fn get(&self)->Option<&T> {self.0.get()} pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}}
}
thread_local! {static CURRENT_SLOT:std::cell::Cell<u32>=const {std::cell::Cell::new(0)};}
static DROPPED_2:AtomicU32=AtomicU32::new(0);
static DROPPED_7:AtomicU32=AtomicU32::new(0);
struct Shell3 {slot:u32,peer_port:Option<u16>}
impl Shell3 {fn new_terminal_reserved(slot:u32,peer_port:Option<u16>)->Self {assert_eq!(CURRENT_SLOT.get(),slot);Self {slot,peer_port}}}
impl Drop for Shell3 {fn drop(&mut self) {assert_eq!(CURRENT_SLOT.get(),self.slot);if self.slot==2 {DROPPED_2.fetch_add(1,Ordering::Relaxed);} else {DROPPED_7.fetch_add(1,Ordering::Relaxed);}}}
mod tty {
use super::*;
pub struct Terminal {pub shell:Shell3,pub input_bytes:Vec<u8>,pub output:Vec<u8>,pub overflow:bool,pub closing:bool}
impl Terminal {pub fn reconcile_matrix_selection(&mut self) {} pub fn new(shell:Shell3)->Self {Self {shell,input_bytes:Vec::new(),output:Vec::new(),overflow:false,closing:false}} pub fn input(&mut self,data:&[u8]) {assert_eq!(CURRENT_SLOT.get(),self.shell.slot);self.input_bytes.extend_from_slice(data);self.output.extend_from_slice(data);}}
}
mod service {pub fn release_shell_on_executor(slot:u32){assert_eq!(crate::CURRENT_SLOT.get(),slot);if slot==2 {crate::DROPPED_2.fetch_add(1,crate::Ordering::Relaxed);}else {crate::DROPPED_7.fetch_add(1,crate::Ordering::Relaxed);}}}
#[macro_export] macro_rules! log_info {(target: $target:literal; $($args:tt)*)=>{let _=format!($($args)*);};}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct NetHandle(u32);
enum NetEvent {TcpData {handle:NetHandle,data:Vec<u8>},TcpSent {handle:NetHandle,len:usize},Closed {handle:NetHandle}}
enum NetCommand {SendTcp {handle:NetHandle,data:Vec<u8>},Close {handle:NetHandle},FinishTcp {handle:NetHandle}}
#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)] struct Instant(u64);
impl Instant {fn now()->Self {Self(0)}}
struct Duration(u64); impl Duration {fn from_millis(ms:u64)->Self {Self(ms)}}
impl core::ops::Add<Duration> for Instant {type Output=Self;fn add(self,d:Duration)->Self {Self(self.0+d.0)}}
'''
    source+=extract.item('src/net/adapter.rs','NetQueue')
    adapter=(ROOT/'src/net/adapter.rs').read_text()
    source+=re.search(r'^impl<T> NetQueue<T> \{.*?^}',adapter,re.M|re.S).group()
    source+=(ROOT/'tools/testpy/shell3_ssh_unavailable.rs').read_text()
    net=(ROOT/'src/shell3/net.rs').read_text()
    source+='mod net {use super::*;use super::tty::Terminal;\n'
    source+=net[net.index('const WRITE_TIMEOUT_MS'):net.index('#[trueos_executor::task]')]
    source+='''
#[test] fn sessions_are_created_executed_and_dropped_only_on_their_ap() {
    let commands=NetQueue::new_leaked("test-cmd",8); COMMANDS.call_once(||commands);
    let mut a=WorkerTerminals::new(2); let mut b=WorkerTerminals::new(7);
    a.events.push(WorkerEvent::Accepted(NetHandle(1),Some(49152))).ok().unwrap();
    a.events.push(WorkerEvent::Accepted(NetHandle(2),Some(65535))).ok().unwrap();
    b.events.push(WorkerEvent::Accepted(NetHandle(3),None)).ok().unwrap();
    a.events.push(WorkerEvent::Socket(NetEvent::TcpData {handle:NetHandle(2),data:b"second".to_vec()})).ok().unwrap();
    b.events.push(WorkerEvent::Socket(NetEvent::TcpData {handle:NetHandle(3),data:b"other AP".to_vec()})).ok().unwrap();
    assert!(a.has_events()); assert!(b.has_events());
    CURRENT_SLOT.set(2); a.poll();
    assert_eq!(a.connections.len(),2); assert!(b.connections.is_empty());
    assert!(a.connections[0].terminal.is_none());
    assert_eq!(a.connections[0].pending.as_ref().unwrap().peer_port,Some(49152));
    assert_eq!(a.connections[1].terminal.as_ref().unwrap().shell.peer_port,Some(65535));

    assert_eq!(a.connections[1].terminal.as_ref().unwrap().input_bytes,b"second");
    CURRENT_SLOT.set(7); b.poll();
    assert_eq!(b.connections[0].terminal.as_ref().unwrap().input_bytes,b"other AP");
    assert!(!a.has_events()); assert!(!b.has_events());
    let sent=commands.drain(8); assert_eq!(sent.len(),2);
    assert!(matches!(&sent[0],NetCommand::SendTcp {handle:NetHandle(2),data} if data==b"second"));
    assert!(matches!(&sent[1],NetCommand::SendTcp {handle:NetHandle(3),data} if data==b"other AP"));
    a.events.push(WorkerEvent::Socket(NetEvent::Closed {handle:NetHandle(2)})).ok().unwrap();
    CURRENT_SLOT.set(2); a.poll(); assert_eq!(a.connections.len(),1); assert_eq!(DROPPED_2.load(Ordering::Relaxed),1);
    a.events.push(WorkerEvent::Socket(NetEvent::Closed {handle:NetHandle(1)})).ok().unwrap();
    a.poll(); assert!(a.is_empty()); assert_eq!(DROPPED_2.load(Ordering::Relaxed),2);
    CURRENT_SLOT.set(7); b.events.push(WorkerEvent::Socket(NetEvent::Closed {handle:NetHandle(3)})).ok().unwrap();
    b.poll(); assert!(b.is_empty()); assert_eq!(DROPPED_7.load(Ordering::Relaxed),1);
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-network-owners-') as directory:
        path=Path(directory); (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)


if __name__=='__main__':
    main()
