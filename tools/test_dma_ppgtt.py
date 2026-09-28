#!/usr/bin/env python3
"""Exercise production PPGTT allocation policy with constrained free ranges."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

source = '\n'.join(constant('src/dma.rs', n) for n in ('MIN_DMA_BASE', 'DMA_MAX_PHYS'))
source += item('src/dma.rs', 'alloc_ppgtt_phys')
source += r'''
mod phys {
    use std::cell::RefCell;
    thread_local! { pub static RANGES: RefCell<Vec<(u64,u64)>> = const { RefCell::new(Vec::new()) }; }
    pub fn alloc_phys_range(size: usize, align: usize, min: u64, max: Option<u64>) -> Option<u64> {
        RANGES.with(|ranges| {
            let mut ranges = ranges.borrow_mut();
            for range in ranges.iter_mut() {
                let start = (range.0.max(min) + align as u64 - 1) & !(align as u64 - 1);
                let end = start.checked_add(size as u64)?;
                if end <= range.1.min(max.unwrap_or(u64::MAX)) {
                    range.0 = end;
                    return Some(start);
                }
            }
            None
        })
    }
}
fn ranges(value: &[(u64,u64)]) { phys::RANGES.with(|r| *r.borrow_mut() = value.to_vec()); }
#[test] fn preserves_low_memory_when_both_ranges_fit() {
    ranges(&[(0x100000,0x8000000),(0x100000000,0x108000000)]);
    assert_eq!(alloc_ppgtt_phys(15364096,4096), Some(0x100000000));
    phys::RANGES.with(|r| assert_eq!(r.borrow()[0].0, 0x100000));
}
#[test] fn scene_state_succeeds_with_fragmented_low_memory() {
    ranges(&[(0x100000,0x200000),(0x400000,0x800000),(0x100000000,0x108000000)]);
    assert_eq!(alloc_ppgtt_phys(15364096,4096), Some(0x100000000));
}
#[test] fn machines_without_high_memory_can_use_low_memory() {
    ranges(&[(0x100000,0x8000000)]);
    assert_eq!(alloc_ppgtt_phys(15364096,4096), Some(0x100000));
}
#[test] fn exhaustion_returns_none() {
    ranges(&[(0x100000,0x200000),(0x100000000,0x100100000)]);
    assert_eq!(alloc_ppgtt_phys(15364096,4096), None);
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-dma-') as tmp:
    src, exe = Path(tmp) / 'tests.rs', Path(tmp) / 'tests'
    src.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
