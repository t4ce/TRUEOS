use super::xhci::{
    EP_STATE_DISABLED, EP_TYPE_BULK_IN, EP_TYPE_BULK_OUT, Trb, TrbRing, XhciContext, context_index,
    endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_max_esit_payload_lo_bits,
    ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo, trb_type,
};
use crate::pci::dma;
use core::cmp;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Deque;
use heapless::Vec;
use spin::Mutex;

macro_rules! usbv {
    ($($tt:tt)*) => {{
        if super::USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_MIDISTREAMING: u8 = 0x03;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MidiAdapterKind {
    Generic,
    CasioCtk3500,
}

fn select_adapter(dev_vid: u16, dev_pid: u16) -> MidiAdapterKind {
    // Casio CTK-3500: 07cf:6803 (class-compliant USB MIDI)
    if dev_vid == 0x07CF && dev_pid == 0x6803 {
        MidiAdapterKind::CasioCtk3500
    } else {
        MidiAdapterKind::Generic
    }
}

static PIANO_SLOT: AtomicU32 = AtomicU32::new(0);
static PIANO_CONTROLLER: AtomicU32 = AtomicU32::new(0);
static PIANO_LAST_HEARTBEAT_SECS: AtomicU64 = AtomicU64::new(u64::MAX);
const PIANO_QUEUE_PKTS: usize = 512;
static PIANO_QUEUE: Mutex<Deque<[u8; 4], PIANO_QUEUE_PKTS>> = Mutex::new(Deque::new());

#[inline]
fn is_active_sensing_heartbeat(pkt: &[u8; 4]) -> bool {
    // USB-MIDI Active Sensing is a single-byte MIDI message (0xFE).
    // It is typically carried as CIN=0xF with one MIDI byte payload.
    (pkt[0] & 0x0F) == 0x0F && pkt[1] == 0xFE && pkt[2] == 0 && pkt[3] == 0
}

#[inline]
fn secs_to_hms(secs: u64) -> (u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    (h, m, s)
}

fn piano_set_connected(controller_id: usize, slot_id: u32) {
    PIANO_CONTROLLER.store(controller_id as u32, Ordering::Release);
    PIANO_SLOT.store(slot_id, Ordering::Release);
}

fn piano_set_disconnected(controller_id: usize, slot_id: u32) {
    let cur_slot = PIANO_SLOT.load(Ordering::Acquire);
    let cur_ctrl = PIANO_CONTROLLER.load(Ordering::Acquire) as usize;
    if cur_slot == slot_id && cur_ctrl == controller_id {
        PIANO_SLOT.store(0, Ordering::Release);
        PIANO_CONTROLLER.store(0, Ordering::Release);
        PIANO_LAST_HEARTBEAT_SECS.store(u64::MAX, Ordering::Release);
        let mut q = PIANO_QUEUE.lock();
        while q.pop_front().is_some() {}
    }
}

fn piano_push_packet(pkt: [u8; 4]) {
    let mut q = PIANO_QUEUE.lock();
    if q.push_back(pkt).is_err() {
        let _ = q.pop_front();
        let _ = q.push_back(pkt);
    }
}

#[embassy_executor::task]
pub async fn piano_drain_loop() {
    async move {
        const IDLE_SLEEP_MS: u64 = 25;

        loop {
            let slot = PIANO_SLOT.load(Ordering::Acquire);
            if slot == 0 {
                Timer::after(EmbassyDuration::from_millis(IDLE_SLEEP_MS)).await;
                continue;
            }

            let pkt_opt = { PIANO_QUEUE.lock().pop_front() };
            let Some(pkt) = pkt_opt else {
                Timer::after(EmbassyDuration::from_millis(IDLE_SLEEP_MS)).await;
                continue;
            };

            // Prefix log lines with last heartbeat time.
            let hb = PIANO_LAST_HEARTBEAT_SECS.load(Ordering::Acquire);
            if hb == u64::MAX {
                crate::log!(
                    "piano: --:--:-- ~ {}.{}.{}.{}\n",
                    pkt[0],
                    pkt[1],
                    pkt[2],
                    pkt[3]
                );
            } else {
                let (h, m, s) = secs_to_hms(hb);
                crate::log!(
                    "piano: {:02}:{:02}:{:02} ~ {}.{}.{}.{}\n",
                    h,
                    m,
                    s,
                    pkt[0],
                    pkt[1],
                    pkt[2],
                    pkt[3]
                );
            }
        }
    }
    .await;
}

#[derive(Copy, Clone, Debug)]
struct MidiEp {
    addr: u8,
    max_packet: u16,
}

#[derive(Copy, Clone, Debug)]
struct MidiInterface {
    configuration_value: u8,
    interface: u8,
    alt_setting: u8,
    ep_in: Option<MidiEp>,
    ep_out: Option<MidiEp>,
}

fn setup_std_nodata(req_type: u8, request: u8, value: u16, index: u16) -> Trb {
    Trb {
        d0: (req_type as u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: (index as u32),
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    }
}

fn parse_midi_interface(cfg: &[u8]) -> Option<MidiInterface> {
    let mut idx = 0usize;
    let mut config_value: u8 = 1;

    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_class: u8 = 0;
    let mut current_sub: u8 = 0;

    let mut best: Option<MidiInterface> = None;

    let mut ep_in: Option<MidiEp> = None;
    let mut ep_out: Option<MidiEp> = None;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }

        match ty {
            2 => {
                // Configuration descriptor
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                // Interface descriptor
                if len >= 9 {
                    // Before switching interfaces, consider committing the previous one.
                    if let Some(iface) = current_iface
                        && current_class == USB_CLASS_AUDIO
                            && current_sub == USB_SUBCLASS_MIDISTREAMING
                            && (ep_in.is_some() || ep_out.is_some())
                        {
                            let candidate = MidiInterface {
                                configuration_value: config_value,
                                interface: iface,
                                alt_setting: current_alt,
                                ep_in,
                                ep_out,
                            };
                            best = Some(candidate);
                            // We could early-return, but continue scanning in case a later altsetting
                            // has a better endpoint pair.
                        }

                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_class = cfg[idx + 5];
                    current_sub = cfg[idx + 6];
                    ep_in = None;
                    ep_out = None;
                } else {
                    current_iface = None;
                }
            }
            5 => {
                // Endpoint descriptor
                if let Some(iface) = current_iface {
                    let _ = iface;
                    if current_class == USB_CLASS_AUDIO && current_sub == USB_SUBCLASS_MIDISTREAMING
                        && len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            let transfer_ty = attrs & 0x3;
                            if transfer_ty == 0x2 {
                                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                                if (ep_addr & 0x80) != 0 {
                                    if ep_in.is_none() {
                                        ep_in = Some(MidiEp {
                                            addr: ep_addr,
                                            max_packet,
                                        });
                                    }
                                } else if ep_out.is_none() {
                                    ep_out = Some(MidiEp {
                                        addr: ep_addr,
                                        max_packet,
                                    });
                                }
                            }
                        }
                }
            }
            _ => {}
        }

        idx += len;
    }

    // Commit last interface.
    if let Some(iface) = current_iface
        && current_class == USB_CLASS_AUDIO
            && current_sub == USB_SUBCLASS_MIDISTREAMING
            && (ep_in.is_some() || ep_out.is_some())
        {
            best = Some(MidiInterface {
                configuration_value: config_value,
                interface: iface,
                alt_setting: current_alt,
                ep_in,
                ep_out,
            });
        }

    best
}

pub fn has_midi_streaming_interface(cfg: &[u8]) -> bool {
    parse_midi_interface(cfg).is_some()
}

struct MidiRuntime {
    controller_id: usize,
    slot_id: u32,
    ctx: XhciContext,

    adapter: MidiAdapterKind,

    interface: u8,
    alt_setting: u8,

    ep_in_target: Option<u32>,
    ring_in: Option<TrbRing>,

    rx_dma_phys: u64,
    rx_dma_virt: *mut u8,
    rx_dma_len: usize,
    rx_posted: bool,

    ring_in_virt: Option<*mut u8>,
    ring_in_bytes: usize,
    ring_out_virt: Option<*mut u8>,
    ring_out_bytes: usize,
}

unsafe impl Send for MidiRuntime {}
unsafe impl Sync for MidiRuntime {}

const MAX_MIDI_DEVICES: usize = 8;
static MIDI_RUNTIMES: Mutex<Vec<MidiRuntime, MAX_MIDI_DEVICES>> = Mutex::new(Vec::new());

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let mut guard = MIDI_RUNTIMES.lock();
    let mut removed = false;
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            let rt = guard.remove(idx);

            if rt.adapter == MidiAdapterKind::CasioCtk3500 {
                piano_set_disconnected(controller_id, slot_id);
            }

            if let Some(v) = rt.ring_in_virt {
                dma::dealloc(v, rt.ring_in_bytes);
            }
            if let Some(v) = rt.ring_out_virt {
                dma::dealloc(v, rt.ring_out_bytes);
            }
            dma::dealloc(rt.rx_dma_virt, rt.rx_dma_len);

            removed = true;
        } else {
            idx += 1;
        }
    }
    removed
}

