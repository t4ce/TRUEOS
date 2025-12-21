use core::{hint::spin_loop, ptr::NonNull, time::Duration};

use crab_usb::{impl_trait, BoxFuture, EventHandler, FutureExt, Kernel, USBHost};

struct KernelImpl;
impl_trait! {
    impl Kernel for KernelImpl {
        fn sleep<'a>(duration: Duration) -> BoxFuture<'a, ()> {
            async move {
                // Crude busy-wait fallback until a proper timer source exists.
                let mut iterations = duration.as_micros().saturating_mul(100);
                while iterations > 0 {
                    spin_loop();
                    iterations -= 1;
                }
            }.boxed()
        }

        fn page_size() -> usize {
            4096
        }
    }
}

#[allow(dead_code)]
static mut USB_HOST: Option<USBHost> = None;
static mut USB_HANDLER: Option<EventHandler> = None;

pub async fn init_xhci_from_mmio(mmio_base: u64) {
    let Some(mmio_ptr) = NonNull::new(mmio_base as *mut u8) else {
        crate::debugcon_write_str("usb: invalid mmio base\n");
        return;
    };

    let mut host = USBHost::new_xhci(mmio_ptr, usize::MAX);
    let handler = host.event_handler();

    match host.init().await {
        Ok(()) => {
            crate::debugcon_write_str("usb: xhci init ok\n");
            unsafe {
                USB_HANDLER = Some(handler);
                USB_HOST = Some(host);
            }
        }
        Err(err) => {
            crate::debugcon_write_str("usb: xhci init failed\n");
            let _ = err;
        }
    }
}

pub fn poll_usb_events() {
    unsafe {
        if let Some(handler) = USB_HANDLER.as_ref() {
            handler.handle_event();
        }
    }
}
