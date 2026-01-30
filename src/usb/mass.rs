use super::xhci::{
    self, context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo,
    trb_type, Trb, TrbRing, TrbRingState, XhciContext, EP_STATE_DISABLED, EP_TYPE_BULK_IN,
    EP_TYPE_BULK_OUT,
};
use super::bot;
use super::scsi;
use crate::pci::dma;
use crate::disc::block as disc_block;
use core::hint::spin_loop;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;
use alloc::boxed::Box;
use alloc::vec::Vec as AllocVec;
use heapless::Vec;
use spin::Mutex;

fn spin_wait_ms(ms: u64) {
    let hz = embassy_time_driver::TICK_HZ as u64;
    let start = embassy_time_driver::now();
    let delta_ticks = if hz == 0 {
        0
    } else {
        // Round up to at least one tick when ms>0.
        let ticks = (ms.saturating_mul(hz) + 999) / 1000;
        if ms > 0 { ticks.max(1) } else { 0 }
    };
    let deadline = start.saturating_add(delta_ticks);
    while embassy_time_driver::now() < deadline {
        crate::time::poll_executor();
        spin_loop();
    }
}

#[inline]
fn sense_is_transient(key: scsi::SenseKey) -> bool {
    matches!(key, scsi::SenseKey::NotReady | scsi::SenseKey::UnitAttention | scsi::SenseKey::AbortedCommand)
}

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;
const MAX_MASS_DEVICES: usize = 4;

pub struct BulkPair {
    pub configuration: u8,
    pub interface: u8,
    pub ep_in: u8,
    pub ep_out: u8,
    pub max_packet_in: u16,
    pub max_packet_out: u16,
}

pub struct MassRuntime {
    pub controller_id: usize,
    pub ctx: XhciContext,
    pub info: BulkPair,
    pub slot_id: u32,
    pub ep0_state: TrbRingState,
    pub ep_in_target: u32,
    pub ep_out_target: u32,
    pub ring_in: TrbRing,
    pub ring_out: TrbRing,
    pub bot_tag: u32,
    pub disc: Option<disc_block::DeviceHandle>,
    pub block_size: u32,
    pub block_count: u64,
}

unsafe impl Send for MassRuntime {}
unsafe impl Sync for MassRuntime {}

impl MassRuntime {
    async fn bot_reset_recovery(&mut self) {
        // Bulk-Only Transport reset recovery (spec):
        // 1) Class-specific Mass Storage Reset (to interface)
        // 2) Clear HALT on both bulk endpoints
        // NOTE: This does not reset xHC endpoint ring dequeue pointers.
        crate::log!(
            "usb: mass reset recovery slot={} iface={} ep_in=0x{:02X} ep_out=0x{:02X}\n",
            self.slot_id,
            self.info.interface,
            self.info.ep_in,
            self.info.ep_out
        );

        let mut ep0_ring = unsafe { TrbRing::from_state(self.ep0_state) };

        // bmRequestType = 0x21 (Host->Dev, Class, Interface)
        // bRequest = 0xFF (Mass Storage Reset)
        let setup_reset = Trb {
            d0: 0x21 | ((0xFFu32) << 8),
            d1: (self.info.interface as u32),
            d2: 8,
            d3: trb_type(2) | (1 << 6),
        };

        let _ = super::control_out(
            &self.ctx,
            &mut ep0_ring,
            self.slot_id,
            setup_reset,
            None,
            0,
            "bot-reset",
            400,
        )
        .await;

        // bmRequestType = 0x02 (Host->Dev, Standard, Endpoint)
        // bRequest = 0x01 (CLEAR_FEATURE)
        // wValue = 0 (ENDPOINT_HALT)
        let clear_halt = |ep_addr: u8| Trb {
            d0: 0x02 | ((1u32) << 8),
            d1: (ep_addr as u32),
            d2: 8,
            d3: trb_type(2) | (1 << 6),
        };

        let _ = super::control_out(
            &self.ctx,
            &mut ep0_ring,
            self.slot_id,
            clear_halt(self.info.ep_out),
            None,
            0,
            "bot-clear-halt-out",
            400,
        )
        .await;
        let _ = super::control_out(
            &self.ctx,
            &mut ep0_ring,
            self.slot_id,
            clear_halt(self.info.ep_in),
            None,
            0,
            "bot-clear-halt-in",
            400,
        )
        .await;

        self.ep0_state = ep0_ring.snapshot();
    }
}

