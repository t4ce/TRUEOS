#!/usr/bin/env python3
"""Exercise the production numeric snapshot/aggregation without hardware.

The registry lock, generations and socket payloads are controlled. No production
network pump is called by the observer; all formatted fields must be numeric.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/mio_compat.rs').read_text()
    function = 'pub(crate) fn progress_lines(' + source.split('pub(crate) fn progress_lines(', 1)[1].split('\nconst MIO_ADDR_BYTES:', 1)[0]
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
#[path = "@PROGRESS@"] mod progress;
use std::collections::VecDeque;
use std::sync::Mutex;
struct Lock(Mutex<Option<Compat>>);
impl Lock { fn try_lock(&self)->Option<std::sync::MutexGuard<'_,Option<Compat>>> { self.0.try_lock().ok() } }
static MIO_COMPAT:Lock=Lock(Mutex::new(None));
static SERIAL:Mutex<()>=Mutex::new(());
#[derive(PartialEq)] enum MioSocketKind { TcpStream, Udp }
struct Socket {
    owner_vm:Option<u8>, progress_generation:u64, kind:MioSocketKind,
    rx_stream:VecDeque<u8>, peer_secret:String,
}
struct Compat { sockets:Vec<Socket>, progress:[progress::Counters;8] }
mod allocators { pub fn with_host_alloc_domain_strong<R>(f:impl FnOnce()->R)->R { f() } }
mod hv {
    pub fn vm_run_generation(vm:u8)->Option<u64> { Some(if vm==3 {3} else {2}) }
    pub fn guest_pause_requested(vm:u8)->bool { vm==2 }
}
fn socket(vm:u8, generation:u64, bytes:usize, udp:bool)->Socket {
    Socket { owner_vm:Some(vm), progress_generation:generation,
        kind:if udp {MioSocketKind::Udp} else {MioSocketKind::TcpStream},
        rx_stream:std::iter::repeat_n(b'x',bytes).collect(), peer_secret:"MUST_NEVER_LEAK".into() }
}
fn reset(sockets:Vec<Socket>)->Compat {
    let mut c=Compat {sockets,progress:[progress::Counters::new();8]};
    for i in 0..8 { c.progress[i].for_generation(2).unwrap().receive((i+1)*100,(i+1)*10); }
    c
}
fn fields(line:&str)->std::collections::HashMap<&str,u64> {
    assert!(line.starts_with("codex-progress-net "));
    assert!(!line.contains("MUST_NEVER_LEAK"));
    line.split_whitespace().skip(1).map(|field| {
        let (key,value)=field.split_once('=').unwrap();
        assert!(key.chars().all(|c|c.is_ascii_lowercase()||c=='_'));
        assert!(value.chars().all(|c|c.is_ascii_digit()));
        (key,value.parse().unwrap())
    }).collect()
}
#[test] fn aggregates_only_tcp_and_matching_owner_generation() {
    let _serial=SERIAL.lock().unwrap();
    *MIO_COMPAT.0.lock().unwrap()=Some(reset(vec![
        socket(0,2,20,false),socket(0,2,30,false),socket(0,1,40,false),
        socket(0,2,999,true),socket(1,2,70,false),socket(2,2,12,false)]));
    let lines=progress_lines(99);
    assert_eq!(lines.len(),4); // hard bound regardless of caller limit
    let zero=fields(&lines[0]);
    assert_eq!((zero["vm"],zero["gen"],zero["current_gen"]),(0,2,2));
    assert_eq!((zero["sockets"],zero["queued"],zero["rx_bytes"]),(2,50,100));
    assert_eq!(fields(&lines[1])["queued"],70);
    assert_eq!(fields(&lines[2])["paused"],1);
    // Retired VM3/gen2 has no socket, so it cannot use a row or inherit gen3.
    assert_eq!(fields(&lines[3])["vm"],4);
    assert!(progress_lines(0).is_empty());
    assert_eq!(progress_lines(1).len(),1);
}
#[test] fn closed_socket_totals_survive_and_stale_generation_stays_explicit() {
    let _serial=SERIAL.lock().unwrap();
    let mut c=reset(vec![socket(3,2,9,false)]);
    c.progress[0].for_generation(2).unwrap().read(40,false,false);
    *MIO_COMPAT.0.lock().unwrap()=Some(c);
    let lines=progress_lines(4);
    let zero=lines.iter().map(|line|fields(line)).find(|row|row["vm"]==0).unwrap();
    assert_eq!((zero["sockets"],zero["queued"],zero["read_bytes"]),(0,0,40));
    let stale=fields(&lines[0]);
    assert_eq!((stale["vm"],stale["gen"],stale["current_gen"]),(3,2,3));
}
#[test] fn active_high_vm_survives_crowd_of_closed_low_vm_history() {
    let _serial=SERIAL.lock().unwrap();
    *MIO_COMPAT.0.lock().unwrap()=Some(reset(vec![socket(7,2,99,false)]));
    let lines=progress_lines(4);
    assert_eq!(lines.len(),4);
    let active=fields(&lines[0]);
    assert_eq!((active["vm"],active["sockets"],active["queued"]),(7,1,99));
    assert_eq!(fields(&progress_lines(1)[0])["vm"],7);
}
#[test] fn observer_never_waits_for_registry_and_never_initializes_it() {
    let _serial=SERIAL.lock().unwrap();
    let mut guard=MIO_COMPAT.0.lock().unwrap();
    *guard=None;
    assert_eq!(progress_lines(4),vec!["codex-progress-net registry=busy"]);
    drop(guard);
    assert!(progress_lines(4).is_empty());
    assert!(MIO_COMPAT.0.lock().unwrap().is_none());
}
'''
    harness = harness.replace('@PROGRESS@', str(ROOT / 'src/mio_compat/progress.rs'))
    with tempfile.TemporaryDirectory(prefix='trueos-mio-progress-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness + function)
        subprocess.run(['rustc', '--edition=2024', '--test', '--target', 'x86_64-unknown-linux-gnu', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
