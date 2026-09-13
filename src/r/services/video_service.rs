//! Owner-scoped video streams. Encoded bytes cross the V boundary once;
//! decoder pictures and the three persistent RGBA textures stay in the kernel.
use crate::gpu::vgpu::{self, DeviceHandle, Principal, RetainedTextureHandle};
use crate::ui4::{DecodedNv12Source, VideoPlaybackSession};
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

const MAX_ENCODED: usize = 64 * 1024 * 1024;
const RING: usize = 3;
const INVALID: i32 = -3;
const BUSY: i32 = -4;
const FAILED: i32 = -2;
static NEXT: AtomicU32 = AtomicU32::new(1);
static STREAMS: Mutex<Vec<Stream>> = Mutex::new(Vec::new());
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Free,
    Writing,
    Ready,
    Reading,
}
struct Slot {
    texture: RetainedTextureHandle,
    state: SlotState,
    sequence: u64,
}
struct Stream {
    id: u32,
    owner: u32,
    device: u64,
    session: VideoPlaybackSession,
    bytes: Vec<u8>,
    received: usize,
    queued: bool,
    running: bool,
    looping: bool,
    closed: bool,
    ended: bool,
    error: i32,
    width: u32,
    height: u32,
    sequence: u64,
    slots: Vec<Slot>,
}
fn principal(owner: u32) -> Principal {
    if owner & 0x8000_0000 != 0 {
        Principal::HullGuest((owner & 0xffff) as u16)
    } else {
        Principal::HostRuntime
    }
}
/// V1 command protocol: begin/write/commit/acquire/release/close. All integers
/// are little endian. Frame payload is texture:u64, sequence:u64, width:u32,
/// height:u32. Acquire: 0 pending, 1 leased frame, 2 end, negative error.
pub fn command(owner: u32, command: u32, a: u64, b: u64, input: &[u8], output: &mut [u8]) -> i32 {
    if command == 0 {
        if input.len() != 1 || input[0] > 1 || b == 0 || b > MAX_ENCODED as u64 {
            return INVALID;
        }
        // Validate owner/device before occupying a global decoder slot.
        if let Err(e) = vgpu::device_info(principal(owner), DeviceHandle::from_raw(a)) {
            return e.errno();
        }
        let Ok(id) = NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            (id < i32::MAX as u32).then_some(id + 1)
        }) else {
            return BUSY;
        };
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(b as usize).is_err() {
            return FAILED;
        }
        bytes.resize(b as usize, 0);
        let Some(session) = crate::ui4::begin_texture_video_player(id) else {
            return BUSY;
        };
        STREAMS.lock().push(Stream {
            id,
            owner,
            device: a,
            session,
            bytes,
            received: 0,
            queued: false,
            running: false,
            looping: input[0] != 0,
            closed: false,
            ended: false,
            error: 0,
            width: 0,
            height: 0,
            sequence: 0,
            slots: Vec::new(),
        });
        return id as i32;
    }
    if a == 0 || a > u32::MAX as u64 {
        return INVALID;
    }
    let mut streams = STREAMS.lock();
    let Some(s) = streams
        .iter_mut()
        .find(|s| s.owner == owner && s.id == a as u32)
    else {
        return -1;
    };
    match command {
        1 if !s.closed && !s.running && !s.queued && !input.is_empty() => {
            if b != s.received as u64 || input.len() > s.bytes.len() - s.received {
                return INVALID;
            }
            s.bytes[s.received..s.received + input.len()].copy_from_slice(input);
            s.received += input.len();
            0
        }
        2 if input.is_empty() && b == 0 && !s.closed && !s.running && !s.queued => {
            if s.received != s.bytes.len() {
                return INVALID;
            }
            s.queued = true;
            0
        }
        3 if input.is_empty() && b == 0 && output.len() == 24 && !s.closed => {
            if let Some(slot) = s
                .slots
                .iter_mut()
                .filter(|x| x.state == SlotState::Ready)
                .min_by_key(|x| x.sequence)
            {
                slot.state = SlotState::Reading;
                output[..8].copy_from_slice(&slot.texture.raw().to_le_bytes());
                output[8..16].copy_from_slice(&slot.sequence.to_le_bytes());
                output[16..20].copy_from_slice(&s.width.to_le_bytes());
                output[20..24].copy_from_slice(&s.height.to_le_bytes());
                1
            } else if s.error != 0 {
                s.error
            } else if s.ended {
                2
            } else {
                0
            }
        }
        4 if input.is_empty() => {
            let Some(slot) = s
                .slots
                .iter_mut()
                .find(|x| x.sequence == b && x.state == SlotState::Reading)
            else {
                return INVALID;
            };
            slot.state = SlotState::Free;
            0
        }
        5 if input.is_empty() && b == 0 => {
            s.closed = true;
            s.session.cancel();
            0
        }
        _ => INVALID,
    }
}
pub fn release_owner(owner: u32) {
    for s in STREAMS.lock().iter_mut().filter(|s| s.owner == owner) {
        s.closed = true;
        s.session.cancel();
        // VM teardown eliminates logical consumers; GPU reader leases remain
        // authoritative and prevent cleanup from unmapping unfinished draws.
        for slot in &mut s.slots {
            if slot.state == SlotState::Reading {
                slot.state = SlotState::Free;
            }
        }
    }
}
/// Called by the shared lane scheduler before dequeueing, so a stalled
/// consumer cannot starve another texture stream or a shell video window.
pub fn conversion_ready(id: u32) -> bool {
    let streams = STREAMS.lock();
    streams.iter().find(|s| s.id == id).is_none_or(|s| {
        s.closed || s.slots.is_empty() || s.slots.iter().any(|x| x.state == SlotState::Free)
    })
}
async fn pause() {
    Timer::after(Duration::from_millis(1)).await;
}

