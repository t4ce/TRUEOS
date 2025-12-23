use crate::{debugconf, xhci};
use core::{ptr::read_volatile, ptr::NonNull, time::Duration};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use crab_usb::{impl_trait, BoxFuture, Kernel, USBHost};
use dma_api::{Direction, Osal};
use futures::FutureExt;
use spin::{Mutex, Once};

const LEGACY_DMA_MASK: usize = 0xFFFF_FFFF;

static USB_INIT: Once<()> = Once::new();
static USB_HOST: Mutex<Option<USBHost>> = Mutex::new(None);
static DMA_OSAL_ONCE: Once<()> = Once::new();

fn ensure_dma_api_initialized() {
    DMA_OSAL_ONCE.call_once(|| {
        dma_api::init(&DMA_OSAL);
    });
}

struct DmaOsal;

impl Osal for DmaOsal {
    fn map(&self, addr: NonNull<u8>, _size: usize, _direction: Direction) -> u64 {
        match crate::dma::virt_to_phys(addr.as_ptr()) {
            Some(phys) => phys,
            None => {
                debugconf!(
                    "usb: dma_osal map failed for virt=0x{:X}\n",
                    addr.as_ptr() as usize
                );
                0
            }
        }
    }

    unsafe fn alloc(&self, _dma_mask: u64, layout: core::alloc::Layout) -> *mut u8 {
        let align = layout.align().max(64);
        match crate::dma::alloc(layout.size(), align) {
            Some((_phys, virt)) => virt,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        crate::dma::dealloc(ptr, layout.size());
    }

    fn unmap(&self, _addr: NonNull<u8>, _size: usize) {}
    fn flush(&self, _addr: NonNull<u8>, _size: usize) {}
    fn invalidate(&self, _addr: NonNull<u8>, _size: usize) {}
}

static DMA_OSAL: DmaOsal = DmaOsal;

pub fn init_crab_controller(spawner: &Spawner) {
    USB_INIT.call_once(|| {
        if let Some(info) = xhci::controller_info() {
            if let Err(e) = spawner.spawn(crab_usb_init_task(info)) {
                debugconf!("usb: failed to spawn init task: {:?}\n", e);
            }
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
        fn sleep<'a>(duration: Duration) -> BoxFuture<'a, ()> {
            async move {
                let micros = duration.as_micros().min(u64::MAX as u128) as u64;
                Timer::after(EmbassyDuration::from_micros(micros)).await;
            }
            .boxed()
        }

        fn page_size() -> usize {
            4096
        }
    }
}

#[embassy_executor::task]
async fn crab_usb_init_task(info: xhci::ControllerInfo) {
    ensure_dma_api_initialized();
    let mask = dma_mask(info.supports_64bit);

    let mut host = USBHost::new_xhci(info.mmio_base, mask);

    debugconf!(
        "usb: starting CrabUSB init bus={:02X}:{:02X}.{} mmio=0x{:X} mask=0x{:X}\n",
        info.bus,
        info.slot,
        info.function,
        info.mmio_base.as_ptr() as usize,
        mask,
    );

    if let Err(e) = host.init().await {
        debugconf!("usb: CrabUSB init failed: {:?}\n", e);
        return;
    }

    debugconf!("usb: CrabUSB init ok\n");

    *USB_HOST.lock() = Some(host);

    debugconf!(
        "usb: CrabUSB host registered bus={:02X}:{:02X}.{} mmio=0x{:X} mask=0x{:X}\n",
        info.bus,
        info.slot,
        info.function,
        info.mmio_base.as_ptr() as usize,
        mask,
    );
}
