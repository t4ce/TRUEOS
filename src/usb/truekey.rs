use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};
use heapless::Deque;
use spin::Mutex;
use super::cdc_acm::{self, CdcAttachEvent};
use embassy_time::{Duration as EmbassyDuration, Timer};

static TRUEKEY_SLOT: AtomicU32 = AtomicU32::new(0);
static TRUEKEY_CONTROLLER: AtomicU32 = AtomicU32::new(0);
static SERIAL_SENT_FOR_SLOT: AtomicU32 = AtomicU32::new(0);
const LOG_CACHE_BYTES: usize = 1024 * 1024;
static LOG_CACHE: Mutex<Deque<u8, LOG_CACHE_BYTES>> = Mutex::new(Deque::new());

pub fn init() {
	let _ = cdc_acm::register_attach_callback(on_cdc_attach);
	let _ = cdc_acm::register_detach_callback(on_cdc_detach);
}

#[embassy_executor::task]
pub async fn drain_loop() {
	const CHUNK: usize = 1024;
	const IDLE_SLEEP_MS: u64 = 100;

	let mut buf = [0u8; CHUNK];
	loop {
		let slot = TRUEKEY_SLOT.load(Ordering::Acquire);
		let controller_id = TRUEKEY_CONTROLLER.load(Ordering::Acquire);
		if slot == 0 {
			Timer::after(EmbassyDuration::from_millis(IDLE_SLEEP_MS)).await;
			continue;
		}

		let n = {
			let mut q = LOG_CACHE.lock();
			let mut i = 0usize;
			while i < CHUNK {
				match q.pop_front() {
					Some(b) => {
						buf[i] = b;
						i += 1;
					}
					None => break,
				}
			}
			i
		};
		if n == 0 {
			Timer::after(EmbassyDuration::from_millis(IDLE_SLEEP_MS)).await;
			continue;
		}
		let _ = cdc_acm::write_all(controller_id as usize, slot, &buf[..n]).await;
	}
}

pub fn push_bytes(data: &[u8]) {
	let mut q = LOG_CACHE.lock();
	for &b in data {
		if q.push_back(b).is_err() {
			let _ = q.pop_front();
			let _ = q.push_back(b);
		}
	}
}

pub fn push_fmt(args: fmt::Arguments<'_>) {
	struct Writer;
	impl fmt::Write for Writer {
		fn write_str(&mut self, s: &str) -> fmt::Result {
			push_bytes(s.as_bytes());
			Ok(())
		}
	}
	let _ = fmt::write(&mut Writer, args);
}

pub fn slot_id() -> Option<u32> {
	let slot = TRUEKEY_SLOT.load(Ordering::Acquire);
	if slot == 0 { None } else { Some(slot) }
}

fn on_cdc_attach(evt: CdcAttachEvent) {
	let prev = TRUEKEY_SLOT.load(Ordering::Acquire);
	if prev == 0 {
		TRUEKEY_SLOT.store(evt.slot_id, Ordering::Release);
		TRUEKEY_CONTROLLER.store(evt.controller_id as u32, Ordering::Release);
		SERIAL_SENT_FOR_SLOT.store(0, Ordering::Release);
		crate::log!(
			"truekey: bound to cdc slot={} vid=0x{:04X} pid=0x{:04X}\n",
			evt.slot_id,
			evt.vid,
			evt.pid
		);

		if let Some(bound_slot) = slot_id() {
			crate::log!("truekey: slot{}\n", bound_slot);
		}

	}
}

fn on_cdc_detach(evt: CdcAttachEvent) {
	let slot = TRUEKEY_SLOT.load(Ordering::Acquire);
	let controller_id = TRUEKEY_CONTROLLER.load(Ordering::Acquire) as usize;
	if slot != 0 && slot == evt.slot_id && controller_id == evt.controller_id {
		TRUEKEY_SLOT.store(0, Ordering::Release);
		TRUEKEY_CONTROLLER.store(0, Ordering::Release);
		SERIAL_SENT_FOR_SLOT.store(0, Ordering::Release);
		crate::log!(
			"truekey: unbound (cdc disconnected) slot={} vid=0x{:04X} pid=0x{:04X}\n",
			evt.slot_id,
			evt.vid,
			evt.pid
		);
	}
}
