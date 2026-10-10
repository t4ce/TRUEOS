#!/usr/bin/env python3
"""Exercise the real HV launch task with deterministic host VM/time services."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = (root / 'src/hv/launcher.rs').read_text()
source = source[source.index('use alloc::'):]
a = source.index('pub(crate) fn enqueue(')
b = source.index('#[trueos_executor::task', a)
source = source[:a] + source[source.index('async fn launch_task', b):]
harness = r'''
extern crate alloc;
use std::sync::{Mutex, Arc};
static TEST_LOCK: Mutex<()> = Mutex::new(());
static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static READY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
    }
}
mod trueos_time {
    pub use std::time::Duration;
    #[derive(PartialEq, PartialOrd)] pub struct Instant(u64);
    impl Instant { pub fn now() -> Self { Self(crate::CLOCK.load(std::sync::atomic::Ordering::SeqCst)) } }
    impl std::ops::Add<Duration> for Instant {
        type Output = Self;
        fn add(self, d: Duration) -> Self { Self(self.0 + d.as_millis() as u64) }
    }
    pub struct Timer;
    impl Timer { pub async fn after(d: Duration) { crate::CLOCK.fetch_add(d.as_millis() as u64, std::sync::atomic::Ordering::SeqCst); } }
}
mod trueos_executor {
    pub struct Spawner;
    impl Spawner { pub async unsafe fn for_current_executor() -> Self { Self } }
}
mod shell3 { #[derive(Clone)] pub struct MatrixTarget; }
mod r { pub mod readiness { pub fn mask() -> u32 { crate::READY.load(std::sync::atomic::Ordering::SeqCst) } } }
mod log_os { pub fn blueprint_important_line(_: std::fmt::Arguments) {} }
mod hv {
    use super::*;
    #[derive(Default)] pub struct BlueprintInstanceRequest;
    pub enum BlueprintConsoleSurface { Text }
    pub struct HvVmState { pub running: bool, pub starting: bool }
    pub static CALLS: Mutex<Vec<(String,Vec<u8>,Vec<String>,Option<String>,bool)>> = Mutex::new(Vec::new());
    pub static FREE: Mutex<Option<u8>> = Mutex::new(Some(3));
    pub fn first_free_vm_id() -> Option<u8> { *FREE.lock().unwrap() }
    pub fn vm_run_generation(_: u8) -> Option<u64> { Some(4) }
    pub fn vm_state(_: u8) -> HvVmState { HvVmState {running:false,starting:false} }
    pub fn start_blueprint_app_vm(_:u8,_:&trueos_executor::Spawner,archive:String,bytes:Vec<u8>,args:Vec<String>,script:Option<String>,_:BlueprintInstanceRequest,target:Option<shell3::MatrixTarget>,_:BlueprintConsoleSurface) -> Result<(), &'static str> {
        CALLS.lock().unwrap().push((archive,bytes,args,script,target.is_some())); Ok(())
    }
    pub mod launcher {
        use crate::{spin, trueos_time, trueos_executor};
'''
tests = r'''
    }
}
#[derive(Default)] struct Events { live: bool, starts: Mutex<Vec<u8>>, ends: Mutex<Vec<(Option<u8>,Option<String>)>> }
impl hv::launcher::LaunchObserver for Arc<Events> {
    fn is_live(&self) -> bool { self.live }
    fn started(&self, vm:u8) { self.starts.lock().unwrap().push(vm); }
    fn finished(&self, vm:Option<u8>, error:Option<&str>) { self.ends.lock().unwrap().push((vm,error.map(str::to_owned))); }
}
fn run(f: impl std::future::Future<Output=()>) {
    struct Wake; impl std::task::Wake for Wake { fn wake(self:Arc<Self>) {} }
    let waker = std::task::Waker::from(Arc::new(Wake));
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(Box::pin(f).as_mut().poll(&mut cx).is_ready());
}
fn reset() { CLOCK.store(0,std::sync::atomic::Ordering::SeqCst); READY.store(0,std::sync::atomic::Ordering::SeqCst); hv::CALLS.lock().unwrap().clear(); *hv::FREE.lock().unwrap() = Some(3); }
fn request() -> hv::launcher::LaunchRequest { hv::launcher::LaunchRequest::new("app.bp".into(),vec![1,2]) }
#[test] fn unattended_launch_preserves_args_and_script_without_terminal() {
    let _guard=TEST_LOCK.lock().unwrap(); reset(); let mut req=request();
    req.args=vec!["argument".into()]; req.script=Some("pull app\n".into());
    run(hv::launcher::launch_task(req,0,None));
    assert_eq!(*hv::CALLS.lock().unwrap(),vec![("app.bp".into(),vec![1,2],vec!["argument".into()],Some("pull app\n".into()),false)]);
}
#[test] fn expired_owner_cancels_before_admission() {
    let _guard=TEST_LOCK.lock().unwrap(); reset(); let events=Arc::new(Events::default());
    run(hv::launcher::launch_task(request(),0,Some(Box::new(events.clone()))));
    assert!(hv::CALLS.lock().unwrap().is_empty()); assert_eq!(*events.ends.lock().unwrap(),vec![(None,None)]);
}
#[test] fn readiness_timeout_reports_failure_without_starting_vm() {
    let _guard=TEST_LOCK.lock().unwrap(); reset(); let events=Arc::new(Events {live:true,..Default::default()});
    run(hv::launcher::launch_task(request(),1,Some(Box::new(events.clone()))));
    assert!(hv::CALLS.lock().unwrap().is_empty()); assert!(events.ends.lock().unwrap()[0].1.as_ref().unwrap().contains("readiness timeout"));
}
#[test] fn admission_failure_and_success_deliver_lifecycle_events() {
    let _guard=TEST_LOCK.lock().unwrap(); reset(); let events=Arc::new(Events {live:true,..Default::default()});
    *hv::FREE.lock().unwrap()=None;
    run(hv::launcher::launch_task(request(),0,Some(Box::new(events.clone()))));
    assert_eq!(events.ends.lock().unwrap()[0],(None,Some("no free VM".into())));
    *hv::FREE.lock().unwrap()=Some(3);
    run(hv::launcher::launch_task(request(),0,Some(Box::new(events.clone()))));
    assert_eq!(*events.starts.lock().unwrap(),vec![3]); assert_eq!(events.ends.lock().unwrap()[1],(Some(3),None));
}
'''
# Expose the actual private task only to this host harness.
source = source.replace('async fn launch_task', 'pub(crate) async fn launch_task')
with tempfile.TemporaryDirectory(prefix='hv-launcher-') as temp:
    path = Path(temp)
    (path / 'test.rs').write_text(harness + source + tests)
    subprocess.run(['rustc', '--edition=2024', '--test', str(path/'test.rs'), '-o', str(path/'test')], check=True)
    subprocess.run([str(path/'test')], check=True)
