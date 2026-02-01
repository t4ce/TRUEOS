use super::cdc;
use super::xhci::{
    self, context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo,
    trb_type, Trb, TrbRing, XhciContext, EP_STATE_DISABLED, EP_TYPE_BULK_IN, EP_TYPE_BULK_OUT,
};
use crate::pci::dma;
use core::cmp;
use core::future::poll_fn;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::task::{Poll, Waker};
use heapless::{Deque, Vec};
use spin::Mutex;

const MAX_CDC_DEVICES: usize = 2;
const MAX_CDC_CALLBACKS: usize = 4;
const CDC_TX_QUEUE_CAP: usize = 16 * 1024;
const CDC_DMA_CHUNK: usize = 512;

#[repr(C, packed)]
struct LineCoding {
    dte_rate: u32,
    char_format: u8,
    parity_type: u8,
    data_bits: u8,
}

pub struct AttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub cmd_ring: &'a mut TrbRing,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub dev_vid: u16,
    pub dev_pid: u16,
    pub cfg: &'a [u8],
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub target_port: u8,
    pub desired_baud: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct CdcAttachEvent {
    pub controller_id: usize,
    pub slot_id: u32,
    pub vid: u16,
    pub pid: u16,
}

struct CdcRuntime {
    controller_id: usize,
    slot_id: u32,
    vid: u16,
    pid: u16,
    ctx: XhciContext,
    ep_in_target: u32,
    ep_out_target: u32,
    ring_in: TrbRing,
    ring_out: TrbRing,
    tx_dma_phys: u64,
    tx_dma_virt: *mut u8,
    tx_dma_len: usize,
    tx_inflight: bool,
    rx_dma_phys: u64,
    rx_dma_len: usize,
    rx_posted: bool,
    tx_queue: Deque<u8, CDC_TX_QUEUE_CAP>,
    tx_waker: Option<Waker>,
}

unsafe impl Send for CdcRuntime {}
unsafe impl Sync for CdcRuntime {}

impl CdcRuntime {
    fn kick_tx_locked(&mut self) {
        if self.tx_inflight || self.tx_queue.is_empty() {
            return;
        }
        let chunk = cmp::min(self.tx_queue.len(), self.tx_dma_len);
        if chunk == 0 {
            return;
        }
        unsafe {
            for (idx, byte) in self.tx_queue.iter().take(chunk).enumerate() {
                *self.tx_dma_virt.add(idx) = *byte;
            }
        }
        let trb = Trb {
            d0: lo(self.tx_dma_phys),
            d1: hi(self.tx_dma_phys),
            d2: chunk as u32,
            d3: trb_type(1) | (1 << 5),
        };
        if !self.ring_out.push(trb) {
            crate::log!(
                "usb: cdc tx ring full slot={} chunk={}\n",
                self.slot_id,
                chunk
            );
            return;
        }
        for _ in 0..chunk {
            let _ = self.tx_queue.pop_front();
        }
        self.tx_inflight = true;
        unsafe {
            write_volatile(
                self.ctx.doorbell.add(self.slot_id as usize),
                self.ep_out_target,
            );
        }
    }

    fn post_rx_locked(&mut self) -> bool {
        if self.rx_posted {
            return true;
        }
        let trb = Trb {
            d0: lo(self.rx_dma_phys),
            d1: hi(self.rx_dma_phys),
            d2: self.rx_dma_len as u32,
            d3: trb_type(1) | (1 << 5),
        };
        if !self.ring_in.push(trb) {
            crate::log!("usb: cdc rx ring full slot={}\n", self.slot_id);
            return false;
        }
        self.rx_posted = true;
        unsafe {
            write_volatile(
                self.ctx.doorbell.add(self.slot_id as usize),
                self.ep_in_target,
            );
        }
        true
    }

    fn on_tx_complete(&mut self, completion: u32) {
        self.tx_inflight = false;
        if completion != 1 {
            crate::log!(
                "usb: cdc tx completion cc={} slot={}\n",
                completion,
                self.slot_id
            );
        }
        self.kick_tx_locked();
    }

    fn on_rx_complete(&mut self, completion: u32, residual: u32) {
        self.rx_posted = false;
        if completion != 1 && completion != 13 {
            crate::log!(
                "usb: cdc rx completion cc={} slot={}\n",
                completion,
                self.slot_id
            );
        }
        let _ = residual;
        let _ = self.post_rx_locked();
    }
}

static CDC_RUNTIMES: Mutex<Vec<CdcRuntime, MAX_CDC_DEVICES>> = Mutex::new(Vec::new());

static ATTACH_CALLBACKS: Mutex<Vec<fn(CdcAttachEvent), MAX_CDC_CALLBACKS>> = Mutex::new(Vec::new());
static DETACH_CALLBACKS: Mutex<Vec<fn(CdcAttachEvent), MAX_CDC_CALLBACKS>> = Mutex::new(Vec::new());

