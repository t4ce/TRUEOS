#![cfg(test)]

use std::{alloc::Layout, cell::RefCell, collections::HashMap, ptr::NonNull};
use dma_api::{DeviceDma, DmaCoherency, DmaConstraints, DmaDeviceInfo, DmaDirection, DmaDomainId, DmaError};

mod usb3 {
    pub mod lib {
        pub struct TrueosCrabKernel;
    }
    #[path = "@BACKEND@"]
    mod dma_backend;
}

static KERNEL: usb3::lib::TrueosCrabKernel = usb3::lib::TrueosCrabKernel;

#[derive(Default)]
struct State {
    pages: HashMap<usize, u64>,
    allocations: HashMap<usize, (Layout, u64)>,
    frees: usize,
    requests: Vec<(usize, usize, Option<u64>)>,
    forced_phys: Option<u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

// Replace only PMM allocation and virtual-to-physical translation. All DMA
// handle validation and ownership transitions use the actual production code.
mod phys {
    pub fn virt_to_phys_checked<T>(ptr: *const T) -> Option<u64> {
        let address = ptr as usize;
        super::STATE.with(|state| state.borrow().pages.get(&(address & !4095))
            .map(|phys| phys + (address & 4095) as u64))
    }
}

mod dma {
    use super::*;

    pub fn alloc_with_max(size: usize, align: usize, max: Option<u64>) -> Option<(u64, *mut u8)> {
        let backing = Layout::from_size_align(size.max(1).next_multiple_of(4096), align.max(4096)).ok()?;
        let pointer = NonNull::new(unsafe { std::alloc::alloc(backing) })?;
        unsafe { pointer.as_ptr().write_bytes(0xa5, backing.size()); }
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.requests.push((size, align, max));
            let phys = state.forced_phys.take().unwrap_or(0x0100_0000u64.next_multiple_of(align as u64));
            for offset in (0..backing.size()).step_by(4096) {
                state.pages.insert(pointer.as_ptr() as usize + offset, phys + offset as u64);
            }
            state.allocations.insert(pointer.as_ptr() as usize, (backing, phys));
            Some((phys, pointer.as_ptr()))
        })
    }

    pub fn dealloc(pointer: *mut u8, size: usize) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let (layout, _) = state.allocations.remove(&(pointer as usize))
                .expect("must release the original allocator-owned pointer exactly once");
            assert!(size <= layout.size());
            for offset in (0..layout.size()).step_by(4096) {
                state.pages.remove(&(pointer as usize + offset));
            }
            state.frees += 1;
            unsafe { std::alloc::dealloc(pointer, layout); }
        });
    }
}

struct Input {
    pointer: NonNull<u8>,
    layout: Layout,
}

impl Input {
    fn new(physical_pages: &[u64]) -> Self {
        let layout = Layout::from_size_align(physical_pages.len() * 4096, 4096).unwrap();
        let pointer = NonNull::new(unsafe { std::alloc::alloc_zeroed(layout) }).unwrap();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            for (index, physical) in physical_pages.iter().enumerate() {
                state.pages.insert(pointer.as_ptr() as usize + index * 4096, *physical);
            }
        });
        Self { pointer, layout }
    }

    fn bytes(&mut self, offset: usize, len: usize) -> &mut [u8] {
        assert!(offset + len <= self.layout.size());
        unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr().add(offset), len) }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            for offset in (0..self.layout.size()).step_by(4096) {
                state.pages.remove(&(self.pointer.as_ptr() as usize + offset));
            }
        });
        unsafe { std::alloc::dealloc(self.pointer.as_ptr(), self.layout); }
    }
}

fn device(constraints: DmaConstraints) -> DeviceDma {
    DeviceDma::new(DmaDeviceInfo::new(DmaDomainId::Direct, DmaCoherency::Coherent, constraints), &KERNEL)
}

fn assert_released(frees: usize) {
    STATE.with(|state| {
        let state = state.borrow();
        assert!(state.allocations.is_empty(), "DMA storage must not leak");
        assert_eq!(state.frees, frees);
    });
}

#[test]
fn reachable_contiguous_input_uses_original_memory() {
    let mut input = Input::new(&[0x0020_0000]);
    let bytes = input.bytes(0, 64);
    bytes.fill(0x42);
    let map = device(DmaConstraints::new(u32::MAX as u64))
        .map_streaming_slice(bytes, 64, DmaDirection::Bidirectional).unwrap();
    assert_eq!(map.dma_addr().as_u64(), 0x0020_0000);
    assert!(map.bounce_ptr().is_none());
    map.prepare_for_device(0..64);
    map.complete_for_cpu(0..64);
    assert_eq!(map.to_vec_cpu(), vec![0x42; 64]);
    drop(map);
    assert_released(0);
}

