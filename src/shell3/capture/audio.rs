use super::{Recording, interrupted};
use crate::r::fs::trueosfs as fs;
use crate::r::services::audio_http::{append_wav_header, rtp_mic};
use crate::shell3::MatrixTarget;
use alloc::{format, string::String, vec, vec::Vec};
use trueos_time::{Duration, Timer};

const DIRECTORY: &str = "recordings";
const PART_BYTES: usize = 1024 * 1024;
const COPY_BYTES: usize = 64 * 1024;
const DRAIN_FRAMES: usize = 4_800;
const SOURCE_WAIT_NS: u64 = 10_000_000_000;
const END_GRACE_NS: u64 = 200_000_000;
const CHANNELS: u8 = 2;
const FRAME_BYTES: usize = CHANNELS as usize * 2;

pub(super) fn source_status() -> &'static str {
    if rtp_mic::status().is_some() {
        "ESP32 Wi-Fi mic is receiving audio."
    } else {
        "Waiting for ESP32 Wi-Fi mic audio."
    }
}

pub(super) fn request(target: MatrixTarget, recording: Recording) -> Result<(), &'static str> {
    let spawner =
        crate::workers::pick_background_spawner().ok_or("No recording worker available.")?;
    let task =
        record_task(target, recording).map_err(|_| "A microphone recording is already open.")?;
    spawner.spawn(task);
    Ok(())
}

#[trueos_executor::task]
async fn record_task(target: MatrixTarget, recording: Recording) {
    let (message, saved) = match record(&target, &recording).await {
        Ok(message) => (message, true),
        Err(error) => (error, false),
    };
    recording.finish(&message, saved);
}

