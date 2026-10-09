#!/usr/bin/env python3
"""Exercise the actual recorder with deterministic PCM/time/TRUEOSFS fakes."""
from pathlib import Path
import subprocess
import tempfile
from test_screenfilm import HARNESS

ROOT = Path(__file__).resolve().parents[2]

CAPTURE = r'''
pub mod services { pub mod hda_capture_lane {
    pub struct CaptureCursor { pub channels: u8, frames: u64 }
    pub struct CaptureRead { pub samples: usize }
    pub struct Status { pub state: &'static str }
    pub fn status() -> Status { Status { state: "offline" } }
    pub fn ensure_started_on_current_worker() -> bool { true }
    pub fn recording_cursor() -> Option<CaptureCursor> {
        if crate::S.lock().no_root { None } else { Some(CaptureCursor { channels: 2, frames: crate::NOW.load(crate::Ordering::SeqCst)*48_000/1_000_000_000 }) }
    }
    pub fn copy_recording_i16(c: &mut CaptureCursor, out: &mut [i16]) -> Result<CaptureRead, &'static str> {
        let now = crate::NOW.load(crate::Ordering::SeqCst);
        if crate::S.lock().fail_encode.is_some() && now >= 2_000_000_000 {
            return Err("microphone capture restarted; recording stopped at discontinuity");
        }
        let end = now * 48_000 / 1_000_000_000;
        let count = (end - c.frames).min((out.len()/2) as u64) as usize;
        for frame in 0..count {
            let value = (c.frames + frame as u64) as i16;
            out[frame*2] = value;
            out[frame*2+1] = value.wrapping_neg();
        }
        c.frames += count as u64;
        Ok(CaptureRead { samples: count*2 })
    }
} }
'''
TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    use crate::{run, S, State, NOW, Ordering};
    fn setup() {
        *S.lock() = State::default(); NOW.store(0, Ordering::SeqCst);
        *CONTROL.lock() = Control { busy: true, stop: false };
    }
    fn record(minutes: Option<u8>) {
        shell2::set_matrix_target_active(&MatrixTarget(2), true);
        run(record_task(RecordRequest { capture: None, minutes, disk: crate::disc::block::DeviceHandle,
            path: "recordings/test.wav".into(), target: MatrixTarget(2), origin: MatrixTarget(1) }));
        assert_eq!(S.lock().active, 0);
        assert!(!CONTROL.lock().busy);
    }
    fn verify(frames: usize) {
        let s = S.lock();
        let bytes = &s.records[*s.paths.get("recordings/test.wav").unwrap()];
        assert_eq!(bytes.len(), 44 + frames*4);
        assert_eq!(&bytes[..4], b"RIFF"); assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len()-8);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48000);
        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), 192000);
        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 4);
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize, frames*4);
        for (frame, data) in bytes[44..].chunks_exact(4).enumerate() {
            let value = frame as i16;
            assert_eq!(&data[..2], &value.to_le_bytes());
            assert_eq!(&data[2..], &value.wrapping_neg().to_le_bytes());
        }
        assert_eq!(s.paths.len(), 1, "parts removed only after successful final save");
        assert!(s.max_chunk <= CHUNK_BYTES + 4*960);
    }
    #[test] fn native_three_seconds_has_exact_pcm_and_no_preroll() {
        setup();
        let recording=crate::shell3::capture::recording(2,3,"recordings/test.wav");
        shell2::set_matrix_target_active(&MatrixTarget(2),true);
        run(record_task(RecordRequest {capture:Some(recording.clone()),minutes:None,disk:crate::disc::block::DeviceHandle,path:recording.path.clone(),target:MatrixTarget(2),origin:MatrixTarget(2)}));
        let status=recording.status.lock();assert!(status.finished && status.saved);assert_eq!(status.started_ns,100_000_000);
        let s=S.lock();let bytes=&s.records[*s.paths.get("recordings/test.wav").unwrap()];
        assert_eq!(bytes.len(),44+3*48000*4);assert_eq!(s.active,0);
        assert_eq!(i16::from_le_bytes(bytes[44..46].try_into().unwrap()),4800);
    }
    #[test] fn duration_saves_exact_consecutive_pcm_in_playable_wav() {
        setup(); record(Some(1)); verify(60*48000);
    }
    #[test] fn matrix_stop_preserves_completed_tail() {
        setup(); S.lock().stop_ns = Some(3_500_000_000);
        record(None); verify(168000);
        assert!(S.lock().lines.iter().any(|l| l.contains("stopped; saving WAV")));
    }
    #[test] fn explicit_stop_before_capture_does_not_create_an_empty_wav() {
        setup(); CONTROL.lock().stop = true;
        record(None); assert!(S.lock().paths.is_empty());
    }
    #[test] fn failed_capture_finalizes_valid_prefix() {
        setup(); S.lock().fail_encode = Some(1);
        record(None); verify(95040);
        assert!(S.lock().lines.iter().any(|l| l.contains("discontinuity")));
    }
    #[test] fn final_copy_failure_retains_recovery_parts_and_aborts_writer() {
        setup(); { let mut s = S.lock(); s.stop_ns = Some(3_500_000_000); s.fail_copy = true; }
        record(None);
        let s = S.lock(); assert_eq!(s.aborts, 1); assert_eq!(s.deletes, 0);
        assert!(!s.paths.contains_key("recordings/test.wav"));
        assert!(s.paths.keys().all(|p| p.contains(".part")));
        assert!(s.lines.iter().any(|l| l.contains("retained")));
    }
    #[test] fn microphone_timeout_releases_admission_without_fabricating_audio() {
        setup(); S.lock().no_root = true;
        record(None); assert!(S.lock().paths.is_empty());
        assert!(S.lock().lines.iter().any(|l| l.contains("within 10 seconds")));
    }
    #[test] fn failed_commit_never_deletes_recovery_parts() {
        setup(); { let mut s = S.lock(); s.stop_ns = Some(3_500_000_000); s.fail_commit = true; }
        record(None); let s = S.lock(); assert_eq!(s.deletes, 0);
        assert!(!s.paths.contains_key("recordings/test.wav"));
    }
    #[test] fn mono_header_has_correct_alignment_and_rate() {
        let h = wav_header(1, 96000);
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 96000);
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 2);
    }
}
'''


CURSOR_HARNESS = r'''
mod cursor_tests {
    use crate::Mutex;
    const CAPTURE_DMA_BYTES: usize = 256*1024;
    const PCM_SAMPLE_RATE_HZ: u32 = 48000;
    const PCM_SAMPLE_BITS: u8 = 16;
    #[derive(PartialEq)] enum CaptureState { Running, Faulted }
    struct CaptureStatus { state: CaptureState }
    struct Engine { last_progress_ms: u64, total_frames: u64, restarts: u32, channels: u8, last_lpib: u32, dma_virt: usize }
    pub(crate) struct CaptureRead { samples: usize, channels: u8, sample_rate_hz: u32, sample_bits: u8, total_frames: u64 }
    static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
    static STATUS: Mutex<CaptureState> = Mutex::new(CaptureState::Running);
    fn status() -> CaptureStatus { CaptureStatus { state: match *STATUS.lock() {
        CaptureState::Running => CaptureState::Running, CaptureState::Faulted => CaptureState::Faulted,
    } } }
    fn uptime_ms() -> u64 { crate::NOW.load(crate::Ordering::SeqCst) / 1_000_000 }
    @CURSOR@
    #[test] fn independent_cursors_read_consecutive_frames_across_wrap_without_repeats() {
        crate::NOW.store(0, crate::Ordering::SeqCst);
        let channels = 2usize;
        let capacity = CAPTURE_DMA_BYTES / (channels*2);
        let mut dma = vec![0i16; CAPTURE_DMA_BYTES/2];
        for frame in 0..capacity {
            dma[frame*2] = frame as i16; dma[frame*2+1] = (frame as i16).wrapping_neg();
        }
        *STATUS.lock() = CaptureState::Running;
        *ENGINE.lock() = Some(Engine { last_progress_ms: 0, total_frames: capacity as u64 - 10, restarts: 0,
            channels: 2, last_lpib: ((capacity-10)*4) as u32, dma_virt: dma.as_mut_ptr() as usize });
        let mut a = recording_cursor().unwrap(); let mut b = recording_cursor().unwrap();
        { let mut e = ENGINE.lock(); let e = e.as_mut().unwrap(); e.total_frames += 20; e.last_lpib = 40; }
        let mut first = [0i16; 16]; let read = copy_recording_i16(&mut a, &mut first).unwrap();
        assert_eq!(read.samples, 16); assert_eq!(read.total_frames, capacity as u64-2);
        let mut second = [0i16; 24]; assert_eq!(copy_recording_i16(&mut a, &mut second).unwrap().samples, 24);
        let mut all = [0i16; 40]; assert_eq!(copy_recording_i16(&mut b, &mut all).unwrap().samples, 40);
        assert_eq!(&all[..16], &first); assert_eq!(&all[16..], &second);
        for (i, pair) in all.chunks_exact(2).enumerate() {
            let value = ((capacity-10+i) % capacity) as i16;
            assert_eq!(pair, &[value, value.wrapping_neg()]);
        }
        assert_eq!(copy_recording_i16(&mut a, &mut first).unwrap().samples, 0);
        // DMA keeps advancing independently; both failures must leave the cursor unchanged.
        let previous = a.frames;
        { let mut e = ENGINE.lock(); e.as_mut().unwrap().total_frames += capacity as u64; }
        assert!(matches!(copy_recording_i16(&mut a, &mut first), Err(e) if e.contains("overrun")));
        assert_eq!(a.frames, previous);
        { let mut e = ENGINE.lock(); e.as_mut().unwrap().restarts += 1; }
        assert!(matches!(copy_recording_i16(&mut a, &mut first), Err(e) if e.contains("restarted")));
        assert_eq!(a.frames, previous);
        let mut fresh = recording_cursor().unwrap();
        crate::NOW.store(2_000_000_000, crate::Ordering::SeqCst);
        assert!(matches!(copy_recording_i16(&mut fresh, &mut first), Err(e) if e.contains("read gap")));
        *ENGINE.lock() = None;
    }
}
'''


def main():
    source = HARNESS[:HARNESS.index('mod ui4 {')]
    source = source.replace('mod r { pub mod fs', 'mod r { ' + CAPTURE + ' pub mod fs')
    recorder = (ROOT / 'src/shell2/cmds/rec.rs').read_text()
    recorder = '\n'.join(l for l in recorder.splitlines() if not l.startswith('//!'))
    recorder = recorder.replace('use crate::shell2::shell2_cmd::ParseOutcome;', '')
    recorder = recorder.replace('use crate::shell2::{self, MatrixTarget, ShellBackend2};', 'use crate::shell2::{self, MatrixTarget};')
    recorder = recorder.replace('use trueos_executor::Spawner;', '').replace('#[trueos_executor::task]', '')
    start, end = recorder.index('pub(crate) fn try_parse'), recorder.index('fn stopped(')
    recorder = recorder[:start] + recorder[end:]
    source += '\nmod rec {\n' + recorder + TESTS + '\n}\n'
    capture = (ROOT / 'src/r/services/hda_capture_lane.rs').read_text()
    start = capture.index('pub(crate) struct CaptureCursor')
    end = capture.index('pub(crate) fn ensure_started_on_current_worker')
    source += CURSOR_HARNESS.replace('@CURSOR@', capture[start:end])
    with tempfile.TemporaryDirectory(prefix='trueos-rec-') as tmp:
        rust, binary = Path(tmp)/'test.rs', Path(tmp)/'tests'
        rust.write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(rust), '-o', str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary), '--test-threads=1'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
