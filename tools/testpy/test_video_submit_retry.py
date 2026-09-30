#!/usr/bin/env python3
"""Test production video submission result handling at the GuC boundary."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item


def main():
    path = 'src/intel/gpgpu/rcs/commands.rs'
    source = r'''
#![allow(dead_code, unused_variables)]
#[macro_export] macro_rules! log_error { ($($t:tt)*) => {}; }
#[derive(Clone, Copy)] struct Dev;
mod gpu {
    pub mod executor { pub type KernelSubmission = u32; }
    pub mod vgpu {
        #[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum KernelClient { Ui4Compositor }
        #[derive(Clone, Copy)] pub enum VgpuError { DeviceLost }
    }
}
mod tested {
use super::*;
#[derive(Debug, PartialEq)] enum Ui4CompositorSubmitError { Busy, SubmissionRejected }
struct DirectRcsState;
struct DirectRcsSubmitRuntime { next: Option<DirectRcsSubmitAttempt> }
thread_local! { static QUARANTINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
fn ui4_compositor_rcs_context_is_quarantined() -> bool { QUARANTINED.get() }
fn quarantine_ui4_compositor_rcs_context(_: &str) { QUARANTINED.set(true); }
fn direct_rcs_submit_batch_with_runtime_inner(_: Dev, _: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime, _: gpu::vgpu::KernelClient, _: bool) -> DirectRcsSubmitAttempt {
    runtime.next.take().unwrap()
}
'''
    source += item(path, 'DirectRcsSubmitAttempt')
    source += item(path, 'direct_rcs_try_submit_batch_with_runtime')
    source += r'''
fn submit(next: DirectRcsSubmitAttempt) -> Result<u32, Ui4CompositorSubmitError> {
    direct_rcs_try_submit_batch_with_runtime(Dev, DirectRcsState,
        &mut DirectRcsSubmitRuntime { next: Some(next) }, gpu::vgpu::KernelClient::Ui4Compositor, true)
}
#[test] fn context_save_deferral_is_retryable_and_next_submission_can_succeed() {
    QUARANTINED.set(false);
    assert_eq!(submit(DirectRcsSubmitAttempt::Deferred), Err(Ui4CompositorSubmitError::Busy));
    assert!(!QUARANTINED.get());
    assert_eq!(submit(DirectRcsSubmitAttempt::Submitted(42)), Ok(42));
}
#[test] fn hard_rejection_does_not_become_busy() {
    QUARANTINED.set(false);
    assert_eq!(submit(DirectRcsSubmitAttempt::Rejected), Err(Ui4CompositorSubmitError::SubmissionRejected));
}
#[test] fn ambiguous_publication_is_quarantined_never_retried() {
    QUARANTINED.set(false);
    assert_eq!(submit(DirectRcsSubmitAttempt::Ambiguous { error: gpu::vgpu::VgpuError::DeviceLost,
        old_tail_bytes: 0, published_tail_bytes: 16, submission_sequence: 1 }),
        Err(Ui4CompositorSubmitError::SubmissionRejected));
    assert!(QUARANTINED.get());
    assert_eq!(submit(DirectRcsSubmitAttempt::Submitted(42)), Err(Ui4CompositorSubmitError::SubmissionRejected));
}
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-video-submit-') as tmp:
        rs, exe = Path(tmp) / 'test.rs', Path(tmp) / 'test'
        rs.write_text(source)
        subprocess.run(['rustc', '--edition=2021', '--test', str(rs), '-o', str(exe)], check=True)
        subprocess.run([str(exe)], check=True)

if __name__ == '__main__':
    main()
