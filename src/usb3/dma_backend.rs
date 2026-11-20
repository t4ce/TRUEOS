//! Direct, cache-coherent x86 DMA backend for CrabUSB.

use core::{alloc::Layout, num::NonZeroUsize, ptr::NonNull};
use dma_api::{
    DmaAddr, DmaAllocHandle, DmaConstraints, DmaDirection, DmaError, DmaMapHandle, DmaOp,
};

use super::lib::TrueosCrabKernel;

/// Fold boundary constraints into the allocation alignment so a contiguous
/// allocation cannot straddle a device boundary. The PMM accepts power-of-two
/// alignment, so unsupported boundary layouts fail rather than being ignored.
fn allocation_layout(constraints: DmaConstraints, layout: Layout) -> Result<Layout, DmaError> {
    let mut align = layout.align().max(constraints.align.max(1));
    if let Some(max) = constraints.max_segment_size {
        if layout.size() > max {
            return Err(DmaError::SegmentTooLarge {
                size: layout.size(),
                max,
            });
        }
    }
    if let Some(boundary) = constraints.boundary {
        // Validate before using the boundary for alignment or division.
        Layout::from_size_align(layout.size(), boundary)?;
        if layout.size() > boundary {
            return Err(DmaError::BoundaryCross {
                addr: DmaAddr::from(0),
                size: layout.size(),
                boundary,
            });
        }
        align = align.max(boundary);
    }
    Ok(Layout::from_size_align(layout.size(), align)?)
}

fn range_fits(phys: u64, size: usize, constraints: DmaConstraints, align: usize) -> bool {
    let Some(end) = phys.checked_add(size.saturating_sub(1) as u64) else {
        return false;
    };
    end <= constraints.addr_mask
        && phys.is_multiple_of(align as u64)
        && constraints.max_segment_size.is_none_or(|max| size <= max)
        && constraints
            .boundary
            .is_none_or(|boundary| boundary != 0 && phys / boundary as u64 == end / boundary as u64)
}

/// A virtually contiguous slice need not be physically contiguous. Check each
/// page and the last byte before exposing one device-visible address range.
fn contiguous_phys(addr: NonNull<u8>, size: usize) -> Option<u64> {
    let start = addr.as_ptr() as usize;
    let last = start.checked_add(size.checked_sub(1)?)?;
    let phys = crate::phys::virt_to_phys_checked(addr.as_ptr())?;
    let mut next = (start | 4095).checked_add(1)?;
    while next <= last {
        if crate::phys::virt_to_phys_checked(next as *const u8)?
            != phys.checked_add((next - start) as u64)?
        {
            return None;
        }
        next = next.checked_add(4096)?;
    }
    if crate::phys::virt_to_phys_checked(last as *const u8)?
        != phys.checked_add((last - start) as u64)?
    {
        return None;
    }
    Some(phys)
}

impl DmaOp for TrueosCrabKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn alloc_contiguous(
        &self,
        constraints: DmaConstraints,
        layout: Layout,
    ) -> Option<DmaAllocHandle> {
        let layout = allocation_layout(constraints, layout).ok()?;
        // Preserve the existing controller-metadata allocation policy. Some
        // attached xHCI devices require low memory even on a 64-bit platform.
        let max_phys = Some(
            constraints
                .addr_mask
                .checked_add(1)
                .unwrap_or(u64::MAX)
                .min(0x1_0000_0000),
        );
        let (phys, virt) = crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys)?;
        if !range_fits(phys, layout.size(), constraints, layout.align()) {
            crate::dma::dealloc(virt, layout.size());
            return None;
        }
        let ptr = NonNull::new(virt)?;
        Some(unsafe { DmaAllocHandle::new(ptr, ptr, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_contiguous(&self, handle: DmaAllocHandle) {
        crate::dma::dealloc(handle.allocation_ptr().as_ptr(), handle.size());
    }

    unsafe fn alloc_coherent(
        &self,
        _constraints: DmaConstraints,
        _layout: Layout,
    ) -> Option<DmaAllocHandle> {
        // This hook is exclusively for NON-coherent devices. The native x86
        // backend has no uncached-alias allocator; do not claim to provide one.
        // Coherent devices use alloc_contiguous through DeviceDma instead.
        None
    }

    unsafe fn dealloc_coherent(&self, _handle: DmaAllocHandle) -> Result<(), DmaError> {
        // No allocations can originate from the unsupported branch above.
        // Retain any unexpected handle rather than recycling unknown aliases.
        Err(DmaError::CoherentReleaseFailed)
    }

    unsafe fn map_streaming(
        &self,
        constraints: DmaConstraints,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let size = size.get();
        let requested = Layout::from_size_align(size, constraints.align.max(1))?;
        let bounce_layout = allocation_layout(constraints, requested)?;
        if let Some(phys) = contiguous_phys(addr, size) {
            if range_fits(phys, size, constraints, requested.align()) {
                return Ok(unsafe {
                    DmaMapHandle::new(addr, DmaAddr::from(phys), requested, None)
                });
            }
        }

        // The original buffer may be high, unaligned, or physically scattered.
        // The library copies to/from this bounce allocation at the explicit
        // prepare_for_device / complete_for_cpu transitions, even for coherent
        // devices. Mapping alone does not transfer CPU/device ownership.
        let max_phys = constraints.addr_mask.checked_add(1);
        let (phys, virt) = crate::dma::alloc_with_max(size, bounce_layout.align(), max_phys)
            .ok_or(DmaError::NoMemory)?;
        if !range_fits(phys, size, constraints, bounce_layout.align()) {
            crate::dma::dealloc(virt, size);
            return Err(DmaError::DmaMaskNotMatch {
                addr: DmaAddr::from(phys),
                mask: constraints.addr_mask,
            });
        }
        let bounce = NonNull::new(virt).ok_or(DmaError::NullPointer)?;
        Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), bounce_layout, Some(bounce)) })
    }

    unsafe fn unmap_streaming(&self, handle: DmaMapHandle) {
        if let Some(virt) = handle.bounce_ptr() {
            crate::dma::dealloc(virt.as_ptr(), handle.size());
        }
    }
}
