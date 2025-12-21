use core::{
	hint::spin_loop,
	task::{Poll},
	time::Duration,
};
use crab_usb::{impl_trait, BoxFuture, EventHandler, FutureExt, Kernel};
struct KernelImpl;
impl_trait! {
	impl Kernel for KernelImpl {
		fn sleep<'a>(duration: Duration) -> BoxFuture<'a, ()> {
			async move {
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

static mut USB_HANDLER: Option<EventHandler> = None;

#[embassy_executor::task]
pub async fn usb_poll_task() {
	loop {
		poll_usb_events();
		yield_once().await;
	}
}

pub fn poll_usb_events() {
	unsafe {
		if let Some(handler) = USB_HANDLER.as_ref() {
			handler.handle_event();
		}
	}
}

async fn yield_once() {
	core::future::poll_fn(|cx| {
		cx.waker().wake_by_ref();
		Poll::<()>::Pending
	})
	.await;
}