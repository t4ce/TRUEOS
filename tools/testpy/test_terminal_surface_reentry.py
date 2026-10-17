#!/usr/bin/env python3
"""Exercise production lease transitions used by a Crossterm surface poll."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    path = ROOT / 'src/hv/mod.rs'
    source = path.read_text()
    definitions = ''
    for name in ('BlueprintConsoleSurface', 'BlueprintTerminalLeaseState',
                 'BlueprintTerminalLeaseError', 'BlueprintTerminalReentryPoll',
                 'BlueprintTerminalSurfaceSnapshot', 'BlueprintTerminalReentryRequest'):
        definitions += '#[derive(Clone,Copy,Debug,Eq,PartialEq)]\n'
        definitions += re.search(
            rf'^(?:pub\(crate\) )?(?:enum|struct) {name} \{{.*?^}}',
            source, re.M | re.S,
        ).group() + '\n'
    definitions += re.search(r'^impl BlueprintTerminalLeaseState \{.*?^}', source, re.M | re.S).group()
    for name in ('blueprint_terminal_lease_current', 'blueprint_terminal_lease_release',
                 'blueprint_console_request_tui', 'blueprint_terminal_lease_poll_reentry',
                 'blueprint_terminal_surface_snapshot', 'blueprint_terminal_reentry_requested'):
        definitions += '\n' + function(path, name)
    harness = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone)] struct MatrixTarget;
struct Context {
    console_attached:bool,
    console_target:Option<MatrixTarget>,
    terminal_lease:BlueprintTerminalLeaseState,
}
static BLUEPRINT_PROCESS_CONTEXTS:[Mutex<Option<Context>>;1]=[Mutex::new(None)];
static BLUEPRINT_TERMINAL_TRANSITIONS:[Mutex<()>;1]=[Mutex::new(())];
static CAN_CLAIM:AtomicBool=AtomicBool::new(true);
static OWNED:AtomicBool=AtomicBool::new(false);
static GENERATION:AtomicU64=AtomicU64::new(1);
static WAKES:AtomicU64=AtomicU64::new(0);
fn notify_blueprint_console_input(vm:u8) {assert_eq!(vm,0);WAKES.fetch_add(1,Ordering::Relaxed);}
mod log_os {pub fn blueprint_important_line(_:core::fmt::Arguments) {}}
mod shell3 {pub mod tui {
    use super::super::*;
    pub fn claim(_:&MatrixTarget,_:u8)->Option<bool> {
        if !CAN_CLAIM.load(Ordering::Relaxed) {return Some(false);}
        if !OWNED.swap(true,Ordering::Relaxed) {GENERATION.fetch_add(1,Ordering::Relaxed);}
        Some(true)
    }
    pub fn release(_:&MatrixTarget,_:u8)->Option<bool> {OWNED.store(false,Ordering::Relaxed);Some(true)}
    pub fn surface(_:&MatrixTarget)->Option<BlueprintTerminalSurfaceSnapshot> {
        assert!(OWNED.load(Ordering::Relaxed));
        Some(BlueprintTerminalSurfaceSnapshot {generation:GENERATION.load(Ordering::Relaxed),cols:120,rows:40})
    }
}}
fn blueprint_console_target(_:u8)->Option<MatrixTarget> {
    BLUEPRINT_PROCESS_CONTEXTS[0].lock().as_ref()?.console_target.clone()
}
fn setup() {
    *BLUEPRINT_PROCESS_CONTEXTS[0].lock()=Some(Context {
        console_attached:true,console_target:Some(MatrixTarget),terminal_lease:BlueprintTerminalLeaseState::Reserved,
    });
    CAN_CLAIM.store(true,Ordering::Relaxed);OWNED.store(false,Ordering::Relaxed);GENERATION.store(1,Ordering::Relaxed);WAKES.store(0,Ordering::Relaxed);
}
#[test] fn surface_poll_accepts_requested_reentry_but_never_steals_parked_terminal() {
    setup();let first=blueprint_terminal_surface_snapshot(0).unwrap();
    assert_eq!(blueprint_terminal_lease_release(0,1),Ok(1));
    for _ in 0..3 {
        assert_eq!(blueprint_terminal_surface_snapshot(0),Err(BlueprintTerminalLeaseError::NotActive));
        assert!(!OWNED.load(Ordering::Relaxed));
    }
    assert_eq!(blueprint_console_request_tui(0),BlueprintTerminalReentryRequest::Requested{ticket:1,epoch:2});
    assert_eq!(WAKES.load(Ordering::Relaxed),1);assert!(blueprint_terminal_reentry_requested(0));
    let resumed=blueprint_terminal_surface_snapshot(0).unwrap();
    assert!(resumed.generation>first.generation);assert_eq!((resumed.cols,resumed.rows),(120,40));
    assert_eq!(blueprint_terminal_lease_current(0,2),Ok(2));
    assert!(!blueprint_terminal_reentry_requested(0));
    assert_eq!(blueprint_terminal_surface_snapshot(0),Ok(resumed));
    assert_eq!(blueprint_terminal_lease_release(0,1),Err(BlueprintTerminalLeaseError::Stale));
    assert_eq!(blueprint_terminal_lease_release(0,2),Ok(2));
    assert_eq!(blueprint_console_request_tui(0),BlueprintTerminalReentryRequest::Requested{ticket:2,epoch:3});
    assert!(blueprint_terminal_surface_snapshot(0).unwrap().generation>resumed.generation);
}
#[test] fn busy_surface_remains_requested_until_selected_route_can_claim() {
    setup();blueprint_terminal_surface_snapshot(0).unwrap();blueprint_terminal_lease_release(0,1).unwrap();
    blueprint_console_request_tui(0);CAN_CLAIM.store(false,Ordering::Relaxed);
    assert_eq!(blueprint_terminal_surface_snapshot(0),Err(BlueprintTerminalLeaseError::Busy));
    assert!(!OWNED.load(Ordering::Relaxed));
    assert_eq!(blueprint_console_request_tui(0),BlueprintTerminalReentryRequest::AlreadyRequested);
    CAN_CLAIM.store(true,Ordering::Relaxed);assert!(blueprint_terminal_surface_snapshot(0).is_ok());
}
#[test] fn detached_surface_cannot_acknowledge_a_request() {
    setup();blueprint_terminal_surface_snapshot(0).unwrap();blueprint_terminal_lease_release(0,1).unwrap();
    blueprint_console_request_tui(0);
    BLUEPRINT_PROCESS_CONTEXTS[0].lock().as_mut().unwrap().console_attached=false;
    assert_eq!(blueprint_terminal_surface_snapshot(0),Err(BlueprintTerminalLeaseError::Detached));
    assert!(!OWNED.load(Ordering::Relaxed));
}
'''
    with tempfile.TemporaryDirectory(prefix='terminal-surface-reentry-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness + definitions)
        subprocess.run(['rustc', '--edition=2024', '--test', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