/// Called in PTS order by the existing conversion lanes. Source release remains
/// the caller's responsibility, and happens only after this future completes.
pub async fn convert(id: u32, session: VideoPlaybackSession, source: DecodedNv12Source) -> bool {
    let (owner, device, needs_storage) = {
        let streams = STREAMS.lock();
        let Some(s) = streams
            .iter()
            .find(|s| s.id == id && s.session == session && !s.closed)
        else {
            return false;
        };
        if s.width != 0 && (s.width != source.visible_width || s.height != source.visible_height) {
            return false;
        }
        (s.owner, s.device, s.slots.is_empty())
    };
    let width = source.visible_width;
    let height = source.visible_height;
    if !crate::ui4::video_frame_extent_admitted(width, height) {
        return false;
    }
    if needs_storage {
        let zeros = vec![0; width as usize * height as usize * 4];
        for _ in 0..RING {
            let texture = loop {
                if session.is_cancelled() {
                    return false;
                }
                match vgpu::create_retained_texture(
                    principal(owner),
                    DeviceHandle::from_raw(device),
                    width,
                    height,
                    width * 4,
                    &zeros,
                ) {
                    Ok(t) => break t,
                    Err(vgpu::VgpuError::Busy | vgpu::VgpuError::NotComplete) => pause().await,
                    Err(_) => return false,
                }
            };
            let mut streams = STREAMS.lock();
            let s = streams
                .iter_mut()
                .find(|s| s.id == id)
                .expect("producer owns stream until drain");
            s.width = width;
            s.height = height;
            s.slots.push(Slot {
                texture,
                state: SlotState::Free,
                sequence: 0,
            });
        }
    }
    let (index, texture) = loop {
        if session.is_cancelled() {
            return false;
        }
        let slot = {
            let mut streams = STREAMS.lock();
            let Some(s) = streams.iter_mut().find(|s| s.id == id && !s.closed) else {
                return false;
            };
            s.slots
                .iter_mut()
                .enumerate()
                .find(|(_, x)| x.state == SlotState::Free)
                .map(|(i, x)| {
                    x.state = SlotState::Writing;
                    (i, x.texture)
                })
        };
        if let Some(slot) = slot {
            break slot;
        }
        pause().await;
    };
    let result = convert_into(owner, device, texture, session, source).await;
    let mut streams = STREAMS.lock();
    let s = streams
        .iter_mut()
        .find(|s| s.id == id)
        .expect("producer owns stream until drain");
    let published = result && !s.closed && !session.is_cancelled();
    if published {
        s.sequence += 1;
        s.slots[index].sequence = s.sequence;
        s.slots[index].state = SlotState::Ready;
    } else {
        s.slots[index].state = SlotState::Free;
    }
    published
}
async fn convert_into(
    owner: u32,
    device: u64,
    texture: RetainedTextureHandle,
    session: VideoPlaybackSession,
    source: DecodedNv12Source,
) -> bool {
    let write = loop {
        if session.is_cancelled() {
            return false;
        }
        match vgpu::acquire_retained_texture_write(
            principal(owner),
            DeviceHandle::from_raw(device),
            texture,
        ) {
            Ok(write) => break write,
            Err(vgpu::VgpuError::Busy) => pause().await,
            Err(_) => return false,
        }
    };
    let (Ok(pitch), Ok(uv_offset)) =
        (u32::try_from(source.pitch_bytes), u32::try_from(source.uv_offset))
    else {
        return false;
    };
    let Some(native) = crate::intel::gpgpu::GpgpuNv12Tile64Surface::new(
        source.phys,
        source.gpu,
        source.byte_len,
        source.width,
        source.height,
        pitch,
        uv_offset,
    ) else {
        return false;
    };
    let destination = write.surface();
    let submission = loop {
        if session.is_cancelled() {
            return false;
        }
        match crate::intel::gpgpu::queue_ui4_video_frame_nv12_tile64_to_rgba8(
            native,
            destination,
            0,
            0,
            source.visible_width,
            source.visible_height,
            0,
            0,
            source.video_full_range,
            source.matrix_coefficients,
        ) {
            Ok(submission) => break submission,
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => pause().await,
            Err(_) => return false,
        }
    };
    let mut logged = false;
    loop {
        match crate::intel::gpgpu::poll_ui4_video_frame_submission(submission, destination) {
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Complete { .. } => return true,
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Pending => pause().await,
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Failed => {
                if !logged {
                    crate::log_error!(target: "service";
                    "vmedia-video: completion unproven texture={} action=quarantine-source-and-destination\n", texture.raw());
                    logged = true;
                }
                // Same failure rule as UI4: keep both allocations pinned.
                pause().await;
            }
        }
    }
}

