#!/usr/bin/env python3
"""Exercise production single-target capture ownership, drain and age metadata."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use core::{future::Future, sync::atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering},task::{Context,Poll,Waker}};
use spin::Mutex;
static NOW_NS:AtomicU64=AtomicU64::new(0);
static DRIVER:AtomicUsize=AtomicUsize::new(0); // 0 idle,1 manual shot,2 stream
static STOPS:AtomicUsize=AtomicUsize::new(0);
static RELEASES:AtomicUsize=AtomicUsize::new(0);
static STOP_OK:AtomicBool=AtomicBool::new(true);
static TIMER_EXPIRED:AtomicBool=AtomicBool::new(false);
pub const TICK_HZ:u64=1000;
#[path="@TIMING@"] mod capture_timing;
mod allcaps {pub mod media_encode {pub const REALTIME_HZ:usize=33;}}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
struct Instant(u64);
impl Instant {
    fn now()->Self{Self(NOW_NS.load(Ordering::SeqCst)/1_000_000)}
    fn from_ticks(ticks:u64)->Self{Self(ticks)}
    fn as_ticks(&self)->u64{self.0}
}
#[macro_export] macro_rules! log_error {($($tt:tt)*)=>{{}};}
mod chronos {pub fn monotonic_nanos()->u64{crate::NOW_NS.load(crate::Ordering::SeqCst)}}
mod intel {
    pub mod media {pub mod wd_xyuv8888 {
        #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct WdXyuv8888DmaSurface(pub u64);
        pub fn try_claim_stream_capture()->bool{
            crate::DRIVER.compare_exchange(0,2,crate::Ordering::SeqCst,crate::Ordering::SeqCst).is_ok()
            || crate::DRIVER.load(crate::Ordering::SeqCst)==2
        }
        pub fn release_stream_capture(){
            assert_eq!(crate::DRIVER.swap(0,crate::Ordering::SeqCst),2);
            crate::RELEASES.fetch_add(1,crate::Ordering::SeqCst);
        }
    }}
    pub fn stop_ui4_wd_xyuv8888_capture()->Result<(),()>{
        assert_eq!(crate::DRIVER.load(crate::Ordering::SeqCst),2,"must never stop manual owner");
        crate::STOPS.fetch_add(1,crate::Ordering::SeqCst);
        if crate::STOP_OK.load(crate::Ordering::SeqCst){Ok(())}else{Err(())}
    }
}
struct Duration;
impl Duration {fn from_millis(_:u64)->Self{Self}}
struct Timer(Option<u64>);
impl Timer {fn after(_:Duration)->Self{Self(None)}fn at(at:Instant)->Self{Self(Some(at.as_ticks()))}}
impl Future for Timer {type Output=();fn poll(self:core::pin::Pin<&mut Self>,_:&mut Context<'_>)->Poll<()>{
    if TIMER_EXPIRED.load(Ordering::SeqCst)||self.0.is_some_and(|deadline|Instant::now().as_ticks()>=deadline){Poll::Ready(())}else{Poll::Pending}
}}
struct Signal(AtomicBool);
impl Signal {
    const fn new()->Self{Self(AtomicBool::new(false))}
    fn signal(&self,_:()){self.0.store(true,Ordering::SeqCst);}
    fn wait(&self)->impl Future<Output=()>+'_ {core::future::poll_fn(|_| {
        if self.0.swap(false,Ordering::SeqCst){Poll::Ready(())}else{Poll::Pending}
    })}
}
const PREPARE_SLOT_COUNT:usize=1;
const PREPARE_DRAIN_TIMEOUT_MS:u64=2000;
static PREPARE_WAKE:Signal=Signal::new();
static PREPARE_DRAIN_WAKE:Signal=Signal::new();
static PREPARE_PIPELINE:Mutex<PreparePipeline>=Mutex::new(PreparePipeline::new());
@DEFINITIONS@
fn context()->Context<'static>{Context::from_waker(Waker::noop())}
fn ready<T>(future:impl Future<Output=T>)->T {
    let mut future=core::pin::pin!(future);
    match future.as_mut().poll(&mut context()){Poll::Ready(v)=>v,Poll::Pending=>panic!("unexpected pending")}
}
fn reset(driver:usize){
    *PREPARE_PIPELINE.lock()=PreparePipeline::new();
    DRIVER.store(driver,Ordering::SeqCst);STOPS.store(0,Ordering::SeqCst);RELEASES.store(0,Ordering::SeqCst);
    STOP_OK.store(true,Ordering::SeqCst);TIMER_EXPIRED.store(false,Ordering::SeqCst);
    NOW_NS.store(0,Ordering::SeqCst);
    PREPARE_WAKE.0.store(false,Ordering::SeqCst);PREPARE_DRAIN_WAKE.0.store(false,Ordering::SeqCst);
}
fn publish_ready(job:&PrepareJob){
    let mut p=PREPARE_PIPELINE.lock();let slot=&mut p.slots[job.slot_index];
    assert_eq!(slot.state,PrepareSlotState::Filling);
    slot.state=PrepareSlotState::Ready;slot.valid=true;
    slot.capture_begin_ns=10_000_000;slot.wd_complete_ns=17_000_000;slot.ready_ns=20_000_000;
    slot.xyuv8888=Some(intel::media::wd_xyuv8888::WdXyuv8888DmaSurface(123));
}
#[test] fn healthy_rollover_keeps_one_claim_and_target_until_full_cleanup(){
    reset(0);assert!(begin_preparation_session(10,330));
    let job=take_prepare_job().unwrap();assert_eq!(claim_prepare_capture(&job),Some(true));publish_ready(&job);
    NOW_NS.store(25_000_000,Ordering::SeqCst);
    let mut frame=take_prepared_scanout(10,0).unwrap();release_prepared_scanout(&mut frame);
    assert!(ready(finish_preparation_session(10,true)));
    assert_eq!((DRIVER.load(Ordering::SeqCst),STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(2,0,0));
    assert!(PREPARE_PIPELINE.lock().capture_claimed);
    assert!(begin_preparation_session(11,330));let next=take_prepare_job().unwrap();
    release_prepared_scanout(&mut frame); // prior generation must not empty the new job
    assert_eq!(PREPARE_PIPELINE.lock().slots[0].state,PrepareSlotState::Filling);
    assert_eq!(claim_prepare_capture(&next),Some(true));publish_ready(&next);
    assert!(ready(end_preparation_session(11)));
    let p=PREPARE_PIPELINE.lock();assert!(!p.capture_claimed);assert!(p.slots[0].xyuv8888.is_none());
    assert_eq!((DRIVER.load(Ordering::SeqCst),STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(0,1,1));
}
#[test] fn begin_rejects_active_or_retained_reader_without_resetting_generation(){
    reset(0);assert!(begin_preparation_session(10,330));let generation=PREPARE_PIPELINE.lock().generation;
    assert!(!begin_preparation_session(11,330));
    {let mut p=PREPARE_PIPELINE.lock();p.active=false;p.slots[0].state=PrepareSlotState::Consuming;}
    assert!(!begin_preparation_session(11,330));
    let p=PREPARE_PIPELINE.lock();assert_eq!((p.generation,p.session_id,p.slots[0].state),(generation,10,PrepareSlotState::Consuming));
}
#[test] fn cancelled_manual_claim_never_stops_or_releases_manual_capture(){
    reset(1);assert!(begin_preparation_session(10,330));let job=take_prepare_job().unwrap();
    assert_eq!(claim_prepare_capture(&job),Some(false));
    let mut drain=Box::pin(end_preparation_session(10));assert!(drain.as_mut().poll(&mut context()).is_pending());
    assert_eq!(claim_prepare_capture(&job),None);PREPARE_DRAIN_WAKE.signal(());
    assert_eq!(drain.as_mut().poll(&mut context()),Poll::Ready(true));
    assert_eq!((DRIVER.load(Ordering::SeqCst),STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(1,0,0));
}
#[test] fn filling_drain_waits_for_published_completion_then_releases_ready_descriptor(){
    reset(0);assert!(begin_preparation_session(10,330));let job=take_prepare_job().unwrap();assert_eq!(claim_prepare_capture(&job),Some(true));
    let mut drain=Box::pin(end_preparation_session(10));assert!(drain.as_mut().poll(&mut context()).is_pending());
    assert_eq!(STOPS.load(Ordering::SeqCst),0);
    {let mut p=PREPARE_PIPELINE.lock();p.slots[0].state=PrepareSlotState::Empty;p.slots[0].reset_metadata();}
    PREPARE_DRAIN_WAKE.signal(());
    assert_eq!(drain.as_mut().poll(&mut context()),Poll::Ready(true));assert_eq!(DRIVER.load(Ordering::SeqCst),0);
}
#[test] fn drain_timeout_quarantines_claim_and_late_job_cannot_restart_capture(){
    reset(0);assert!(begin_preparation_session(10,330));let job=take_prepare_job().unwrap();assert_eq!(claim_prepare_capture(&job),Some(true));
    let mut drain=Box::pin(end_preparation_session(10));assert!(drain.as_mut().poll(&mut context()).is_pending());
    TIMER_EXPIRED.store(true,Ordering::SeqCst);assert_eq!(drain.as_mut().poll(&mut context()),Poll::Ready(false));drop(drain);
    assert!(PREPARE_PIPELINE.lock().quarantined);assert!(!begin_preparation_session(11,330));
    assert_eq!(claim_prepare_capture(&job),None);assert!(PREPARE_PIPELINE.lock().quarantined);
    assert_eq!((DRIVER.load(Ordering::SeqCst),STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(2,0,0));
}
#[test] fn consuming_and_quarantined_vcs_readers_block_teardown_and_reuse(){
    for state in [PrepareSlotState::Consuming,PrepareSlotState::Quarantined] {
        reset(0);assert!(begin_preparation_session(10,330));PREPARE_PIPELINE.lock().slots[0].state=state;
        assert!(!ready(end_preparation_session(10)));assert!(!begin_preparation_session(11,330));
        assert_eq!((STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(0,0));
        assert_eq!(DRIVER.load(Ordering::SeqCst),2);
    }
}
#[test] fn screenshot_pin_or_driver_stop_failure_retains_claim_and_blocks_next_owner(){
    reset(0);assert!(begin_preparation_session(10,330));STOP_OK.store(false,Ordering::SeqCst);
    assert!(!ready(end_preparation_session(10)));assert!(!ready(release_retained_preparation_capture()));
    assert!(!begin_preparation_session(11,330));assert!(PREPARE_PIPELINE.lock().capture_claimed);
    assert_eq!((DRIVER.load(Ordering::SeqCst),STOPS.load(Ordering::SeqCst),RELEASES.load(Ordering::SeqCst)),(2,1,0));
}
#[test] fn stale_end_and_stale_job_cannot_modify_live_generation(){
    reset(0);assert!(begin_preparation_session(10,330));let old=take_prepare_job().unwrap();
    assert_eq!(claim_prepare_capture(&old),Some(true));publish_ready(&old);assert!(ready(finish_preparation_session(10,true)));
    assert!(begin_preparation_session(11,330));let current=take_prepare_job().unwrap();
    assert_eq!(claim_prepare_capture(&old),None);assert!(!ready(end_preparation_session(10)));
    let p=PREPARE_PIPELINE.lock();assert!(p.active);assert_eq!(p.session_id,11);assert_eq!(p.slots[0].generation,current.generation);
    assert_eq!(p.slots[0].state,PrepareSlotState::Filling);assert_eq!(STOPS.load(Ordering::SeqCst),0);
}
#[test] fn timing_records_trigger_complete_ready_and_consume_boundaries_before_slot_reset(){
    reset(0);assert!(begin_preparation_session(10,330));let job=take_prepare_job().unwrap();publish_ready(&job);
    NOW_NS.store(25_000_000,Ordering::SeqCst);let mut frame=take_prepared_scanout(10,0).unwrap();
    let mut stats=LiveEncodeStats::default();stats.record_prepared_timing(&frame);release_prepared_scanout(&mut frame);
    assert_eq!((stats.prepared_samples,stats.prepared_age_us,stats.capture_to_consume_us,stats.wd_complete_to_ready_us),(1,5000,15000,3000));
    frame.ready_ns=18_000_000;frame.consume_begin_ns=30_000_000;stats.record_prepared_timing(&frame);
    assert_eq!((stats.prepared_samples,stats.prepared_age_max_us,stats.capture_to_consume_max_us),(2,12000,20000));
    assert_eq!((stats.prepared_age_us/2,stats.capture_to_consume_us/2),(8500,17500));
    let slot=&PREPARE_PIPELINE.lock().slots[0];assert!(slot.xyuv8888.is_none());assert_eq!((slot.capture_begin_ns,slot.wd_complete_ns,slot.ready_ns,slot.consume_begin_ns),(0,0,0,0));
}
fn scheduled_after_first_frame(deadline:u64){
    assert!(begin_preparation_session(10,330));
    let first=take_prepare_job().unwrap();assert_eq!(claim_prepare_capture(&first),Some(true));publish_ready(&first);
    PREPARE_PIPELINE.lock().capture_timing.observe(4_000_000);
    assert!(schedule_next_capture(10,1,Instant::from_ticks(deadline)));
    NOW_NS.store(25_000_000,Ordering::SeqCst);
    let mut frame=take_prepared_scanout(10,0).unwrap();release_prepared_scanout(&mut frame);
}
#[test] fn scheduled_capture_stays_empty_until_recent_peak_plus_guard_lead(){
    reset(0);scheduled_after_first_frame(100);
    assert_eq!(preparation_wake_deadline(),Some(Instant::from_ticks(94)));
    assert!(take_prepare_job().is_none());
    {let p=PREPARE_PIPELINE.lock();assert_eq!((p.next_sequence,p.slots[0].state),(1,PrepareSlotState::Empty));}
    NOW_NS.store(93_000_000,Ordering::SeqCst);assert!(take_prepare_job().is_none());
    NOW_NS.store(94_000_000,Ordering::SeqCst);let job=take_prepare_job().unwrap();
    assert_eq!(job.sequence,1);assert_eq!(claim_prepare_capture(&job),Some(true));
}
#[test] fn scheduled_target_never_overwrites_a_consuming_frame_even_when_due(){
    reset(0);assert!(begin_preparation_session(10,330));let first=take_prepare_job().unwrap();publish_ready(&first);
    assert!(schedule_next_capture(10,1,Instant::from_ticks(30)));
    let mut current=take_prepared_scanout(10,0).unwrap();NOW_NS.store(1_000_000_000,Ordering::SeqCst);
    assert!(take_prepare_job().is_none());assert!(preparation_wake_deadline().is_none());
    assert_eq!(PREPARE_PIPELINE.lock().slots[0].state,PrepareSlotState::Consuming);
    release_prepared_scanout(&mut current);assert_eq!(take_prepare_job().unwrap().sequence,1);
}
#[test] fn cancellation_wakes_a_pending_capture_timer_before_any_filling_or_trigger(){
    reset(0);scheduled_after_first_frame(100);PREPARE_WAKE.0.store(false,Ordering::SeqCst);
    let mut waiting=Box::pin(wait_for_prepare_work(preparation_wake_deadline()));
    assert!(waiting.as_mut().poll(&mut context()).is_pending());
    assert!(ready(end_preparation_session(10)));
    assert!(waiting.as_mut().poll(&mut context()).is_ready());drop(waiting);
    assert!(take_prepare_job().is_none());assert_eq!(DRIVER.load(Ordering::SeqCst),0);
    assert_eq!(PREPARE_PIPELINE.lock().slots[0].state,PrepareSlotState::Empty);
}
#[test] fn deadline_update_wakes_timer_and_rechecks_exact_sequence_schedule(){
    reset(0);scheduled_after_first_frame(100);PREPARE_WAKE.0.store(false,Ordering::SeqCst);
    let mut waiting=Box::pin(wait_for_prepare_work(preparation_wake_deadline()));
    assert!(waiting.as_mut().poll(&mut context()).is_pending());
    assert!(schedule_next_capture(10,1,Instant::from_ticks(80)));
    assert!(waiting.as_mut().poll(&mut context()).is_ready());drop(waiting);
    assert_eq!(preparation_wake_deadline(),Some(Instant::from_ticks(74)));
    NOW_NS.store(74_000_000,Ordering::SeqCst);assert_eq!(take_prepare_job().unwrap().sequence,1);
}
#[test] fn already_due_timer_rechecks_without_waiting_for_an_unrelated_signal(){
    reset(0);scheduled_after_first_frame(100);PREPARE_WAKE.0.store(false,Ordering::SeqCst);
    let deadline=preparation_wake_deadline();NOW_NS.store(95_000_000,Ordering::SeqCst);
    assert_eq!(preparation_wake_deadline(),deadline);ready(wait_for_prepare_work(deadline));
    assert_eq!(take_prepare_job().unwrap().sequence,1);
}
#[test] fn late_capture_deadline_runs_asap_without_touching_future_generation(){
    reset(0);scheduled_after_first_frame(20);
    assert_eq!(take_prepare_job().unwrap().sequence,1);
    assert!(!schedule_next_capture(10,1,Instant::from_ticks(100)));
    assert!(!schedule_next_capture(9,2,Instant::from_ticks(100)));
    assert_eq!(PREPARE_PIPELINE.lock().slots[0].state,PrepareSlotState::Filling);
}
#[test] fn timing_budget_survives_successful_rollover_and_resets_after_release(){
    reset(0);assert!(begin_preparation_session(10,330));PREPARE_PIPELINE.lock().capture_timing.observe(4_000_000);
    assert!(ready(finish_preparation_session(10,true)));assert!(begin_preparation_session(11,330));
    assert_eq!(PREPARE_PIPELINE.lock().capture_timing.lead_ticks(1000,33),6);
    assert!(ready(end_preparation_session(11)));assert!(begin_preparation_session(12,330));
    assert_eq!(PREPARE_PIPELINE.lock().capture_timing.lead_ticks(1000,33),30);
    // The first frame is immediate even with a cold full-frame lead budget.
    assert_eq!(take_prepare_job().unwrap().sequence,0);
}
'''


def main():
    source = (ROOT / "src/ui4/h264_encode_stream.rs").read_text()
    definitions = "#[derive(Default)]\n" + item(source, "struct LiveEncodeStats")
    definitions += "\n" + item(source, "impl LiveEncodeStats")
    definitions += "\n#[derive(Clone,Copy,Debug,Eq,PartialEq)]\n" + item(source, "enum PrepareSlotState")
    definitions += "\n#[derive(Clone,Copy)]\n" + item(source, "struct CaptureDeadline")
    definitions += "\n" + "\n".join(item(source, signature) for signature in [
        "struct PrepareSlot", "impl PrepareSlot", "struct PreparePipeline", "impl PreparePipeline",
        "struct PrepareJob", "struct PreparedScanout", "fn take_prepare_job(",
        "fn capture_not_before(", "fn preparation_wake_deadline(", "fn schedule_next_capture(",
        "async fn wait_for_prepare_work(",
        "fn claim_prepare_capture(", "fn begin_preparation_session(",
        "async fn end_preparation_session(", "async fn release_retained_preparation_capture(",
        "async fn finish_preparation_session(", "fn take_prepared_scanout(",
        "fn release_prepared_scanout(",
    ])
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-capture-lifetime-") as directory:
        directory = Path(directory)
        (directory / "lib.rs").write_text(HARNESS.replace("@DEFINITIONS@", definitions).replace("@TIMING@", str(ROOT / "src/ui4/h264_capture_timing.rs")))
        (directory / "Cargo.toml").write_text('[package]\nname="capture-lifetime-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\n')
        subprocess.run(["cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"), "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/capture-lifetime"), "--", "--test-threads=1"], cwd=directory, check=True)


if __name__ == "__main__":
    main()
