//! Shell2 screen recording, serviced by the existing H.264 producer while its
//! UDP subscriber wait is idle. TRUEOSFS Put records need a known byte length:
//! persist bounded chunks, then join their pinned read handles into one Annex-B
//! file. A failed join never deletes the recovery chunks.

use alloc::{format, string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use crate::r::fs::trueosfs as fs;
use crate::shell2::{self, MatrixTarget};
use crate::ui4::h264_capture_session::CaptureSessionGate;

const DIRECTORY: &str = "screenfilms";
const CHUNK_BYTES: usize = 16 * 1024 * 1024;
const COPY_BYTES: usize = 1024 * 1024;
const PROGRESS_NS: u64 = 5_000_000_000;
static ENCODER_READY: AtomicBool = AtomicBool::new(false);
static SEQUENCE: AtomicU32 = AtomicU32::new(1);
static ADMISSION: Mutex<CaptureSessionGate> = Mutex::new(CaptureSessionGate::new());
static REQUEST: Mutex<Option<FilmRequest>> = Mutex::new(None);

struct FilmRequest {
    minutes: u8,
    session: u32,
    disk: crate::disc::block::DeviceHandle,
    path: String,
    target: MatrixTarget,
    origin: MatrixTarget,
}

pub(super) fn set_encoder_ready() {
    ENCODER_READY.store(true, Ordering::Release);
}

pub(crate) fn request_film(minutes: u8, origin: MatrixTarget) -> Result<(), &'static str> {
    if !(1..=10).contains(&minutes) {
        return Err("duration must be an integer from 1 to 10 minutes");
    }
    if !ENCODER_READY.load(Ordering::Acquire) {
        return Err("hardware encoder is not ready");
    }
    let disk = crate::ui4::screenshot::writable_capture_root_handle()
        .ok_or("no writable TRUEOSFS root is mounted")?;
    let now = crate::chronos::monotonic_nanos();
    let mut admission = ADMISSION.lock();
    admission.claim_film(now)?;
    if !crate::intel::media::wd_xyuv8888::try_reserve_stream_capture() {
        admission.finish_film(true);
        return Err("WD capture is busy; retry after the current capture finishes");
    }
    let Some(target) = shell2::claim_matrix_target_for_named_app_slot(&origin, "film", "film")
    else {
        crate::intel::media::wd_xyuv8888::release_stream_capture();
        admission.finish_film(true);
        return Err("Matrix slot film is occupied or the requesting shell has closed");
    };
    let session = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let wall = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
    let path = format!("{DIRECTORY}/wd-postblend-{wall}-{}-film{session:06}.h264", now / 1_000_000);
    shell2::set_matrix_target_active(&target, true);
    shell2::print_matrix_target_line(
        &target,
        &format!(
            "film: armed for {minutes} min; {}x{} at {} fps; trueosfs:/{path}; stop and save with §film§",
            super::ENCODE_WIDTH,
            super::ENCODE_HEIGHT,
            crate::allcaps::media_encode::REALTIME_HZ,
        ),
    );
    *REQUEST.lock() = Some(FilmRequest {
        minutes,
        session,
        disk,
        path,
        target,
        origin,
    });
    drop(admission);
    crate::ui4::h264_encode_udp::wake_producer();
    Ok(())
}

pub(in crate::ui4) struct RdpViewLease;

pub(in crate::ui4) fn claim_rdp_view() -> Option<RdpViewLease> {
    let claimed = ADMISSION.lock().claim_view();
    if claimed { Some(RdpViewLease) } else { None }
}

impl Drop for RdpViewLease {
    fn drop(&mut self) {
        ADMISSION
            .lock()
            .finish_view(crate::chronos::monotonic_nanos());
    }
}

pub(in crate::ui4) async fn run_pending() {
    let request = REQUEST.lock().take();
    let Some(request) = request else {
        return;
    };
    run_recording(request).await;
}

struct FilmSpool {
    disk: crate::disc::block::DeviceHandle,
    path: String,
    pending: Vec<u8>,
    parts: Vec<(String, fs::FileReadHandle)>,
    saved_bytes: u64,
}

