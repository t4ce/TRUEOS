//! Isochronous transport helper built on xHCI primitives.
//! This keeps TRB/endpoint-context details inside xHCI while exposing a small API
//! for class drivers (e.g., UAC) to queue periodic packets.

use crate::pci::dma;
use crate::usb::xhci::{
    self, EP_STATE_DISABLED, EP_TYPE_ISOCH_OUT, Trb, TrbRing, XhciContext, context_index,
    endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits, ep_max_burst_bits,
    ep_max_esit_payload_hi_bits, ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_mult_bits,
    ep_state_bits, ep_type_bits, hi, lo, trb_type,
};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;

/// Minimal configuration needed to stand up an isoch OUT endpoint.
pub struct IsochOutConfig {
    pub slot_id: u32,
    pub ep_addr: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub max_esit_payload: u16,
    /// bMaxBurst for SuperSpeed, otherwise unused.
    pub ss_max_burst: u8,
    /// bmAttributes[1:0] from the SuperSpeed companion descriptor.
    pub ss_mult: u8,
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub speed_code: u32,
    pub target_port: u8,
}

pub struct IsochOutPipe {
    pub ep_target: u32,
    pub ring: TrbRing,
    pub max_packet: u16,
}

impl IsochOutPipe {
    /// Create an isoch OUT pipe: configures the endpoint context and returns the ring.
    pub async fn create(
        ctx: &XhciContext,
        cmd_ring: &mut TrbRing,
        cfg: IsochOutConfig,
    ) -> Result<Self, ()> {
        let IsochOutConfig {
            slot_id,
            ep_addr,
            max_packet,
            interval,
            max_esit_payload,
            ss_max_burst,
            ss_mult,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            speed_code,
            target_port,
        } = cfg;

        // Allocate transfer ring.
        // Note: `TrbRing` does not currently do producer/consumer fullness tracking.
        // UAC keeps an explicit in-flight budget; this ring only needs to be "reasonably big".
        const ISOCH_TRBS: usize = 256;
        let ring_bytes = ISOCH_TRBS * size_of::<Trb>();
        let (ep_ring_phys, ep_ring_virt) = dma::alloc(ring_bytes, 64).ok_or(())?;
        unsafe { write_bytes(ep_ring_virt, 0, ring_bytes) };
        let ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, ISOCH_TRBS) };

        // Allocate input context for Configure Endpoint.
        let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64).ok_or(())?;
        unsafe { write_bytes(input_cfg_virt, 0, 4096) };

        let ep_target = endpoint_target(ep_addr);
        let ep_ctx_index = context_index(ep_addr);
        let ep_add_bit = ep_ctx_index - 1; // Add Context Flags bit index

        // Derive the raw packet size fields once; `packet_bytes` is also returned in the pipe.
        let is_high_speed = speed_code == 3;
        let is_super_speed = speed_code >= 4;
        let raw_mps = max_packet as u32;
        let packet_bytes = raw_mps & 0x7FF;
        let hs_additional = (raw_mps >> 11) & 0x3;

        unsafe {
            let add_flags_ptr = input_cfg_virt as *mut u32;
            write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_add_bit));

            let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
            let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
            let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

            // Copy current device slot context.
            let dev_slot_ctx = dev_ctx_virt as *const u32;
            for i in 0..ctx_stride_words {
                write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
            }

            // Update Context Entries field to include this ep.
            let mut dw0 = read_volatile(slot_ctx.add(0));
            dw0 = (dw0 & !(0x1F << 27)) | (ep_add_bit << 27);
            write_volatile(slot_ctx.add(0), dw0);

            // Ensure root port remains correct.
            let mut dw1 = read_volatile(slot_ctx.add(1));
            dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
            write_volatile(slot_ctx.add(1), dw1);

            // Endpoint context
            let mult_field = if is_super_speed {
                (ss_mult & 0x3) as u32
            } else {
                0
            };
            let burst_field = if is_super_speed {
                ss_max_burst as u32
            } else if is_high_speed {
                hs_additional
            } else {
                0
            };

            // Interval encoding in xHCI endpoint contexts is in units of microframes,
            // expressed as log2(period_in_microframes).
            //
            // - High-/SuperSpeed: USB `bInterval` is already the exponent for microframes,
            //   so Interval = bInterval - 1.
            // - Full-/LowSpeed: USB `bInterval` is in frames (1ms), so convert to
            //   microframes first: period = 8 * bInterval, then take log2-ceil.
            //
            // Getting this wrong causes the controller to schedule at the wrong cadence
            // (classic "audio bursts / gaps" symptom).
            let interval_field = {
                let raw = if speed_code == 1 || speed_code == 2 {
                    // FS/LS: frames -> microframes -> log2-ceil.
                    let period_uf = core::cmp::max(1u32, interval as u32).saturating_mul(8);
                    let mut log2 = 0u32;
                    let mut pow2 = 1u32;
                    while pow2 < period_uf && log2 < 15 {
                        pow2 <<= 1;
                        log2 += 1;
                    }
                    log2
                } else {
                    // HS/SS: exponent already.
                    interval.saturating_sub(1) as u32
                };
                core::cmp::min(15u32, raw)
            };

            // One-time sanity log: interval programming is the #1 reason for
            // "isoch runs at ~80Hz instead of 1kHz" failures.
            crate::log!(
                "usb: uac xhci interval speed_code={} bInterval={} interval_field={} period={}us\n",
                speed_code,
                interval,
                interval_field,
                125u32.saturating_mul(1u32 << interval_field)
            );

            let mut esit_payload = core::cmp::max(packet_bytes, max_esit_payload as u32);
            if is_high_speed {
                let hs_payload = packet_bytes * (burst_field + 1);
                esit_payload = core::cmp::max(esit_payload, hs_payload);
            }

            let mut ep_info = ep_state_bits(EP_STATE_DISABLED);
            ep_info |= ep_mult_bits(mult_field);
            ep_info |= ep_interval_bits(interval_field);
            ep_info |= ep_max_esit_payload_hi_bits(esit_payload);
            write_volatile(ep_ctx.add(0), ep_info);

            let mut ep_info2 = ep_cerr_bits(3);
            ep_info2 |= ep_type_bits(EP_TYPE_ISOCH_OUT);
            ep_info2 |= ep_max_burst_bits(burst_field);
            ep_info2 |= ep_max_packet_bits(packet_bytes);
            write_volatile(ep_ctx.add(1), ep_info2);

            let dq = ep_ring.dequeue_ptr();
            write_volatile(ep_ctx.add(2), lo(dq));
            write_volatile(ep_ctx.add(3), hi(dq));

            let mut dw4 = ep_avg_trb_len_bits(packet_bytes);
            dw4 |= ep_max_esit_payload_lo_bits(esit_payload);
            write_volatile(ep_ctx.add(4), dw4);
        }

        // Issue Configure Endpoint.
        let cfg_ep_cmd = Trb {
            d0: xhci::lo(input_cfg_phys),
            d1: xhci::hi(input_cfg_phys),
            d2: 0,
            d3: trb_type(12) | (slot_id << 24),
        };
        let cfg_res = xhci::submit_cmd_and_wait(
            ctx,
            cmd_ring,
            cfg_ep_cmd,
            Some(slot_id),
            "isoch-config-ep",
            400,
            EmbassyDuration::from_millis(5),
        )
        .await;
        // Configure-endpoint input context is temporary and can be freed regardless of outcome.
        dma::dealloc(input_cfg_virt, 4096);
        if cfg_res.is_err() {
            dma::dealloc(ep_ring_virt, ring_bytes);
            return Err(());
        }

        Ok(IsochOutPipe {
            ep_target,
            ring: ep_ring,
            max_packet: packet_bytes as u16,
        })
    }

    /// Queue a single isochronous OUT packet (caller provides DMA buffer phys address).
    ///
    /// If `chain` is set, this TRB is part of a larger Transfer Descriptor (TD) and the
    /// controller should continue to the next TRB in the TD.
    pub fn push_isoch_trb(
        &mut self,
        buf_phys: u64,
        len: u32,
        chain: bool,
        ioc: bool,
        frame_id: Option<u16>,
    ) -> bool {
        let mut trb = Trb {
            d0: lo(buf_phys),
            d1: hi(buf_phys),
            d2: len & 0x1FFFF, // 17-bit length field
            d3: trb_type(5),   // Isoch TRB
        };
        if let Some(fid) = frame_id {
            // Schedule for a future frame; SIA=0.
            trb.d3 |= ((fid as u32) & 0x7FF) << 20;
        } else {
            // Schedule Immediately (SIA) when no frame-id is provided.
            trb.d3 |= 1 << 31;
        }
        if chain {
            trb.d3 |= 1 << 4; // CH (Chain bit)
        }
        if ioc {
            trb.d3 |= 1 << 5; // IOC for final packet of a frame batch
        }
        self.ring.push(trb)
    }
}