fn with_runtime_mut_by_slot<R, F>(controller_id: usize, slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut MidiRuntime) -> R,
{
    let mut guard = MIDI_RUNTIMES.lock();
    guard
        .iter_mut()
        .find(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)
        .map(f)
}

pub fn runtime_exists(controller_id: usize, slot_id: u32) -> bool {
    let guard = MIDI_RUNTIMES.lock();
    guard
        .iter()
        .any(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)
}

impl MidiRuntime {
    fn post_rx_locked(&mut self) -> bool {
        if self.rx_posted {
            return true;
        }
        let Some(ref mut ring_in) = self.ring_in else {
            return false;
        };
        let Some(ep_in_target) = self.ep_in_target else {
            return false;
        };

        let trb = Trb {
            d0: lo(self.rx_dma_phys),
            d1: hi(self.rx_dma_phys),
            d2: self.rx_dma_len as u32,
            d3: trb_type(1) | (1 << 5),
        };

        if !ring_in.push(trb) {
            crate::log!("usb: midi rx ring full slot={}\n", self.slot_id);
            return false;
        }

        self.rx_posted = true;
        unsafe {
            write_volatile(self.ctx.doorbell.add(self.slot_id as usize), ep_in_target);
        }
        true
    }

    fn on_rx_complete(&mut self, completion: u32, residual: u32) {
        self.rx_posted = false;

        if completion != 1 && completion != 13 {
            usbv!(
                "usb: midi rx completion cc={} slot={}\n",
                completion,
                self.slot_id
            );
        }

        // Compute actual received bytes best-effort.
        let rx_total = self.rx_dma_len as u32;
        let rx_actual = rx_total.saturating_sub(residual);
        let rx_actual = cmp::min(rx_actual as usize, self.rx_dma_len);

        if rx_actual >= 4 {
            let data = unsafe { core::slice::from_raw_parts(self.rx_dma_virt, rx_actual) };

            if self.adapter == MidiAdapterKind::CasioCtk3500 {
                for chunk in data.chunks_exact(4) {
                    let pkt = [chunk[0], chunk[1], chunk[2], chunk[3]];
                    if is_active_sensing_heartbeat(&pkt) {
                        let now = crate::time::unix_time_seconds()
                            .unwrap_or_else(crate::time::uptime_seconds);
                        PIANO_LAST_HEARTBEAT_SECS.store(now, Ordering::Release);
                        continue;
                    }
                    piano_push_packet(pkt);
                }
            }

            // USB-MIDI Event Packets are 4 bytes each.
            let packets = rx_actual / 4;
            if packets > 0 {
                usbv!(
                    "usb: midi rx slot={} if={} alt={} packets={} bytes={}\n",
                    self.slot_id,
                    self.interface,
                    self.alt_setting,
                    packets,
                    rx_actual
                );

                // High-signal: dump first few packets in verbose mode.
                let dump = cmp::min(packets, 4);
                for i in 0..dump {
                    let off = i * 4;
                    let p = &data[off..off + 4];
                    usbv!(
                        "usb: midi pkt{} [{:02X} {:02X} {:02X} {:02X}]\n",
                        i,
                        p[0],
                        p[1],
                        p[2],
                        p[3]
                    );
                }
            }
        }

        let _ = self.post_rx_locked();
    }
}

