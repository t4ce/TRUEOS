use core::{
	future::Future,
	hint::spin_loop,
	ptr::NonNull,
	task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
	time::Duration,
};

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

#[embassy_executor::task]
pub async fn usb_poll_task() {
	loop {
		poll_usb_events();
		yield_once().await;
	}
}

pub async fn init_xhci_from_mmio(mmio_base: u64) {
	crate::debugcon_write_str("usb: init_xhci_from_mmio enter base=");
	crate::debugcon_write_byte(b' ');
	write_hex_u64(mmio_base);
	crate::debugcon_write_byte(b'\n');
	let Some(mmio_ptr) = NonNull::new(mmio_base as *mut u8) else {
		crate::debugcon_write_str("usb: invalid mmio base\n");
		return;
	};

	let mut host = USBHost::new_xhci(mmio_ptr, usize::MAX);
	let handler = host.event_handler();

	let mut init_fut = host.init();
	// Poll the init future manually so we can service events while waiting.
	// CrabUSB expects the event handler to be driven during initialization.
	let init_result = loop {
		let mut pinned = unsafe { core::pin::Pin::new_unchecked(&mut init_fut) };
		match pinned.as_mut().poll(&mut Context::from_waker(&dummy_waker())) {
			Poll::Ready(result) => break result,
			Poll::Pending => {
				handler.handle_event();
				core::hint::spin_loop();
			}
		}
	};

	// Ensure the future is dropped before we move `host`.
	drop(init_fut);

	match init_result {
		Ok(()) => {
			crate::debugcon_write_str("usb: xhci init ok\n");
			unsafe {
				USB_HANDLER = Some(handler);
				USB_HOST = Some(host);
			}
		}
		Err(_err) => {
			crate::debugcon_write_str("usb: xhci init failed\n");
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

fn dummy_waker() -> Waker {
	// Minimal no-op waker suitable for polling futures in a spin loop.
	unsafe { Waker::from_raw(dummy_raw_waker()) }
}

unsafe fn dummy_raw_waker() -> RawWaker {
	RawWaker::new(core::ptr::null(), &DUMMY_WAKER_VTABLE)
}

unsafe fn waker_clone(_: *const ()) -> RawWaker {
	dummy_raw_waker()
}
unsafe fn waker_wake(_: *const ()) {}
unsafe fn waker_wake_by_ref(_: *const ()) {}
unsafe fn waker_drop(_: *const ()) {}

static DUMMY_WAKER_VTABLE: RawWakerVTable =
	RawWakerVTable::new(waker_clone, waker_wake, waker_wake_by_ref, waker_drop);

async fn yield_once() {
	core::future::poll_fn(|cx| {
		cx.waker().wake_by_ref();
		Poll::<()>::Pending
	})
	.await;
}

#[inline(always)]
fn write_hex_u64(v: u64) {
	write_hex_u32((v >> 32) as u32);
	write_hex_u32(v as u32);
}

#[inline(always)]
fn write_hex_u32(v: u32) {
	write_hex_u16((v >> 16) as u16);
	write_hex_u16(v as u16);
}

#[inline(always)]
fn write_hex_u16(v: u16) {
	write_hex_u8((v >> 8) as u8);
	write_hex_u8(v as u8);
}

#[inline(always)]
fn write_hex_u8(v: u8) {
	write_hex_nibble(v >> 4);
	write_hex_nibble(v & 0x0F);
}

#[inline(always)]
fn write_hex_nibble(v: u8) {
	let v = v & 0x0F;
	let c = if v < 10 { b'0' + v } else { b'A' + (v - 10) };
	crate::debugcon_write_byte(c);
}