async fn record(target: &MatrixTarget, recording: &Recording) -> Result<String, String> {
    if !matches!(
        fs::dir_create_all_async(recording.disk, DIRECTORY).await,
        Ok(true)
    ) {
        return Err("Cannot create the recordings directory.".into());
    }
    let timeout = crate::chronos::monotonic_nanos() + SOURCE_WAIT_NS;
    while rtp_mic::status().is_none() {
        if stopped(target, recording) {
            return Err("Recording stopped before the ESP32 sent audio.".into());
        }
        if crate::chronos::monotonic_nanos() >= timeout {
            return Err("No ESP32 Wi-Fi mic audio arrived within 10 seconds.".into());
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    recording
        .ready()
        .await
        .ok_or("Recording stopped before capture started.")?;
    let mut spool = WavSpool::new(recording);
    let capture = capture_pcm(target, recording, &mut spool).await;
    recording.stop();
    let outcome = match capture {
        Ok(reason) => spool.finish().await.map(|()| reason),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(reason) => Ok(format!(
            "Saved trueosfs:/{} ({} audio frames). {reason}",
            recording.path,
            spool.saved_bytes / FRAME_BYTES as u64
        )),
        Err(error) if spool.wrote_parts => Err(format!(
            "{error}; retained PCM recovery chunks at trueosfs:/{}.part*",
            recording.path
        )),
        Err(error) => Err(error.into()),
    }
}

fn stopped(target: &MatrixTarget, recording: &Recording) -> bool {
    recording.stopped() || interrupted(target)
}

async fn capture_pcm(
    target: &MatrixTarget,
    recording: &Recording,
    spool: &mut WavSpool<'_>,
) -> Result<&'static str, &'static str> {
    let input = rtp_mic::start_capture()?;
    let started = crate::chronos::monotonic_nanos();
    // Let the final jitter-buffered packets arrive while capping the WAV at the
    // exact number of frames requested by the selected duration.
    let deadline = recording.deadline(started) + END_GRACE_NS;
    let requested = recording.seconds() as usize * rtp_mic::CAPTURE_SAMPLE_RATE;
    let mut remaining = requested;
    let mut frames = vec![[0i16; CHANNELS as usize]; DRAIN_FRAMES];
    recording.started(started, CHANNELS);
    loop {
        let now = crate::chronos::monotonic_nanos();
        let count = input.drain(&mut frames[..remaining.min(DRAIN_FRAMES)]);
        for frame in &frames[..count] {
            for &sample in frame {
                spool.pending.extend_from_slice(&sample.to_le_bytes());
            }
        }
        remaining -= count;

        if remaining == 0 {
            return Ok("Duration complete.");
        }
        if count < DRAIN_FRAMES {
            // A full read can leave queued audio even after an input error.
            if let Some(error) = input.input_error() {
                return if remaining == requested {
                    Err(error)
                } else {
                    Ok(error)
                };
            }
        }
        if stopped(target, recording) {
            return Ok("Stopped by user.");
        }
        if now >= deadline {
            return Ok("The recording window ended before the full duration arrived.");
        }
        if spool.pending.len() >= PART_BYTES {
            spool.flush().await?;
        }
        if count < DRAIN_FRAMES {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

// TRUEOSFS needs the final length before opening a stream. Commit bounded PCM
// chunks first, then write and verify a WAV before removing the recovery data.
struct WavSpool<'a> {
    recording: &'a Recording,
    pending: Vec<u8>,
    parts: Vec<fs::FileReadHandle>,
    saved_bytes: u64,
    wrote_parts: bool,
}

impl<'a> WavSpool<'a> {
    fn new(recording: &'a Recording) -> Self {
        Self {
            recording,
            pending: Vec::with_capacity(PART_BYTES + DRAIN_FRAMES * FRAME_BYTES),
            parts: Vec::new(),
            saved_bytes: 0,
            wrote_parts: false,
        }
    }

    fn part_path(&self, index: usize) -> String {
        format!("{}.part{index:06}", self.recording.path)
    }

    async fn flush(&mut self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let disk = self.recording.disk;
        let path = self.part_path(self.parts.len());
        if !fs::file_in_async(disk, &path, &self.pending)
            .await
            .map_err(|_| "Cannot write the recording chunk.")?
        {
            return Err("No space for the recording chunk.");
        }
        self.wrote_parts = true;
        let handle = fs::file_read_open_async(disk, &path)
            .await
            .map_err(|_| "Cannot verify the recording chunk.")?
            .filter(|handle| handle.data_len() == self.pending.len() as u64)
            .ok_or("Recording chunk length verification failed.")?;
        self.saved_bytes += handle.data_len();
        self.parts.push(handle);
        self.pending.clear();
        Ok(())
    }

    async fn finish(&mut self) -> Result<(), &'static str> {
        self.flush().await?;
        if self.saved_bytes == 0 {
            return Err("No ESP32 audio samples were recorded.");
        }
        let mut header = Vec::new();
        append_wav_header(&mut header, self.saved_bytes as u32);
        let len = header.len() as u64 + self.saved_bytes;
        let disk = self.recording.disk;
        let path = &self.recording.path;
        let write = fs::file_write_begin_async(disk, path, len)
            .await
            .map_err(|_| "Cannot open the WAV recording.")?
            .ok_or("No space for the WAV recording.")?;
        if let Err(error) = self.copy_parts(write, &header).await {
            let _ = fs::file_write_abort_async(write).await;
            return Err(error);
        }
        fs::file_write_finish_async(write)
            .await
            .map_err(|_| "Cannot commit the WAV recording.")?;
        let final_file = fs::file_read_open_async(disk, path)
            .await
            .map_err(|_| "Cannot verify the WAV recording.")?;
        if final_file.is_none_or(|file| file.data_len() != len) {
            return Err("WAV recording length verification failed.");
        }
        for index in 0..self.parts.len() {
            let _ = fs::file_delete_async(disk, &self.part_path(index)).await;
        }
        Ok(())
    }

    async fn copy_parts(&self, write: u32, header: &[u8]) -> Result<(), &'static str> {
        fs::file_write_chunk_async(write, header)
            .await
            .map_err(|_| "Cannot write the WAV header.")?;
        let mut buffer = vec![0; COPY_BYTES];
        for &handle in &self.parts {
            let mut offset = 0;
            while offset < handle.data_len() {
                let count = (handle.data_len() - offset).min(COPY_BYTES as u64) as usize;
                if fs::file_read_handle_range_async(handle, offset, &mut buffer[..count])
                    .await
                    .map_err(|_| "Cannot read the recording chunk.")?
                    != Some(count)
                {
                    return Err("Recording chunk is incomplete.");
                }
                fs::file_write_chunk_async(write, &buffer[..count])
                    .await
                    .map_err(|_| "Cannot write the WAV recording.")?;
                offset += count as u64;
            }
        }
        Ok(())
    }
}
