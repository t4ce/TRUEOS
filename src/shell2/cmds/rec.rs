//! Shell2 microphone recording: an independent consecutive HDA PCM tap.
use crate::r::fs::trueosfs as fs;
use crate::r::services::hda_capture_lane as capture;
use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{self, MatrixTarget, ShellBackend2};
use alloc::{format, string::String, vec, vec::Vec};
use spin::Mutex;
use trueos_executor::Spawner;
use trueos_time::{Duration, Timer};

const DIRECTORY: &str = "recordings";
const CHUNK_BYTES: usize = 192 * 1024;
const COPY_BYTES: usize = 64 * 1024;
const PROGRESS_NS: u64 = 5_000_000_000;
// Classic RIFF has a 32-bit length. Finish before reaching its limit.
const MAX_PCM_BYTES: u64 = u32::MAX as u64 - 36 - 4;
struct Control {
    busy: bool,
    stop: bool,
}
static CONTROL: Mutex<Control> = Mutex::new(Control {
    busy: false,
    stop: false,
});

struct RecordRequest {
    minutes: Option<u8>,
    disk: crate::disc::block::DeviceHandle,
    path: String,
    target: MatrixTarget,
    origin: MatrixTarget,
}

pub(crate) fn try_parse(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let rest = rest.trim();
    if rest == "stop" {
        let mut control = CONTROL.lock();
        let message = if control.busy {
            control.stop = true;
            "rec: stopping and saving recording"
        } else {
            "rec: no microphone recording is running"
        };
        shell2::print_shell_line(io, message);
        return ParseOutcome::Handled;
    }
    let minutes = if rest.is_empty() {
        None
    } else {
        let value = rest.parse::<u8>().ok().filter(|v| (1..=10).contains(v));
        if !rest.bytes().all(|b| b.is_ascii_digit()) || value.is_none() {
            shell2::print_shell_line(
                io,
                "rec: usage rec [1-10] (minutes), or rec stop; omit duration to record until stopped; §rec§ stops and saves a WAV in trueosfs:/recordings",
            );
            return ParseOutcome::Handled;
        }
        value
    };
    if let Err(error) = request_recording(minutes, shell2::matrix_target_for_backend(io)) {
        shell2::print_shell_line(io, &format!("rec: {error}"));
    }
    ParseOutcome::Handled
}

fn request_recording(minutes: Option<u8>, origin: MatrixTarget) -> Result<(), &'static str> {
    let mut control = CONTROL.lock();
    if control.busy {
        return Err("microphone recording is already running; use rec stop or §rec§");
    }
    let spawner =
        crate::workers::pick_background_spawner().ok_or("no background worker is ready")?;
    let disk =
        crate::ui4::writable_capture_root_handle().ok_or("no writable TRUEOSFS root is mounted")?;
    let target = shell2::claim_matrix_target_for_named_app_slot(&origin, "rec", "rec")
        .ok_or("Matrix slot rec is occupied or the requesting shell has closed")?;
    let now = crate::chronos::monotonic_nanos();
    let wall = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
    let path = format!("{DIRECTORY}/microphone-{wall}-{now}.wav");
    let token = record_task(RecordRequest {
        minutes,
        disk,
        path: path.clone(),
        target: target.clone(),
        origin,
    })
    .map_err(|_| "microphone recording task is already running")?;
    control.busy = true;
    control.stop = false;
    shell2::set_matrix_target_active(&target, true);
    let duration = minutes
        .map(|m| format!("for {m} min"))
        .unwrap_or_else(|| String::from("until stopped"));
    shell2::print_matrix_target_line(
        &target,
        &format!("rec: armed {duration}; trueosfs:/{path}; stop and save with rec stop or §rec§"),
    );
    spawner.spawn(token);
    Ok(())
}

fn stopped(target: &MatrixTarget) -> bool {
    CONTROL.lock().stop || shell2::matrix_target_interrupted(target)
}

fn wav_header(channels: u8, bytes: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(bytes + 36).to_le_bytes());
    header[8..16].copy_from_slice(b"WAVEfmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&u16::from(channels).to_le_bytes());
    header[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    header[28..32].copy_from_slice(&(48_000u32 * u32::from(channels) * 2).to_le_bytes());
    header[32..34].copy_from_slice(&(u16::from(channels) * 2).to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&bytes.to_le_bytes());
    header
}

struct AudioSpool {
    disk: crate::disc::block::DeviceHandle,
    path: String,
    pending: Vec<u8>,
    parts: Vec<(String, fs::FileReadHandle)>,
    saved_bytes: u64,
    channels: u8,
}

impl AudioSpool {
    fn new(request: &RecordRequest, channels: u8) -> Self {
        Self {
            disk: request.disk,
            path: request.path.clone(),
            pending: Vec::new(),
            parts: Vec::new(),
            saved_bytes: 0,
            channels,
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
            return Err("no microphone samples were recorded");
        }
        let write = fs::file_write_begin_async(self.disk, &self.path, self.saved_bytes + 44)
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
        if final_file.data_len() != self.saved_bytes + 44 {
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
        fs::file_write_chunk_async(write, &wav_header(self.channels, self.saved_bytes as u32))
            .await
            .map_err(|_| "WAV header write failed")?;
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
                        &format!("rec: saving {copied}/{} bytes", self.saved_bytes,),
                    );
                    next_progress = now + PROGRESS_NS;
                }
            }
        }
        Ok(())
    }
}

