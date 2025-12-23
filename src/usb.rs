use crate::{debugconf, xhci};
use crab_usb::{impl_trait, BoxFuture, Kernel, USBHost};
use futures::{future::ready, FutureExt};
use spin::{Mutex, Once};
use core::time::Duration;

const LEGACY_DMA_MASK: usize = 0xFFFF_FFFF;

static USB_INIT: Once<()> = Once::new();
static USB_HOST: Mutex<Option<USBHost>> = Mutex::new(None);

pub fn init_crab_controller() {
    USB_INIT.call_once(|| {
        if let Some(info) = xhci::controller_info() {
            let mask = dma_mask(info.supports_64bit);
            let host = USBHost::new_xhci(info.mmio_base, mask);

            *USB_HOST.lock() = Some(host);

            debugconf!(
                "usb: CrabUSB host registered bus={:02X}:{:02X}.{} mmio=0x{:X} mask=0x{:X}\n",
                info.bus,
                info.slot,
                info.function,
                info.mmio_base.as_ptr() as usize,
                mask,
            );
            
        } else {
            debugconf!("usb: xHCI controller info missing\n");
        }
    });
}

fn dma_mask(supports_64bit: bool) -> usize {
    if supports_64bit {
        usize::MAX
    } else {
        LEGACY_DMA_MASK
    }
}

struct KernelImpl;

impl_trait! {
    impl Kernel for KernelImpl {
        fn sleep<'a>(_duration: Duration) -> BoxFuture<'a, ()> {
            ready(()).boxed()
        }

        fn page_size() -> usize {
            4096
        }
    }
}