pub fn handle_transfer_event(controller_id: usize, evt: &Trb) -> bool {
    let slot_id = (evt.d3 >> 24) & 0xFF ;
    let ep_target = (evt.d3 >> 16) & 0x1F;
    let completion = (evt.d2 >> 24) & 0xFF;
    let residual = evt.d2 & 0x00FF_FFFF;

    with_runtime_mut_by_slot(controller_id, slot_id, |runtime| {
        if Some(ep_target) == runtime.ep_in_target {
            runtime.on_rx_complete(completion, residual);
            true
        } else {
            false
        }
    })
    .unwrap_or(false)
}

async fn configure_bulk_endpoint(
    ctx: &XhciContext,
    cmd_ring: &mut TrbRing,
    slot_id: u32,
    dev_ctx_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    target_port: u8,
    ep_addr: u8,
    ep_max_packet: u16,
    ep_type: u32,
    highest_ep_ctx: u32,
    add_flags_bits: u32,
) -> Result<(u32, TrbRing, *mut u8, usize), ()> {
    let trbs = 64usize;
    let ring_bytes = trbs * size_of::<Trb>();
    let (ep_ring_phys, ep_ring_virt) = dma::alloc(ring_bytes, 64).ok_or(())?;
    unsafe { write_bytes(ep_ring_virt, 0, ring_bytes) };
    let ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, trbs) };

    let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64).ok_or(())?;
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_target = endpoint_target(ep_addr);
    let ep_ctx_index = context_index(ep_addr);

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        // Add Slot Context (bit0) plus endpoint add bits.
        write_volatile(add_flags_ptr.add(1), (1 << 0) | add_flags_bits);

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

        // Copy the output slot context into the input slot context.
        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        // Context Entries = highest valid endpoint context index in *device* context.
        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | ((highest_ep_ctx - 1) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        let mps = (ep_max_packet as u32) & 0x7FF;

        write_volatile(ep_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_cfg = ep_cerr_bits(3);
        ep_cfg |= ep_type_bits(ep_type);
        ep_cfg |= ep_max_packet_bits(mps);
        write_volatile(ep_ctx.add(1), ep_cfg);

        let dq = ep_ring.dequeue_ptr();
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));

        write_volatile(
            ep_ctx.add(4),
            ep_avg_trb_len_bits(mps) | ep_max_esit_payload_lo_bits(mps),
        );
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };

    let res = super::xhci::submit_cmd_and_wait(
        ctx,
        cmd_ring,
        cfg_ep_cmd,
        Some(slot_id),
        "midi-config-ep",
        400,
        EmbassyDuration::from_millis(5),
    )
    .await;

    dma::dealloc(input_cfg_virt, 4096);

    if res.is_err() {
        dma::dealloc(ep_ring_virt, ring_bytes);
        return Err(());
    }

    Ok((ep_target, ep_ring, ep_ring_virt, ring_bytes))
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
    pub target_port: u8,
    pub dev_vid: u16,
    pub dev_pid: u16,
}

pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx,
        cmd_ring,
        ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        target_port,
        dev_vid,
        dev_pid,
    } = params;

    let info = parse_midi_interface(cfg).ok_or(())?;

    // SET_CONFIGURATION
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_std_nodata(0x00, 0x09, info.configuration_value as u16, 0),
        None,
        0,
        "midi-set-configuration",
        800,
    )
    .await?;

    // SET_INTERFACE
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_std_nodata(0x01, 0x0B, info.alt_setting as u16, info.interface as u16),
        None,
        0,
        "midi-set-interface",
        800,
    )
    .await?;

    // Prepare context indices for whichever endpoints exist.
    let mut highest_ep_ctx: u32 = 1;

    let ep_in_ctx = info.ep_in.map(|ep| context_index(ep.addr));
    let ep_out_ctx = info.ep_out.map(|ep| context_index(ep.addr));

    let add_flags_in: u32 = ep_in_ctx.map(|ci| 1 << (ci - 1)).unwrap_or(0);
    let add_flags_out: u32 = ep_out_ctx.map(|ci| 1 << (ci - 1)).unwrap_or(0);

    if let Some(ci) = ep_in_ctx {
        highest_ep_ctx = cmp::max(highest_ep_ctx, ci);
    }
    if let Some(co) = ep_out_ctx {
        highest_ep_ctx = cmp::max(highest_ep_ctx, co);
    }

    let mut ep_in_target: Option<u32> = None;
    let mut ring_in: Option<TrbRing> = None;
    let mut ring_in_virt: Option<*mut u8> = None;
    let mut ring_in_bytes: usize = 0;

    let mut ring_out_virt: Option<*mut u8> = None;
    let mut ring_out_bytes: usize = 0;

    // Configure endpoints. Each call submits a Configure Endpoint command, but we pass the
    // final Context Entries so the slot context stays consistent.
    if let Some(ep) = info.ep_in {
        let (target, ring, virt, bytes) = configure_bulk_endpoint(
            ctx,
            cmd_ring,
            slot_id,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            target_port,
            ep.addr,
            ep.max_packet,
            EP_TYPE_BULK_IN,
            highest_ep_ctx,
            add_flags_in,
        )
        .await?;
        ep_in_target = Some(target);
        ring_in = Some(ring);
        ring_in_virt = Some(virt);
        ring_in_bytes = bytes;
    }

    if let Some(ep) = info.ep_out {
        let (_target, _ring, virt, bytes) = configure_bulk_endpoint(
            ctx,
            cmd_ring,
            slot_id,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            target_port,
            ep.addr,
            ep.max_packet,
            EP_TYPE_BULK_OUT,
            highest_ep_ctx,
            add_flags_out,
        )
        .await?;
        ring_out_virt = Some(virt);
        ring_out_bytes = bytes;
    }

    // Allocate a small RX buffer and begin polling the IN endpoint (if present).
    let rx_len = info
        .ep_in
        .map(|ep| cmp::max(64usize, ep.max_packet as usize))
        .unwrap_or(64usize);

    let (rx_dma_phys, rx_dma_virt) = dma::alloc(rx_len, 64).ok_or(())?;
    unsafe { write_bytes(rx_dma_virt, 0, rx_len) };

    let adapter = select_adapter(dev_vid, dev_pid);
    let runtime = MidiRuntime {
        controller_id: ctx.controller_id,
        slot_id,
        ctx: *ctx,
        adapter,
        interface: info.interface,
        alt_setting: info.alt_setting,
        ep_in_target,
        ring_in,
        rx_dma_phys,
        rx_dma_virt,
        rx_dma_len: rx_len,
        rx_posted: false,
        ring_in_virt,
        ring_in_bytes,
        ring_out_virt,
        ring_out_bytes,
    };

    let mut guard = MIDI_RUNTIMES.lock();
    if let Some(existing) = guard
        .iter_mut()
        .find(|rt| rt.controller_id == runtime.controller_id && rt.slot_id == runtime.slot_id)
    {
        *existing = runtime;
    } else {
        let _ = guard.push(runtime);
    }
    drop(guard);

    if adapter == MidiAdapterKind::CasioCtk3500 {
        piano_set_connected(ctx.controller_id, slot_id);
        crate::v::readiness::set(crate::v::readiness::PIANO_CLAIMED);
    }

    let posted = with_runtime_mut_by_slot(ctx.controller_id, slot_id, |rt| rt.post_rx_locked());
    if posted != Some(true) {
        crate::log!(
            "usb: midi warn: failed to post initial rx slot={} ep_in={:?}\n",
            slot_id,
            info.ep_in.map(|e| e.addr)
        );
    }

    crate::log!(
        "usb: midi attached slot={} if={} alt={} ep_in={:?} ep_out={:?} adapter={:?}\n",
        slot_id,
        info.interface,
        info.alt_setting,
        info.ep_in.map(|e| e.addr),
        info.ep_out.map(|e| e.addr),
        adapter,
    );

    Ok(())
}
