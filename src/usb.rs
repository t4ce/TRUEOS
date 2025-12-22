use core::{
	hint::spin_loop,
	task::{Poll},
	time::Duration,
};
use spin::Mutex;

#[embassy_executor::task]
pub async fn usb_poll_task() {
	loop {
		poll_usb_events();
		yield_once().await;
	}
}

pub fn poll_usb_events() {
}

async fn yield_once() {
	core::future::poll_fn(|cx| {
		cx.waker().wake_by_ref();
		Poll::<()>::Pending
	})
	.await;
}