impl FilmSpool {
    fn new(request: &FilmRequest) -> Self {
        Self {
            disk: request.disk,
            path: request.path.clone(),
            pending: Vec::new(),
            parts: Vec::new(),
            saved_bytes: 0,
        }
    }

    async fn flush(&mut self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let path = format!("{}.part{:06}", self.path, self.parts.len());
        if !fs::file_in_async(self.disk, &path, &self.pending)
            .await
            .map_err(|_| "chunk write failed")?
        {
            return Err("no space for recording chunk");
        }
        // Pin this exact committed record, not a later path lookup that might
        // accidentally substitute an older or replaced file during the join.
        let handle = fs::file_read_open_async(self.disk, &path)
            .await
            .map_err(|_| "cannot verify recording chunk")?
            .filter(|handle| handle.data_len() == self.pending.len() as u64)
            .ok_or("recording chunk length verification failed")?;
        self.saved_bytes += handle.data_len();
        self.parts.push((path, handle));
        self.pending.clear();
        Ok(())
    }

    async fn finish(&mut self, target: &MatrixTarget) -> Result<(), &'static str> {
        self.flush().await?;
        if self.saved_bytes == 0 {
            return Err("no frames were recorded");
        }
        let write = fs::file_write_begin_async(self.disk, &self.path, self.saved_bytes)
            .await
            .map_err(|_| "cannot open final recording")?
            .ok_or("no space for final recording; recovery chunks retained")?;
        let result = self.copy_parts(write, target).await;
        if result.is_err() {
            let _ = fs::file_write_abort_async(write).await;
            return result;
        }
        fs::file_write_finish_async(write)
            .await
            .map_err(|_| "final recording commit failed")?;
        let final_file = fs::file_read_open_async(self.disk, &self.path)
            .await
            .map_err(|_| "cannot verify final recording")?
            .ok_or("final recording is missing")?;
        if final_file.data_len() != self.saved_bytes {
            return Err("final recording length verification failed");
        }
        // Only the verified final file replaces the temporary parts. Slot
        // retirement never enters a delete/abort path for recorded frames.
        for (path, _) in &self.parts {
            let _ = fs::file_delete_async(self.disk, path).await;
        }
        Ok(())
    }

    async fn copy_parts(&self, write: u32, target: &MatrixTarget) -> Result<(), &'static str> {
        let mut buffer = vec![0; COPY_BYTES];
        let mut copied = 0u64;
        let mut next_progress = crate::chronos::monotonic_nanos() + PROGRESS_NS;
        for (_, handle) in &self.parts {
            let mut offset = 0;
            while offset < handle.data_len() {
                let count = (handle.data_len() - offset).min(buffer.len() as u64) as usize;
                let read = fs::file_read_handle_range_async(*handle, offset, &mut buffer[..count])
                    .await
                    .map_err(|_| "recording chunk read failed")?;
                if read != Some(count) {
                    return Err("recording chunk is incomplete");
                }
                fs::file_write_chunk_async(write, &buffer[..count])
                    .await
                    .map_err(|_| "final recording write failed")?;
                offset += count as u64;
                copied += count as u64;
                let now = crate::chronos::monotonic_nanos();
                if now >= next_progress {
                    shell2::print_matrix_target_line(
                        target,
                        &format!("film: saving {copied}/{} bytes", self.saved_bytes,),
                    );
                    next_progress = now + PROGRESS_NS;
                }
            }
        }
        Ok(())
    }
}

