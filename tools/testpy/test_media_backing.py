#!/usr/bin/env python3
"""Fault-inject the production decoder backing transaction on the host."""
import subprocess
import tempfile
from pathlib import Path
from test_clip_position3_uv_texture import constant, item
from test_rdp_pipeline import block


def main():
    engine = 'src/intel/media/engine.rs'
    harness = r'''
#![allow(dead_code, unused_variables, unfulfilled_lint_expectations)]
use std::sync::atomic::{AtomicBool, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(v: T) -> Self { Self(std::sync::Mutex::new(v)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Default)]
struct Rig {
    calls: usize, fail_alloc: usize, maps: usize, fail_map: usize,
    fail_ppgtt: bool, fail_unmap: bool,
    live: std::collections::BTreeMap<u64, usize>,
    aliases: std::collections::BTreeMap<u64, u64>,
}
static RIG: Mutex<Option<Rig>> = Mutex::new(None);
#[macro_export] macro_rules! log_error { ($($x:tt)*) => {}; }
mod dma {
    pub fn alloc_with_max(bytes: usize, _: usize, max: Option<u64>) -> Option<(u64, *mut u8)> {
        let mut state = super::RIG.lock(); let r = state.as_mut().unwrap();
        r.calls += 1;
        if r.calls == r.fail_alloc { return None; }
        // Only high memory remains. Fail if the caller still imposes DMA32.
        let phys = 0x2_0000_0000 + r.calls as u64 * 0x1000_0000;
        if phys + bytes as u64 > max.unwrap_or(u64::MAX) { return None; }
        r.live.insert(phys, bytes);
        Some((phys, phys as *mut u8))
    }
    pub fn dealloc(ptr: *mut u8, bytes: usize) {
        let mut state = super::RIG.lock(); let r = state.as_mut().unwrap();
        assert!(!r.aliases.values().any(|p| *p == ptr as u64), "freed a GGTT-mapped buffer");
        assert_eq!(r.live.remove(&(ptr as u64)), Some(bytes), "double/wrong free");
    }
}
mod intel {
    #[derive(Clone, Copy)] pub struct Dev;
    pub const WARM_ALIGN: usize = 4096;
    pub fn map_ggtt(_: Dev, phys: u64, _: usize, gpu: u64) -> bool {
        let mut state = super::RIG.lock(); let r = state.as_mut().unwrap();
        assert!(r.live.contains_key(&phys));
        r.maps += 1;
        r.aliases.insert(gpu, phys); // Also model a partially written failing map.
        r.maps != r.fail_map
    }
    pub fn ggtt_invalidate(_: Dev) {}
    pub fn unmap_display_scanout_ggtt(_: Dev, _: usize, gpu: u64) -> bool {
        let mut state = super::RIG.lock(); let r = state.as_mut().unwrap();
        if r.fail_unmap { return false; }
        r.aliases.remove(&gpu); true
    }
    pub mod ppgtt {
        pub struct PpgttRange { pub gpu: u64, pub phys: u64, pub bytes: usize }
        pub struct SparsePpgtt { root: u64, _ranges: Vec<PpgttRange> }
        impl SparsePpgtt { pub fn pml4_phys(&self) -> u64 { self.root } }
        pub fn build_sparse_ppgtt_for_ranges(ranges: &[PpgttRange]) -> Option<SparsePpgtt> {
            if super::super::RIG.lock().as_ref().unwrap().fail_ppgtt { return None; }
            Some(SparsePpgtt { root: ranges[3].phys + 4096,
                _ranges: ranges.iter().map(|r| PpgttRange { gpu: r.gpu, phys: r.phys, bytes: r.bytes }).collect() })
        }
    }
'''
    harness += constant('src/intel/mod.rs', 'GEN12_GGTT_PTE_ADDR_MASK')
    harness += item('src/intel/mod.rs', 'alloc_ggtt_backing')
    harness += '\npub mod engine { use super::super::*;\n'
    for name in ['RING', 'CONTEXT', 'BATCH', 'RESULT', 'BITSTREAM', 'OUTPUT_SURFACE', 'AVC_SCRATCH']:
        harness += constant(engine, f'MEDIA_DEFAULT_{name}_BYTES') + '\n'
    for name in ['MediaGpuWindowLayout', 'MediaBitstreamBacking', 'UnsubmittedDecodeBuffer',
                 'report_decode_backing_failure', 'ensure_decode_backing',
                 'AvcSessionBacking', 'ensure_avc_session_backing']:
        harness += item(engine, name) + '\n'
    harness += block(engine, 'impl UnsubmittedDecodeBuffer {')
    harness += block(engine, 'impl Drop for UnsubmittedDecodeBuffer {')
    harness += r'''
unsafe impl Send for MediaBitstreamBacking {}
static MEDIA_BACKING: Mutex<Option<MediaBitstreamBacking>> = Mutex::new(None);
static AVC_SESSION_BACKINGS: [Mutex<Option<AvcSessionBacking>>; 2] = [const { Mutex::new(None) }; 2];
static DECODE_BACKING_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);
fn install_media_ppgtt(_: &[super::ppgtt::PpgttRange]) -> Option<u64> {
    if RIG.lock().as_ref().unwrap().fail_ppgtt { None } else { Some(0x3_0000_0000) }
}
fn windows() -> MediaGpuWindowLayout {
    MediaGpuWindowLayout {
        ring_gpu_addr: 0x10000, context_gpu_addr: 0x20000,
        batch_gpu_addr: 0x30000, result_gpu_addr: 0x40000,
        bitstream_gpu_addr: 0x1000000, output_surface_gpu_addr: 0x2000000,
        avc_scratch_gpu_addr: 0x5000000,
    }
}
fn reset() { for slot in &AVC_SESSION_BACKINGS { *slot.lock() = None; } *MEDIA_BACKING.lock() = None; *RIG.lock() = Some(Rig::default()); }
#[test]
fn every_allocation_failure_rolls_back_for_200_playback_retries() {
    for fail in 1..=7 {
        reset();
        for _ in 0..200 {
            { let mut s = RIG.lock(); let r = s.as_mut().unwrap(); r.calls = 0; r.fail_alloc = fail; }
            assert!(ensure_decode_backing(super::Dev, windows()).is_none());
            let s = RIG.lock(); let r = s.as_ref().unwrap();
            assert!(r.live.is_empty(), "leak at allocation {fail}");
            assert!(r.aliases.is_empty());
        }
    }
}
#[test]
fn every_partial_ggtt_failure_unmaps_before_freeing() {
    for fail in 1..=7 {
        reset(); RIG.lock().as_mut().unwrap().fail_map = fail;
        assert!(ensure_decode_backing(super::Dev, windows()).is_none());
        let s = RIG.lock(); let r = s.as_ref().unwrap();
        assert!(r.live.is_empty()); assert!(r.aliases.is_empty());
    }
}
#[test]
fn ppgtt_failure_rolls_back_then_retry_succeeds_above_4g() {
    reset(); RIG.lock().as_mut().unwrap().fail_ppgtt = true;
    assert!(ensure_decode_backing(super::Dev, windows()).is_none());
    { let mut s = RIG.lock(); let r = s.as_mut().unwrap();
      assert!(r.live.is_empty()); assert!(r.aliases.is_empty()); r.fail_ppgtt = false; }
    let backing = ensure_decode_backing(super::Dev, windows()).unwrap();
    assert!(backing.output_surface_phys > u32::MAX as u64);
    let calls = RIG.lock().as_ref().unwrap().calls;
    for _ in 0..200 {
        assert_eq!(ensure_decode_backing(super::Dev, windows()).unwrap().output_surface_phys,
                   backing.output_surface_phys);
    }
    let s = RIG.lock(); let r = s.as_ref().unwrap();
    assert_eq!(r.calls, calls); assert_eq!(r.live.len(), 7); assert_eq!(r.aliases.len(), 7);
}
#[test]
fn three_dpbs_are_isolated_and_reused_without_retargeting_ggtt() {
    reset();
    let backings: Vec<_> = (0..3).map(|slot| ensure_avc_session_backing(super::Dev, windows(), slot).unwrap()).collect();
    for a in 0..3 { for b in 0..3 {
        if a == b { continue; }
        assert_ne!(backings[a].output_surface_phys, backings[b].output_surface_phys);
        assert_ne!(backings[a].avc_scratch_phys, backings[b].avc_scratch_phys);
        assert_ne!(backings[a].ppgtt_pml4_phys, backings[b].ppgtt_pml4_phys);
        assert_eq!(backings[a].ring_phys, backings[b].ring_phys);
    }}
    for _ in 0..200 { for slot in 0..3 {
        let next = ensure_avc_session_backing(super::Dev, windows(), slot).unwrap();
        assert_eq!(next.output_surface_phys, backings[slot].output_surface_phys);
    }}
    let state = RIG.lock(); let rig = state.as_ref().unwrap();
    assert_eq!(rig.calls, 11); assert_eq!(rig.live.len(), 11); assert_eq!(rig.maps, 7);
}
#[test]
fn extra_dpb_allocation_and_ppgtt_failures_leave_common_backing_intact() {
    for fail in 8..=10 {
        reset(); let common = ensure_decode_backing(super::Dev, windows()).unwrap();
        { let mut s = RIG.lock(); let r = s.as_mut().unwrap();
          r.fail_alloc = fail; r.fail_ppgtt = fail == 10; }
        assert!(ensure_avc_session_backing(super::Dev, windows(), 1).is_none());
        let s = RIG.lock(); let r = s.as_ref().unwrap();
        assert_eq!(r.live.len(), 7); assert_eq!(r.aliases.len(), 7);
        assert_eq!(r.aliases[&windows().output_surface_gpu_addr], common.output_surface_phys);
    }
}
#[test]
fn failed_unmap_retains_aliased_memory() {
    reset(); { let mut s = RIG.lock(); let r = s.as_mut().unwrap(); r.fail_map = 1; r.fail_unmap = true; }
    assert!(ensure_decode_backing(super::Dev, windows()).is_none());
    let s = RIG.lock(); let r = s.as_ref().unwrap();
    assert_eq!(r.live.len(), 1); assert_eq!(r.aliases.len(), 1);
}
}}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-media-backing-') as tmp:
        src = Path(tmp) / 'test.rs'
        src.write_text(harness)
        binary = Path(tmp) / 'test'
        subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
