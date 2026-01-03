use super::xhci::{
    self, context_index, endpoint_target, hi, lo, trb_type, Trb, TrbRing, XhciContext,
};
use crate::pci::dma;
use core::cmp;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use heapless::{Deque, Vec};
use spin::Mutex;

const USB_CLASS_COMM: u8 = 0x02;
const USB_SUBCLASS_ACM: u8 = 0x02;
const USB_CLASS_DATA: u8 = 0x0A;
const MAX_CDC_DEVICES: usize = 2;
// Generic CDC-ACM transport buffering.
// Keep this modest; policy-specific buffering (e.g. ESP32 log backend) should live
// in the policy layer, not in the generic CDC driver.
//
// NOTE: This capacity is reserved per CDC runtime (static memory via `heapless`).
const CDC_TX_QUEUE_CAP: usize = 16 * 1024;
const CDC_RX_QUEUE_CAP: usize = 2 * 1024;
const CDC_DMA_CHUNK: usize = 512;

#[derive(Clone, Copy, Debug)]
struct EndpointInfo {
    address: u8,
    max_packet: u16,
}

#[derive(Clone, Copy, Debug)]
struct CdcInterface {
    configuration: u8,
    control_interface: u8,
    data_interface: u8,
    ep_in: EndpointInfo,
    ep_out: EndpointInfo,
}

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
    pub cfg: &'a [u8],
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub speed_code: u32,
    pub target_port: u8,
    pub desired_baud: u32,
}

struct CdcRuntime {
    info: CdcInterface,
    slot_id: u32,
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
    rx_dma_virt: *mut u8,
    rx_dma_len: usize,
    rx_posted: bool,
    tx_queue: Deque<u8, CDC_TX_QUEUE_CAP>,
    rx_queue: Deque<u8, CDC_RX_QUEUE_CAP>,
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
        let requested = self.rx_dma_len as u32;
        let consumed = requested.saturating_sub(residual.min(requested));
        unsafe {
            for idx in 0..(consumed as usize) {
                let byte = *self.rx_dma_virt.add(idx);
                let _ = self.rx_queue.push_back(byte);
            }
        }
        let _ = self.post_rx_locked();
    }
}

static CDC_RUNTIMES: Mutex<Vec<CdcRuntime, MAX_CDC_DEVICES>> = Mutex::new(Vec::new());

static TX_COMPLETE_CALLBACK: Mutex<Option<fn(u32)>> = Mutex::new(None);

