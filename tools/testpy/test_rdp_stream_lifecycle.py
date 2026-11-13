#!/usr/bin/env python3
"""Run actual bounded RDP producer/egress futures through ownership changes."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unused_variables, unused_imports)]
extern crate alloc;
use core::{future::Future, pin::Pin, sync::atomic::{AtomicU64, AtomicUsize}, task::{Context, Poll, Waker}};
use std::{cell::{Cell, RefCell}, sync::Arc, task::Wake};
#[macro_export] macro_rules! log_info { ($($tt:tt)*) => {{}}; }
#[macro_export] macro_rules! log_warn { ($($tt:tt)*) => {{}}; }
static TICKS: AtomicU64 = AtomicU64::new(1);
static TIMER_WAKERS: spin::Mutex<Vec<(u64,Waker)>> = spin::Mutex::new(Vec::new());
struct Clock;
impl embassy_time_driver::Driver for Clock {
    fn now(&self)->u64{TICKS.load(Ordering::SeqCst)}
    fn schedule_wake(&self,at:u64,waker:&Waker){TIMER_WAKERS.lock().push((at,waker.clone()));}
}
embassy_time_driver::time_driver_impl!(static CLOCK: Clock = Clock);
fn advance(ticks:u64){
    let now=TICKS.fetch_add(ticks,Ordering::SeqCst)+ticks;
    let mut ready=Vec::new();{let mut timers=TIMER_WAKERS.lock();let mut index=0;
    while index<timers.len(){if timers[index].0<=now{ready.push(timers.swap_remove(index).1);}else{index+=1;}}}
    for waker in ready{waker.wake();}
}
mod chronos {pub fn monotonic_nanos()->u64{crate::TICKS.load(crate::Ordering::SeqCst)*1_000_000}}
mod allcaps {pub mod media_encode {pub const STREAM_MAX_ACCESS_UNIT_BYTES:usize=4*1024*1024;}}
mod percpu {pub fn current_slot()->usize{3}}
mod wait {
    use spin::Mutex;
    use embassy_sync::blocking_mutex::raw::RawMutex;
    @RAW_MUTEX@
    @RAW_IMPL@
}
static MEDIA_WAKES:AtomicUsize=AtomicUsize::new(0);
mod shell3 {pub mod net {pub fn notify_owner(slot:u32){
    assert_eq!(slot,3);assert!(crate::SSH_MEDIA.try_lock().is_some());
    crate::MEDIA_WAKES.fetch_add(1,crate::Ordering::SeqCst);
}}}
@PRODUCTION@
fn reset(){
    *EGRESS_PIPELINE.lock()=EgressPipeline::new();*SSH_MEDIA.lock()=rdp_media_queue::Queue::new();
    EGRESS_WAKE.reset();PRODUCER_WAKE.reset();TICKS.store(1,Ordering::SeqCst);TIMER_WAKERS.lock().clear();
    assert!(rdp_media_subscribe(7));
}
fn idr()->Vec<u8>{vec![0,0,0,1,0x65,1,2,3]}
fn poll<F:Future>(future:Pin<&mut F>)->Poll<F::Output>{future.poll(&mut Context::from_waker(Waker::noop()))}
async fn serve_one(){
    let request=loop{if let Some(request)=take_egress_session_request(){break request;}EGRESS_WAKE.wait().await;};
    let report=run_egress_session(request).await;complete_egress_session(request.session_id,report);
}
fn settle<P:Future<Output=MediaUdpStreamReport>,E:Future<Output=()>>(
    mut producer:Pin<&mut P>, mut egress:Pin<&mut E>,
)->MediaUdpStreamReport{
    let mut egress_done=false;
    for _ in 0..16{
        if !egress_done{egress_done=poll(egress.as_mut()).is_ready();}
        if let Poll::Ready(report)=poll(producer.as_mut()){assert!(egress_done);return report;}
    }
    panic!("producer/egress completion handshake did not settle");
}
#[test]
fn disconnect_during_prefill_finishes_both_sides_without_waiting_for_a_frame(){
    reset();let began=Cell::new(0);let encoded=Cell::new(0);
    let mut producer=std::pin::pin!(stream_generated_annex_b(91,7,2,60,
        ||{began.set(began.get()+1);true},|_,_|true,|_|false,async |_|{encoded.set(encoded.get()+1);Some(idr())}));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(began.get(),1);
    rdp_media_unsubscribe(7);
    let report=settle(producer.as_mut(),egress.as_mut());
    assert_eq!(encoded.get(),0);assert_eq!(report.sent_access_units,0);assert_eq!(EGRESS_PIPELINE.lock().phase,EgressSessionPhase::Idle);
}
#[test]
fn replacement_before_ready_cannot_start_old_preparation_or_redirect_its_stream(){
    reset();let began=Cell::new(0);
    let mut producer=std::pin::pin!(stream_generated_annex_b(92,7,1,60,
        ||{began.set(began.get()+1);true},|_,_|true,|_|true,async |_|Some(idr())));
    assert!(poll(producer.as_mut()).is_pending());
    rdp_media_unsubscribe(7);assert!(rdp_media_subscribe(8));
    let mut egress=std::pin::pin!(serve_one());
    let report=settle(producer.as_mut(),egress.as_mut());
    assert_eq!(began.get(),0);assert_eq!(report.sent_access_units,0);assert_eq!(current_subscriber(),Some(8));
    assert!(rdp_media_take_packet(8).is_none());
}
#[test]
fn disconnect_while_waiting_for_next_sequence_finishes_without_preparing_it(){
    reset();let checked=Cell::new(0);let encoded=Cell::new(0);
    let mut producer=std::pin::pin!(stream_generated_annex_b(93,7,2,1000,||true,|_,_|true,
        |sequence|{checked.set(sequence);sequence==0},async |_|{encoded.set(encoded.get()+1);Some(idr())}));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(encoded.get(),1);
    assert!(poll(egress.as_mut()).is_pending());advance(2);
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(checked.get(),1);
    rdp_media_unsubscribe(7);let report=settle(producer.as_mut(),egress.as_mut());
    assert_eq!(encoded.get(),1);assert_eq!(report.sent_access_units,1);
}
#[test]
fn disconnect_during_prepared_check_cannot_lose_its_wake_or_start_encoding(){
    reset();let encoded=Cell::new(false);
    let mut producer=std::pin::pin!(stream_generated_annex_b(94,7,1,60,||true,|_,_|true,
        |_|{rdp_media_unsubscribe(7);false},async |_|{encoded.set(true);Some(idr())}));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    let report=settle(producer.as_mut(),egress.as_mut());assert!(!encoded.get());assert_eq!(report.sent_access_units,0);
}
#[test]
fn failed_preparation_claim_completes_without_calling_prepared_or_encoder(){
    reset();let mut producer=std::pin::pin!(stream_generated_annex_b(95,7,1,60,||false,|_,_|panic!("scheduled after refused claim"),
        |_|panic!("prepared after refused claim"),async |_|panic!("encode after refused claim")));
    assert!(poll(producer.as_mut()).is_pending());let mut egress=std::pin::pin!(serve_one());
    let report=settle(producer.as_mut(),egress.as_mut());assert_eq!(report.sent_access_units,0);assert_eq!(report.dropped_access_units,1);
}
#[test]
fn disconnected_in_flight_encoder_is_awaited_to_retirement_and_its_bytes_are_not_published(){
    reset();let started=Cell::new(false);let can_retire=Cell::new(false);let retired=Cell::new(false);
    let mut producer=std::pin::pin!(stream_generated_annex_b(96,7,1,60,||true,|_,_|true,|_|true,async |_|{
        started.set(true);core::future::poll_fn(|_|if can_retire.get(){Poll::Ready(())}else{Poll::Pending}).await;
        retired.set(true);Some(idr())
    }));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    assert!(poll(producer.as_mut()).is_pending());assert!(started.get());
    rdp_media_unsubscribe(7);assert!(poll(producer.as_mut()).is_pending());assert!(!retired.get());
    assert!(poll(egress.as_mut()).is_pending(),"egress completed before producer retirement");
    can_retire.set(true);let report=settle(producer.as_mut(),egress.as_mut());
    assert!(retired.get());assert_eq!(report.sent_access_units,0);assert_eq!(report.dropped_access_units,1);
    assert!(SSH_MEDIA.lock().pop(7).is_none());
}
#[test]
fn successful_bounded_rollover_keeps_exact_subscriber_and_starts_new_wire_session(){
    reset();
    for session_id in [97,98]{
        let mut producer=std::pin::pin!(stream_generated_annex_b(session_id,7,1,60,||true,
            |_,_|panic!("scheduled a capture beyond the last access unit"),|_|true,async |_|Some(idr())));
        let mut egress=std::pin::pin!(serve_one());assert!(poll(producer.as_mut()).is_pending());
        let report=settle(producer.as_mut(),egress.as_mut());assert_eq!(report.sent_access_units,1);assert_eq!(report.dropped_access_units,0);
        assert_eq!(current_subscriber(),Some(7));let packet=rdp_media_take_packet(7).unwrap();
        assert_eq!(u32::from_be_bytes(packet[8..12].try_into().unwrap()),session_id);
        assert_ne!(packet[5]&FLAG_SESSION_END,0);assert!(rdp_media_take_packet(7).is_none());
    }
}
#[test]
fn prefill_does_not_schedule_or_encode_until_the_first_frame_is_ready(){
    reset();let ready=Cell::new(false);let scheduled=Cell::new(0);let encoded=Cell::new(0);
    let mut producer=std::pin::pin!(stream_generated_annex_b(101,7,2,60,||true,
        |sequence,deadline|{assert_eq!(sequence,1);assert_eq!(deadline.as_ticks(),517);scheduled.set(1);true},
        |_|ready.get(),async |_|{assert_eq!(scheduled.get(),1);encoded.set(encoded.get()+1);Some(idr())}));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    assert!(poll(producer.as_mut()).is_pending());advance(500);
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(scheduled.get(),0);assert_eq!(encoded.get(),0);
    ready.set(true);wake_producer();assert!(poll(producer.as_mut()).is_pending());assert_eq!(encoded.get(),1);
    rdp_media_unsubscribe(7);advance(16);let _=settle(producer.as_mut(),egress.as_mut());
}
#[test]
fn next_deadline_is_published_before_encoding_and_fractional_cadence_stays_exact(){
    for hz in [33,60]{
        reset();let schedules=RefCell::new(Vec::new());let encoded=Cell::new(0);
        let mut producer=std::pin::pin!(stream_generated_annex_b(102,7,hz+1,hz,||true,
            |sequence,deadline|{schedules.borrow_mut().push((sequence,deadline.as_ticks()));true},
            |_|true,async |sequence|{
                assert_eq!(encoded.get(),sequence as usize);
                let published=schedules.borrow();
                assert_eq!(published.len(),(sequence as usize+1).min(hz));
                if (sequence as usize)<hz{assert_eq!(published.last().unwrap().0,sequence+1);}
                encoded.set(encoded.get()+1);Some(idr())
            }));
        let mut egress=std::pin::pin!(serve_one());
        assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
        assert!(poll(producer.as_mut()).is_pending());assert_eq!(encoded.get(),1);
        for sequence in 1..=hz{
            let (published_sequence,deadline)=schedules.borrow()[sequence-1];
            assert_eq!(published_sequence,sequence as u32);
            assert!(poll(egress.as_mut()).is_pending());
            advance(deadline-TICKS.load(Ordering::SeqCst));
            assert!(poll(producer.as_mut()).is_pending());assert_eq!(encoded.get(),sequence+1);
        }
        let report=settle(producer.as_mut(),egress.as_mut());
        assert_eq!(report.sent_access_units,hz+1);assert_eq!(report.late_access_units,0);
        let published=schedules.borrow();assert_eq!(published.len(),hz);assert_eq!(published.last().unwrap().1,1_001);
        let mut previous=1;
        for &(_,deadline) in published.iter(){let period=deadline-previous;assert!(period==1_000/hz as u64||period==1_000/hz as u64+1);previous=deadline;}
    }
}
#[test]
fn late_frame_rebases_the_next_deadline_without_resetting_fractional_phase(){
    reset();let schedules=RefCell::new(Vec::new());let encoded=Cell::new(0);
    let mut producer=std::pin::pin!(stream_generated_annex_b(103,7,4,60,||true,
        |sequence,deadline|{schedules.borrow_mut().push((sequence,deadline.as_ticks()));true},
        |_|true,async |_|{encoded.set(encoded.get()+1);Some(idr())}));
    let mut egress=std::pin::pin!(serve_one());
    assert!(poll(producer.as_mut()).is_pending());assert!(poll(egress.as_mut()).is_pending());
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(schedules.borrow().as_slice(),&[(1,17)]);
    assert!(poll(egress.as_mut()).is_pending());advance(20);
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(encoded.get(),2);
    // The late frame starts at tick 21; the next fractional period is 17,
    // rather than catching up to 34 or restarting the cadence with 16.
    assert_eq!(schedules.borrow().as_slice(),&[(1,17),(2,38)]);
    assert!(poll(egress.as_mut()).is_pending());advance(17);
    assert!(poll(producer.as_mut()).is_pending());assert_eq!(schedules.borrow().as_slice(),&[(1,17),(2,38),(3,55)]);
    assert!(poll(egress.as_mut()).is_pending());advance(17);
    assert!(poll(producer.as_mut()).is_pending());let report=settle(producer.as_mut(),egress.as_mut());
    assert_eq!(report.sent_access_units,4);assert_eq!(report.late_access_units,1);assert_eq!(report.max_late_us,4_000);
}
#[test]
fn failed_next_capture_schedule_aborts_before_the_current_encode(){
    reset();let mut producer=std::pin::pin!(stream_generated_annex_b(104,7,2,60,||true,
        |sequence,_|{assert_eq!(sequence,1);false},|_|true,async |_|panic!("encoded without publishing its successor")));
    let mut egress=std::pin::pin!(serve_one());assert!(poll(producer.as_mut()).is_pending());
    let report=settle(producer.as_mut(),egress.as_mut());assert_eq!(report.sent_access_units,0);
    assert_eq!(EGRESS_PIPELINE.lock().phase,EgressSessionPhase::Idle);assert!(rdp_media_take_packet(7).is_none());
}
#[test]
fn disconnect_during_schedule_callback_prevents_the_current_encode(){
    reset();let mut producer=std::pin::pin!(stream_generated_annex_b(105,7,2,60,||true,
        |_,_|{rdp_media_unsubscribe(7);true},|_|true,async |_|panic!("encoded after callback lost its owner")));
    let mut egress=std::pin::pin!(serve_one());assert!(poll(producer.as_mut()).is_pending());
    let report=settle(producer.as_mut(),egress.as_mut());assert_eq!(report.sent_access_units,0);
    assert_eq!(EGRESS_PIPELINE.lock().phase,EgressSessionPhase::Idle);assert_eq!(current_subscriber(),None);
}
#[derive(Default)] struct Count(AtomicUsize);
impl Wake for Count{fn wake(self:Arc<Self>){self.0.fetch_add(1,Ordering::SeqCst);}}
#[test]
fn subscription_lifetime_wakes_the_idle_producer_after_queue_publication(){
    reset();PRODUCER_WAKE.reset();let count=Arc::new(Count::default());let waker=Waker::from(count.clone());
    let mut waiting=Box::pin(wait_producer_work());assert!(waiting.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
    rdp_media_unsubscribe(7);assert_eq!(count.0.load(Ordering::SeqCst),1);assert_eq!(current_subscriber(),None);
    assert!(waiting.as_mut().poll(&mut Context::from_waker(&waker)).is_ready());drop(waiting);
    let mut waiting=Box::pin(wait_producer_work());assert!(waiting.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
    assert!(rdp_media_subscribe(8));assert_eq!(count.0.load(Ordering::SeqCst),2);assert!(subscriber_is(8));
    assert!(waiting.as_mut().poll(&mut Context::from_waker(&waker)).is_ready());
}
'''


def main():
    source = (ROOT / "src/ui4/h264_encode_udp.rs").read_text()
    worker = item(source, "pub(crate) async fn ui4_h264_encode_udp_egress_task()")
    source = source.replace('#[trueos_executor::task(pool_size = 1)]\n' + worker, '')
    source = source.replace('pub(super)', 'pub(crate)')
    source = source.replace('#[path = "rdp_media_queue.rs"]', f'#[path = "{ROOT / "src/ui4/rdp_media_queue.rs"}"]')
    source = re.sub(r'^//!.*\n', '', source, flags=re.MULTILINE)
    raw = (ROOT / "src/wait.rs").read_text()
    harness = HARNESS.replace("@RAW_MUTEX@", item(raw, "pub struct EmbassySpinRawMutex").split(';', 1)[0] + ';')
    harness = harness.replace("@RAW_IMPL@", item(raw, "unsafe impl RawMutex for EmbassySpinRawMutex"))
    harness = harness.replace("@PRODUCTION@", source)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-rdp-stream-lifecycle-") as directory:
        directory = Path(directory)
        (directory / "lib.rs").write_text(harness)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="rdp-stream-lifecycle-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\ncrc32fast="1.5"\n'
            'embassy-sync={version="0.8.0",default-features=false}\n'
            'embassy-time-driver={version="0.2.2",features=["tick-hz-1_000"]}\n'
            f'trueos-time={{path="{ROOT / "crates/trueos-executor/embassy-time"}",default-features=false}}\n')
        subprocess.run(["cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/rdp-stream-lifecycle"),
            "--", "--test-threads=1"], cwd=directory, check=True, timeout=120)


if __name__ == "__main__":
    main()