static MASS_RUNTIMES: Mutex<Vec<MassRuntime, MAX_MASS_DEVICES>> = Mutex::new(Vec::new());

fn take_runtime(controller_id: usize, slot_id: u32) -> Option<MassRuntime> {
    let mut guard = MASS_RUNTIMES.lock();
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            return Some(guard.remove(idx));
        }
        idx += 1;
    }
    None
}

pub fn register_runtime(rt: MassRuntime) {
    let mut guard = MASS_RUNTIMES.lock();
    if let Some(existing) = guard.iter_mut().find(|r| {
        r.controller_id == rt.controller_id
            && r.slot_id == rt.slot_id
            && r.ep_in_target == rt.ep_in_target
            && r.ep_out_target == rt.ep_out_target
    }) {
        *existing = rt;
        return;
    }
    let _ = guard.push(rt);
}

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let mut guard = MASS_RUNTIMES.lock();
    let mut removed = false;
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            let _ = guard.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }
    removed
}

pub fn has_runtime() -> bool {
    !MASS_RUNTIMES.lock().is_empty()
}

pub fn with_runtime_by_slot<R, F>(controller_id: usize, slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&MassRuntime) -> R,
{
    MASS_RUNTIMES
        .lock()
        .iter()
        .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
        .map(f)
}

