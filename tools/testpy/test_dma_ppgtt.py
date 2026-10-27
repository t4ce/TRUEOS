#!/usr/bin/env python3
"""Exercise production PPGTT allocation policy with constrained free ranges."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item, constant

source = '\n'.join(constant('src/dma.rs', n) for n in ('MIN_DMA_BASE', 'DMA_MAX_PHYS'))
source += item('src/dma.rs', 'alloc_ppgtt_phys')
source += '\nmod resident {\n' + (ROOT / 'src/gpu/resident.rs').read_text() + '\n}\n'
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
// Keep the real resident-buffer implementation and physical selection policy.
// Only the CPU mapping/flush boundary is replaced with ordinary host storage.
mod dma {
    use std::cell::RefCell;
    thread_local! {
        static BACKING: RefCell<Vec<(u64, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }
    fn mapped(phys: u64, size: usize) -> Option<(u64, *mut u8)> {
        let mut bytes = vec![0xa5; size];
        let pointer = bytes.as_mut_ptr();
        BACKING.with(|backing| backing.borrow_mut().push((phys, bytes)));
        Some((phys, pointer))
    }
    pub fn alloc_ppgtt(size: usize, alignment: usize) -> Option<(u64, *mut u8)> {
        mapped(super::alloc_ppgtt_phys(size, alignment)?, size)
    }
    #[allow(dead_code)]
    pub fn alloc(size: usize, alignment: usize) -> Option<(u64, *mut u8)> {
        mapped(super::phys::alloc_phys_range(size, alignment,
            super::MIN_DMA_BASE, Some(super::DMA_MAX_PHYS))?, size)
    }
    pub fn dealloc(pointer: *mut u8, size: usize) {
        BACKING.with(|backing| {
            let mut backing = backing.borrow_mut();
            let index = backing.iter().position(|(_, bytes)| bytes.as_ptr() == pointer.cast_const())
                .expect("resident buffer must release its original CPU mapping");
            assert_eq!(backing[index].1.len(), size);
            backing.remove(index);
        });
    }
    pub fn bytes(phys: u64) -> Vec<u8> {
        BACKING.with(|backing| backing.borrow().iter()
            .find(|(address, _)| *address == phys).unwrap().1.clone())
    }
    pub fn allocation_count() -> usize { BACKING.with(|backing| backing.borrow().len()) }
}
mod intel {
    pub fn dma_flush(_: *mut u8, _: usize) {}
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
#[test] fn resident_assets_use_high_memory_when_low_dma_cannot_fit() {
    const SIZE: usize = 32 * 1024 * 1024;
    let low = [(0x100000,0x500000),(0x800000,0xe00000)];
    ranges(&[low[0],low[1],(0x100000000,0x104000000)]);
    let allocation = resident::ResidentDmaBuffer::allocate_zeroed(SIZE - 123, 4096)
        .expect("PPGTT resident assets must not require a contiguous low-DMA range");
    assert_eq!(allocation.phys(), 0x100000000);
    assert_eq!(allocation.bytes(), SIZE);
    phys::RANGES.with(|ranges| assert_eq!(&ranges.borrow()[..2], &low));
    assert!(dma::bytes(allocation.phys()).iter().all(|byte| *byte == 0));
    assert!(allocation.write(SIZE - 4, &[1,2,3,4]));
    assert!(!allocation.write(SIZE - 3, &[5,6,7,8]));
    assert_eq!(&dma::bytes(allocation.phys())[SIZE - 4..], &[1,2,3,4]);
    allocation.flush();
    drop(allocation);
    assert_eq!(dma::allocation_count(), 0);
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-dma-') as tmp:
    src, exe = Path(tmp) / 'tests.rs', Path(tmp) / 'tests'
    src.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
