use core::ptr::NonNull;
use dma_api::{Direction, Osal};
use spin::Once;

static DMA_OSAL_ONCE: Once<()> = Once::new();
static DMA_OSAL: DmaOsal = DmaOsal;

pub fn ensure_dma_api_initialized() {
    DMA_OSAL_ONCE.call_once(|| {
        dma_api::init(&DMA_OSAL);
    });
}

struct DmaOsal;

impl Osal for DmaOsal {
    fn map(&self, addr: NonNull<u8>, _size: usize, _direction: Direction) -> u64 {
        match super::dma::virt_to_phys(addr.as_ptr()) {
            Some(phys) => phys,
            None => {
                crate::log!(
                    "usb: dma_osal map failed for virt=0x{:X}\n",
                    addr.as_ptr() as usize
                );
                0
            }
        }
    }

    unsafe fn alloc(&self, _dma_mask: u64, layout: core::alloc::Layout) -> *mut u8 {
        let align = layout.align().max(64);
        match super::dma::alloc_with_mask(layout.size(), align, _dma_mask) {
            Some((_phys, virt)) => virt,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        super::dma::dealloc(ptr, layout.size());
    }

    fn unmap(&self, _addr: NonNull<u8>, _size: usize) {}
    fn flush(&self, _addr: NonNull<u8>, _size: usize) {}
    fn invalidate(&self, _addr: NonNull<u8>, _size: usize) {}
}