pub fn set_tx_complete_callback(cb: Option<fn(u32)>) {
    *TX_COMPLETE_CALLBACK.lock() = cb;
}
pub fn unregister_runtime(slot_id: u32) -> bool {
    let mut guard = CDC_RUNTIMES.lock();
    let mut idx = 0usize;
    let mut removed = false;
    while idx < guard.len() {
        if guard[idx].slot_id == slot_id {
            let _ = guard.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }
    drop(guard);
    removed
}

fn register_runtime(runtime: CdcRuntime) {
    let mut guard = CDC_RUNTIMES.lock();
    if let Some(existing) = guard.iter_mut().find(|rt| rt.slot_id == runtime.slot_id) {
        *existing = runtime;
        return;
    }
    let _ = guard.push(runtime);
}

fn with_runtime_mut_by_slot<R, F>(slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut CdcRuntime) -> R,
{
    let mut guard = CDC_RUNTIMES.lock();
    guard.iter_mut().find(|rt| rt.slot_id == slot_id).map(f)
}

pub fn queue_tx_bytes(slot_id: u32, data: &[u8]) -> usize {
    with_runtime_mut_by_slot(slot_id, |runtime| {
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

pub fn pop_rx_byte(slot_id: u32) -> Option<u8> {
    with_runtime_mut_by_slot(slot_id, |runtime| runtime.rx_queue.pop_front()).flatten()
}

pub fn handle_transfer_event(evt: &Trb) -> bool {
    let slot_id = ((evt.d3 >> 24) & 0xFF) as u32;
    let ep_target = (evt.d3 >> 16) & 0x1F;
    let completion = (evt.d2 >> 24) & 0xFF;
    let residual = evt.d2 & 0x00FF_FFFF;

    let mut tx_complete = false;
    let handled = with_runtime_mut_by_slot(slot_id, |runtime| {
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
        if let Some(cb) = *TX_COMPLETE_CALLBACK.lock() {
            cb(slot_id);
        }
    }

    handled
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx,
        mut cmd_ring,
        mut ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        target_port,
        desired_baud,
        ..
    } = params;

    let Some(interface) = parse_cdc_interface(cfg) else {
        return Err(());
    };

    crate::log!(
        "usb: cdc-acm cfg={} ctrl_if={} data_if={}\n",
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

        const EP_TYPE_BULK_OUT: u32 = 2;
        const EP_TYPE_BULK_IN: u32 = 6;

        write_volatile(ep_in_ctx.add(0), 0);
        write_volatile(
            ep_in_ctx.add(1),
            ((interface.ep_in.max_packet as u32) << 16) | (EP_TYPE_BULK_IN << 3) | 3,
        );
        let dq_in = ring_in.dequeue_ptr();
        write_volatile(ep_in_ctx.add(2), lo(dq_in));
        write_volatile(ep_in_ctx.add(3), hi(dq_in));
        write_volatile(
            ep_in_ctx.add(4),
            (interface.ep_in.max_packet as u32) << 16 | (interface.ep_in.max_packet as u32),
        );

        write_volatile(ep_out_ctx.add(0), 0);
        write_volatile(
            ep_out_ctx.add(1),
            ((interface.ep_out.max_packet as u32) << 16) | (EP_TYPE_BULK_OUT << 3) | 3,
        );
        let dq_out = ring_out.dequeue_ptr();
        write_volatile(ep_out_ctx.add(2), lo(dq_out));
        write_volatile(ep_out_ctx.add(3), hi(dq_out));
        write_volatile(
            ep_out_ctx.add(4),
            (interface.ep_out.max_packet as u32) << 16 | (interface.ep_out.max_packet as u32),
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

    let mut runtime = CdcRuntime {
        info: interface,
        slot_id,
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
        rx_dma_virt: rx_virt,
        rx_dma_len: CDC_DMA_CHUNK,
        rx_posted: false,
        tx_queue: Deque::new(),
        rx_queue: Deque::new(),
    };

    if program_line_coding(
        ctx,
        &mut ep0_ring,
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
        &mut ep0_ring,
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
    let _ = with_runtime_mut_by_slot(slot_id, |rt| rt.post_rx_locked());

    crate::log!(
        "usb: cdc-acm attached slot={} ep_in=0x{:02X} ep_out=0x{:02X}\n",
        slot_id,
        interface.ep_in.address,
        interface.ep_out.address
    );

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
        d2: 8 | (1 << 16),
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

fn parse_cdc_interface(cfg: &[u8]) -> Option<CdcInterface> {
    let mut idx = 0usize;
    let mut config_value: u8 = 1;
    let mut current_iface: Option<u8> = None;
    let mut current_class: u8 = 0;
    let mut current_subclass: u8 = 0;
    let mut data_iface: Option<u8> = None;
    let mut data_alt: u8 = 0;
    let mut control_iface: Option<u8> = None;
    let mut ep_in: Option<EndpointInfo> = None;
    let mut ep_out: Option<EndpointInfo> = None;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 9 {
                    let iface = cfg[idx + 2];
                    current_iface = Some(iface);
                    current_class = cfg[idx + 5];
                    current_subclass = cfg[idx + 6];
                    let protocol = cfg[idx + 7];
                    if current_class == USB_CLASS_COMM && current_subclass == USB_SUBCLASS_ACM {
                        control_iface = Some(iface);
                    } else if current_class == USB_CLASS_DATA {
                        data_iface = Some(iface);
                        data_alt = cfg[idx + 3];
                        let _ = protocol;
                        ep_in = None;
                        ep_out = None;
                    } else {
                        data_iface = None;
                    }
                } else {
                    current_iface = None;
                }
            }
            5 => {
                if let (Some(iface), Some(data_if)) = (current_iface, data_iface) {
                    if iface == data_if && data_alt == 0 && current_class == USB_CLASS_DATA {
                        if len >= 7 {
                            let attrs = cfg[idx + 3];
                            if (attrs & 0x3) == 0x2 {
                                let ep_addr = cfg[idx + 2];
                                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                                if (ep_addr & 0x80) != 0 {
                                    if ep_in.is_none() {
                                        ep_in = Some(EndpointInfo {
                                            address: ep_addr,
                                            max_packet,
                                        });
                                    }
                                } else if ep_out.is_none() {
                                    ep_out = Some(EndpointInfo {
                                        address: ep_addr,
                                        max_packet,
                                    });
                                }
                                if let (Some(ctrl), Some(in_ep), Some(out_ep)) =
                                    (control_iface, ep_in, ep_out)
                                {
                                    return Some(CdcInterface {
                                        configuration: config_value,
                                        control_interface: ctrl,
                                        data_interface: data_if,
                                        ep_in: in_ep,
                                        ep_out: out_ep,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        idx += len;
    }

    None
}