use core::{
    alloc::Layout,
    convert::TryFrom,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
    time::Duration,
};

use crab_usb::{err::USBError, impl_trait, Kernel, USBHost};
use dma_api::{Direction, Osal};
use futures::{future::BoxFuture, task::noop_waker, FutureExt};
use spin::{Mutex, Once};

use crate::{debugconf, dma, xhci};

static USB_HOST: Mutex<Option<USBHost>> = Mutex::new(None);
static INIT: AtomicBool = AtomicBool::new(false);

static DMA_OSAL: DmaOsal = DmaOsal;
static DMA_ONCE: Once<()> = Once::new();

struct DmaOsal;

impl Osal for DmaOsal {
    fn map(&self, addr: NonNull<u8>, _size: usize, _direction: Direction) -> u64 {
        dma::virt_to_phys(addr.as_ptr()).expect("dma: pointer outside HHDM")
    }

    fn unmap(&self, _addr: NonNull<u8>, _size: usize) {}

    fn flush(&self, _addr: NonNull<u8>, _size: usize) {
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    fn invalidate(&self, _addr: NonNull<u8>, _size: usize) {
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    unsafe fn alloc(&self, _dma_mask: u64, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::null_mut();
        }

        dma::alloc(layout.size(), layout.align())
            .map(|(_, ptr)| ptr)
            .unwrap_or(core::ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        dma::dealloc(ptr, layout.size());
    }
}

struct CrabKernel;

impl_trait! {
    impl Kernel for CrabKernel {
        fn sleep<'a>(duration: Duration) -> BoxFuture<'a, ()> {
            async move {
                busy_sleep(duration);
            }.boxed()
        }

        fn page_size() -> usize {
            4096
        }
    }
}

#[derive(Debug)]
enum UsbInitError {
    NoController,
    Host(USBError),
}

pub fn init_once() {
    if INIT.swap(true, Ordering::AcqRel) {
        return;
    }

    match try_init() {
        Ok(()) => debugconf!("crab-usb: init ok\n"),
        Err(err) => {
            debugconf!("crab-usb: init failed: {:?}\n", err);
            INIT.store(false, Ordering::Release);
        }
    }
}

fn try_init() -> Result<(), UsbInitError> {
    ensure_dma_api_initialized();

    let info = xhci::controller_info().ok_or(UsbInitError::NoController)?;
    let dma_mask_bits = if info.supports_64bit {
        u64::MAX
    } else {
        (1u64 << 32) - 1
    };
    let dma_mask = usize::try_from(dma_mask_bits).unwrap_or(usize::MAX);
    debugconf!("crab-usb: dma mask=0x{:X}\n", dma_mask);

    let mut host = USBHost::new_xhci(info.mmio_base, dma_mask);

    block_on(async {
        host.init().await
    })
    .map_err(UsbInitError::Host)?;

    USB_HOST.lock().replace(host);
    Ok(())
}

fn ensure_dma_api_initialized() {
    DMA_ONCE.call_once(|| {
        dma_api::init(&DMA_OSAL);
    });
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = future;
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    loop {
        let poll = unsafe { Pin::new_unchecked(&mut future) }.poll(&mut cx);
        match poll {
            Poll::Ready(val) => return val,
            Poll::Pending => busy_sleep(Duration::from_micros(50)),
        }
    }
}

fn busy_sleep(duration: Duration) {
    let micros = duration.as_micros().min(u128::from(u64::MAX));
    let iterations = (micros as u64).saturating_mul(64).max(1);
    for _ in 0..iterations {
        core::hint::spin_loop();
    }
}