pub fn parse_mass_interface(cfg: &[u8]) -> Option<BulkPair> {
    let mut idx = 0usize;
    let mut config_value: u8 = 1;
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_subclass: u8 = 0;
    let mut current_proto: u8 = 0;
    let mut ep_in: Option<(u8, u16)> = None;
    let mut ep_out: Option<(u8, u16)> = None;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 9 {
                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_subclass = cfg[idx + 6];
                    current_proto = cfg[idx + 7];
                    ep_in = None;
                    ep_out = None;
                } else {
                    current_iface = None;
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    if current_alt == 0
                        && current_subclass == USB_SUBCLASS_SCSI
                        && current_proto == USB_PROTO_BULK_ONLY
                    {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            if (attrs & 0x3) == 0x2 {
                                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                                if (ep_addr & 0x80) != 0 {
                                    if ep_in.is_none() {
                                        ep_in = Some((ep_addr, max_packet));
                                    }
                                } else if ep_out.is_none() {
                                    ep_out = Some((ep_addr, max_packet));
                                }

                                if let (Some((in_addr, in_mps)), Some((out_addr, out_mps))) =
                                    (ep_in, ep_out)
                                {
                                    return Some(BulkPair {
                                        configuration: config_value,
                                        interface: iface,
                                        ep_in: in_addr,
                                        ep_out: out_addr,
                                        max_packet_in: in_mps,
                                        max_packet_out: out_mps,
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
}

pub async fn attach_mass_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx,
        cmd_ring,
        ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code: _,
        target_port,
    } = params;

    let Some(pair) = parse_mass_interface(cfg) else {
        return Err(());
    };

    crate::log!(
        "usb: mass storage iface={} cfg={} ep_in=0x{:02X} ep_out=0x{:02X}\n",
        pair.interface,
        pair.configuration,
        pair.ep_in,
        pair.ep_out
    );

    // Set configuration once.
    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((pair.configuration as u32) << 16),
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
        crate::log!("usb: ep0 ring overflow for set_configuration (mass)\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let Some(set_cfg_evt) = xhci::wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
        },
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        crate::log!("usb: timeout waiting for set-configuration (mass)\n");
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    // Prepare endpoint rings.
    let (ring_in_phys, ring_in_virt) = match dma::alloc(64 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc bulk IN ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(ring_in_virt, 0, 64 * size_of::<Trb>()) };
    let ring_in = unsafe { TrbRing::new(ring_in_phys, ring_in_virt as *mut Trb, 64) };

    let (ring_out_phys, ring_out_virt) = match dma::alloc(64 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc bulk OUT ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(ring_out_virt, 0, 64 * size_of::<Trb>()) };
    let ring_out = unsafe { TrbRing::new(ring_out_phys, ring_out_virt as *mut Trb, 64) };

    let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc input ctx for mass endpoints\n");
            return Err(());
        }
    };
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_in_target = endpoint_target(pair.ep_in);
    let ep_out_target = endpoint_target(pair.ep_out);
    let ep_in_ctx_index = context_index(pair.ep_in);
    let ep_out_ctx_index = context_index(pair.ep_out);
    let highest_ep_ctx = core::cmp::max(ep_in_ctx_index, ep_out_ctx_index);

    // Add Context Flags bit index (slot=0, ep0=1, ep1out=2, ep1in=3, ...)
    let ep_in_add_bit = ep_in_ctx_index - 1;
    let ep_out_add_bit = ep_out_ctx_index - 1;

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        write_volatile(
            add_flags_ptr.add(1),
            (1 << 0) | (1 << ep_in_add_bit) | (1 << ep_out_add_bit),
        );

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_in_ctx_off: usize = ctx_stride_bytes * (ep_in_ctx_index as usize);
        let ep_out_ctx_off: usize = ctx_stride_bytes * (ep_out_ctx_index as usize);
        let ep_in_ctx = input_cfg_virt.add(ep_in_ctx_off) as *mut u32;
        let ep_out_ctx = input_cfg_virt.add(ep_out_ctx_off) as *mut u32;

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

        let mps_in = (pair.max_packet_in as u32) & 0x7FF;
        let mps_out = (pair.max_packet_out as u32) & 0x7FF;

        // Bulk IN endpoint
        write_volatile(ep_in_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_in_cfg = ep_cerr_bits(3);
        ep_in_cfg |= ep_type_bits(EP_TYPE_BULK_IN);
        ep_in_cfg |= ep_max_packet_bits(mps_in);
        write_volatile(ep_in_ctx.add(1), ep_in_cfg);
        let dq_in = ring_in.dequeue_ptr();
        write_volatile(ep_in_ctx.add(2), lo(dq_in));
        write_volatile(ep_in_ctx.add(3), hi(dq_in));
        write_volatile(
            ep_in_ctx.add(4),
            ep_avg_trb_len_bits(mps_in) | ep_max_esit_payload_lo_bits(mps_in),
        );

        // Bulk OUT endpoint
        write_volatile(ep_out_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_out_cfg = ep_cerr_bits(3);
        ep_out_cfg |= ep_type_bits(EP_TYPE_BULK_OUT);
        ep_out_cfg |= ep_max_packet_bits(mps_out);
        write_volatile(ep_out_ctx.add(1), ep_out_cfg);
        let dq_out = ring_out.dequeue_ptr();
        write_volatile(ep_out_ctx.add(2), lo(dq_out));
        write_volatile(ep_out_ctx.add(3), hi(dq_out));
        write_volatile(
            ep_out_ctx.add(4),
            ep_avg_trb_len_bits(mps_out) | ep_max_esit_payload_lo_bits(mps_out),
        );
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    xhci::submit_cmd_and_wait(
        ctx,
        cmd_ring,
        cfg_ep_cmd,
        Some(slot_id),
        "mass-config-ep",
        400,
        EmbassyDuration::from_millis(5),
    )
    .await?;

    // Build the runtime locally first so we can perform best-effort async probing
    // without ever holding MASS_RUNTIMES across an .await.
    let controller_id = ctx.controller_id;
    let mut rt = MassRuntime {
        controller_id,
        ctx: *ctx,
        info: pair,
        slot_id,
        ep0_state: ep0_ring.snapshot(),
        ep_in_target,
        ep_out_target,
        ring_in,
        ring_out,
        bot_tag: 1,
        disc: None,
        block_size: 512,
        block_count: 0,
    };

    // Prove the SCSI/BOT path early: attempt a basic INQUIRY + READ CAPACITY.
    // This is best-effort; failures should not prevent runtime registration.
    let tag_tur = rt.bot_tag;
    if bot::scsi_test_unit_ready(
        ctx,
        &mut rt.ring_out,
        &mut rt.ring_in,
        slot_id,
        rt.ep_out_target,
        rt.ep_in_target,
        tag_tur,
    )
    .await
    .is_ok()
    {
        rt.bot_tag = rt.bot_tag.wrapping_add(1);
    }

    let tag0 = rt.bot_tag;
    if let Ok(inq) = bot::scsi_inquiry_basic(
        ctx,
        &mut rt.ring_out,
        &mut rt.ring_in,
        slot_id,
        rt.ep_out_target,
        rt.ep_in_target,
        tag0,
    )
    .await
    {
        rt.bot_tag = rt.bot_tag.wrapping_add(1);
        crate::log!(
            "usb: mass inquiry pdt={} removable={} vendor={:?} product={:?} rev={:?}\n",
            inq.peripheral_type,
            inq.removable,
            &inq.vendor,
            &inq.product,
            &inq.revision
        );
    }

    let tag1 = rt.bot_tag;
    if let Ok(cap) = bot::scsi_read_capacity_10(
        ctx,
        &mut rt.ring_out,
        &mut rt.ring_in,
        slot_id,
        rt.ep_out_target,
        rt.ep_in_target,
        tag1,
    )
    .await
    {
        rt.bot_tag = rt.bot_tag.wrapping_add(1);
        rt.block_size = cap.block_size.max(1);
        rt.block_count = (cap.last_lba as u64).saturating_add(1);
        crate::log!(
            "usb: mass capacity last_lba={} block_size={} bytes\n",
            cap.last_lba,
            cap.block_size
        );
    }

    let block_size = rt.block_size;
    let block_count = rt.block_count;
    register_runtime(rt);

    // Only register a block device if we got a sane capacity.
    if block_count > 0 {
        let descriptor = disc_block::DeviceDescriptor::new(disc_block::DeviceKind::Unknown)
            .with_label("usbms");

        let dev = UsbMassBlockDevice {
            controller_id,
            slot_id,
            block_size,
            block_count,
        };
        let handle = disc_block::register_device(descriptor, dev);

        // IMPORTANT: drop the MASS_RUNTIMES lock before doing any filesystem probing.
        // Probing can read blocks, which routes back through this USBMS block device and
        // needs to take MASS_RUNTIMES again.
        {
            let mut guard = MASS_RUNTIMES.lock();
            if let Some(rt) = guard
                .iter_mut()
                .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
            {
                rt.disc = Some(handle);
            }
        }

        crate::log!(
            "usb: mass registered block device id={} blocks={} block_size={}\n",
            handle.id().raw(),
            block_count,
            block_size
        );

        // Best-effort: defer TRUEOSFS probing/mounting.
        // Doing this synchronously here can stall USB enumeration and starve xHCI poll tasks.
        crate::v::fs::trueosfs::request_mount_root(handle);

        // If we're booting from a single USB pen drive, trigger the TrueOSFS BSP smoke test
        // after USB mass storage has registered into the block registry.
        // This is intentionally deferred (see `bsp_smoke_service_task`) to avoid stalling
        // USB enumeration with synchronous filesystem I/O.
        crate::v::fs::trueosfs::request_bsp_smoke_test();
    }

    Ok(())
}

struct UsbMassBlockDevice {
    controller_id: usize,
    slot_id: u32,
    block_size: u32,
    block_count: u64,
}

impl disc_block::BlockDevice for UsbMassBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(&'a mut self, lba: u64, blocks: usize) -> disc_block::BoxFuture<'a, disc_block::Result<AllocVec<u8>>> {
        Box::pin(async move {
            let block_size = self.block_size_bytes() as usize;
            if block_size == 0 {
                return Err(disc_block::Error::InvalidParam);
            }
            if blocks == 0 {
                return Ok(AllocVec::new());
            }

            let blocks_total = blocks as u64;
            let end = lba.checked_add(blocks_total).ok_or(disc_block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(disc_block::Error::OutOfBounds);
            }

            let total_bytes = blocks
                .checked_mul(block_size)
                .ok_or(disc_block::Error::InvalidParam)?;
            let mut out = alloc::vec![0u8; total_bytes];

            // Copy-based DMA IO: xHCI requires DMA-safe buffers.
            const MAX_IO_BYTES: usize = 64 * 1024;
            let mut remaining = out.as_mut_slice();
            let mut cur_lba = lba;

            let mut rt = take_runtime(self.controller_id, self.slot_id).ok_or(disc_block::Error::NotReady)?;
            let ctx = rt.ctx;
            let mut tag = rt.bot_tag;

            while !remaining.is_empty() {
                let max_blocks = (MAX_IO_BYTES / block_size).max(1);
                let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                let bytes_here = blocks_here * block_size;

                let (_dma_phys, dma_virt) = dma::alloc_with_max(bytes_here, 64, None)
                    .ok_or(disc_block::Error::DmaUnavailable)?;
                unsafe { write_bytes(dma_virt, 0, bytes_here) };

                let mut ok = false;
                let mut attempts = 0u8;
                while attempts < 10 {
                    let c = bot::scsi_read_10(
                        &ctx,
                        &mut rt.ring_out,
                        &mut rt.ring_in,
                        self.slot_id,
                        rt.ep_out_target,
                        rt.ep_in_target,
                        tag,
                        cur_lba as u32,
                        blocks_here as u16,
                        unsafe { core::slice::from_raw_parts_mut(dma_virt, bytes_here) },
                    )
                    .await;
                    tag = tag.wrapping_add(1);
                    match c {
                        Ok(csw) if csw.status == bot::BotStatus::Passed => {
                            ok = true;
                            break;
                        }
                        Ok(csw) => {
                            if let Some(sense) = bot::scsi_request_sense_fixed_async(
                                &ctx,
                                &mut rt.ring_out,
                                &mut rt.ring_in,
                                self.slot_id,
                                rt.ep_out_target,
                                rt.ep_in_target,
                                tag,
                            )
                            .await
                            {
                                tag = tag.wrapping_add(1);
                                crate::log!(
                                    "usb: mass read csw={:?} sense rc={:#x} key={:?} asc={:#x} ascq={:#x}\n",
                                    csw.status,
                                    sense.response_code,
                                    sense.sense_key,
                                    sense.asc,
                                    sense.ascq
                                );
                                if sense_is_transient(sense.sense_key) {
                                    attempts = attempts.wrapping_add(1);
                                    embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                                    continue;
                                }
                            } else {
                                crate::log!("usb: mass read csw={:?} request-sense failed\n", csw.status);
                                attempts = attempts.wrapping_add(1);
                                embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                                continue;
                            }
                            break;
                        }
                        Err(()) => {
                            if attempts == 0 {
                                rt.bot_reset_recovery().await;
                            }
                            attempts = attempts.wrapping_add(1);
                            embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                            continue;
                        }
                    }
                }

                if !ok {
                    dma::dealloc(dma_virt, bytes_here);
                    rt.bot_tag = tag;
                    register_runtime(rt);
                    return Err(disc_block::Error::Io);
                }

                unsafe {
                    let src = core::slice::from_raw_parts(dma_virt, bytes_here);
                    remaining[..bytes_here].copy_from_slice(src);
                }
                dma::dealloc(dma_virt, bytes_here);

                remaining = &mut remaining[bytes_here..];
                cur_lba += blocks_here as u64;
            }

            rt.bot_tag = tag;
            register_runtime(rt);
            Ok(out)
        })
    }

    fn write_blocks<'a>(&'a mut self, lba: u64, buf: &'a [u8]) -> disc_block::BoxFuture<'a, disc_block::Result<()>> {
        Box::pin(async move {
            let block_size = self.block_size_bytes() as usize;
            if block_size == 0 || buf.len() % block_size != 0 {
                return Err(disc_block::Error::InvalidParam);
            }

            let blocks_total = (buf.len() / block_size) as u64;
            if blocks_total == 0 {
                return Ok(());
            }

            let end = lba.checked_add(blocks_total).ok_or(disc_block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(disc_block::Error::OutOfBounds);
            }

            const MAX_IO_BYTES: usize = 64 * 1024;
            let mut remaining = buf;
            let mut cur_lba = lba;

            let mut rt = take_runtime(self.controller_id, self.slot_id).ok_or(disc_block::Error::NotReady)?;
            let ctx = rt.ctx;
            let mut tag = rt.bot_tag;

            while !remaining.is_empty() {
                let max_blocks = (MAX_IO_BYTES / block_size).max(1);
                let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                let bytes_here = blocks_here * block_size;

                let (_dma_phys, dma_virt) = dma::alloc_with_max(bytes_here, 64, None)
                    .ok_or(disc_block::Error::DmaUnavailable)?;
                unsafe {
                    core::ptr::copy_nonoverlapping(remaining.as_ptr(), dma_virt, bytes_here);
                }

                let mut ok = false;
                let mut attempts = 0u8;
                while attempts < 10 {
                    let c = bot::scsi_write_10(
                        &ctx,
                        &mut rt.ring_out,
                        &mut rt.ring_in,
                        self.slot_id,
                        rt.ep_out_target,
                        rt.ep_in_target,
                        tag,
                        cur_lba as u32,
                        blocks_here as u16,
                        unsafe { core::slice::from_raw_parts(dma_virt, bytes_here) },
                    )
                    .await;
                    tag = tag.wrapping_add(1);
                    match c {
                        Ok(csw) if csw.status == bot::BotStatus::Passed => {
                            ok = true;
                            break;
                        }
                        Ok(csw) => {
                            if let Some(sense) = bot::scsi_request_sense_fixed_async(
                                &ctx,
                                &mut rt.ring_out,
                                &mut rt.ring_in,
                                self.slot_id,
                                rt.ep_out_target,
                                rt.ep_in_target,
                                tag,
                            )
                            .await
                            {
                                tag = tag.wrapping_add(1);
                                crate::log!(
                                    "usb: mass write csw={:?} sense rc={:#x} key={:?} asc={:#x} ascq={:#x}\n",
                                    csw.status,
                                    sense.response_code,
                                    sense.sense_key,
                                    sense.asc,
                                    sense.ascq
                                );
                                if sense_is_transient(sense.sense_key) {
                                    attempts = attempts.wrapping_add(1);
                                    embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                                    continue;
                                }
                            } else {
                                crate::log!("usb: mass write csw={:?} request-sense failed\n", csw.status);
                                attempts = attempts.wrapping_add(1);
                                embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                                continue;
                            }
                            break;
                        }
                        Err(()) => {
                            if attempts == 0 {
                                rt.bot_reset_recovery().await;
                            }
                            attempts = attempts.wrapping_add(1);
                            embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                            continue;
                        }
                    }
                }

                dma::dealloc(dma_virt, bytes_here);
                if !ok {
                    rt.bot_tag = tag;
                    register_runtime(rt);
                    return Err(disc_block::Error::Io);
                }

                remaining = &remaining[bytes_here..];
                cur_lba += blocks_here as u64;
            }

            rt.bot_tag = tag;
            register_runtime(rt);
            Ok(())
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        // USB mass storage (BOT/SCSI) does *not* DMA into the caller's buffer.
        // We always transfer via our own xHCI DMA buffers and then copy.
        // Therefore the caller buffer does not need any special DMA alignment.
        1
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush<'a>(&'a mut self) -> disc_block::BoxFuture<'a, disc_block::Result<()>> {
        Box::pin(async move {
        // Best-effort cache flush for USB mass storage.
        // Many flash drives are fine without it, but some will not make data durable
        // across power-loss/reboot unless we issue SYNCHRONIZE CACHE.
        let mut rt = take_runtime(self.controller_id, self.slot_id).ok_or(disc_block::Error::NotReady)?;

        let ctx = rt.ctx;
        let mut tag = rt.bot_tag;

        let mut attempts = 0u8;
        while attempts < 10 {
            let c = bot::scsi_synchronize_cache_10_sync(
                &ctx,
                &mut rt.ring_out,
                &mut rt.ring_in,
                self.slot_id,
                rt.ep_out_target,
                rt.ep_in_target,
                tag,
            );
            tag = tag.wrapping_add(1);

            match c {
                Ok(csw) if csw.status == bot::BotStatus::Passed => {
                    rt.bot_tag = tag;
                    register_runtime(rt);
                    return Ok(());
                }
                Ok(csw) => {
                    // Some devices report IllegalRequest for sync-cache; treat as success.
                    if let Some(sense) = bot::scsi_request_sense_fixed_sync(
                        &ctx,
                        &mut rt.ring_out,
                        &mut rt.ring_in,
                        self.slot_id,
                        rt.ep_out_target,
                        rt.ep_in_target,
                        tag,
                    ) {
                        tag = tag.wrapping_add(1);
                        crate::log!(
                            "usb: mass flush csw={:?} sense rc={:#x} key={:?} asc={:#x} ascq={:#x}\n",
                            csw.status,
                            sense.response_code,
                            sense.sense_key,
                            sense.asc,
                            sense.ascq
                        );

                        if sense.sense_key == scsi::SenseKey::IllegalRequest {
                            rt.bot_tag = tag;
                            register_runtime(rt);
                            return Ok(());
                        }
                        if sense_is_transient(sense.sense_key) {
                            attempts = attempts.wrapping_add(1);
                            embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                            continue;
                        }
                    } else {
                        crate::log!("usb: mass flush csw={:?} request-sense failed\n", csw.status);
                        attempts = attempts.wrapping_add(1);
                        embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                        continue;
                    }

                    rt.bot_tag = tag;
                    register_runtime(rt);
                    return Err(disc_block::Error::Io);
                }
                Err(()) => {
                    attempts = attempts.wrapping_add(1);
                    embassy_time::Timer::after(EmbassyDuration::from_millis(25)).await;
                    continue;
                }
            }
        }

        rt.bot_tag = tag;
        register_runtime(rt);
        Err(disc_block::Error::Io)
        })
    }
}
