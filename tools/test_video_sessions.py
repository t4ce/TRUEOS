#!/usr/bin/env python3
"""Host checks of production playback admission, cancellation and queue ownership."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant
from test_rdp_pipeline import block

ENGINE = 'src/intel/media/engine.rs'
VIDEO = 'src/ui4/video_frame.rs'


def main():
    src = r'''
#![allow(dead_code, unused_variables, private_interfaces)]
use std::{collections::VecDeque, sync::atomic::{AtomicBool, AtomicU64, Ordering}};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[macro_export] macro_rules! log_info { ($($x:tt)*) => {}; }
mod media {
use super::*;
const MAX_MEDIA_ENGINES: usize = 1;
#[derive(Copy, Clone)] struct EngineId { instance: usize }
#[derive(Copy, Clone)] struct MediaEngineDescriptor { id: EngineId, name: &'static str }
fn default_decode_engine_and_window() -> (MediaEngineDescriptor, ()) {
    (MediaEngineDescriptor { id: EngineId { instance: 0 }, name: "test-vdbox" }, ())
}
'''
    for name in ['MediaCodecMode', 'MediaSubmissionOwner', 'MediaBatchLevel', 'MediaJobMode',
                 'MediaActiveJob', 'MediaSessionReservation', 'MediaExecutionState',
                 'MediaLaneAcquireError', 'MediaLaneGuard', 'MediaSessionGuard',
                 'try_reserve_avc_decode_session', 'avc_decode_session_slot', 'try_acquire_media_lane']:
        src += item(ENGINE, name)
    for signature in ['impl MediaJobMode {', 'impl MediaExecutionState {', 'impl MediaLaneGuard {',
                      'impl Drop for MediaLaneGuard {', 'impl MediaSessionGuard {',
                      'impl Drop for MediaSessionGuard {']:
        src += block(ENGINE, signature)
    src += r'''
static MEDIA_ENGINE_EXECUTION: Mutex<[MediaExecutionState; 1]> = Mutex::new([MediaExecutionState::EMPTY]);
#[test] fn three_sessions_serialize_jobs_and_reject_stale_generations() {
    let sessions: Vec<_> = (0..3).map(|_| try_reserve_avc_decode_session().unwrap()).collect();
    assert!(matches!(try_reserve_avc_decode_session(), Err(MediaLaneAcquireError::Busy)));
    assert_eq!(avc_decode_session_slot(None), None);
    let (engine, _) = default_decode_engine_and_window();
    for current in 0..3 {
        assert_eq!(avc_decode_session_slot(Some(sessions[current].generation())), Some(current));
        let mut lane = try_acquire_media_lane(engine, MediaJobMode::AVC_DECODE_GUC,
            Some(sessions[current].generation())).unwrap();
        assert!(matches!(try_acquire_media_lane(engine, MediaJobMode::AVC_DECODE_GUC,
            Some(sessions[(current + 1) % 3].generation())), Err(MediaLaneAcquireError::Busy)));
        lane.complete(); drop(lane);
    }
    let mut sessions = sessions;
    let old = sessions.remove(1); let stale = old.generation(); drop(old);
    let replacement = try_reserve_avc_decode_session().unwrap();
    assert_eq!(avc_decode_session_slot(Some(replacement.generation())), Some(1));
    assert_eq!(avc_decode_session_slot(Some(stale)), None);
    assert!(matches!(try_acquire_media_lane(engine, MediaJobMode::AVC_DECODE_GUC,
        Some(stale)), Err(MediaLaneAcquireError::Busy)));
    drop(replacement); drop(sessions);
    assert_eq!(avc_decode_session_slot(None), Some(0));
}
}
fn video_frame_extent_admitted(w: u32, h: u32) -> bool { w != 0 && h != 0 }
struct WindowPlane;
impl WindowPlane { #[allow(non_snake_case)] fn Universal(_: u8) -> Self { Self } }
struct WindowInteraction;
impl WindowInteraction { const APPLICATION_FIXED_FRAME: Self = Self; }
#[derive(Copy, Clone)] struct Ui4CursorSource;
mod intel { pub fn active_scanout_dimensions() -> Option<(u32, u32)> { Some((2560,1440)) } }
mod video {
use super::*;
#[derive(Copy, Clone, PartialEq, Eq)] struct WindowOwner(u64);
#[derive(Copy, Clone, PartialEq, Eq)] struct WindowId(u64);
const VIDEO_OWNER: WindowOwner = WindowOwner(2);
type WindowSessionId = u64;
type FrameHandle = u64;
const VIDEO_OUTPUT: u64 = 0;
const VIDEO_PLANE_SLOT: usize = 1;
struct DecodedVideoFrameSpec { coded_width: u32, coded_height: u32, visible_width: u32, visible_height: u32 }
impl DecodedVideoFrameSpec { fn valid(&self) -> bool { self.coded_width > 0 && self.coded_height > 0 } }
#[derive(Copy, Clone)] struct WindowPlacement { x:i32,y:i32,width:u32,height:u32,z:i32,opacity:u8,visible:bool }
struct WindowCreate { owner:WindowOwner,session:u64,frame:u64,output:u64,plane:WindowPlane,placement:WindowPlacement,interaction:WindowInteraction }
static BROKER_WINDOWS: Mutex<Vec<(u64, WindowId)>> = Mutex::new(Vec::new());
static BROKER_NEXT: AtomicU64 = AtomicU64::new(1);
fn begin_window_session(_: WindowOwner) -> Result<u64, ()> {
    BROKER_WINDOWS.lock().clear(); // The broker's documented replace-owner contract.
    Ok(BROKER_NEXT.fetch_add(1, Ordering::Relaxed))
}
fn begin_additional_window_session(_: WindowOwner) -> Result<u64, ()> { Ok(BROKER_NEXT.fetch_add(1, Ordering::Relaxed)) }
fn create_video_frame(_:u32,_:u32) -> Result<u64, ()> { Ok(1) }
fn destroy_frame(_:u64) -> Result<(), ()> { Ok(()) }
fn create_window(request:WindowCreate) -> Result<WindowId, ()> {
    let id = WindowId(request.session); BROKER_WINDOWS.lock().push((request.session,id)); Ok(id)
}
fn centered_crop_origin(extent:u32, viewport:u32) -> u32 { extent.saturating_sub(viewport)/2 }
fn native_viewport_layout(_:u32,_:u32,_:u32,_:u32,_:u32,_:u32) -> Option<()> { Some(()) }
#[derive(Default)] struct WindowSessionCloseRequest;
impl WindowSessionCloseRequest { fn direct_plane_animate_and_retire_frames(self) -> Self { self } }
fn finish_window_session_with_request(_: WindowOwner, session: u64, _: WindowSessionCloseRequest) -> Result<(), ()> { BROKER_WINDOWS.lock().retain(|(s,_)| *s != session); Ok(()) }
fn finish_window_session(_: WindowOwner, _: u64) -> Result<(), ()> { Ok(()) }
fn retire_video_frame(_: u64) {}
#[derive(Copy, Clone)] struct DecodedNv12Source;
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)] struct DecodedVideoConversionProbeReport;
struct DecodedVideoConversionProbeState;
impl DecodedVideoConversionProbeState {
    const fn new() -> Self { Self }
    fn report(&self) -> DecodedVideoConversionProbeReport { DecodedVideoConversionProbeReport }
    fn record(&mut self, _: ()) {}
}
struct DecodedVideoConversionOutcome { published: bool, probe: () }
fn video_conversion_ticks_to_micros(ticks: u64) -> u64 { ticks }
'''
    for name in ['VIDEO_PLAYBACK_SESSIONS', 'VIDEO_CONVERSION_PRESENT_ERROR']:
        src += constant(VIDEO, name) + '\n'
    for name in ['VideoStream', 'create_stream', 'VideoPlaybackSession', 'VideoPlaybackState', 'DecodedVideoConversionState',
                 'DecodedVideoConversionReport', 'DecodedVideoConversionRequest',
                 'decoded_video_window_closed', 'decoded_video_conversion_idle',
                 'begin_shell_decoded_video_player', 'stop_decoded_nv12_stream',
                 'take_decoded_video_conversion_request', 'complete_decoded_video_conversion']:
        src += item(VIDEO, name)
    for signature in ['impl VideoPlaybackSession {', 'impl VideoPlaybackState {',
                      'impl DecodedVideoConversionState {']:
        src += block(VIDEO, signature)
    src += r'''
static VIDEO_SESSIONS: [VideoPlaybackState; 3] = [const { VideoPlaybackState::new() }; 3];
static VIDEO_CONVERSION_NEXT_SESSION: AtomicU64 = AtomicU64::new(0);
fn queue(session: VideoPlaybackSession, count: usize) {
    let mut state = session.state().conversion.lock();
    assert!(state.reset_batch());
    for order in 1..=count {
        let generation = state.generation;
        state.queue.push_back(DecodedVideoConversionRequest { session, generation, order,
            playback_frame: order, enqueued_tick: 0, source: DecodedNv12Source });
        state.queued += 1;
    }
}
#[test] fn close_is_local_and_slot_reuse_waits_for_queue_and_active_work() {
    let a = begin_shell_decoded_video_player(768, 512).unwrap();
    let b = begin_shell_decoded_video_player(768, 512).unwrap();
    let c = begin_shell_decoded_video_player(768, 512).unwrap();
    assert!(begin_shell_decoded_video_player(768, 512).is_none());
    assert_eq!(BROKER_WINDOWS.lock().len(), 3, "new video replaced a live sibling window");
    queue(a, 2); queue(b, 2); queue(c, 2);
    let a1 = take_decoded_video_conversion_request().unwrap();
    let b1 = take_decoded_video_conversion_request().unwrap();
    let c1 = take_decoded_video_conversion_request().unwrap();
    assert_eq!([a1.session.slot, b1.session.slot, c1.session.slot], [0, 1, 2]);
    let closed_window = a.state().stream.lock().unwrap().window;
    decoded_video_window_closed(VIDEO_OWNER, closed_window);
    assert!(a.is_cancelled()); assert!(!b.is_cancelled()); assert!(!c.is_cancelled());
    a.finish("test-active");
    assert_eq!(BROKER_WINDOWS.lock().len(), 2);
    assert!(a.state().occupied.load(Ordering::Acquire));
    complete_decoded_video_conversion(a1, DecodedVideoConversionOutcome { published: false, probe: () }, 1);
    a.finish("test-queued");
    assert!(a.state().occupied.load(Ordering::Acquire));
    assert!(begin_shell_decoded_video_player(768, 512).is_none());
    for first in [b1, c1] {
        complete_decoded_video_conversion(first, DecodedVideoConversionOutcome { published: true, probe: () }, 1);
    }
    for _ in 0..3 {
        let request = take_decoded_video_conversion_request().unwrap();
        assert_eq!(request.order, 2);
        complete_decoded_video_conversion(request, DecodedVideoConversionOutcome { published: !request.session.is_cancelled(), probe: () }, 1);
    }
    assert_eq!(a.state().conversion.lock().first_failure_frame, 0);
    assert_eq!(b.state().conversion.lock().published, 2);
    a.finish("test-drained");
    let replacement = begin_shell_decoded_video_player(768, 512).unwrap();
    assert_eq!(replacement.slot, a.slot); assert_ne!(replacement.generation, a.generation);
    decoded_video_window_closed(VIDEO_OWNER, closed_window);
    a.finish("stale-owner");
    assert!(!replacement.is_cancelled()); assert!(!b.is_cancelled()); assert!(!c.is_cancelled());
    replacement.finish("done"); b.finish("done"); c.finish("done");
}
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-video-sessions-') as tmp:
        path = Path(tmp) / 'test.rs'
        path.write_text(src)
        binary = Path(tmp) / 'test'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