async fn run_recording(request: FilmRequest) {
    let mut spool = FilmSpool::new(&request);
    let mut stats = super::LiveEncodeStats::default();
    let mut reason = "duration complete";
    let mut started_capture = false;
    let started = crate::chronos::monotonic_nanos();
    let deadline = started + u64::from(request.minutes) * 60_000_000_000;
    let mut next_progress = started + PROGRESS_NS;
    let mut next_flush = next_progress;
    let mut next_frame = Instant::now();
    let mut cadence = crate::ui4::h264_encode_udp::FractionalCadenceTicks::new(
        trueos_time::TICK_HZ,
        crate::allcaps::media_encode::REALTIME_HZ,
    );
    let mut sequence = 0;

    if shell2::matrix_target_interrupted(&request.target) {
        reason = "stopped before first frame";
    } else if !matches!(fs::dir_create_all_async(request.disk, DIRECTORY).await, Ok(true)) {
        reason = "cannot create screenfilms directory";
    } else {
        super::begin_preparation_session(request.session, usize::MAX);
        started_capture = true;
        loop {
            if shell2::matrix_target_interrupted(&request.target) {
                reason = "stopped by Matrix slot";
                break;
            }
            let now = crate::chronos::monotonic_nanos();
            if now >= deadline {
                break;
            }
            if now >= next_progress {
                progress(&request, &stats, &spool, started, now);
                next_progress = now + PROGRESS_NS;
            }
            if now >= next_flush || spool.pending.len() >= CHUNK_BYTES {
                if let Err(error) = spool.flush().await {
                    reason = error;
                    break;
                }
                next_flush = crate::chronos::monotonic_nanos() + PROGRESS_NS;
            }
            if !super::prepared_scanout_ready(request.session, sequence)
                || Instant::now() < next_frame
            {
                Timer::after(Duration::from_millis(1)).await;
                continue;
            }
            // Measure the next deadline from this frame's start, not from
            // encode completion (which would add encode time to every period).
            next_frame = next_frame.max(Instant::now());
            next_frame += Duration::from_ticks(cadence.next());
            let Some(bytes) =
                super::encode_prepared_scanout(request.session, sequence, &mut stats).await
            else {
                reason = "hardware capture or encode failed";
                break;
            };
            if bytes.len() > crate::allcaps::media_encode::STREAM_MAX_ACCESS_UNIT_BYTES {
                reason = "encoded frame exceeded recording limit";
                break;
            }
            spool.pending.extend_from_slice(&bytes);
            sequence += 1;
            // Preserve fractional cadence without a burst after a delayed disk
            // write. No frame is dropped from the encoder reference chain.
        }
    }

    let retired = if started_capture {
        super::end_preparation_session(request.session).await
    } else {
        crate::intel::media::wd_xyuv8888::release_stream_capture();
        true
    };
    let finished = crate::chronos::monotonic_nanos();
    progress(&request, &stats, &spool, started, finished);
    shell2::print_matrix_target_line(&request.target, &format!("film: {reason}; saving recording"));
    let saved = spool.finish(&request.target).await;
    let result = match saved {
        Ok(()) => format!(
            "film: saved trueosfs:/{}; {} frames, {} bytes; {reason}",
            request.path, sequence, spool.saved_bytes
        ),
        Err(error) => format!(
            "film: {reason}; {error}; retained {} chunks ({} bytes) at trueosfs:/{}.part*",
            spool.parts.len(),
            spool.saved_bytes,
            request.path
        ),
    };
    shell2::print_matrix_target_line(&request.target, &result);
    if !shell2::matrix_targets_same_slot_lifetime(&request.target, &request.origin) {
        shell2::print_matrix_target_line(&request.origin, &result);
    }
    crate::log_info!(target: "intel/media-encode"; "{}\n", result);
    if !retired {
        ENCODER_READY.store(false, Ordering::Release);
        shell2::print_matrix_target_line(
            &request.target,
            "film: capture hardware retained because retirement could not be confirmed",
        );
    }
    shell2::set_matrix_target_active(&request.target, false);
    ADMISSION.lock().finish_film(retired);
}

fn progress(
    request: &FilmRequest,
    stats: &super::LiveEncodeStats,
    spool: &FilmSpool,
    started: u64,
    now: u64,
) {
    let elapsed_ms = now.saturating_sub(started) / 1_000_000;
    let fps_milli = stats.frames as u64 * 1_000_000 / elapsed_ms.max(1);
    shell2::print_matrix_target_line(
        &request.target,
        &format!(
            "film: {}s/{}s frames={} fps={}.{:03} saved={} bytes buffered={} bytes capture_avg={}us encode_avg={}us",
            elapsed_ms / 1_000,
            u64::from(request.minutes) * 60,
            stats.frames,
            fps_milli / 1_000,
            fps_milli % 1_000,
            spool.saved_bytes,
            spool.pending.len(),
            super::average_u64(stats.capture_us, stats.frames),
            super::average_u64(stats.encode_us, stats.frames),
        ),
    );
}