fn reap() {
    let mut finished = Vec::new();
    let mut streams = STREAMS.lock();
    streams.retain_mut(|s| {
        if !s.closed || s.running {
            return true;
        }
        s.slots.retain(|slot| {
            slot.state == SlotState::Reading
                || vgpu::destroy_retained_texture(
                    principal(s.owner),
                    DeviceHandle::from_raw(s.device),
                    slot.texture,
                )
                .is_err_and(|e| matches!(e, vgpu::VgpuError::Busy | vgpu::VgpuError::NotComplete))
        });
        if !s.slots.is_empty() {
            return true;
        }
        finished.push(s.session);
        false
    });
    drop(streams);
    for session in finished {
        session.finish("texture-stream-closed");
    }
}
#[trueos_executor::task(pool_size = 3)]
pub async fn worker_task() {
    loop {
        reap();
        let request = {
            let mut streams = STREAMS.lock();
            streams
                .iter_mut()
                .find(|s| s.queued && !s.running && !s.closed)
                .map(|s| {
                    s.queued = false;
                    s.running = true;
                    (s.id, s.session, s.looping, core::mem::take(&mut s.bytes))
                })
        };
        let Some((id, session, looping, bytes)) = request else {
            Timer::after(Duration::from_millis(5)).await;
            continue;
        };
        loop {
            if session.is_cancelled() {
                break;
            }
            let result = crate::intel::media::hw_vid::run_memory_texture_video_playback(
                session,
                bytes.clone(),
            )
            .await;
            if let Err(reason) = result {
                if !session.is_cancelled() {
                    crate::log_warn!(target: "service"; "vmedia-video: stream={} failed reason={}\n", id, reason);
                    if let Some(s) = STREAMS.lock().iter_mut().find(|s| s.id == id) {
                        s.error = FAILED;
                    }
                }
                break;
            }
            if !looping {
                break;
            }
        }
        let mut streams = STREAMS.lock();
        if let Some(s) = streams.iter_mut().find(|s| s.id == id) {
            s.running = false;
            s.ended = true;
            if session.is_cancelled() {
                s.closed = true;
            }
        }
    }
}
