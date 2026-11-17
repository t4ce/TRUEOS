#!/usr/bin/env python3
"""Host-check production ready-queue scheduling, media clock, and texture release.

This compiles the pure clock and the real queue-selection / texture-publication
functions against small ownership records. It does not build the kernel or
claim hardware timing/scanout validation.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
VIDEO = 'src/ui4/video_frame.rs'
TEXTURE = 'src/r/services/video_service.rs'


def source():
    harness = '#![allow(dead_code)]\n#[path="' + str(ROOT / 'src/ui4/video_presentation_timing.rs') + '"] mod video_presentation_timing;\n'
    harness += r'''
mod trueos_time { pub const TICK_HZ: u64 = 1000; }
mod video {
use crate::trueos_time;
use super::video_presentation_timing::PresentationClock;
#[derive(Copy,Clone)] struct Session(bool);
impl Session { fn is_cancelled(self)->bool { self.0 } }
#[derive(Copy,Clone)] struct Request { session: Session, order: usize, presentation_ticks: u64 }
struct Outcome { ready: Option<()> }
struct ReadyVideoConversion { request: Request, outcome: Outcome, ready_tick: u64, presentation_deadline: Option<u64> }
#[derive(Copy,Clone)] struct Pending { published_tick: u64 }
#[derive(Default)] struct DecodedVideoConversionState {
    ready: Vec<ReadyVideoConversion>, completed: usize, clock: PresentationClock,
    awaiting_latch: Option<Pending>, latch_wait_expirations: usize,
    presentation_waited_frames: usize, presentation_wait_ticks: u64,
    publication_late_frames: usize, max_publication_late_ticks: u64,
}
fn ready(order:usize, pts:u64, tick:u64)->ReadyVideoConversion {
    ReadyVideoConversion { request: Request { session:Session(false),order,presentation_ticks:pts },
        outcome:Outcome{ready:Some(())},ready_tick:tick,presentation_deadline:None }
}
'''
    for name in ['VIDEO_PUBLICATION_LEAD_TICKS', 'VIDEO_LATCH_WAIT_LIMIT_TICKS']:
        harness += constant(VIDEO, name) + '\n'
    harness += item(VIDEO, 'take_ready_video_conversion')
    harness += r'''
#[test] fn conversion_completion_order_does_not_reorder_presentation() {
    let mut state=DecodedVideoConversionState::default();
    state.ready.push(ready(2,40,105)); state.ready.push(ready(1,0,100));
    let mut wake=None;
    assert_eq!(take_ready_video_conversion(&mut state,100,&mut wake).unwrap().request.order,1);
    assert_eq!(state.ready.len(),1);
    state.completed=1;
    assert!(take_ready_video_conversion(&mut state,110,&mut wake).is_none());
    assert_eq!(wake,Some(140));
    assert_eq!(state.ready.len(),1); // the converted write/source leases remain held
    let frame=take_ready_video_conversion(&mut state,140,&mut None).unwrap();
    assert_eq!(frame.presentation_deadline,Some(142));
    assert_eq!(state.presentation_wait_ticks,35);
    assert_eq!(state.publication_late_frames,0);
}
#[test] fn pause_rebases_preconverted_frames_without_publishing_or_timer_polling() {
    let mut state=DecodedVideoConversionState::default();
    state.clock.deadline(100,0,false); state.clock.set_paused(true,110);
    state.ready.push(ready(1,40,105));
    let mut wake=None;
    assert!(take_ready_video_conversion(&mut state,190,&mut wake).is_none());
    assert_eq!(wake,None); assert_eq!(state.ready.len(),1);
    state.clock.set_paused(false,210);
    assert!(take_ready_video_conversion(&mut state,220,&mut wake).is_none());
    assert_eq!(wake,Some(238));
    assert!(take_ready_video_conversion(&mut state,238,&mut None).is_some());
}
#[test] fn missing_display_ack_expires_without_counting_scanout_and_unblocks_eos() {
    let mut state=DecodedVideoConversionState::default();
    state.clock.deadline(100,0,false);
    state.awaiting_latch=Some(Pending{published_tick:100});
    state.ready.push(ready(1,40,105));
    let mut wake=None;
    assert!(take_ready_video_conversion(&mut state,199,&mut wake).is_none());
    assert_eq!(wake,Some(200)); assert_eq!(state.ready.len(),1);
    assert!(take_ready_video_conversion(&mut state,200,&mut None).is_some());
    assert!(state.awaiting_latch.is_none()); assert_eq!(state.latch_wait_expirations,1);
    // Last publication has no successor, but still gets its bounded expiry.
    state.awaiting_latch=Some(Pending{published_tick:210});
    let mut wake=None;
    assert!(take_ready_video_conversion(&mut state,250,&mut wake).is_none());
    assert_eq!(wake,Some(310));
    assert!(take_ready_video_conversion(&mut state,310,&mut None).is_none());
    assert!(state.awaiting_latch.is_none()); assert_eq!(state.latch_wait_expirations,2);
}
#[test] fn cancelled_or_failed_ready_entries_drain_even_while_paused() {
    let mut state=DecodedVideoConversionState::default();
    state.clock.set_paused(true,100);
    let mut frame=ready(1,10_000,100); frame.request.session=Session(true);
    state.ready.push(frame);
    assert!(take_ready_video_conversion(&mut state,101,&mut None).is_some());
    let mut frame=ready(1,10_000,100); frame.outcome.ready=None;
    state.ready.push(frame);
    assert!(take_ready_video_conversion(&mut state,102,&mut None).is_some());
}
#[test] fn empty_or_waiting_for_predecessor_has_no_idle_timer() {
    let mut state=DecodedVideoConversionState::default(); let mut wake=None;
    assert!(take_ready_video_conversion(&mut state,100,&mut wake).is_none());
    assert_eq!(wake,None);
    state.ready.push(ready(2,40,100));
    assert!(take_ready_video_conversion(&mut state,100,&mut wake).is_none());
    assert_eq!(wake,None); assert_eq!(state.ready.len(),1);
}
}
mod texture {
#[derive(Copy,Clone,PartialEq,Eq)] struct VideoPlaybackSession(bool);
impl VideoPlaybackSession { fn is_cancelled(self)->bool { self.0 } }
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> { const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()} }
struct Slot {state:SlotState,sequence:u64}
struct Stream {id:u32,session:VideoPlaybackSession,closed:bool,sequence:u64,slots:Vec<Slot>}
static STREAMS: Mutex<Vec<Stream>>=Mutex::new(Vec::new());
'''
    for name in ['SlotState', 'ConvertedVideoTexture', 'finish_conversion']:
        harness += item(TEXTURE, name) + '\n'
    harness += r'''
#[test] fn converted_texture_is_writing_until_timed_publication_or_cancellation() {
    let session=VideoPlaybackSession(false);
    STREAMS.lock().push(Stream{id:1,session,closed:false,sequence:0,slots:vec![Slot{state:SlotState::Writing,sequence:0}]});
    assert!(STREAMS.lock()[0].slots[0].state==SlotState::Writing);
    assert!(finish_conversion(ConvertedVideoTexture{id:1,index:0,session},true));
    assert!(STREAMS.lock()[0].slots[0].state==SlotState::Ready);
    assert_eq!(STREAMS.lock()[0].sequence,1);
    assert!(!finish_conversion(ConvertedVideoTexture{id:1,index:0,session},true));
    assert_eq!(STREAMS.lock()[0].sequence,1);
    STREAMS.lock()[0].slots[0].state=SlotState::Writing;
    assert!(!finish_conversion(ConvertedVideoTexture{id:1,index:0,session},false));
    assert!(STREAMS.lock()[0].slots[0].state==SlotState::Free);
    let cancelled=VideoPlaybackSession(true);
    STREAMS.lock().push(Stream{id:2,session:cancelled,closed:false,sequence:0,slots:vec![Slot{state:SlotState::Writing,sequence:0}]});
    assert!(!finish_conversion(ConvertedVideoTexture{id:2,index:0,session:cancelled},true));
    assert!(STREAMS.lock()[1].slots[0].state==SlotState::Free);
}
}
'''
    return harness


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-video-presentation-') as directory:
        directory = Path(directory)
        harness = directory / 'presentation.rs'
        harness.write_text(source())
        for mode, flags in [('debug', []), ('optimized', ['-O'])]:
            binary = directory / mode
            subprocess.run(['rustup', 'run', channel, 'rustc', '--edition=2024', '--test',
                            *flags, str(harness), '-o', str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == '__main__':
    main()
