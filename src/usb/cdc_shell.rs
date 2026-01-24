use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::cdc_acm::{self, CdcAttachEvent, UsbSerial};

static SHELL_SLOT: AtomicU32 = AtomicU32::new(0);
static SHELL_CONTROLLER: AtomicU32 = AtomicU32::new(0);
static TARGET_SERIAL: Mutex<UsbSerial> = Mutex::new(UsbSerial::none());

/// If set, only bind to CDC ACM devices with this serial.
pub fn configure_target_serial(serial: &str) {
    *TARGET_SERIAL.lock() = UsbSerial::from_str(serial);
}

/// Clear any serial filter so the first CDC ACM device can bind.
pub fn clear_target_serial() {
    *TARGET_SERIAL.lock() = UsbSerial::none();
}

pub fn init() {
    let _ = cdc_acm::register_attach_callback(on_cdc_attach);
    let _ = cdc_acm::register_detach_callback(on_cdc_detach);
}

pub fn is_bound() -> bool {
    SHELL_SLOT.load(Ordering::Acquire) != 0
}

pub fn write(data: &[u8]) -> usize {
    let slot = SHELL_SLOT.load(Ordering::Acquire);
    if slot == 0 {
        return 0;
    }
    let controller_id = SHELL_CONTROLLER.load(Ordering::Acquire) as usize;
    cdc_acm::queue_tx_bytes(controller_id, slot, data)
}

pub fn read_byte() -> Option<u8> {
    let slot = SHELL_SLOT.load(Ordering::Acquire);
    if slot == 0 {
        return None;
    }
    let controller_id = SHELL_CONTROLLER.load(Ordering::Acquire) as usize;
    cdc_acm::pop_rx_byte(controller_id, slot)
}

fn serial_matches(evt: &CdcAttachEvent) -> bool {
    let target = *TARGET_SERIAL.lock();
    if target.is_some() {
        evt.serial == target
    } else {
        true
    }
}

fn on_cdc_attach(evt: CdcAttachEvent) {
    if !serial_matches(&evt) {
        return;
    }
    if SHELL_SLOT.load(Ordering::Acquire) != 0 {
        return;
    }

    SHELL_SLOT.store(evt.slot_id, Ordering::Release);
    SHELL_CONTROLLER.store(evt.controller_id as u32, Ordering::Release);

    crate::log!(
        "cdc-shell: bound slot={} vid=0x{:04X} pid=0x{:04X}\n",
        evt.slot_id,
        evt.vid,
        evt.pid
    );
}

fn on_cdc_detach(evt: CdcAttachEvent) {
    let slot = SHELL_SLOT.load(Ordering::Acquire);
    let controller_id = SHELL_CONTROLLER.load(Ordering::Acquire) as usize;
    if slot != 0 && slot == evt.slot_id && controller_id == evt.controller_id {
        SHELL_SLOT.store(0, Ordering::Release);
        SHELL_CONTROLLER.store(0, Ordering::Release);
        crate::log!(
            "cdc-shell: unbound slot={} vid=0x{:04X} pid=0x{:04X}\n",
            evt.slot_id,
            evt.vid,
            evt.pid
        );
    }
}
