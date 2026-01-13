//! Isochronous transport helper built on xHCI primitives.
//! This keeps TRB/endpoint-context details inside xHCI while exposing a small API
//! for class drivers (e.g., UAC) to queue periodic packets.

use crate::usb::xhci::{
    self,
    context_index,
    endpoint_target,
    ep_avg_trb_len_bits,
    ep_cerr_bits,
    ep_interval_bits,
    ep_max_burst_bits,
    ep_max_esit_payload_hi_bits,
    ep_max_esit_payload_lo_bits,
    ep_max_packet_bits,
    ep_mult_bits,
    ep_type_bits,
    ep_state_bits,
    hi,
    lo,
    trb_type,
    Trb,
    TrbRing,
    XhciContext,
    EP_STATE_DISABLED,
    EP_TYPE_ISOCH_OUT,
};
use crate::pci::dma;
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
    pub slot_id: u32,
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
        // Keeping this reasonably large helps avoid lapping the controller in steady-state.
        const ISOCH_TRBS: usize = 1024;
        let (ep_ring_phys, ep_ring_virt) =
            dma::alloc(ISOCH_TRBS * size_of::<Trb>(), 64).ok_or(())?;
        unsafe { write_bytes(ep_ring_virt, 0, ISOCH_TRBS * size_of::<Trb>()) };
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
            let mult_field = if is_super_speed { (ss_mult & 0x3) as u32 } else { 0 };
            let burst_field = if is_super_speed {
                ss_max_burst as u32
            } else if is_high_speed {
                hs_additional
            } else {
                0
            };

            // Interval encoding differs by speed; HS encodes exponent-1.
            let interval_field = if is_high_speed {
                core::cmp::min(15u32, interval.saturating_sub(1) as u32)
            } else {
                interval as u32
            };

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
        if !cmd_ring.push(cfg_ep_cmd) {
            return Err(());
        }
        unsafe { core::ptr::write_volatile(ctx.doorbell.add(0), 0) };

        let cfg_evt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 33 {
                    return false;
                }
                let cc = (evt.d2 >> 24) & 0xFF;
                cc == 1
            },
            400,
            EmbassyDuration::from_millis(5),
        )
        .await
        .ok_or(())?;

        let completion = (cfg_evt.d2 >> 24) & 0xFF;
        if completion != 1 {
            return Err(());
        }

        Ok(IsochOutPipe {
            ep_target,
            slot_id,
            ring: ep_ring,
            max_packet: packet_bytes as u16,
        })
    }

    /// Queue a single isochronous OUT packet (caller provides DMA buffer phys address).
    pub fn push_isoch_trb(&mut self, buf_phys: u64, len: u32, last: bool) -> bool {
        let mut trb = Trb {
            d0: lo(buf_phys),
            d1: hi(buf_phys),
            d2: len & 0x1FFFF, // 17-bit length field
            d3: trb_type(5),   // Isoch TRB
        };
        // Schedule Immediately (SIA) helps avoid frame-id related gaps.
        trb.d3 |= 1 << 31;
        if last {
            trb.d3 |= 1 << 5; // IOC for final packet of a frame batch
        }
        self.ring.push(trb)
    }
}
