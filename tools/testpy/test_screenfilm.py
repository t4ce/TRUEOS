#!/usr/bin/env python3
"""Run the real film orchestration against deterministic time/encoder/disk fakes."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(v: T) -> Self { Self(std::sync::Mutex::new(v)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
static NOW: AtomicU64 = AtomicU64::new(0);
static WD: AtomicBool = AtomicBool::new(false);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] pub struct Instant(u64);
#[derive(Clone, Copy)] pub struct Duration(u64);
pub const TICK_HZ: u64 = 1000;
impl Instant { pub fn now() -> Self { Self(NOW.load(Ordering::SeqCst)/1_000_000) } }
impl std::ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) { self.0 += rhs.0; }
}
impl Duration {
    pub fn from_millis(ms: u64) -> Self { Self(ms) }
    pub fn from_ticks(ticks: u64) -> Self { Self(ticks) }
}
pub struct Timer;
impl Timer { pub async fn after(d: Duration) { NOW.fetch_add(d.0*1_000_000, Ordering::SeqCst); } }
fn run<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(result) => result,
        _ => panic!("unexpected wait"),
    }
}
#[macro_export] macro_rules! log_info { ($($t:tt)*) => {}; }
#[derive(Default)] struct State {
    stop_ns: Option<u64>,
    fail_encode: Option<u32>,
    frame_bytes: usize,
    fail_part: Option<usize>,
    fail_copy: bool,
    short_read: bool,
    fail_commit: bool,
    quarantine: bool,
    no_root: bool,
    slot_busy: bool,
    slot_claims: usize,
    starts: usize,
    ends: usize,
    active: i32,
    lines: Vec<String>,
    encoded: Vec<u8>,
    paths: std::collections::BTreeMap<String, usize>,
    records: Vec<Vec<u8>>,
    write: Option<(String, usize, Vec<u8>)>,
    max_chunk: usize,
    copies: usize,
    aborts: usize,
    deletes: usize,
}
static S: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(|| Mutex::new(State::default()));
mod chronos {
    pub fn monotonic_nanos() -> u64 { crate::NOW.load(crate::Ordering::SeqCst) }
    pub fn best_effort_unix_time_seconds() -> Option<u64> { Some(123456) }
}
mod allcaps { pub mod media_encode {
    pub const REALTIME_HZ: usize = 33;
    pub const STREAM_MAX_ACCESS_UNIT_BYTES: usize = 4*1024*1024;
} }
mod disc { pub mod block {
    #[derive(Clone, Copy, PartialEq, Eq)] pub struct DiscId(pub u32);
    #[derive(Clone, Copy)] pub struct DeviceHandle;
} }
mod intel { pub mod media { pub mod wd_xyuv8888 {
    pub fn try_reserve_stream_capture() -> bool { !crate::WD.swap(true, crate::Ordering::SeqCst) }
    pub fn release_stream_capture() { crate::WD.store(false, crate::Ordering::SeqCst); }
} } }
mod shell2 {
    #[derive(Clone)] pub struct MatrixTarget(pub u32);
    pub fn claim_matrix_target_for_named_app_slot(_: &MatrixTarget, name: &str, _: &str) -> Option<MatrixTarget> {
        assert_eq!(name, "film");
        let mut s = crate::S.lock(); s.slot_claims += 1;
        if s.slot_busy { None } else { Some(MatrixTarget(2)) }
    }
    pub fn set_matrix_target_active(_: &MatrixTarget, active: bool) { crate::S.lock().active += if active {1} else {-1}; }
    pub fn print_matrix_target_line(_: &MatrixTarget, line: &str) { crate::S.lock().lines.push(line.into()); }
    pub fn matrix_target_interrupted(t: &MatrixTarget) -> bool {
        t.0 == 2 && crate::S.lock().stop_ns.is_some_and(|ns| crate::chronos::monotonic_nanos() >= ns)
    }
    pub fn matrix_targets_same_slot_lifetime(a: &MatrixTarget, b: &MatrixTarget) -> bool { a.0 == b.0 }
}
mod r { pub mod fs { pub mod trueosfs {
    use crate::{S, disc::block::DeviceHandle};
    #[derive(Clone, Copy)] pub struct FileReadHandle(usize);
    impl FileReadHandle { pub fn data_len(&self) -> u64 { S.lock().records[self.0].len() as u64 } }
    pub async fn dir_create_all_async(_: DeviceHandle, _: &str) -> Result<bool, ()> { Ok(true) }
    pub async fn file_in_async(_: DeviceHandle, path: &str, bytes: &[u8]) -> Result<bool, ()> {
        let mut s = S.lock();
        let part = path.rsplit(".part").next().unwrap().parse::<usize>().unwrap();
        if s.fail_part == Some(part) { return Ok(false); }
        let record = s.records.len(); s.records.push(bytes.to_vec()); s.paths.insert(path.into(), record);
        s.max_chunk = s.max_chunk.max(bytes.len());
        Ok(true)
    }
    pub async fn file_read_open_async(_: DeviceHandle, path: &str) -> Result<Option<FileReadHandle>, ()> {
        Ok(S.lock().paths.get(path).map(|id| FileReadHandle(*id)))
    }
    pub async fn file_read_handle_range_async(h: FileReadHandle, offset: u64, out: &mut [u8]) -> Result<Option<usize>, ()> {
        let s = S.lock();
        if s.short_read { return Ok(Some(out.len()-1)); }
        out.copy_from_slice(&s.records[h.0][offset as usize..offset as usize+out.len()]);
        Ok(Some(out.len()))
    }
    pub async fn file_write_begin_async(_: DeviceHandle, path: &str, len: u64) -> Result<Option<u32>, ()> {
        S.lock().write = Some((path.into(), len as usize, Vec::new())); Ok(Some(1))
    }
    pub async fn file_write_chunk_async(_: u32, bytes: &[u8]) -> Result<(), ()> {
        let mut s = S.lock(); s.copies += 1;
        if s.fail_copy { s.write = None; return Err(()); }
        s.write.as_mut().unwrap().2.extend_from_slice(bytes); Ok(())
    }
    pub async fn file_write_abort_async(_: u32) -> Result<(), ()> { let mut s = S.lock(); s.aborts += 1; s.write = None; Ok(()) }
    pub async fn file_write_finish_async(_: u32) -> Result<(), ()> {
        let mut s = S.lock();
        let (path, len, bytes) = s.write.take().unwrap();
        if s.fail_commit { return Err(()); }
        assert_eq!(len, bytes.len());
        let record = s.records.len(); s.records.push(bytes); s.paths.insert(path, record); Ok(())
    }
    pub async fn file_delete_async(_: DeviceHandle, path: &str) -> Result<bool, ()> {
        let mut s = S.lock(); s.deletes += 1; Ok(s.paths.remove(path).is_some())
    }
} } }
mod ui4 {
    #[path = "@ROOT@/src/ui4/h264_capture_session.rs"] mod h264_capture_session;
    mod screenshot {
        pub fn writable_capture_root_handle() -> Option<crate::disc::block::DeviceHandle> {
            if crate::S.lock().no_root { None } else { Some(crate::disc::block::DeviceHandle) }
        }
    }
    mod h264_encode_udp {
        pub(super) fn wake_producer() {}
        @CADENCE@
    }
    mod h264_encode_stream {
        const ENCODE_WIDTH: usize = 2560;
        const ENCODE_HEIGHT: usize = 1440;
        #[derive(Default)] struct LiveEncodeStats { frames: usize, capture_us: u64, encode_us: u64 }
        fn average_u64(total: u64, count: usize) -> u64 { total / count.max(1) as u64 }
        fn begin_preparation_session(_: u32, _: usize) { crate::S.lock().starts += 1; }
        async fn end_preparation_session(_: u32) -> bool {
            let mut s = crate::S.lock(); s.ends += 1;
            if s.quarantine { false } else { crate::WD.store(false, crate::Ordering::SeqCst); true }
        }
        fn prepared_scanout_ready(_: u32, _: u32) -> bool { true }
        async fn encode_prepared_scanout(_: u32, sequence: u32, stats: &mut LiveEncodeStats) -> Option<Vec<u8>> {
            if crate::S.lock().fail_encode == Some(sequence) { return None; }
            let mut bytes = vec![sequence as u8; crate::S.lock().frame_bytes.max(4)];
            bytes[..4].copy_from_slice(&sequence.to_le_bytes());
            stats.frames += 1; stats.encode_us += 1_000;
            crate::NOW.fetch_add(1_000_000, crate::Ordering::SeqCst);
            crate::S.lock().encoded.extend_from_slice(&bytes);
            Some(bytes)
        }
        mod film {
            @FILM@
            @TESTS@
        }
    }
}
mod gate_tests {
    use crate::disc::block;
    #[path = "@ROOT@/src/r/fs/trueosfs_write_gate.rs"] mod gate;
    #[test] fn streamed_writer_excludes_same_root_until_commit_abort_or_error_drop() {
        let one = block::DiscId(1); let two = block::DiscId(2);
        let lease = gate::RootWriteLease::try_acquire(one).unwrap();
        assert!(gate::RootWriteLease::try_acquire(one).is_none());
        let other = gate::RootWriteLease::try_acquire(two).unwrap();
        drop(lease);
        let again = gate::RootWriteLease::try_acquire(one).unwrap();
        drop(other); drop(again);
        assert!(gate::RootWriteLease::try_acquire(two).is_some());
    }
}
mod parser_tests {
    @PARSER@
    #[test] fn only_one_integer_from_one_through_ten_is_accepted() {
        for n in 1..=10 { assert_eq!(film_minutes(&format!(" {n} ")), Some(n)); }
        for s in ["", "0", "11", "-1", "+1", "1.0", "1 2", "1 min", "999999999999"] { assert_eq!(film_minutes(s), None, "{s}"); }
    }
}
'''
TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    use crate::{run, S, State, NOW, WD};
    fn setup() {
        *S.lock() = State::default(); NOW.store(0, Ordering::SeqCst); WD.store(false, Ordering::SeqCst);
        *ADMISSION.lock() = CaptureSessionGate::new(); *REQUEST.lock() = None;
        set_encoder_ready();
    }
    fn record(minutes: u8) {
        request_film(minutes, MatrixTarget(1)).unwrap();
        assert!(claim_rdp_view().is_none());
        assert!(request_film(minutes, MatrixTarget(1)).is_err());
        run(run_pending());
        assert_eq!(S.lock().active, 0);
    }
    fn verify_final() {
        let s = S.lock();
        assert_eq!(s.paths.len(), 1);
        let (path, record) = s.paths.iter().next().unwrap();
        assert!(path.starts_with("screenfilms/wd-postblend-123456-0-film"));
        assert!(path.ends_with(".h264"));
        assert_eq!(s.records[*record], s.encoded);
        assert!(s.write.is_none()); assert!(s.deletes > 0);
    }
    #[test] fn full_minute_uses_wall_clock_and_commits_all_frames_in_order() {
        setup(); record(1); verify_final();
        let s = S.lock();
        assert_eq!(s.starts, 1); assert_eq!(s.ends, 1);
        assert!(s.encoded.len()/4 >= 1900);
        assert!(NOW.load(Ordering::SeqCst) >= 60_000_000_000);
        assert!(NOW.load(Ordering::SeqCst) < 60_010_000_000);
        assert!(s.lines.iter().filter(|s| s.contains("fps=")).count() >= 12);
        assert!(!WD.load(Ordering::SeqCst));
    }
    #[test] fn ten_minutes_is_bounded_and_uses_the_same_encoder() {
        setup(); record(10); verify_final();
        assert!(NOW.load(Ordering::SeqCst) >= 600_000_000_000);
        assert!(NOW.load(Ordering::SeqCst) < 600_010_000_000);
        assert_eq!(S.lock().starts, 1);
    }
    #[test] fn slot_snipe_flushes_tail_and_preserves_recording() {
        setup(); S.lock().stop_ns = Some(6_500_000_000);
        record(10); verify_final();
        assert!(NOW.load(Ordering::SeqCst) < 6_510_000_000);
        assert_eq!(S.lock().deletes, 2);
        assert!(claim_rdp_view().is_some());
    }
    #[test] fn cancellation_before_worker_start_releases_wd_without_capture() {
        setup(); S.lock().stop_ns = Some(0); record(1);
        assert_eq!(S.lock().starts, 0); assert!(!WD.load(Ordering::SeqCst));
        assert!(S.lock().paths.is_empty());
    }
    #[test] fn final_copy_failure_keeps_every_committed_part() {
        setup(); { let mut s = S.lock(); s.stop_ns = Some(6_500_000_000); s.fail_copy = true; }
        record(1);
        let s = S.lock(); assert_eq!(s.deletes, 0); assert_eq!(s.aborts, 1);
        assert_eq!(s.paths.len(), 2); assert!(s.paths.keys().all(|p| p.contains(".part")));
        let recovered: Vec<u8> = s.paths.values().flat_map(|i| s.records[*i].clone()).collect();
        assert_eq!(recovered, s.encoded);
    }
    #[test] fn short_read_cannot_publish_or_delete_the_parts() {
        setup(); { let mut s = S.lock(); s.stop_ns = Some(100_000_000); s.short_read = true; }
        record(1); let s = S.lock(); assert_eq!(s.deletes, 0); assert_eq!(s.aborts, 1); assert!(s.write.is_none());
    }
    #[test] fn commit_failure_keeps_recovery_data() {
        setup(); { let mut s = S.lock(); s.stop_ns = Some(100_000_000); s.fail_commit = true; }
        record(1); let s = S.lock(); assert_eq!(s.deletes, 0); assert_eq!(s.paths.len(), 1);
        assert!(s.paths.keys().next().unwrap().contains(".part"));
    }
    #[test] fn full_disk_keeps_previously_saved_chunks_and_stops_capture() {
        setup(); S.lock().fail_part = Some(1); record(1);
        let s = S.lock(); assert_eq!(s.paths.len(), 1); assert_eq!(s.deletes, 0);
        assert_eq!(s.ends, 1); assert!(!WD.load(Ordering::SeqCst));
        assert!(NOW.load(Ordering::SeqCst) < 11_000_000_000);
    }
    #[test] fn large_access_units_flush_at_the_byte_limit_and_join_in_bounded_reads() {
        setup(); { let mut s = S.lock(); s.frame_bytes = 3*1024*1024; s.fail_encode = Some(7); }
        record(1); verify_final();
        let s = S.lock();
        assert_eq!(s.max_chunk, 18*1024*1024);
        assert!(s.max_chunk <= CHUNK_BYTES + crate::allcaps::media_encode::STREAM_MAX_ACCESS_UNIT_BYTES);
        assert_eq!(s.copies, 21);
    }
    #[test] fn encoder_failure_finalizes_all_preceding_frames() {
        setup(); S.lock().fail_encode = Some(42); record(1); verify_final();
        assert_eq!(S.lock().encoded.len()/4, 42);
    }
    #[test] fn quarantine_saves_frames_but_never_reopens_capture_admission() {
        setup(); { let mut s = S.lock(); s.fail_encode = Some(42); s.quarantine = true; }
        record(1); verify_final(); assert!(WD.load(Ordering::SeqCst));
        assert!(claim_rdp_view().is_none()); assert!(request_film(1, MatrixTarget(1)).is_err());
    }
    #[test] fn busy_view_rejected_but_input_only_and_cancelled_claims_are_allowed() {
        setup(); let view = claim_rdp_view().unwrap();
        assert!(claim_rdp_view().is_none());
        assert!(request_film(1, MatrixTarget(1)).is_err()); assert_eq!(S.lock().slot_claims, 0);
        drop(view); assert!(request_film(1, MatrixTarget(1)).is_err());
        NOW.store(2_000_000_000, Ordering::SeqCst);
        S.lock().slot_busy = true;
        assert!(request_film(1, MatrixTarget(1)).is_err()); assert!(!WD.load(Ordering::SeqCst));
        S.lock().slot_busy = false;
        assert!(request_film(1, MatrixTarget(1)).is_ok());
    }
    #[test] fn unavailable_storage_and_busy_wd_do_not_claim_a_slot() {
        setup(); S.lock().no_root = true;
        assert!(request_film(1, MatrixTarget(1)).is_err());
        S.lock().no_root = false; WD.store(true, Ordering::SeqCst);
        assert!(request_film(1, MatrixTarget(1)).is_err()); assert_eq!(S.lock().slot_claims, 0);
        WD.store(false, Ordering::SeqCst);
        assert!(request_film(1, MatrixTarget(1)).is_ok());
    }
}
'''


def main():
    film = (ROOT / 'src/ui4/screenfilm.rs').read_text()
    film = '\n'.join(line for line in film.splitlines() if not line.startswith('//!'))
    udp = (ROOT / 'src/ui4/h264_encode_udp.rs').read_text()
    start = udp.index('pub(super) struct FractionalCadenceTicks')
    end = udp.index('\n#[derive(Debug)]', start)
    source = HARNESS.replace('@ROOT@', str(ROOT)).replace('@FILM@', film)
    source = source.replace('@CADENCE@', udp[start:end]).replace('@TESTS@', TESTS)
    source = source.replace('@PARSER@', item('src/shell2/shell2_cmd_registry.rs', 'film_minutes'))
    with tempfile.TemporaryDirectory(prefix='trueos-film-') as tmp:
        rust, binary = Path(tmp) / 'test.rs', Path(tmp) / 'tests'
        rust.write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(rust), '-o', str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary), '--test-threads=1'], cwd=ROOT, check=True)


def test_matrix():
    harness = r"""
