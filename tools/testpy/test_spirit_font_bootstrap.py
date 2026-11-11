#!/usr/bin/env python3
"""Run the production TGL startup gate and first-producer release on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item
from test_mouse_visual_cursors import block

ROOT = Path(__file__).resolve().parents[2]
FONT = "src/r/services/shell3_font_service.rs"


def harness():
    code = r'''
#![allow(dead_code,unused_variables)]
extern crate alloc;
use std::sync::atomic::{AtomicBool,AtomicU8,AtomicU64,Ordering};
use std::sync::Mutex;
static TGL:AtomicBool=AtomicBool::new(true);
static READY:AtomicBool=AtomicBool::new(true);
static QUARANTINED:AtomicBool=AtomicBool::new(false);
static NOW:AtomicU64=AtomicU64::new(0);
static EVENTS:Mutex<Vec<&'static str>>=Mutex::new(Vec::new());
thread_local! {static TIMER_HOOK:std::cell::Cell<Option<fn()>>=const {std::cell::Cell::new(None)};}
#[macro_export] macro_rules! log_important {($($token:tt)*)=>{{}};}
mod pci {pub fn experimental_tgl_9a49_active()->bool {crate::TGL.load(crate::Ordering::Acquire)}}
mod intel {
 pub fn guc_submission_ready()->bool {crate::READY.load(crate::Ordering::Acquire)}
 pub mod render {pub fn warm_state()->Option<()> {super::guc_submission_ready().then_some(())}}
 pub mod gpgpu {
  pub fn execution_rcs_context_is_quarantined()->bool {crate::QUARANTINED.load(crate::Ordering::Acquire)}
  #[derive(Clone,Copy)]pub struct GpgpuRgba8ReleaseFence {pub phys:u64,pub bytes:usize,pub sequence:u64}
  impl GpgpuRgba8ReleaseFence {
   pub fn matches(self,phys:u64,bytes:usize)->bool {self.phys==phys&&self.bytes==bytes}
   pub fn sequence(self)->u64 {self.sequence}
  }
 }
}
mod trueos_time {
 #[derive(Clone,Copy)]pub struct Instant(u64);
 pub struct Duration(u64);
 impl Instant {
  pub fn now()->Self {Self(crate::NOW.load(crate::Ordering::Acquire))}
  pub fn saturating_duration_since(self,other:Self)->Duration {Duration(self.0.saturating_sub(other.0))}
 }
 impl Duration {pub fn from_millis(ms:u64)->Self {Self(ms)} pub fn as_millis(self)->u64 {self.0}}
 pub struct Timer {duration:Duration,pending:bool}
 impl Timer {pub fn after(duration:Duration)->Self {Self{duration,pending:true}}}
 impl std::future::Future for Timer {
  type Output=();
  fn poll(mut self:std::pin::Pin<&mut Self>,cx:&mut std::task::Context<'_>)->std::task::Poll<()> {
   if self.pending {
    self.pending=false;crate::NOW.fetch_add(self.duration.0,crate::Ordering::AcqRel);
    if let Some(hook)=crate::TIMER_HOOK.with(std::cell::Cell::get) {hook();}
    cx.waker().wake_by_ref();std::task::Poll::Pending
   } else {std::task::Poll::Ready(())}
  }
 }
}
mod spin {
 pub struct Once<T>(std::sync::OnceLock<T>);
 impl<T> Once<T> {
  pub const fn new()->Self {Self(std::sync::OnceLock::new())}
  pub fn get(&self)->Option<&T> {self.0.get()}
  pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}
 }
 pub struct Mutex<T>(std::sync::Mutex<T>);
 impl<T> Mutex<T> {
  pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
  pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
 }
}
mod r {
 pub mod blocking {
  pub fn try_spawn_blocking_job_with_purpose<F:FnOnce()>(job:F,_:&str)->Result<(),F> {
   crate::EVENTS.lock().unwrap().push("blocking-job");job();Ok(())
  }
 }
 pub mod services {pub(crate) use crate::font as shell3_font_service;}
}
pub(crate) mod font {
 use alloc::boxed::Box;
 use crate::{AtomicBool,AtomicU8,Ordering};
 use crate::trueos_time::{Duration,Instant,Timer};
 static FONT:crate::spin::Once<()>=crate::spin::Once::new();
 static PREPARING:AtomicBool=AtomicBool::new(false);
 struct Reply(std::sync::Mutex<Option<Result<(),&'static str>>>);
 impl Reply {
  fn new()->Self {Self(std::sync::Mutex::new(None))}
  fn signal(&self,value:Result<(),&'static str>) {*self.0.lock().unwrap()=Some(value);}
  async fn wait(&self)->Result<(),&'static str> {self.0.lock().unwrap().take().unwrap()}
 }
 fn prepare_once()->Result<(),&'static str> {
  crate::EVENTS.lock().unwrap().push("atlas");FONT.call_once(||());Ok(())
 }
'''
    for name in ("SPIRIT_BOOT_WAITING", "SPIRIT_BOOT_CLAIMED", "SPIRIT_BOOT_RETIRED",
                 "SPIRIT_BOOT_SKIPPED", "SPIRIT_BOOT_FAILED", "SPIRIT_BOOT_WAIT_MS"):
        code += constant(FONT, name) + "\n"
    code += re.search(r"^static SPIRIT_BOOT:.*;$", (ROOT / FONT).read_text(), re.MULTILINE).group() + "\n"
    for name in ("laptop_context_bootstrap_enabled", "claim_first_spirit_producer",
                 "cancel_first_spirit_claim", "first_spirit_producer_retired",
                 "spirit_font_first_required", "skip_unclaimed_spirit_bootstrap"):
        code += item(FONT, name) + "\n"
    font_source = (ROOT / FONT).read_text()
    code += block(font_source, "async fn await_spirit_bootstrap_before_font") + "\n"
    code += block(font_source, "pub(crate) async fn prepare") + "\n"
    code += r'''
 pub fn state()->u8 {SPIRIT_BOOT.load(Ordering::Acquire)}
 pub fn skip()->bool {skip_unclaimed_spirit_bootstrap()}
}
mod spirit {
 use crate::spin::Mutex;
 #[derive(Clone,Copy)]struct Id;
 impl Id {fn index(self)->usize {0}}
 #[derive(Clone,Copy)]struct Fence {id:Id}
 #[derive(Clone,Copy)]struct Surface {phys:u64,byte_len:usize,surface:u8}
 #[derive(Clone,Copy)]struct SpiritFrameLease {fence:Fence,surface:Surface}
 #[derive(Clone,Copy)]struct SpiritBarrierSet(u8);
 impl SpiritBarrierSet {const GPU:Self=Self(1);}
 impl core::ops::BitOr for SpiritBarrierSet {type Output=Self;fn bitor(self,other:Self)->Self {Self(self.0|other.0)}}
 #[derive(Debug,PartialEq)]enum SpiritSubmitError {InvalidGpuRelease,Busy}
 struct Pending {released:SpiritBarrierSet}
 struct Mailbox {last_gpgpu_release_sequence:[u64;1],pending:Pending}
 static MAILBOXES:[Mutex<Mailbox>;1]=[Mutex::new(Mailbox{
  last_gpgpu_release_sequence:[0],pending:Pending{released:SpiritBarrierSet(0)}})];
 fn matching_pending_mut(mailbox:&mut Mailbox,_:SpiritFrameLease)->Result<&mut Pending,SpiritSubmitError> {Ok(&mut mailbox.pending)}
 fn require_unreleased(pending:&Pending,barrier:SpiritBarrierSet)->Result<(),SpiritSubmitError> {
  if pending.released.0&barrier.0==0 {Ok(())}else{Err(SpiritSubmitError::Busy)}
 }
'''
    code += item("src/spirit/mod.rs", "release_gpgpu")
    code += r'''
 fn release(phys:u64,bytes:usize,sequence:u64)->Result<bool,SpiritSubmitError> {
  release_gpgpu(SpiritFrameLease{fence:Fence{id:Id},surface:Surface{phys:0x12000,byte_len:4096,surface:0}},
   crate::intel::gpgpu::GpgpuRgba8ReleaseFence{phys,bytes,sequence})
 }
 #[test]fn first_retired_requires_the_actual_matching_release_and_fresh_sequence() {
  assert_eq!(crate::font::claim_first_spirit_producer(),Some(true));
  assert_eq!(release(0x13000,4096,1),Err(SpiritSubmitError::InvalidGpuRelease));
  assert_eq!(release(0x12000,2048,1),Err(SpiritSubmitError::InvalidGpuRelease));
  assert_eq!(release(0x12000,4096,0),Err(SpiritSubmitError::InvalidGpuRelease));
  assert_eq!(crate::font::state(),1,"invalid release must never open the font gate");
  assert_eq!(release(0x12000,4096,1),Ok(true));assert_eq!(crate::font::state(),2);
  assert_eq!(release(0x12000,4096,1),Err(SpiritSubmitError::InvalidGpuRelease));
  assert_eq!(release(0x12000,4096,2),Err(SpiritSubmitError::Busy));
  assert_eq!(crate::font::state(),2);
 }
}
fn run(future:impl std::future::Future<Output=()> ) {
 struct Wake;impl std::task::Wake for Wake {fn wake(self:std::sync::Arc<Self>) {}}
 let waker=std::task::Waker::from(std::sync::Arc::new(Wake));
 let mut cx=std::task::Context::from_waker(&waker);let mut future=std::pin::pin!(future);
 for _ in 0..10000 {if std::future::Future::poll(future.as_mut(),&mut cx).is_ready(){return}}
 panic!("startup gate did not make progress");
}
#[test]fn healthy_order_has_no_fixed_delay_and_blocks_other_producers_until_atlas() {
 assert_eq!(font::claim_first_spirit_producer(),Some(true));assert!(!font::skip());
 TIMER_HOOK.with(|hook|hook.set(Some(|| {
  if NOW.load(Ordering::Acquire)==10 {
   assert!(EVENTS.lock().unwrap().is_empty(),"font work cannot start while first producer is owned");
   assert!(font::first_spirit_producer_retired());
   EVENTS.lock().unwrap().push("first-retired");
   assert_eq!(font::claim_first_spirit_producer(),None,"other producers wait for atlas publication");
  }
 })));
 run(async {font::prepare().await.unwrap();});
 assert_eq!(*EVENTS.lock().unwrap(),["first-retired","blocking-job","atlas"]);
 assert_eq!(NOW.load(Ordering::Acquire),10);
 assert_eq!(font::claim_first_spirit_producer(),Some(false));
}
#[test]fn ready_window_starts_after_gpu_readiness_and_atomically_skips_missing_spirit() {
 READY.store(false,Ordering::Release);
 TIMER_HOOK.with(|hook|hook.set(Some(|| {if NOW.load(Ordering::Acquire)==3000 {READY.store(true,Ordering::Release);}})));
 run(async {font::prepare().await.unwrap();});
 assert_eq!(NOW.load(Ordering::Acquire),5000);
 assert_eq!(font::state(),3);assert!(font::spirit_font_first_required());
 assert_eq!(font::claim_first_spirit_producer(),Some(false));assert!(!font::first_spirit_producer_retired());
}
#[test]fn claimed_gpu_job_cannot_timeout_open_but_quarantine_opens_failed_without_retirement() {
 assert_eq!(font::claim_first_spirit_producer(),Some(true));
 TIMER_HOOK.with(|hook|hook.set(Some(|| {
  let now=NOW.load(Ordering::Acquire);
  if now<=5000 {assert!(EVENTS.lock().unwrap().is_empty());assert_eq!(font::state(),1);assert!(!font::skip());}
  if now==5000 {QUARANTINED.store(true,Ordering::Release);}
 })));
 run(async {font::prepare().await.unwrap();});
 assert_eq!(font::state(),4);assert!(!font::first_spirit_producer_retired());
 assert_eq!(NOW.load(Ordering::Acquire),5000);
}
#[test]fn claim_skip_race_and_rejected_admission_preserve_exclusive_ordering() {
 assert_eq!(font::claim_first_spirit_producer(),Some(true));assert!(!font::skip());
 font::cancel_first_spirit_claim(false);assert_eq!(font::state(),1);
 font::cancel_first_spirit_claim(true);assert_eq!(font::state(),0);
 assert!(font::skip());assert_eq!(font::claim_first_spirit_producer(),None);
 font::cancel_first_spirit_claim(true);assert_eq!(font::state(),3);
 assert!(!font::first_spirit_producer_retired());
 run(async {font::prepare().await.unwrap();});assert_eq!(font::claim_first_spirit_producer(),Some(false));
}
#[test]fn desktop_has_no_wait_claim_or_startup_state_changes() {
 TGL.store(false,Ordering::Release);READY.store(false,Ordering::Release);
 assert_eq!(font::claim_first_spirit_producer(),Some(false));
 assert!(!font::first_spirit_producer_retired());assert!(!font::spirit_font_first_required());
 run(async {font::prepare().await.unwrap();});assert_eq!(NOW.load(Ordering::Acquire),0);
 assert_eq!(font::state(),0);assert_eq!(*EVENTS.lock().unwrap(),["blocking-job","atlas"]);
}
'''
    return code


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-font-bootstrap-") as directory:
        source, binary = Path(directory) / "tests.rs", Path(directory) / "tests"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        names = [line.removesuffix(": test") for line in
                 subprocess.check_output([str(binary), "--list"], text=True).splitlines()
                 if line.endswith(": test")]
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