pub fn register_attach_callback(cb: fn(CdcAttachEvent)) -> bool {
    let mut guard = ATTACH_CALLBACKS.lock();
    if guard.iter().any(|existing| core::ptr::fn_addr_eq(*existing, cb)) {
        return true;
    }
    guard.push(cb).is_ok()
}

pub fn register_detach_callback(cb: fn(CdcAttachEvent)) -> bool {
    let mut guard = DETACH_CALLBACKS.lock();
    if guard.iter().any(|existing| core::ptr::fn_addr_eq(*existing, cb)) {
        return true;
    }
    guard.push(cb).is_ok()
}

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let mut detached: Option<CdcAttachEvent> = None;
    let mut guard = CDC_RUNTIMES.lock();
    let mut idx = 0usize;
    let mut removed = false;
    let mut waker: Option<Waker> = None;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            waker = guard[idx].tx_waker.take();
            detached = Some(CdcAttachEvent {
                controller_id,
                slot_id,
                vid: guard[idx].vid,
                pid: guard[idx].pid,
            });
            let _ = guard.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }
    drop(guard);

    // Wake any waiter so it can observe the detach.
    if let Some(w) = waker {
        w.wake();
    }

    if removed {
        if let Some(evt) = detached {
            let guard = DETACH_CALLBACKS.lock();
            for cb in guard.iter() {
                cb(evt);
            }
        }
    }
    removed
}

fn register_runtime(runtime: CdcRuntime) {
    let runtime = runtime;

    let mut guard = CDC_RUNTIMES.lock();
    if let Some(existing) = guard
        .iter_mut()
        .find(|rt| rt.controller_id == runtime.controller_id && rt.slot_id == runtime.slot_id)
    {
        *existing = runtime;
        return;
    }
    let _ = guard.push(runtime);
}

fn with_runtime_mut_by_slot<R, F>(controller_id: usize, slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut CdcRuntime) -> R,
{
    let mut guard = CDC_RUNTIMES.lock();
    guard
        .iter_mut()
        .find(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)
        .map(f)
}

fn with_runtime_by_slot<R, F>(controller_id: usize, slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&CdcRuntime) -> R,
{
    let guard = CDC_RUNTIMES.lock();
    guard
        .iter()
        .find(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)
        .map(f)
}

fn register_tx_waker(controller_id: usize, slot_id: u32, waker: &Waker) {
    let _ = with_runtime_mut_by_slot(controller_id, slot_id, |rt| {
        let should_replace = match rt.tx_waker.as_ref() {
            Some(existing) => !existing.will_wake(waker),
            None => true,
        };
        if should_replace {
            rt.tx_waker = Some(waker.clone());
        }
    });
}

fn wake_tx(controller_id: usize, slot_id: u32) {
    let waker = with_runtime_mut_by_slot(controller_id, slot_id, |rt| rt.tx_waker.take()).flatten();
    if let Some(w) = waker {
        w.wake();
    }
}

fn tx_queue_has_room(controller_id: usize, slot_id: u32) -> bool {
    with_runtime_by_slot(controller_id, slot_id, |rt| rt.tx_queue.len() < CDC_TX_QUEUE_CAP)
        .unwrap_or(false)
}

fn queue_tx_bytes(controller_id: usize, slot_id: u32, data: &[u8]) -> usize {
    with_runtime_mut_by_slot(controller_id, slot_id, |runtime| {
        let mut written = 0usize;
        for &byte in data {
            if runtime.tx_queue.push_back(byte).is_err() {
                break;
            }
            written += 1;
        }
        if written > 0 {
            runtime.kick_tx_locked();
        }
        written
    })
    .unwrap_or(0)
}

