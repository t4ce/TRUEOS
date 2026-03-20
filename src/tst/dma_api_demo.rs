/// DMA API demo - demonstrates usage of the dma-api crate
/// Shows basic allocation, coherent memory, and DMA buffer operations
use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use dma_api::{DeviceDma, DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};

/// Minimal DMA implementation for demo purposes
/// (In production, use the one from usb2/crabusb_service.rs)
pub struct DemoDmaImpl;

impl DmaOp for DemoDmaImpl {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        _dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        _align: usize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let layout = Layout::from_size_align(size.get(), 1).map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;
        Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        drop(handle);
    }

    unsafe fn alloc_coherent(&self, _dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let (phys, virt) = crate::pci::dma::alloc_with_max(layout.size(), layout.align(), None)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::pci::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

static DEMO_DMA_KERNEL: DemoDmaImpl = DemoDmaImpl;

/// Run a quick demo of dma-api capabilities
pub fn demo_dma_api() {
    crate::log!("=== DMA API Demo ===\n");

    let device = DeviceDma::new(0xFFFFFFFF, &DEMO_DMA_KERNEL);

    // Demo 1: Allocate a DMA array
    crate::log!("Demo 1: DMA Array allocation\n");
    match device.array_zero_with_align::<u32>(16, 64, DmaDirection::ToDevice) {
        Ok(mut arr) => {
            let _ = arr.set(0, 0xDEADBEEF);
            let _ = arr.set(1, 0xCAFEBABE);

            if let Some(v0) = arr.read(0) {
                crate::log!("  Array[0] = 0x{:08X}\n", v0);
            }
            if let Some(v1) = arr.read(1) {
                crate::log!("  Array[1] = 0x{:08X}\n", v1);
            }

            let dma_addr = arr.dma_addr();
            crate::log!("  DMA address: 0x{:X}\n", dma_addr.as_u64());
        }
        Err(e) => {
            crate::log!("  Failed to allocate array: {:?}\n", e);
        }
    }

    // Demo 2: Allocate a DMA box (single value)
    crate::log!("\nDemo 2: DMA Box allocation\n");

    #[repr(C)]
    #[derive(Copy, Clone, Default, Debug)]
    struct DemoDescriptor {
        addr: u64,
        length: u32,
        flags: u32,
    }

    match device.box_zero_with_align::<DemoDescriptor>(64, DmaDirection::Bidirectional) {
        Ok(mut dma_box) => {
            dma_box.write(DemoDescriptor {
                addr: 0x12345678,
                length: 4096,
                flags: 0x01,
            });

            let desc = dma_box.read();
            crate::log!(
                "  Descriptor: addr=0x{:X}, len={}, flags=0x{:X}\n",
                desc.addr,
                desc.length,
                desc.flags
            );

            let box_dma_addr = dma_box.dma_addr();
            crate::log!("  DMA address: 0x{:X}\n", box_dma_addr.as_u64());
        }
        Err(e) => {
            crate::log!("  Failed to allocate box: {:?}\n", e);
        }
    }

    // Demo 3: DmaAddr operations
    crate::log!("\nDemo 3: DmaAddr operations\n");
    let base_addr = DmaAddr::from(0x1000u64);
    crate::log!("  Base DMA address: 0x{:X}\n", base_addr.as_u64());

    // Try to add to DMA address
    if let Some(offset_addr) = base_addr.checked_add(0x100) {
        crate::log!("  After checked_add(0x100): 0x{:X}\n", offset_addr.as_u64());
    }

    // Demo overflow case
    let high_addr = DmaAddr::from(0xFFFFFFFFFFFFFF00u64);
    if let Some(overflowed) = high_addr.checked_add(0x200) {
        crate::log!(
            "  High address overflow result: 0x{:X}\n",
            overflowed.as_u64()
        );
    } else {
        crate::log!("  High address overflow detected (checked_add returned None)\n");
    }

    // Demo 4: DmaError handling
    crate::log!("\nDemo 4: DmaError cases\n");

    // Try allocating with invalid alignment (zero layout size should error)
    match Layout::from_size_align(0, 1) {
        Ok(zero_layout) => match device.box_zero_with_align::<u32>(64, DmaDirection::ToDevice) {
            Ok(_) => crate::log!("  Unexpected success\n"),
            Err(DmaError::NoMemory) => crate::log!("  Got DmaError::NoMemory\n"),
            Err(DmaError::LayoutError(_)) => {
                crate::log!("  Got DmaError::LayoutError\n")
            }
            Err(e) => crate::log!("  Got other error: {:?}\n", e),
        },
        Err(_) => crate::log!("  Layout creation failed as expected\n"),
    }

    // Show error variant names
    crate::log!("  Example error types:\n");
    crate::log!("    - DmaError::NoMemory\n");
    crate::log!("    - DmaError::LayoutError\n");
    crate::log!("    - DmaError::DmaMaskNotMatch\n");
    crate::log!("    - DmaError::AlignMismatch\n");

    crate::log!("=== DMA API Demo Complete ===\n");
}