#![allow(dead_code)]
extern crate alloc;
#[derive(Clone)] struct TranscriptEntry { text: alloc::string::String, transient: bool }
type OutputMask = u16;
const OUTPUT_SYSTEM_MASK: OutputMask = 1 << 11;
const OUTPUT_SCOPE_COUNT: usize = 12;
enum MatrixVmUnbindResult { TargetExpired, Unbound, AlreadyAbsent, DifferentOwner }
mod crypt { pub fn has_authenticated_two_factor_session(_: u8) -> bool { false } }
mod user_input_record { pub fn capture(_: u8, _: &str) {} }
#[path = "@ROOT@/src/shell2/matrix.rs"] mod matrix;
"""
    with tempfile.TemporaryDirectory(prefix='trueos-film-matrix-') as tmp:
        directory = Path(tmp)
        (directory / 'src').mkdir()
        (directory / 'Cargo.toml').write_text(
            '[package]\nname = "film-matrix-tests"\nversion = "0.1.0"\nedition = "2024"\n'
            '[dependencies]\nspin = "0.10"\nheapless = { version = "0.9", default-features = false }\n')
        (directory / 'src/lib.rs').write_text(harness.replace('@ROOT@', str(ROOT)))
        subprocess.run(['cargo', 'test', '--offline', '--quiet', '--', '--test-threads=1'],
                       cwd=directory, check=True)


if __name__ == '__main__':
    main()
    test_matrix()