pub(crate) async fn write_all(controller_id: usize, slot_id: u32, mut data: &[u8]) -> usize {
    if !runtime_exists(controller_id, slot_id) {
        return 0;
    }

    let mut total = 0;
    while !data.is_empty() {
        let n = queue_tx_bytes(controller_id, slot_id, data);
        if n == 0 {
            // If the runtime vanished (detach), stop to avoid waiting forever.
            if !runtime_exists(controller_id, slot_id) {
                break;
            }
            poll_fn(|cx| {
                // Fast-path if space became available between attempts.
                if tx_queue_has_room(controller_id, slot_id) {
                    return Poll::Ready(());
                }
                register_tx_waker(controller_id, slot_id, cx.waker());
                // Re-check after registration to avoid missing a wake.
                if tx_queue_has_room(controller_id, slot_id)
                    || !runtime_exists(controller_id, slot_id)
                {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            continue;
        }
        total += n;
        data = &data[n..];
    }
    total
}

pub fn handle_transfer_event(controller_id: usize, evt: &Trb) -> bool {
    let slot_id = ((evt.d3 >> 24) & 0xFF) as u32;
    let ep_target = (evt.d3 >> 16) & 0x1F;
    let completion = (evt.d2 >> 24) & 0xFF;
    let residual = evt.d2 & 0x00FF_FFFF;

    let mut tx_complete = false;
    let handled = with_runtime_mut_by_slot(controller_id, slot_id, |runtime| {
        if ep_target == runtime.ep_out_target {
            runtime.on_tx_complete(completion);
            tx_complete = true;
            true
        } else if ep_target == runtime.ep_in_target {
            runtime.on_rx_complete(completion, residual);
            true
        } else {
            false
        }
    })
    .unwrap_or(false);

    if handled && tx_complete {
        wake_tx(controller_id, slot_id);
    }

    handled
}

pub(crate) fn runtime_exists(controller_id: usize, slot_id: u32) -> bool {
    let guard = CDC_RUNTIMES.lock();
    guard
        .iter()
        .any(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx,
        cmd_ring,
        ep0_ring,
        slot_id,
        dev_vid,
        dev_pid,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        target_port,
        desired_baud,
        ..
    } = params;

    let Some(interface) = cdc::parse_cdc_interface(cfg) else {
        return Err(());
    };

    crate::log!(
        "usb: cdc-acm vid=0x{:04X} pid=0x{:04X} cfg={} ctrl_if={} data_if={}\n",
        dev_vid,
        dev_pid,
        interface.configuration,
        interface.control_interface,
        interface.data_interface
    );

    // Set configuration similar to the mass driver path.
    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((interface.configuration as u32) << 16),
        d1: 0,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        crate::log!("usb: cdc set_configuration overflow\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let Some(evt) = xhci::wait_for_event(
        ctx,
        |evt| ((evt.d3 >> 10) & 0x3F) == 32 && ((evt.d3 >> 24) & 0xFF) as u32 == slot_id,
        400,
        embassy_time::Duration::from_millis(5),
    )
    .await
    else {
        crate::log!("usb: cdc set_configuration timeout\n");
        return Err(());
    };
    if (evt.d2 >> 24) & 0xFF != 1 {
        return Err(());
    }

    // Prepare endpoint rings.
    let ring_len = 64 * size_of::<Trb>();
    let (ring_in_phys, ring_in_virt) = dma::alloc(ring_len, 64).ok_or(())?;
    unsafe { write_bytes(ring_in_virt, 0, ring_len) };
    let ring_in = unsafe { TrbRing::new(ring_in_phys, ring_in_virt as *mut Trb, 64) };

    let (ring_out_phys, ring_out_virt) = dma::alloc(ring_len, 64).ok_or(())?;
    unsafe { write_bytes(ring_out_virt, 0, ring_len) };
    let ring_out = unsafe { TrbRing::new(ring_out_phys, ring_out_virt as *mut Trb, 64) };

    let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64).ok_or(())?;
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_in_target = endpoint_target(interface.ep_in.address);
    let ep_out_target = endpoint_target(interface.ep_out.address);
    let ep_in_ctx_index = context_index(interface.ep_in.address);
    let ep_out_ctx_index = context_index(interface.ep_out.address);
    let highest_ep_ctx = cmp::max(ep_in_ctx_index, ep_out_ctx_index);

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        write_volatile(
            add_flags_ptr.add(1),
            (1 << 0) | (1 << (ep_in_ctx_index - 1)) | (1 << (ep_out_ctx_index - 1)),
        );

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_in_ctx = input_cfg_virt.add(ctx_stride_bytes * ep_in_ctx_index as usize) as *mut u32;
        let ep_out_ctx =
            input_cfg_virt.add(ctx_stride_bytes * ep_out_ctx_index as usize) as *mut u32;

        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | ((highest_ep_ctx - 1) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        write_volatile(ep_in_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_in_cfg = ep_cerr_bits(3);
        ep_in_cfg |= ep_type_bits(EP_TYPE_BULK_IN);
        ep_in_cfg |= ep_max_packet_bits(interface.ep_in.max_packet as u32);
        write_volatile(ep_in_ctx.add(1), ep_in_cfg);
        let dq_in = ring_in.dequeue_ptr();
        write_volatile(ep_in_ctx.add(2), lo(dq_in));
        write_volatile(ep_in_ctx.add(3), hi(dq_in));
        let ep_in_mps = interface.ep_in.max_packet as u32;
        write_volatile(
            ep_in_ctx.add(4),
            ep_avg_trb_len_bits(ep_in_mps) | ep_max_esit_payload_lo_bits(ep_in_mps),
        );

        write_volatile(ep_out_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_out_cfg = ep_cerr_bits(3);
        ep_out_cfg |= ep_type_bits(EP_TYPE_BULK_OUT);
        ep_out_cfg |= ep_max_packet_bits(interface.ep_out.max_packet as u32);
        write_volatile(ep_out_ctx.add(1), ep_out_cfg);
        let dq_out = ring_out.dequeue_ptr();
        write_volatile(ep_out_ctx.add(2), lo(dq_out));
        write_volatile(ep_out_ctx.add(3), hi(dq_out));
        let ep_out_mps = interface.ep_out.max_packet as u32;
        write_volatile(
            ep_out_ctx.add(4),
            ep_avg_trb_len_bits(ep_out_mps) | ep_max_esit_payload_lo_bits(ep_out_mps),
        );
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    if !cmd_ring.push(cfg_ep_cmd) {
        crate::log!("usb: cdc configure-endpoint ring full\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };
    let Some(cfg_evt) = xhci::wait_for_event(
        ctx,
        |evt| ((evt.d3 >> 10) & 0x3F) == 33,
        400,
        embassy_time::Duration::from_millis(5),
    )
    .await
    else {
        crate::log!("usb: cdc configure-endpoint timeout\n");
        return Err(());
    };
    if (cfg_evt.d2 >> 24) & 0xFF != 1 {
        return Err(());
    }

    let (tx_phys, tx_virt) = dma::alloc(CDC_DMA_CHUNK, 64).ok_or(())?;
    let (rx_phys, rx_virt) = dma::alloc(CDC_DMA_CHUNK, 64).ok_or(())?;
    unsafe {
        write_bytes(tx_virt, 0, CDC_DMA_CHUNK);
        write_bytes(rx_virt, 0, CDC_DMA_CHUNK);
    }

    let runtime = CdcRuntime {
        controller_id: ctx.controller_id,
        slot_id,
        vid: dev_vid,
        pid: dev_pid,
        ctx: *ctx,
        ep_in_target,
        ep_out_target,
        ring_in,
        ring_out,
        tx_dma_phys: tx_phys,
        tx_dma_virt: tx_virt,
        tx_dma_len: CDC_DMA_CHUNK,
        tx_inflight: false,
        rx_dma_phys: rx_phys,
        rx_dma_len: CDC_DMA_CHUNK,
        rx_posted: false,
        tx_queue: Deque::new(),
        tx_waker: None,
    };

    if program_line_coding(
        ctx,
        &mut *ep0_ring,
        slot_id,
        interface.control_interface,
        desired_baud,
    )
    .await
    .is_err()
    {
        crate::log!("usb: cdc set_line_coding failed\n");
    }
    if set_control_line_state(
        ctx,
        &mut *ep0_ring,
        slot_id,
        interface.control_interface,
        0x0003,
    )
    .await
    .is_err()
    {
        crate::log!("usb: cdc set_control_line_state failed\n");
    }

    // Important ordering: don't start bulk RX/TX while we're still using EP0 control
    // transfers that wait on generic Transfer Events.
    register_runtime(runtime);
    let _: Option<bool> =
        with_runtime_mut_by_slot(ctx.controller_id, slot_id, |rt| rt.post_rx_locked());

    crate::log!(
        "usb: cdc-acm attached slot={} ep_in=0x{:02X} ep_out=0x{:02X}\n",
        slot_id,
        interface.ep_in.address,
        interface.ep_out.address
    );

    let evt = CdcAttachEvent {
        controller_id: ctx.controller_id,
        slot_id,
        vid: dev_vid,
        pid: dev_pid,
    };
    let guard = ATTACH_CALLBACKS.lock();
    for cb in guard.iter() {
        cb(evt);
    }

    Ok(())
}

async fn program_line_coding(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    baud: u32,
) -> Result<(), ()> {
    let (phys, virt) = dma::alloc(16, 16).ok_or(())?;
    unsafe {
        let lc = virt as *mut LineCoding;
        lc.write(LineCoding {
            dte_rate: baud,
            char_format: 0,
            parity_type: 0,
            data_bits: 8,
        });
    }

    let setup = Trb {
        d0: 0x21 | (0x20 << 8),
        d1: (iface as u32) | ((7u32) << 16),
        // Setup Stage TRB transfer length must be 8 (one setup packet).
        // We previously leaked a bit into the length field (0x10008), which
        // makes the host issue an invalid control transfer and yields CC != 1.
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        Some(phys),
        7,
        "set-line-coding",
        400,
    )
    .await
}

async fn set_control_line_state(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    value: u16,
) -> Result<(), ()> {
    let setup = Trb {
        d0: 0x21 | (0x22 << 8) | ((value as u32) << 16),
        d1: iface as u32,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        None,
        0,
        "set-control-line-state",
        400,
    )
    .await
}