#[trueos_executor::task]
async fn record_task(request: RecordRequest) {
    let result = run_recording(&request).await;
    shell2::print_matrix_target_line(&request.target, &result);
    if !shell2::matrix_targets_same_slot_lifetime(&request.target, &request.origin) {
        shell2::print_matrix_target_line(&request.origin, &result);
    }
    crate::log_info!(target: "hda/recording"; "{}\n", result);
    shell2::set_matrix_target_active(&request.target, false);
    CONTROL.lock().busy = false;
}

async fn run_recording(request: &RecordRequest) -> String {
    if !capture::ensure_started_on_current_worker() {
        return String::from("rec: cannot start microphone capture on the background worker");
    }
    if !matches!(fs::dir_create_all_async(request.disk, DIRECTORY).await, Ok(true)) {
        return String::from("rec: cannot create recordings directory");
    }
    let waiting = crate::chronos::monotonic_nanos();
    let mut cursor = loop {
        if stopped(&request.target) {
            return String::from("rec: stopped before microphone became ready; no file saved");
        }
        if let Some(cursor) = capture::recording_cursor() {
            break cursor;
        }
        if crate::chronos::monotonic_nanos().saturating_sub(waiting) >= 10_000_000_000 {
            return format!(
                "rec: microphone did not become ready within 10 seconds; state={:?}; no file saved",
                capture::status().state
            );
        }
        Timer::after(Duration::from_millis(20)).await;
    };
    let channels = cursor.channels;
    let mut spool = AudioSpool::new(request, channels);
    // One full DMA ring lets stop drain the completed tail in a single copy.
    let mut pcm = vec![0i16; 128 * 1024];
    let started = crate::chronos::monotonic_nanos();
    let deadline = request
        .minutes
        .map(|m| started + u64::from(m) * 60_000_000_000);
    let mut next_progress = started + PROGRESS_NS;
    let mut reason = "duration complete";
    let mut frames = 0u64;
    let mut peak = 0u16;
    let mut nonzero = 0u64;
    let mut samples = 0u64;
    shell2::print_matrix_target_line(
        &request.target,
        &format!("rec: recording microphone; 48000 Hz, {channels} channels, signed 16-bit PCM"),
    );
    loop {
        let now = crate::chronos::monotonic_nanos();
        let stop = stopped(&request.target);
        let done = deadline.is_some_and(|d| now >= d);
        // Drain completed audio once on stop, preserving the tail.
        let read = match capture::copy_recording_i16(&mut cursor, &mut pcm) {
            Ok(read) => read,
            Err(error) => {
                reason = error;
                break;
            }
        };
        if spool.saved_bytes + spool.pending.len() as u64 + read.samples as u64 * 2 > MAX_PCM_BYTES
        {
            reason = "WAV size limit reached";
            break;
        }
        for sample in &pcm[..read.samples] {
            peak = peak.max(sample.unsigned_abs());
            nonzero += u64::from(*sample != 0);
            spool.pending.extend_from_slice(&sample.to_le_bytes());
        }
        frames += (read.samples / usize::from(channels)) as u64;
        samples += read.samples as u64;
        if stop {
            reason = "stopped";
        }
        if stop || done {
            break;
        }
        if spool.pending.len() >= CHUNK_BYTES {
            if let Err(error) = spool.flush().await {
                reason = error;
                break;
            }
        }
        if now >= next_progress {
            shell2::print_matrix_target_line(
                &request.target,
                &format!(
                    "rec: {}s audio, {} frames; peak={} nonzero={} permille; saved={} buffered={} bytes",
                    frames / 48_000,
                    frames,
                    peak,
                    nonzero * 1000 / samples.max(1),
                    spool.saved_bytes,
                    spool.pending.len()
                ),
            );
            next_progress = now + PROGRESS_NS;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    shell2::print_matrix_target_line(&request.target, &format!("rec: {reason}; saving WAV"));
    match spool.finish(&request.target).await {
        Ok(()) => format!(
            "rec: saved trueosfs:/{}; {} frames ({} ms), {} channels, peak={} nonzero={} permille; {reason}",
            request.path,
            frames,
            frames * 1000 / 48_000,
            channels,
            peak,
            nonzero * 1000 / samples.max(1)
        ),
        Err(error) => format!(
            "rec: {reason}; {error}; retained {} PCM chunks ({} bytes) at trueosfs:/{}.part*; format=s16le-48000Hz-{}ch",
            spool.parts.len(),
            spool.saved_bytes,
            request.path,
            channels
        ),
    }
}
