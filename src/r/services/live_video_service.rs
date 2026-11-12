//! Owner-scoped live Annex-B ingress into the existing native UI4 player.
//! Only encoded access units cross the ABI; VDBOX and GuC own decoded pixels.
use crate::ui4::VideoPlaybackSession;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

#[path = "live_video_queue.rs"]
mod queue;
use queue::{AccessUnits, BUSY, INVALID};

struct Stream {
    owner: u32,
    id: u32,
    session: VideoPlaybackSession,
    units: AccessUnits,
    playing: bool,
    ended: bool,
    closed: bool,
    error: i32,
}
static STREAMS: Mutex<Vec<Stream>> = Mutex::new(Vec::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

/// 10=open (a=fps, b=0); 11=begin AU (a=id,b=length); 12=write
/// (a=id,b=offset,input=chunk); 13=commit; 14=status; 15=close; 16=abort.
/// Status: 0 waiting, 1 decoder started, 2 ended, negative error.
pub(super) fn command(owner: u32, op: u32, a: u64, b: u64, input: &[u8], output: &mut [u8]) -> i32 {
    if !output.is_empty() {
        return INVALID;
    }
    if op == 10 {
        if !(1..=60).contains(&a) || b != 0 || !input.is_empty() {
            return INVALID;
        }
        let Some(worker) = crate::workers::pick_background_spawner() else {
            return BUSY;
        };
        let mut streams = STREAMS.lock();
        // The native video player has three shared playback slots.
        if streams.len() >= 3 {
            return BUSY;
        }
        let Ok(id) = NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            (id < i32::MAX as u32).then_some(id + 1)
        }) else {
            return BUSY;
        };
        let Some(session) = crate::ui4::reserve_shell_decoded_video_player() else {
            return BUSY;
        };
        streams.push(Stream {
            owner,
            id,
            session,
            units: AccessUnits::new(),
            playing: false,
            ended: false,
            closed: false,
            error: 0,
        });
        match playback_task(id, session, a as u16) {
            Ok(task) => worker.spawn(task),
            Err(_) => {
                streams.retain(|s| s.id != id);
                session.finish("live-video-worker-unavailable");
                return BUSY;
            }
        }
        return id as i32;
    }
    if a == 0 || a > u32::MAX as u64 {
        return INVALID;
    }
    let mut streams = STREAMS.lock();
    let Some(s) = streams
        .iter_mut()
        .find(|s| s.id == a as u32 && s.owner == owner)
    else {
        return -1;
    };
    if op == 15 && b == 0 && input.is_empty() {
        s.closed = true;
        s.units.clear();
        s.session.cancel();
        if s.ended {
            streams.retain(|s| s.id != a as u32);
        }
        return 0;
    }
    if op == 14 && b == 0 && input.is_empty() {
        return if s.error != 0 {
            s.error
        } else if s.ended || s.closed || s.session.is_cancelled() {
            2
        } else {
            s.playing as i32
        };
    }
    if s.closed || s.ended || s.session.is_cancelled() {
        return -2;
    }
    match op {
        11 if input.is_empty() && b <= queue::MAX_BYTES as u64 => s.units.begin(b as usize),
        12 if b <= queue::MAX_BYTES as u64 && input.len() <= 3072 => {
            s.units.write(b as usize, input)
        }
        13 if b == 0 && input.is_empty() => s.units.commit(),
        16 if b == 0 && input.is_empty() => {
            s.units.abort();
            0
        }
        _ => INVALID,
    }
}

pub(super) fn release_owner(owner: u32) {
    let mut streams = STREAMS.lock();
    for stream in streams.iter_mut().filter(|s| s.owner == owner) {
        stream.closed = true;
        stream.units.clear();
        stream.session.cancel();
    }
    streams.retain(|s| !(s.owner == owner && s.ended));
}

pub(crate) async fn next_access_unit(id: u32, session: VideoPlaybackSession) -> Option<Vec<u8>> {
    loop {
        if session.is_cancelled() {
            return None;
        }
        {
            let mut streams = STREAMS.lock();
            let stream = streams
                .iter_mut()
                .find(|s| s.id == id && s.session == session)?;
            if stream.closed {
                return None;
            }
            if let Some(unit) = stream.units.pop() {
                return Some(unit);
            }
        }
        Timer::after(Duration::from_millis(2)).await;
    }
}

pub(crate) fn decoder_started(id: u32) {
    if let Some(stream) = STREAMS.lock().iter_mut().find(|s| s.id == id) {
        stream.playing = true;
    }
}

#[trueos_executor::task(pool_size = 3)]
async fn playback_task(id: u32, session: VideoPlaybackSession, fps: u16) {
    let result = crate::intel::media::hw_vid::run_live_ui4_video_playback(session, id, fps).await;
    if let Err(reason) = result {
        if !session.is_cancelled() {
            crate::log_warn!(target: "service"; "live-video: stream={} failed reason={}\n", id, reason);
        }
    }
    session.finish("live-video-finished");
    let mut streams = STREAMS.lock();
    if let Some(stream) = streams.iter_mut().find(|s| s.id == id) {
        stream.ended = true;
        stream.units.clear();
        if result.is_err() && !stream.closed {
            stream.error = -2;
        }
    }
    streams.retain(|s| !(s.id == id && s.closed));
}
