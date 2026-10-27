#!/usr/bin/env python3
"""Exercise the production R8 allocator when only high memory can fit a mask."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


source = '\n'.join(constant('src/dma.rs', name)
                   for name in ('MIN_DMA_BASE', 'DMA_MAX_PHYS'))
source += item('src/dma.rs', 'alloc_ppgtt_phys')
source += constant('src/intel/gpgpu/rcs/constants.rs',
                   'DIRECT_RCS_FONT_COVERAGE_MASK_MAX_BYTES')
source += item('src/intel/gpgpu/types/surfaces.rs', 'GpgpuMask8Surface')
surfaces = (ROOT / 'src/intel/gpgpu/types/surfaces.rs').read_text()
impl_start = surfaces.index('impl GpgpuMask8Surface {')
impl_end = surfaces.index('\n}', impl_start) + 2
source += surfaces[impl_start:impl_end]
source += item('src/intel/gpgpu/operations/primitives.rs',
               'allocate_font_coverage_mask').replace('super::WARM_ALIGN', 'WARM_ALIGN') \
    .replace('super::dma_flush', 'intel::dma_flush')
source += r'''
use std::{cell::{Cell, RefCell}, sync::atomic::AtomicBool};
const WARM_ALIGN: usize = 4096;
struct GpgpuOwnedMask8Surface {
    surface: GpgpuMask8Surface,
    virt: *mut u8,
    quarantined: AtomicBool,
}
fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value.checked_add(alignment - 1).map(|end| end & !(alignment - 1))
}
thread_local! { static VA_AVAILABLE: Cell<bool> = const { Cell::new(true) }; }
fn reserve_font_coverage_gpu_va(_: usize) -> Option<u64> {
    VA_AVAILABLE.with(|available| available.get().then_some(0x20000000))
}
fn recycle_font_coverage_gpu_va(_: u64, _: usize) {}
mod intel { pub fn dma_flush(_: *mut u8, _: usize) {} }
mod phys {
    use super::RefCell;
    thread_local! { pub static RANGES: RefCell<Vec<(u64,u64)>> = const { RefCell::new(Vec::new()) }; }
    pub fn alloc_phys_range(size: usize, align: usize, min: u64, max: Option<u64>) -> Option<u64> {
        RANGES.with(|ranges| {
            for range in ranges.borrow_mut().iter_mut() {
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
// Only physical selection and CPU mapping are mocked. The production R8
// allocator and surface validation keep their real padding and error cleanup.
mod dma {
    use super::RefCell;
    thread_local! { static BACKING: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) }; }
    fn mapped(phys: u64, bytes: usize) -> Option<(u64, *mut u8)> {
        let mut backing = vec![0xa5; bytes];
        let pointer = backing.as_mut_ptr();
        BACKING.with(|all| all.borrow_mut().push(backing));
        Some((phys, pointer))
    }
    pub fn alloc_ppgtt(bytes: usize, align: usize) -> Option<(u64, *mut u8)> {
        mapped(super::alloc_ppgtt_phys(bytes, align)?, bytes)
    }
    #[allow(dead_code)]
    pub fn alloc(bytes: usize, align: usize) -> Option<(u64, *mut u8)> {
        mapped(super::phys::alloc_phys_range(bytes, align,
            super::MIN_DMA_BASE, Some(super::DMA_MAX_PHYS))?, bytes)
    }
    pub fn dealloc(pointer: *mut u8, bytes: usize) {
        BACKING.with(|all| {
            let mut all = all.borrow_mut();
            let index = all.iter().position(|backing| backing.as_ptr() == pointer.cast_const())
                .expect("free the original CPU backing");
            assert_eq!(all[index].len(), bytes);
            all.remove(index);
        });
    }
    pub fn allocations() -> usize { BACKING.with(|all| all.borrow().len()) }
}
fn ranges(value: &[(u64,u64)]) { phys::RANGES.with(|ranges| *ranges.borrow_mut() = value.to_vec()); }
#[test] fn font_mask_uses_high_memory_when_low_dma_is_fragmented() {
    let low = [(0x100000,0x500000),(0x800000,0xe00000)];
    ranges(&[low[0],low[1],(0x100000000,0x104000000)]);
    let mask = allocate_font_coverage_mask(4095,2048)
        .expect("an 8-MiB R8 atlas must fit high physical memory");
    assert_eq!(mask.surface.phys, 0x100000000);
    assert_eq!(mask.surface.gpu, 0x20000000);
    assert_eq!(mask.surface.pitch_bytes, 4096);
    assert_eq!(mask.surface.bytes, 8 * 1024 * 1024);
    assert!(mask.surface.is_valid());
    assert!(!mask.quarantined.load(std::sync::atomic::Ordering::Relaxed));
    phys::RANGES.with(|ranges| assert_eq!(&ranges.borrow()[..2], &low));
    assert!(unsafe { std::slice::from_raw_parts(mask.virt, mask.surface.bytes) }
        .iter().all(|&byte| byte == 0));
    dma::dealloc(mask.virt, mask.surface.bytes);
    assert_eq!(dma::allocations(), 0);
}
#[test] fn rejected_virtual_reservation_releases_high_backing() {
    ranges(&[(0x100000000,0x104000000)]);
    VA_AVAILABLE.with(|available| available.set(false));
    assert!(allocate_font_coverage_mask(4095,2048).is_none());
    assert_eq!(dma::allocations(), 0);
}
#[test] fn empty_oversized_and_exhausted_masks_fail_without_retained_backing() {
    ranges(&[(0x100000,0x200000),(0x100000000,0x100100000)]);
    assert!(allocate_font_coverage_mask(0,100).is_none());
    assert!(allocate_font_coverage_mask(8192,8192).is_none());
    assert!(allocate_font_coverage_mask(4096,2048).is_none());
    assert_eq!(dma::allocations(), 0);
}
'''

with tempfile.TemporaryDirectory(prefix='trueos-font-dma-') as directory:
    code, executable = Path(directory) / 'tests.rs', Path(directory) / 'tests'
    code.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(code), '-o', str(executable)],
                   check=True)
    subprocess.run([str(executable)], check=True)
