#!/usr/bin/env python3
"""Check clean Hull exit against production cancellation and lifetime policy.

VM storage, control notifications and parked-worker polling are modeled. The
exit helper, force_stop, control-boundary retirement, cooperative-stop atomics
and native lifetime state transitions are extracted unchanged from production.
"""
from pathlib import Path
import subprocess
import tempfile
from test_thread_control_abi import function

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
use std::{sync::{Mutex as StdMutex,atomic::{AtomicBool,AtomicUsize,AtomicU64,Ordering}}, task::Poll};
use hv::control_kick;
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T>{const fn new(value:T)->Self{Self(StdMutex::new(value))}fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
#[path="@STOP_SOURCE@"]mod cooperative_stop;
@STATE@
static LIFETIME:Mutex<State>=Mutex::new(State::new());
static WAKES:AtomicUsize=AtomicUsize::new(0);
static QUEUED_DISCARDS:AtomicUsize=AtomicUsize::new(0);
static WAIT_DROPS:AtomicUsize=AtomicUsize::new(0);
struct ExecutionPause;
impl ExecutionPause{fn resume(&self){}}
struct TrueosVmId{
 lifecycle_control:Mutex<()>,run_generation:AtomicU64,running:AtomicBool,clean_exit:AtomicBool,
 stop_req:AtomicBool,preserve_req:AtomicBool,pause_latched:AtomicBool,preserve_exit:AtomicBool,
 cooperative_stop:cooperative_stop::CooperativeStop,execution_pause:ExecutionPause,
}
static VM:TrueosVmId=TrueosVmId{
 lifecycle_control:Mutex::new(()),run_generation:AtomicU64::new(19),running:AtomicBool::new(true),clean_exit:AtomicBool::new(true),
 stop_req:AtomicBool::new(false),preserve_req:AtomicBool::new(false),pause_latched:AtomicBool::new(false),preserve_exit:AtomicBool::new(false),
 cooperative_stop:cooperative_stop::CooperativeStop::new(),execution_pause:ExecutionPause,
};
fn vm_slot(vm:u8)->Option<&'static TrueosVmId>{(vm==0).then_some(&VM)}
fn reset_prepare_pause(_: &TrueosVmId){}
fn clear_blueprint_lifecycle_capability(_:u8){}
fn hvimportantf(_:std::fmt::Arguments<'_>){}
fn nudge_vm_control(_:u8,_:hv::control_kick::LifecycleKickAction,_:&str){}
struct ControlWait;
impl ControlWait{fn notify_all(&self){WAKES.fetch_add(1,Ordering::SeqCst);}}
static VM_CONTROL_WAITS:[ControlWait;1]=[ControlWait];
mod log_os{pub fn blueprint_important_line(_:std::fmt::Arguments<'_>){}}
mod hv{
 pub mod control_kick{pub enum LifecycleKickAction{Stop}}
 pub fn guest_kill_requested(_:u8)->bool{crate::VM.cooperative_stop.forced()}
 pub fn guest_native_pause_requested(_:u8)->bool{crate::VM.pause_latched.load(crate::Ordering::Acquire)}
}
mod r{
 pub mod blocking{
  pub fn guest_jobs_in_flight(_:u8)->usize{crate::LIFETIME.lock().in_flight}
  pub fn close_guest_jobs(_:u8)->usize{crate::LIFETIME.lock().close()}
  pub struct Discard{pub queued:usize,pub retained:usize}
  pub fn discard_guest_jobs(_:u8,generation:u64)->Discard{
   assert_eq!(generation,crate::LIFETIME.lock().generation);
   crate::QUEUED_DISCARDS.fetch_add(1,crate::Ordering::SeqCst);Discard{queued:0,retained:0}
  }
 }
 pub mod threads{pub fn wake_guest_control(_:u8){crate::WAKES.fetch_add(1,crate::Ordering::SeqCst);}}
 pub mod fs{pub mod request_broker{pub fn request_release_vm(_:u8)->usize{0}}}
}
@FORCE_STOP@
@EXIT_HELPER@
struct GuestJobOwner{generation:u64,retiring:bool,parked:bool}
impl GuestJobOwner{
 fn retire(&mut self){LIFETIME.lock().retire(self.generation,&mut self.retiring)}
 fn reclaimable(&self)->bool{LIFETIME.lock().reclaimable(self.generation)}
 fn park(&mut self)->bool{LIFETIME.lock().park(self.generation,&mut self.parked)}
 fn unpark(&mut self){LIFETIME.lock().unpark(self.generation,&mut self.parked)}
}
impl Drop for GuestJobOwner{fn drop(&mut self){LIFETIME.lock().release(self.generation,&mut self.parked,self.retiring)}}
struct Admission{_owner:Option<GuestJobOwner>}
struct Wait;
impl Drop for Wait{fn drop(&mut self){WAIT_DROPS.fetch_add(1,Ordering::SeqCst);}}
struct Thread{vm_id:Option<u8>,abandoned:bool,done:bool,job:Option<Box<dyn FnOnce()>>,wait:Option<Wait>,_admission:Admission}
mod diagnostics{pub const CANCELLING:usize=1;}
fn diagnostic_phase(_: &Thread,_:usize,_:usize,_:u64){}
@CONTROL_BOUNDARY@
fn setup(){
 *LIFETIME.lock()=State::new();assert!(LIFETIME.lock().open(19));
 VM.run_generation.store(19,Ordering::Release);VM.running.store(true,Ordering::Release);VM.clean_exit.store(true,Ordering::Release);
 for field in [&VM.stop_req,&VM.preserve_req,&VM.pause_latched,&VM.preserve_exit]{field.store(false,Ordering::Release)}
 VM.cooperative_stop.reset();WAKES.store(0,Ordering::SeqCst);QUEUED_DISCARDS.store(0,Ordering::SeqCst);WAIT_DROPS.store(0,Ordering::SeqCst);
}
fn idle_worker()->Thread{
 assert!(LIFETIME.lock().reserve(19));
 Thread{vm_id:Some(0),abandoned:false,done:false,job:Some(Box::new(||panic!("idle worker resumed after process exit"))),wait:Some(Wait),
  _admission:Admission{_owner:Some(GuestJobOwner{generation:19,retiring:false,parked:false})}}
}
#[test]fn clean_hull_exit_cancels_idle_day_long_worker_and_drains_after_hull_ack(){
 setup();let mut idle=idle_worker();
 // Closing admission alone was the old drain behavior. Its idle wait does not
 // observe an app-level channel change or naturally expire for another day.
 assert_eq!(r::blocking::close_guest_jobs(0),1);assert!(guest_control_boundary(&mut idle).is_none());
 assert!(idle.wait.is_some());assert_eq!(LIFETIME.lock().retiring,0);
 cancel_native_jobs_after_clean_hull_exit(0,19);
 assert!(VM.cooperative_stop.forced());assert!(WAKES.load(Ordering::Acquire)>=2);
 assert!(matches!(guest_control_boundary(&mut idle),Some(Poll::Pending)));
 assert!(idle.abandoned&&idle.done&&idle.wait.is_none());assert_eq!(WAIT_DROPS.load(Ordering::Acquire),1);
 assert_eq!(LIFETIME.lock().in_flight,1);assert!(!LIFETIME.lock().reclaimable(19));
 assert!(LIFETIME.lock().acknowledge_hull_stopped(19));
 assert!(matches!(guest_control_boundary(&mut idle),Some(Poll::Ready(()))));drop(idle);
 assert_eq!(LIFETIME.lock().in_flight,0);assert!(!LIFETIME.lock().accepting);
}
#[test]fn clean_hull_exit_retains_noncooperative_owner_until_its_own_boundary(){
 setup();let mut idle=idle_worker();let mut not_yet_at_boundary=idle_worker();
 cancel_native_jobs_after_clean_hull_exit(0,19);assert!(LIFETIME.lock().acknowledge_hull_stopped(19));
 assert!(matches!(guest_control_boundary(&mut idle),Some(Poll::Pending)));
 assert_eq!(LIFETIME.lock().in_flight,2);assert_eq!(LIFETIME.lock().retiring,1);
 assert!(!LIFETIME.lock().reclaimable(19));assert!(!not_yet_at_boundary.abandoned);
 // The second owner remains counted with its stack retained until it really
 // reaches control. The helper never edits an arbitrary running stack.
 assert!(matches!(guest_control_boundary(&mut not_yet_at_boundary),Some(Poll::Ready(()))));
 assert!(matches!(guest_control_boundary(&mut idle),Some(Poll::Ready(()))));drop(idle);drop(not_yet_at_boundary);
 assert_eq!(LIFETIME.lock().in_flight,0);
}
#[test]fn clean_hull_exit_does_not_cancel_retained_pause_or_preserve(){
 for mode in 0..3{
  setup();let worker=idle_worker();
  match mode{0=>VM.pause_latched.store(true,Ordering::Release),1=>VM.preserve_req.store(true,Ordering::Release),_=>VM.preserve_exit.store(true,Ordering::Release)}
  cancel_native_jobs_after_clean_hull_exit(0,19);
  assert!(!VM.cooperative_stop.forced());assert!(!VM.stop_req.load(Ordering::Acquire));
  assert_eq!(WAKES.load(Ordering::Acquire),0);assert!(LIFETIME.lock().accepting);assert_eq!(LIFETIME.lock().in_flight,1);
  assert!(worker.wait.is_some()&&!worker.abandoned);drop(worker);
 }
}
#[test]fn clean_hull_exit_rejects_stale_generation_inactive_or_nonterminal_hull(){
 for mode in 0..3{
  setup();let worker=idle_worker();
  match mode{0=>VM.run_generation.store(20,Ordering::Release),1=>VM.running.store(false,Ordering::Release),_=>VM.clean_exit.store(false,Ordering::Release)}
  cancel_native_jobs_after_clean_hull_exit(0,19);
  assert!(!VM.cooperative_stop.forced());assert_eq!(WAKES.load(Ordering::Acquire),0);assert!(LIFETIME.lock().accepting);
  assert_eq!(LIFETIME.lock().in_flight,1);drop(worker);
 }
}
#[test]fn clean_hull_exit_with_no_native_owners_does_not_force_stop(){
 setup();cancel_native_jobs_after_clean_hull_exit(0,19);
 assert!(!VM.cooperative_stop.forced());assert_eq!(WAKES.load(Ordering::Acquire),0);assert_eq!(QUEUED_DISCARDS.load(Ordering::Acquire),0);
}
'''


def main():
    hv = (ROOT / "src/hv/mod.rs").read_text()
    task = hv[hv.index("async fn vm_task("):hv.index("fn publish_vm_offline(")]
    # The live Hull must have returned before cancellation; final offline and
    # admission release remain behind the existing native/FS/carrier drain.
    assert task.index("let launched_generation =") < task.index("let launch_result =")
    assert task.index(".await;", task.index("let launch_result =")) < task.index("cancel_native_jobs_after_clean_hull_exit(")
    assert task.index("cancel_native_jobs_after_clean_hull_exit(") < task.index("acknowledge_guest_hull_stopped(") < task.index("drain_guest_jobs(")
    assert task.index("drain_guest_jobs(") < task.index("lane_lease.release_now();", task.index("drain_guest_jobs(")) < task.index("publish_vm_offline(", task.index("drain_guest_jobs("))
    lifetime = (ROOT / "src/r/blocking/lifetime.rs").read_text()
    state = lifetime[lifetime.index("#[derive(Default)]"):lifetime.index("static STATES:")]
    source = HARNESS.replace("@STATE@", state).replace("@STOP_SOURCE@", str(ROOT / "src/hv/cooperative_stop.rs"))
    source = source.replace("@FORCE_STOP@", function(hv, "force_stop"))
    source = source.replace("@EXIT_HELPER@", function(hv, "cancel_native_jobs_after_clean_hull_exit"))
    source = source.replace("@CONTROL_BOUNDARY@", function((ROOT / "src/r/threads.rs").read_text(), "guest_control_boundary"))
    with tempfile.TemporaryDirectory(prefix="trueos-native-clean-exit-") as temporary:
        directory = Path(temporary)
        fixture = directory / "clean_exit.rs"
        fixture.write_text(source)
        for level in ["0", "3"]:
            binary = directory / ("debug" if level == "0" else "optimized")
            subprocess.run(["rustc", "--edition=2024", "--test", "-C", f"opt-level={level}", str(fixture), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "clean_hull_exit_", "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
