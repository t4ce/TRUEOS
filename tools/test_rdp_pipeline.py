#!/usr/bin/env python3
"""Host regressions for real RDP queue and WD-source ownership code."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, constant, item


def block(path, prefix):
    source = (ROOT / path).read_text()
    match = re.search(r'^' + re.escape(prefix) + r'.*?^}\n', source, re.M | re.S)
    if not match:
        raise ValueError(f'missing {prefix} in {path}')
    return match.group()


def main():
    udp = 'src/ui4/h264_encode_udp.rs'
    stream = 'src/ui4/h264_encode_stream.rs'
    source = r'''
#![allow(dead_code)]
use std::collections::VecDeque;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
struct Signal;
impl Signal { fn signal(&self, _: ()) {} }
#[macro_export] macro_rules! log_warn { ($($tokens:tt)*) => {}; }
mod chronos { pub fn monotonic_nanos() -> u64 { 1_000 } }
mod intel {
    #[derive(Debug)] pub struct Error;
    pub fn stop_ui4_wd_xyuv8888_capture() -> Result<(), Error> {
        super::STOP_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if super::STOP_FAILS.load(std::sync::atomic::Ordering::SeqCst) { Err(Error) }
        else { Ok(()) }
    }
    pub mod media { pub mod wd_xyuv8888 {
        #[derive(Clone, Copy)] pub struct WdXyuv8888DmaSurface;
        pub fn release_stream_capture() {
            super::super::super::RELEASE_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    } }
}
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
static STOP_CALLS: AtomicUsize = AtomicUsize::new(0);
static RELEASE_CALLS: AtomicUsize = AtomicUsize::new(0);
static STOP_FAILS: AtomicBool = AtomicBool::new(false);
static PREPARE_WAKE: Signal = Signal;
struct Duration;
impl Duration { fn from_millis(_: u64) -> Self { Self } }
struct Timer;
impl Timer {
    async fn after(_: Duration) {
        // At the suspension point, WD still owns Filling; teardown has not
        // disabled it or released the allocation. Now deliver completion.
        assert_eq!(STOP_CALLS.load(Ordering::SeqCst), 0);
        let mut pipeline = PREPARE_PIPELINE.lock();
        assert!(!pipeline.active);
        pipeline.slots[0].state = PrepareSlotState::Empty;
    }
}
fn run<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    for _ in 0..100 {
        if let std::task::Poll::Ready(result) = future.as_mut().poll(&mut cx) { return result; }
    }
    panic!("future did not finish");
}
'''
    source += '\nmod udp { use super::*;\n'
    source += constant(udp, 'ENCODED_ACCESS_UNIT_QUEUE_CAP') + '\n'
    for name in ['EncodedAccessUnit', 'EgressSessionPhase', 'EgressSessionRequest',
                 'EgressPipeline', 'MediaUdpStreamReport']:
        source += item(udp, name) + '\n'
    source += block(udp, 'impl EgressPipeline {')
    source += '''\nstatic EGRESS_PIPELINE: Mutex<EgressPipeline> = Mutex::new(EgressPipeline::new());
static EGRESS_WAKE: Signal = Signal;
static PRODUCER_WAKE: Signal = Signal;
'''
    for name in ['request_egress_session', 'take_egress_session_request',
                 'mark_egress_session_ready', 'enqueue_access_unit',
                 'take_next_egress_access_unit', 'abort_egress_session',
                 'finish_egress_producer', 'complete_egress_session', 'take_egress_report']:
        source += item(udp, name) + '\n'
    source += r'''
fn start(session: u32) {
    *EGRESS_PIPELINE.lock() = EgressPipeline::new();
    assert!(request_egress_session(EgressSessionRequest {
        session_id: session, access_unit_count: 330, target_hz: 33,
    }));
    assert!(take_egress_session_request().is_some());
    assert!(mark_egress_session_ready(session));
}
fn unit(sequence: u32) -> EncodedAccessUnit {
    EncodedAccessUnit { sequence, keyframe: sequence == 0, bytes: vec![0; 10] }
}
#[test]
fn queue_full_aborts_reference_chain_without_waiting_or_growing() {
    start(7);
    for n in 0..4 { assert!(enqueue_access_unit(7, unit(n))); }
    assert!(!enqueue_access_unit(7, unit(4)));
    let p = EGRESS_PIPELINE.lock();
    assert_eq!(p.phase, EgressSessionPhase::Aborted);
    assert!(p.queue.is_empty());
    assert_eq!(p.queued_bytes, 0);
    assert_eq!(p.producer_dropped_access_units, 5);
    assert_eq!(p.producer_dropped_bytes, 50);
    assert_eq!(p.producer_queue_wait_us, 0);
    drop(p);
    assert!(!enqueue_access_unit(7, unit(5)));
    assert!(matches!(take_next_egress_access_unit(7), Some(None)));
}
#[test]
fn stale_producer_cannot_damage_new_session() {
    start(9);
    assert!(!enqueue_access_unit(8, unit(0)));
    let p = EGRESS_PIPELINE.lock();
    assert_eq!(p.phase, EgressSessionPhase::Ready);
    assert_eq!(p.producer_dropped_access_units, 0);
}
#[test]
fn ordinary_drain_preserves_order_and_completes() {
    start(11);
    for n in 0..330 {
        assert!(enqueue_access_unit(11, unit(n)));
        let Some(Some(au)) = take_next_egress_access_unit(11) else { panic!() };
        assert_eq!(au.sequence, n);
    }
    finish_egress_producer(11, 0, 0);
    assert!(matches!(take_next_egress_access_unit(11), Some(None)));
    complete_egress_session(11, MediaUdpStreamReport::default());
    let report = take_egress_report(11).unwrap();
    assert_eq!(report.queued_access_units, 330);
    assert_eq!(report.dropped_access_units, 0);
    assert_eq!(report.producer_queue_wait_events, 0);
}
}
'''
    source += '\n' + constant(stream, 'PREPARE_SLOT_COUNT') + '\n'
    for name in ['PrepareSlotState', 'PrepareSlot', 'PreparePipeline',
                 'PreparedScanout', 'PrepareJob']:
        source += item(stream, name) + '\n'
    for prefix in ['impl PrepareSlot {', 'impl PreparePipeline {']:
        source += block(stream, prefix)
    source += '\nstatic PREPARE_PIPELINE: Mutex<PreparePipeline> = Mutex::new(PreparePipeline::new());\n'
    for name in ['take_prepare_job', 'take_prepared_scanout', 'release_prepared_scanout']:
        source += item(stream, name) + '\n'
    source += block(stream, 'async fn end_preparation_session(')
    source += r'''
fn setup(state: PrepareSlotState) {
    let mut p = PREPARE_PIPELINE.lock();
    *p = PreparePipeline::new();
    p.active = true;
    p.session_id = 5;
    p.generation = 3;
    p.access_unit_count = 330;
    p.slots[0].state = state;
    p.slots[0].generation = 3;
    p.slots[0].session_id = 5;
    STOP_CALLS.store(0, Ordering::SeqCst);
    RELEASE_CALLS.store(0, Ordering::SeqCst);
    STOP_FAILS.store(false, Ordering::SeqCst);
}
#[test]
fn quarantine_never_frees_or_recaptures_encoder_source() {
    setup(PrepareSlotState::Quarantined);
    assert!(!run(end_preparation_session(5)));
    assert_eq!(STOP_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(RELEASE_CALLS.load(Ordering::SeqCst), 0);
    assert!(take_prepare_job().is_none());
}
#[test]
fn consuming_source_is_also_retained() {
    setup(PrepareSlotState::Consuming);
    assert!(!run(end_preparation_session(5)));
    assert_eq!(STOP_CALLS.load(Ordering::SeqCst), 0);
}
#[test]
fn shutdown_drains_capture_before_disabling_wd() {
    setup(PrepareSlotState::Filling);
    assert!(run(end_preparation_session(5)));
    assert_eq!(STOP_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(RELEASE_CALLS.load(Ordering::SeqCst), 1);
    assert!(take_prepare_job().is_none());
}
#[test]
fn wd_disable_timeout_does_not_release_capture_owner() {
    setup(PrepareSlotState::Ready);
    STOP_FAILS.store(true, Ordering::SeqCst);
    assert!(!run(end_preparation_session(5)));
    assert_eq!(RELEASE_CALLS.load(Ordering::SeqCst), 0);
}
#[test]
fn source_slot_reuse_requires_explicit_encoder_release() {
    setup(PrepareSlotState::Ready);
    let mut prepared = take_prepared_scanout(5, 0).unwrap();
    assert!(take_prepare_job().is_none());
    release_prepared_scanout(&mut prepared);
    assert!(take_prepare_job().is_some());
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-rdp-pipeline-') as temp:
        rust, binary = Path(temp) / 'test.rs', Path(temp) / 'tests'
        rust.write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(rust), '-o', str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), '--test-threads=1'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
