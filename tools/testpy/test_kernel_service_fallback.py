#!/usr/bin/env python3
"""Exercise production worker admission for BSP, one AP, and hybrid topologies.

The executor transport is mocked; the worker registry, placement selectors,
Shell3 pool startup, terminal admission, and GUI admission run production code.
Each topology runs in a fresh process, matching the monotonic boot registry.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[2]

MOCKS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_executor;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Debug)] pub struct SpawnError;
pub struct SpawnToken<S>(std::marker::PhantomData<S>);
#[derive(Clone,Copy)] pub struct Spawner(u32);
#[derive(Clone,Copy)] pub struct SendSpawner(u32);
impl Spawner {pub fn make_send(self)->SendSpawner {SendSpawner(self.0)}}
static SPAWNS:Mutex<Vec<u32>>=Mutex::new(Vec::new());
impl SendSpawner {
    pub fn spawn<S>(&self,_:SpawnToken<S>) {SPAWNS.lock().push(self.0);}
    pub fn spawned_task_count(&self)->usize {0}
    pub fn ready_task_count(&self)->usize {0}
    pub fn current_trueos_executor_task_name(&self)->Option<&'static str> {None}
    pub fn last_trueos_executor_task_name(&self)->Option<&'static str> {None}
}
macro_rules! log {($($t:tt)*)=>{}}
macro_rules! log_warn {($($t:tt)*)=>{}}
pub(crate) use {log,log_warn};
static TOPOLOGY:AtomicUsize=AtomicUsize::new(0);
mod allcaps {
    pub mod hv {pub const VM_CPU_SLOT_LIMIT:usize=32;pub const CODEX_EXECUTOR_EXPERIMENT_ENABLED:bool=true;}
    pub mod wls {pub const CPU_TRACK_COUNT:usize=32;}
}
mod smp {pub fn cpu_count()->usize {crate::TOPOLOGY.load(crate::Ordering::SeqCst)}}
mod percpu {pub const CPU_SLOT_LIMIT:usize=32;pub fn total_slots()->usize {crate::smp::cpu_count()}}
mod r {pub mod blocking {pub fn start_service_lane_for_slot(_:u32)->bool {false}}}
mod remote_work_wake {pub fn wake_cpu_for_remote_work(_:u32)->bool {true}}
'''

TESTS = r'''
fn main() {
    let topology:usize=std::env::args().nth(1).unwrap().parse().unwrap();
    TOPOLOGY.store(topology,Ordering::SeqCst);
    workers::register_core_spawner(0,workers::CORE_KIND_PERF,Spawner(0));
    if topology>1 {
        assert!(workers::ui_service_spawner().is_none(),"do not place UI on BSP before AP1 registers");
        assert!(!shell3_pool_gate(),"wait for the boot topology");
    }
    for slot in 1..topology {
        let kind=if slot%2==0 {workers::CORE_KIND_PERF} else {workers::CORE_KIND_EFF};
        workers::register_core_spawner(slot as u32,kind,Spawner(slot as u32));
        if slot+1<topology {assert!(!shell3_pool_gate());}
    }
    assert!(shell3_pool_gate());
    let service_slot=if topology==1 {0} else {1};
    assert_eq!(workers::ui_service_spawner().unwrap().cpu_slot(),service_slot);
    let expected=((topology-1)/2).max(1);
    let count=shell3::service::start(|_,_|Ok(SpawnToken::<()>(std::marker::PhantomData))).unwrap();
    assert_eq!(count,expected);
    let spawned=SPAWNS.lock().clone();
    if topology<=2 {
        assert_eq!(spawned,vec![service_slot]);
        assert!(workers::pick_background_spawner().is_none());
        assert!(workers::try_claim_compute_worker(workers::ComputeWorkerPolicy::AnyBackground).is_none());
        assert_eq!(shell3::service::request_shell3(),Err(shell3::Shell3Error::NoExecutor),
            "a GUI font wait must not strand the shared SSH owner");
    } else {
        assert!(spawned.iter().all(|slot|*slot>=2));
        assert!(spawned.iter().all(|slot|!workers::is_last_ap_service_slot(*slot)));
        assert!(shell3::service::request_shell3().is_ok());
    }
    let terminal_slot=shell3::service::reserve_terminal_slot().unwrap();
    assert!(spawned.contains(&terminal_slot));
    // Pool startup is retry-safe and must not duplicate an existing owner.
    assert_eq!(shell3::service::start(|_,_|Ok(SpawnToken::<()>(std::marker::PhantomData))).unwrap(),count);
    assert_eq!(*SPAWNS.lock(),spawned);
    println!("topology={topology}: terminal owner={terminal_slot}, pool={spawned:?}");
}
'''


def main():
    service = (ROOT / 'src/shell3/service.rs').read_text()
    source = MOCKS
    workers = (ROOT / 'src/workers.rs').read_text().replace(
        'pub(crate) mod application_policy;',
        f'#[path="{ROOT}/src/workers/application_policy.rs"] pub(crate) mod application_policy;')
    source += '\nmod workers {\n' + workers + '\n}\n'
    source += item('src/r/services/spawn_service.rs', 'shell3_pool_gate')
    source += '\nmod shell3 {\n'
    source += item('src/shell3/shell3.rs', 'Shell3Error')
    source += '''
pub const MAX_SHELL3_INSTANCES:usize=256;
pub mod startup {pub struct Startup;}
pub mod service {
use alloc::{collections::VecDeque,vec::Vec};
use crate::{SpawnToken,SpawnError};
struct WindowSpawn;
struct Notify;
impl Notify {fn notify_all(&self){}}
static SHELL_WORK_AVAILABLE:Notify=Notify;
fn refresh_appdb_names(){}
'''
    source += item('src/shell3/service.rs', 'ShellOwnership')
    source += re.search(r'^impl ShellOwnership \{.*?^}', service, re.M | re.S).group()
    source += '\nstatic SHELL_OWNERSHIP:crate::Mutex<ShellOwnership>=crate::Mutex::new(ShellOwnership::new());\n'
    for name in ('worker_limit', 'start', 'request_shell3', 'request_shell3_with_startup',
                 'request_shell3_configured', 'reserve_terminal_slot',
                 'advance_round_robin', 'warn_instance_limit'):
        source += item('src/shell3/service.rs', name).replace('pub(super)', 'pub(crate)')
    source += '\n}}\n' + TESTS
    with tempfile.TemporaryDirectory(prefix='trueos-service-fallback-') as directory:
        work = Path(directory)
        (work / 'main.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '-Awarnings', str(work / 'main.rs'),
                        '-C', f'linker={ROOT / "tools/rust-host-ld"}',
                        '-o', str(work / 'test')], cwd=ROOT, check=True)
        for topology in (1, 2, 3, 14):
            subprocess.run([str(work / 'test'), str(topology)], check=True)


if __name__ == '__main__':
    main()
