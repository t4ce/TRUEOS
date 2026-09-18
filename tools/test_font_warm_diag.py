#!/usr/bin/env python3
"""Host-only tests: real log macros/policy and coverage function, hardware stubbed."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def item(source, marker):
    start = source.index(marker)
    return source[start:source.index('\n}', start) + 2]


def main():
    log = (ROOT / 'src/log_os.rs').read_text()
    macros = '\n'.join('#[macro_export]\n' + item(log, 'macro_rules! ' + name) for name in
                       ['log_rate_limited', 'log_font_warm_diag', 'log_warn'])
    flags = item(log, 'pub(crate) mod flags')
    source = (ROOT / 'src/intel/gpgpu/operations/submission_2d.rs').read_text()
    function = item(source, 'fn submit_font_outline_coverage_runs_r8_mapped_2d(')
    expected = ['run-count', 'mask-binding-offset', 'mask-binding-range', 'dispatch-shape',
                'ops-binding-offset', 'mask-size-overflow', 'run-binding-contract',
                'font-submit-lock-busy', 'no-claimed-device', 'coverage-kernel-upload', 'font-context']
    for reason in expected:
        assert f'phase=coverage-admission reject={reason}' in function, reason
    assert function.count('return GpgpuDispatchRetirement::NotSubmitted;') == len(expected)
    assert 'first: 2; every: 128;' in macros
    assert 'target: "intel/gpgpu"' in macros
    assert 'crate::log_font_warm_diag!' in (ROOT / 'src/intel/gpu_font.rs').read_text()
    assert 'phase=font-rcs-attempt' in (ROOT / 'src/intel/gpgpu/rcs/commands.rs').read_text()
    shared = ROOT.parent / 'TRUEOS-Blueprints/crates/log-os'
    assert (shared / 'Cargo.toml').is_file(), 'need sibling TRUEOS-Blueprints/crates/log-os'

    harness = r'''
#![allow(dead_code, unused_imports)]
use core::sync::atomic::{AtomicBool, AtomicUsize, AtomicU32, Ordering};
static LINES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
mod log_os {
    pub use log_os_core::{LogLevel, LogRateLimitState};
    __FLAGS__
    pub fn log_with_target_level(target: &str, level: LogLevel, args: core::fmt::Arguments<'_>) {
        let area = log_os_core::target_log_area(target);
        if flags::area_log_enabled(area, level) {
            crate::LINES.lock().unwrap().push(format!("[{}] [{:?}] {}", log_os_core::area_tag(area), level, args));
        }
    }
}
__MACROS__
#[derive(Clone, Copy)] struct Dev { device_id: u16, revision_id: u8 }
fn claimed_device() -> Option<Dev> {
    (coverage::FAIL.load(Ordering::Relaxed) != 9).then_some(Dev { device_id: 0x4680, revision_id: 12 })
}
mod coverage {
    use super::*;
    pub static FAIL: AtomicUsize = AtomicUsize::new(0);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static POLLS: AtomicUsize = AtomicUsize::new(0);
    static SUBMIT: AtomicUsize = AtomicUsize::new(0);
    static POST: AtomicU32 = AtomicU32::new(0);
    static QUARANTINED: AtomicBool = AtomicBool::new(false);
    static FONT_RCS_SUBMIT_LOCK: spin::Mutex<()> = spin::Mutex::new(());
    static FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ: AtomicUsize = AtomicUsize::new(0);
    const FONT_OUTLINE_COVERAGE_BATCH_MAX_RUNS: usize = 32;
    const FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS: u64 = 500;
    const COPY_RECT_PRE_MARKER_SLOT: usize = 0;
    const COPY_RECT_POST_MARKER_SLOT: usize = 2;
    const COPY_RECT_PRE_MARKER: u32 = 0xC001;
    const COPY_RECT_POST_MARKER: u32 = 0xC002;
    #[derive(Debug, PartialEq)] enum GpgpuDispatchRetirement { NotSubmitted, Complete, SubmittedIncomplete }
    #[derive(Clone, Copy, Debug)] enum DirectRcsSubmissionState { Rejected, Submitted, Ambiguous }
    impl DirectRcsSubmissionState {
        fn can_poll(self) -> bool { matches!(self, Self::Submitted) }
        fn may_have_submitted(self) -> bool { !matches!(self, Self::Rejected) }
    }
    #[derive(Clone, Copy)] struct GpgpuMask8Surface { gpu: u64, phys: u64, bytes: usize, pitch_bytes: u32 }
    #[derive(Clone, Copy)] struct Params {
        ops_gpu: u64, mask_gpu: u64, mask_pitch_bytes: u32, mask_width: u32,
        mask_height: u32, rect_width: u32, rect_height: u32, op_count: u32,
    }
    struct FontOutlineCoverageR8BatchRun { params: Params, ops_bytes: usize }
    struct Dispatch { group_x: u32, group_y: u32 }
    #[derive(Clone, Copy)] struct Upload { gpu: u64, phys: u64, mapped_bytes: usize }
    #[derive(Clone, Copy)] struct VAs { batch: u64, result: u64 }
    #[derive(Clone, Copy)] struct State { gpu_va: VAs }
    fn fill_rect_2d_dispatch(w: u32, h: u32) -> Option<Dispatch> {
        (w != 0 && h != 0).then_some(Dispatch { group_x: w.div_ceil(16), group_y: h })
    }
    fn upload_font_outline_coverage_r8_kernel() -> Option<Upload> {
        (FAIL.load(Ordering::Relaxed) != 10).then_some(Upload { gpu: 0x10000, phys: 0x20000, mapped_bytes: 4096 })
    }
    fn font_rcs_state_once(_: Dev) -> Option<State> {
        (FAIL.load(Ordering::Relaxed) != 11).then_some(State { gpu_va: VAs { batch: 0x30000, result: 0x40000 } })
    }
    fn gate(n: usize) -> bool { CALLS.fetch_or(1 << n, Ordering::Relaxed); FAIL.load(Ordering::Relaxed) != n }
    fn direct_rcs_forcewake(_: Dev) -> bool { gate(1) }
    fn direct_rcs_map_state(_: Dev, _: State) -> bool { gate(2) }
    fn font_rcs_init_ppgtt_once(_: State) -> bool { gate(3) }
    fn direct_rcs_map_ppgtt_kernel(_: State, gpu: u64, _: u64, _: usize) -> bool {
        gate(match gpu { 0x10000 => 4, 0x50000 => 5, _ => 6 })
    }
    fn direct_rcs_encode_font_outline_coverage_runs_r8_2d_batch(_: State, _: Upload,
        _: &[FontOutlineCoverageR8BatchRun], _: usize) -> bool { gate(7) }
    fn font_rcs_submit_batch_state(_: Dev, _: State) -> DirectRcsSubmissionState {
        CALLS.fetch_or(1 << 8, Ordering::Relaxed);
        match SUBMIT.load(Ordering::Relaxed) { 1 => DirectRcsSubmissionState::Rejected,
            2 => DirectRcsSubmissionState::Ambiguous, _ => DirectRcsSubmissionState::Submitted }
    }
    fn font_rcs_poll_result_slot_timeout_ms(_: State, _: usize, _: u32, _: u64) -> u32 {
        POLLS.fetch_add(1, Ordering::Relaxed); POST.load(Ordering::Relaxed)
    }
    fn direct_rcs_read_result_slot(_: State, _: usize) -> u32 { COPY_RECT_PRE_MARKER }
    fn font_rcs_context_is_quarantined() -> bool { QUARANTINED.load(Ordering::Relaxed) }
    fn quarantine_font_rcs_context(_: &str) { QUARANTINED.store(true, Ordering::Relaxed); }
    __FUNCTION__
    fn reset(fail: usize, submit: usize, post: u32) {
        FAIL.store(fail, Ordering::Relaxed); SUBMIT.store(submit, Ordering::Relaxed);
        POST.store(post, Ordering::Relaxed); POLLS.store(0, Ordering::Relaxed);
        CALLS.store(0, Ordering::Relaxed); QUARANTINED.store(false, Ordering::Relaxed);
    }
    fn run() -> GpgpuDispatchRetirement {
        let mask = GpgpuMask8Surface { gpu: 0x60000, phys: 0x80000, bytes: 4096, pitch_bytes: 64 };
        let runs = [FontOutlineCoverageR8BatchRun { ops_bytes: 4096,
            params: Params { ops_gpu: 0x50000, mask_gpu: 0x60000, mask_pitch_bytes: 64,
                mask_width: 16, mask_height: 16, rect_width: 16, rect_height: 16, op_count: 4 } }];
        submit_font_outline_coverage_runs_r8_mapped_2d(0x50000, 0x70000, 4096, mask, mask.gpu, mask.bytes, &runs)
    }
    #[test] fn exact_coverage_function_keeps_short_circuit_and_ownership() {
        reset(0, 0, COPY_RECT_POST_MARKER);
        assert_eq!(run(), GpgpuDispatchRetirement::Complete);
        assert_eq!(POLLS.load(Ordering::Relaxed), 1);
        for stage in 1..=7 {
            reset(stage, 0, COPY_RECT_POST_MARKER);
            assert_eq!(run(), GpgpuDispatchRetirement::NotSubmitted);
            assert_eq!(CALLS.load(Ordering::Relaxed), (1 << (stage + 1)) - 2);
            assert_eq!(POLLS.load(Ordering::Relaxed), 0);
            assert!(!font_rcs_context_is_quarantined());
        }
        for stage in 9..=11 {
            reset(stage, 0, 0);
            assert_eq!(run(), GpgpuDispatchRetirement::NotSubmitted);
            assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        }
        reset(0, 0, 0);
        let guard = FONT_RCS_SUBMIT_LOCK.lock();
        assert_eq!(run(), GpgpuDispatchRetirement::NotSubmitted);
        drop(guard);
        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        reset(0, 1, 0);
        assert_eq!(run(), GpgpuDispatchRetirement::NotSubmitted);
        assert!(!font_rcs_context_is_quarantined());
        reset(0, 2, 0);
        assert_eq!(run(), GpgpuDispatchRetirement::SubmittedIncomplete);
        assert!(font_rcs_context_is_quarantined());
        assert_eq!(POLLS.load(Ordering::Relaxed), 0);
        reset(0, 0, 0);
        assert_eq!(run(), GpgpuDispatchRetirement::SubmittedIncomplete);
        assert!(font_rcs_context_is_quarantined());
        assert_eq!(POLLS.load(Ordering::Relaxed), 1);
        for line in LINES.lock().unwrap().iter().filter(|s| s.contains("font-warm")) {
            assert!(line.trim_end().len() <= 320, "half-column clipping: {}", line);
        }
    }
}
#[test] fn macro_uses_existing_profile_lazily_and_samples_per_callsite() {
    static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);
    LINES.lock().unwrap().clear();
    for _ in 0..256 {
        crate::log_font_warm_diag!("phase=macro-test n={}\\n", EVALUATIONS.fetch_add(1, Ordering::Relaxed));
    }
    assert_eq!(EVALUATIONS.load(Ordering::Relaxed), 4); // 1,2,128,256 only
    let lines = LINES.lock().unwrap();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|s| s.starts_with("[gpgpu] [Info]")));
    assert!(lines[2].contains("occurrence=128"));
    assert!(lines[2].contains("suppressed_since_last=125"));
    assert!(!log_os::flags::area_log_enabled(log_os_core::LogArea::Render, log_os_core::LogLevel::Info));
    assert!(!log_os::flags::area_log_enabled(log_os_core::LogArea::Usb, log_os_core::LogLevel::Info));
}
'''.replace('__FLAGS__', flags).replace('__MACROS__', macros).replace('__FUNCTION__', function)
    with tempfile.TemporaryDirectory(prefix='trueos-font-warm-diag-') as temp:
        work = Path(temp)
        (work / 'Cargo.toml').write_text(f'''[package]
name = "trueos-font-warm-diag-test"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "lib.rs"
[dependencies]
spin = "0.10"
log_os_core = {{ package = "log-os", path = "{shared}", default-features = false }}
''')
        (work / 'lib.rs').write_text(harness)
        env = dict(os.environ, RUSTUP_TOOLCHAIN='stable')
        subprocess.run(['cargo', 'test', '--lib', '--manifest-path', str(work / 'Cargo.toml'), '--', '--test-threads=1'],
                       cwd=work, env=env, check=True)
    print('PASS: actual diagnostic macro/policy and coverage admission function; hardware stubbed, not a kernel build')


if __name__ == '__main__':
    main()
