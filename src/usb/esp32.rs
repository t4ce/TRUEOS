use crate::debugconf;
use crate::truelog::{self, BackendRole, SerialBackend};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use heapless::Deque;
use spin::Mutex;

use super::cdc_acm;

const ESPRESSIF_VID: u16 = 0x303A;
const ALLOWED_PIDS: &[u16] = &[0x1001];
const ESP32_TX_QUEUE_CAP: usize = 1024 * 1024;
static ESP32_TX_QUEUE: Mutex<Deque<u8, ESP32_TX_QUEUE_CAP>> = Mutex::new(Deque::new());

fn is_allowed_device(vid: u16, pid: u16) -> bool {
    if vid != ESPRESSIF_VID {
        return false;
    }
    ALLOWED_PIDS.iter().any(|&p| p == pid)
}

pub struct AttachParams<'a> {
    pub ctx: &'a super::xhci::XhciContext,
    pub cmd_ring: &'a mut super::xhci::TrbRing,
    pub ep0_ring: &'a mut super::xhci::TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub speed_code: u32,
    pub target_port: u8,
    pub vid: u16,
    pub pid: u16,
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    if !is_allowed_device(params.vid, params.pid) {
        return Err(());
    }

    let baud = truelog::desired_baud();

    cdc_acm::attach_device(cdc_acm::AttachParams {
        ctx: params.ctx,
        cmd_ring: params.cmd_ring,
        ep0_ring: params.ep0_ring,
        slot_id: params.slot_id,
        cfg: params.cfg,
        dev_ctx_virt: params.dev_ctx_virt,
        ctx_stride_bytes: params.ctx_stride_bytes,
        ctx_stride_words: params.ctx_stride_words,
        speed_code: params.speed_code,
        target_port: params.target_port,
        desired_baud: baud,
    })
    .await?;

    ensure_backend_registered();
    backend().activate(params.slot_id);
    cdc_acm::set_tx_complete_callback(Some(esp32_on_cdc_tx_complete));
    backend().drain();

    if let Err(err) = truelog::promote_backend(backend()) {
        debugconf!(
            "serial: failed to promote esp32 usb backend err={:?} slot={}\n",
            err,
            params.slot_id
        );
    } else {
        debugconf!("serial: promoted esp32 usb backend (primary) slot={}\n", params.slot_id);
        let _ = backend().try_write(b"[esp32] truelog primary\n");
        backend().drain();
    }

    Ok(())
}

pub fn unregister_slot(slot_id: u32) {
    if backend().deactivate(slot_id) {
        cdc_acm::set_tx_complete_callback(None);
        backend().clear_tx_queue();
        let _ = truelog::unregister_backend(backend());
        BACKEND_REGISTERED.store(false, Ordering::Release);
    }
}

fn esp32_on_cdc_tx_complete(slot_id: u32) {
    if backend().active_slot() == Some(slot_id) {
        backend().drain();
    }
}

struct Esp32UsbBackend {
    slot_id: AtomicU32,
}

impl Esp32UsbBackend {
    const fn new() -> Self {
        Self {
            slot_id: AtomicU32::new(0),
        }
    }

    fn activate(&self, slot_id: u32) {
        self.slot_id.store(slot_id, Ordering::Release);
    }

    fn deactivate(&self, slot_id: u32) -> bool {
        self.slot_id
            .compare_exchange(slot_id, 0, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    fn active_slot(&self) -> Option<u32> {
        match self.slot_id.load(Ordering::Acquire) {
            0 => None,
            slot => Some(slot),
        }
    }

    fn clear_tx_queue(&self) {
        let mut q = ESP32_TX_QUEUE.lock();
        while q.pop_front().is_some() {}
    }

    fn enqueue_bytes(&self, bytes: &[u8]) -> usize {
        let mut q = ESP32_TX_QUEUE.lock();
        let mut enqueued = 0usize;
        for &b in bytes {
            if q.push_back(b).is_err() {
                // Ring semantics: drop oldest to keep newest.
                let _ = q.pop_front();
                let _ = q.push_back(b);
            }
            enqueued += 1;
        }
        enqueued
    }

    fn drain(&self) {
        let Some(slot) = self.active_slot() else {
            return;
        };

        // Drain as much as generic CDC will accept right now.
        // We retry on TX completions (or the next log write).
        loop {
            let mut q = ESP32_TX_QUEUE.lock();
            if q.is_empty() {
                return;
            }
            let (wrote, front_len) = {
                let (front, _) = q.as_slices();
                (cdc_acm::queue_tx_bytes(slot, front), front.len())
            };
            for _ in 0..wrote {
                let _ = q.pop_front();
            }
            if wrote < front_len {
                return;
            }
        }
    }
}

impl SerialBackend for Esp32UsbBackend {
    fn name(&self) -> &'static str {
        "esp32-usb"
    }

    fn try_write_byte(&self, byte: u8) -> bool {
        self.try_write(&[byte]) == 1
    }

    fn try_write(&self, bytes: &[u8]) -> usize {
        let enq = self.enqueue_bytes(bytes);
        self.drain();
        enq
    }

    fn try_read_byte(&self) -> Option<u8> {
        self.active_slot().and_then(cdc_acm::pop_rx_byte)
    }
}

static BACKEND_REGISTERED: AtomicBool = AtomicBool::new(false);
static ESP32_BACKEND: Esp32UsbBackend = Esp32UsbBackend::new();

fn backend() -> &'static Esp32UsbBackend {
    &ESP32_BACKEND
}

fn ensure_backend_registered() {
    if BACKEND_REGISTERED.load(Ordering::Acquire) {
        return;
    }

    if truelog::register_backend(backend(), BackendRole::Mirror).is_ok() {
        BACKEND_REGISTERED.store(true, Ordering::Release);
        debugconf!("serial: registered esp32 usb backend (mirror)\n");
    } else {
        debugconf!("serial: failed to register esp32 usb backend\n");
    }
}
