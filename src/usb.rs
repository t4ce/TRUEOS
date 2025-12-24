use crate::{debugconf, osal, xhci};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::time::Duration;
use embassy_time::{Duration as EmbassyDuration, Timer};
use crab_usb::{err::USBError, impl_trait, BoxFuture, InterfaceDescriptor, Kernel, USBHost};
use futures::FutureExt;
use spin::Mutex;
use x86_64::registers::debug;

const LEGACY_DMA_MASK: usize = 0xFFFF_FFFF;

static USB_HOST: Mutex<Option<USBHost>> = Mutex::new(None);

fn take_host_for_async() -> Option<USBHost> {
    USB_HOST.lock().take()
}

fn is_boot_keyboard(desc: &InterfaceDescriptor) -> bool {
    desc.class == 0x03 && desc.subclass == 0x01 && desc.protocol == 0x01
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
pub async fn crab_usb_init_task(info: xhci::ControllerInfo) {
    osal::ensure_dma_api_initialized();
    let mask = dma_mask(info.supports_64bit);
    let mut host = USBHost::new_xhci(info.mmio_base, mask);

    if let Err(err) = host.init().await {
        debugconf!("usb: init failed: {:?}\n", err);
        return;
    }
    debugconf!("usb: init ok\n");

    debugconf!("usb: device_list start\n");

    {
        let handler = host.event_handler();
        let mut device_list_future = host.device_list();
        let waker = dummy_waker();
        let mut cx = Context::from_waker(&waker);

        let mut spin: u32 = 0;
        let device_list_result: Result<alloc::vec::Vec<_>, USBError> = loop {
            let mut pinned = unsafe { Pin::new_unchecked(&mut device_list_future) };
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(res) => break res.map(|iter| iter.collect()),
                Poll::Pending => {
                    handler.handle_event();
                    spin = spin.wrapping_add(1);
                    if spin == 1 {
                        debugconf!("usb: device_list pending first poll\n");
                    }
                    if spin % 1000 == 0 {
                        debugconf!("usb: device_list waiting...\n");
                    }
                    core::hint::spin_loop();
                }
            }
        };

        match device_list_result {
            Ok(devices) => {
                debugconf!("usb: device_list done: {} devices\n", devices.len());
                for device in devices {
                    debugconf!("usb: detected device {}\n", device);
                }
            }
            Err(err) => debugconf!("usb: device list failed: {:?}\n", err),
        }
    }
    *USB_HOST.lock() = Some(host);
}

pub fn poll_crab_events_once() -> bool {
    let handler: Option<crab_usb::EventHandler> = {
        let mut host = USB_HOST.lock();
        host.as_mut().map(|h| h.event_handler())
    };

    if let Some(handler) = handler {
        handler.handle_event()
    } else {
        false
    }
}

fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}

unsafe fn waker_clone(_: *const ()) -> RawWaker {
    dummy_raw_waker()
}
unsafe fn waker_wake(_: *const ()) {}
unsafe fn waker_wake_by_ref(_: *const ()) {}
unsafe fn waker_drop(_: *const ()) {}

static DUMMY_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    waker_clone,
    waker_wake,
    waker_wake_by_ref,
    waker_drop,
);

fn dummy_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &DUMMY_WAKER_VTABLE)
}