#[test]
fn coherent_bounce_tx_copy_occurs_at_explicit_prepare_range() {
    let mut input = Input::new(&[0x1_0020_0000]);
    let bytes = input.bytes(0, 8);
    bytes.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let map = device(DmaConstraints::new(u32::MAX as u64))
        .map_streaming_slice(bytes, 64, DmaDirection::ToDevice).unwrap();
    let bounce = map.bounce_ptr().expect("high memory must bounce");
    let observed = || unsafe { std::slice::from_raw_parts(bounce.as_ptr(), 8).to_vec() };
    assert_eq!(observed(), vec![0xa5; 8], "mapping does not submit CPU data");
    map.prepare_for_device(2..6);
    assert_eq!(observed(), vec![0xa5, 0xa5, 3, 4, 5, 6, 0xa5, 0xa5]);
    drop(map);
    assert_released(1);
    assert_eq!(input.bytes(0, 8), &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn coherent_bounce_rx_copy_occurs_at_explicit_complete_range() {
    let mut input = Input::new(&[0x1_0020_0000]);
    let map = device(DmaConstraints::new(u32::MAX as u64))
        .map_streaming_slice(input.bytes(0, 8), 64, DmaDirection::FromDevice).unwrap();
    let bounce = map.bounce_ptr().unwrap();
    map.prepare_for_device(0..8);
    unsafe { bounce.as_ptr().copy_from_nonoverlapping([11, 12, 13, 14, 15, 16, 17, 18].as_ptr(), 8); }
    assert_eq!(map.to_vec_cpu(), vec![0; 8], "device data is not visible until completion");
    map.complete_for_cpu(1..7);
    assert_eq!(map.to_vec_cpu(), vec![0, 12, 13, 14, 15, 16, 17, 0]);
    drop(map);
    assert_released(1);
}

#[test]
fn physically_scattered_middle_page_bounces_even_when_endpoints_match() {
    let mut input = Input::new(&[0x0020_0000, 0x0030_0000, 0x0020_2000]);
    let map = device(DmaConstraints::new(u64::MAX))
        .map_streaming_slice(input.bytes(0, 3 * 4096), 1, DmaDirection::ToDevice).unwrap();
    assert!(map.bounce_ptr().is_some(), "must validate every page, not only first and last");
    drop(map);
    assert_released(1);
}

#[test]
fn address_mask_applies_to_last_byte_too() {
    let mut input = Input::new(&[0xffff_f000, 0x1_0000_0000]);
    let map = device(DmaConstraints::new(u32::MAX as u64))
        .map_streaming_slice(input.bytes(4096 - 32, 64), 1, DmaDirection::ToDevice).unwrap();
    assert!(map.bounce_ptr().is_some());
    assert!(map.dma_addr().as_u64() + 63 <= u32::MAX as u64);
    drop(map);
    assert_released(1);
}

#[test]
fn alignment_requires_bounce_for_unaligned_physical_start() {
    let mut input = Input::new(&[0x0020_0000]);
    let map = device(DmaConstraints::new(u32::MAX as u64))
        .map_streaming_slice(input.bytes(1, 64), 64, DmaDirection::ToDevice).unwrap();
    assert!(map.bounce_ptr().is_some());
    assert_eq!(map.dma_addr().as_u64() % 64, 0);
    drop(map);
    assert_released(1);
}

#[test]
fn boundary_crossing_input_bounces_into_one_boundary() {
    let mut input = Input::new(&[0x0020_0000, 0x0020_1000]);
    let constraints = DmaConstraints::new(u32::MAX as u64).with_boundary(4096);
    let map = device(constraints)
        .map_streaming_slice(input.bytes(4096 - 64, 128), 1, DmaDirection::ToDevice).unwrap();
    assert!(map.bounce_ptr().is_some());
    let dma = map.dma_addr().as_u64();
    assert_eq!(dma / 4096, (dma + 127) / 4096);
    drop(map);
    assert_released(1);
}

#[test]
fn impossible_segment_and_boundary_constraints_fail_without_allocating() {
    let mut input = Input::new(&[0x0020_0000]);
    assert!(matches!(device(DmaConstraints::new(u64::MAX).with_max_segment_size(32))
        .map_streaming_slice(input.bytes(0, 64), 1, DmaDirection::ToDevice),
        Err(DmaError::SegmentTooLarge { size: 64, max: 32 })));
    assert!(matches!(device(DmaConstraints::new(u64::MAX).with_boundary(32))
        .map_streaming_slice(input.bytes(0, 64), 1, DmaDirection::ToDevice),
        Err(DmaError::BoundaryCross { size: 64, boundary: 32, .. })));
    assert!(matches!(device(DmaConstraints::new(u64::MAX).with_boundary(3))
        .map_streaming_slice(input.bytes(0, 2), 1, DmaDirection::ToDevice),
        Err(DmaError::LayoutError(_))));
    assert_released(0);
}

#[test]
fn coherent_controller_metadata_is_zeroed_aligned_and_kept_below_four_gib() {
    let ring = device(DmaConstraints::new(u64::MAX).with_boundary(65536))
        .coherent_array_zero_with_align::<u64>(128, 64).unwrap();
    assert_eq!(ring.dma_addr().as_u64() % 65536, 0);
    assert!(ring.dma_addr().as_u64() + 1023 < 0x1_0000_0000);
    assert!(ring.as_slice_cpu().iter().all(|word| *word == 0));
    STATE.with(|state| assert_eq!(state.borrow().requests, vec![(1024, 65536, Some(0x1_0000_0000))]));
    drop(ring);
    assert_released(1);
}

#[test]
fn unexpected_out_of_mask_pmm_allocation_is_rejected_and_freed() {
    STATE.with(|state| state.borrow_mut().forced_phys = Some(0x1_0000_0000));
    assert!(matches!(device(DmaConstraints::new(u32::MAX as u64))
        .coherent_array_zero::<u8>(64), Err(DmaError::NoMemory)));
    assert_released(1);
}

#[test]
fn unsupported_noncoherent_mapping_policy_is_not_silently_claimed() {
    let device = DeviceDma::new(DmaDeviceInfo::new(DmaDomainId::Direct,
        DmaCoherency::NonCoherent, DmaConstraints::new(u64::MAX)), &KERNEL);
    assert!(matches!(device.coherent_array_zero::<u8>(64), Err(DmaError::NoMemory)));
    assert_released(0);
}
