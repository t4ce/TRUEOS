use super::xhci;
use super::xhci::{Trb, TrbRing, XhciContext, hi, lo, trb_type};
use crate::pci::dma;
use core::ptr::{write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;
use heapless::String;
const USB_EVENT_POLL_DELAY_MS: u64 = 1;

macro_rules! usbv {
    ($($tt:tt)*) => {{
        if super::USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}

pub(crate) fn trb_cc(evt: &Trb) -> u32 {
    (evt.d2 >> 24) & 0xFF
}

pub(crate) fn setup_get_descriptor(desc_type: u8, desc_index: u8, length: u16) -> Trb {
    // bmRequestType=0x80 (IN|Standard|Device), bRequest=0x06 (GET_DESCRIPTOR)
    let w_value = ((desc_type as u16) << 8) | (desc_index as u16);
    Trb {
        d0: (0x80u32) | (0x06u32 << 8) | ((w_value as u32) << 16),
        d1: (length as u32) << 16,  // wIndex=0
        d2: 8 | (2 << 16),          // 8-byte setup, TRT=IN
        d3: trb_type(2) | (1 << 6), // Setup Stage, IDT
    }
}

pub(crate) fn setup_clear_endpoint_halt(ep_addr: u8) -> Trb {
    // bmRequestType=0x02 (OUT|Standard|Endpoint), bRequest=0x01 (CLEAR_FEATURE)
    // wValue=0 (ENDPOINT_HALT), wIndex=endpoint address
    Trb {
        d0: (0x02u32) | (0x01u32 << 8),
        d1: (ep_addr as u32),
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    }
}

fn setup_get_string_descriptor(desc_index: u8, langid: u16, length: u16) -> Trb {
    // bmRequestType=0x80 (IN|Standard|Device), bRequest=0x06 (GET_DESCRIPTOR)
    // wValue = (STRING << 8) | index, wIndex = langid
    let w_value = ((3u16) << 8) | (desc_index as u16);
    Trb {
        d0: (0x80u32) | (0x06u32 << 8) | ((w_value as u32) << 16),
        d1: (langid as u32) | ((length as u32) << 16),
        d2: 8 | (2 << 16),          // 8-byte setup, TRT=IN
        d3: trb_type(2) | (1 << 6), // Setup Stage, IDT
    }
}

pub(crate) async fn control_in(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: u64,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<(u32, u16), ()> {
    // xHCI Completion Code 6 = Stall Error. Some devices/hubs require an explicit
    // CLEAR_FEATURE(ENDPOINT_HALT) on EP0 to recover.
    const CC_STALL: u32 = 6;
    let mut attempt: u8 = 0;
    loop {
        let data = Trb {
            d0: lo(buf_phys),
            d1: hi(buf_phys),
            d2: length as u32,
            d3: trb_type(3) | (1 << 16), // Data Stage, DIR=IN, no IOC
        };

        let status = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(4) | (1 << 5), // Status Stage, IOC, DIR=OUT
        };

        let Some(setup_trb_phys) = ep0_ring.push_with_phys(setup) else {
            usbv!("usb: {}: ep0 ring full\n", what);
            return Err(());
        };
        let Some(data_trb_phys) = ep0_ring.push_with_phys(data) else {
            usbv!("usb: {}: ep0 ring full\n", what);
            return Err(());
        };

        let Some(status_trb_phys) = ep0_ring.push_with_phys(status) else {
            usbv!("usb: {}: ep0 ring full\n", what);
            return Err(());
        };
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

        let evt = xhci::wait_for_event(
            ctx,
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) & 0xFF;
                if evt_slot != slot_id {
                    return false;
                }

                let evt_target = (evt.d3 >> 16) & 0x1F;
                if evt_target != 1 {
                    return false;
                }

                // On success, the controller usually reports the status-stage TRB.
                // On error, some controllers report the TRB that faulted (setup/data).
                let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
                let evt_ptr = evt_ptr & !0xFu64;
                evt_ptr == (setup_trb_phys & !0xFu64)
                    || evt_ptr == (data_trb_phys & !0xFu64)
                    || evt_ptr == (status_trb_phys & !0xFu64)
            },
            timeout_iters,
            EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
        )
        .await
        .ok_or(())
        .map_err(|_| {
            usbv!("usb: {}: timeout waiting for transfer event\n", what);
        })?;

        let completion = trb_cc(&evt);
        let remaining = (evt.d2 & 0x00FF_FFFF) as u32;
        let requested = length as u32;
        let transferred = requested.saturating_sub(remaining).min(requested) as u16;

        // CC=13 is a normal short packet completion on control-IN reads.
        if completion == 1 || completion == 13 {
            return Ok((completion, transferred));
        }

        if completion == CC_STALL
            && attempt == 0
            && what != "ep0-clear-halt"
            && control_out_cc(
                ctx,
                ep0_ring,
                slot_id,
                setup_clear_endpoint_halt(0),
                None,
                0,
                "ep0-clear-halt",
                200,
            )
            .await
            .ok()
                == Some(1)
        {
            attempt = 1;
            continue;
        }

        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        return Err(());
    }
}

pub(crate) async fn control_out(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: Option<u64>,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<(), ()> {
    const CC_STALL: u32 = 6;
    let mut attempt: u8 = 0;
    loop {
        let Some(setup_trb_phys) = ep0_ring.push_with_phys(setup) else {
            usbv!("usb: {}: ep0 ring full (setup)\n", what);
            return Err(());
        };

        let mut data_trb_phys: Option<u64> = None;

        if let Some(phys) = buf_phys {
            let data = Trb {
                d0: lo(phys),
                d1: hi(phys),
                d2: length as u32,
                d3: trb_type(3),
            };
            let Some(data_phys) = ep0_ring.push_with_phys(data) else {
                usbv!("usb: {}: ep0 ring full (data)\n", what);
                return Err(());
            };
            data_trb_phys = Some(data_phys);
        }

        let status = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(4) | (1 << 5) | (1 << 16),
        };

        let Some(status_trb_phys) = ep0_ring.push_with_phys(status) else {
            usbv!("usb: {}: ep0 ring full (status)\n", what);
            return Err(());
        };

        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

        let evt = xhci::wait_for_event(
            ctx,
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) & 0xFF;
                if evt_slot != slot_id {
                    return false;
                }

                let evt_target = (evt.d3 >> 16) & 0x1F;
                if evt_target != 1 {
                    return false;
                }

                let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
                let evt_ptr = evt_ptr & !0xFu64;
                if evt_ptr == (setup_trb_phys & !0xFu64) {
                    return true;
                }
                if let Some(data_phys) = data_trb_phys
                    && evt_ptr == (data_phys & !0xFu64) {
                        return true;
                    }
                evt_ptr == (status_trb_phys & !0xFu64)
            },
            timeout_iters,
            EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
        )
        .await
        .ok_or(())
        .map_err(|_| {
            usbv!("usb: {}: timeout waiting for transfer event\n", what);
        })?;

        let completion = trb_cc(&evt);
        if completion == 1 {
            return Ok(());
        }

        if completion == CC_STALL
            && attempt == 0
            && what != "ep0-clear-halt"
            && control_out_cc(
                ctx,
                ep0_ring,
                slot_id,
                setup_clear_endpoint_halt(0),
                None,
                0,
                "ep0-clear-halt",
                200,
            )
            .await
            .ok()
                == Some(1)
        {
            attempt = 1;
            continue;
        }

        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        return Err(());
    }
}

pub(crate) async fn control_out_cc(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: Option<u64>,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<u32, ()> {
    let Some(setup_trb_phys) = ep0_ring.push_with_phys(setup) else {
        usbv!("usb: {}: ep0 ring full (setup)\n", what);
        return Err(());
    };

    let mut data_trb_phys: Option<u64> = None;

    if let Some(phys) = buf_phys {
        let data = Trb {
            d0: lo(phys),
            d1: hi(phys),
            d2: length as u32,
            d3: trb_type(3),
        };
        let Some(data_phys) = ep0_ring.push_with_phys(data) else {
            usbv!("usb: {}: ep0 ring full (data)\n", what);
            return Err(());
        };
        data_trb_phys = Some(data_phys);
    }

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };

    let Some(status_trb_phys) = ep0_ring.push_with_phys(status) else {
        usbv!("usb: {}: ep0 ring full (status)\n", what);
        return Err(());
    };

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let evt = xhci::wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            if evt_slot != slot_id {
                return false;
            }

            let evt_target = (evt.d3 >> 16) & 0x1F;
            if evt_target != 1 {
                return false;
            }

            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            let evt_ptr = evt_ptr & !0xFu64;
            if evt_ptr == (setup_trb_phys & !0xFu64) {
                return true;
            }
            if let Some(data_phys) = data_trb_phys
                && evt_ptr == (data_phys & !0xFu64) {
                    return true;
                }
            evt_ptr == (status_trb_phys & !0xFu64)
        },
        timeout_iters,
        EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
    )
    .await
    .ok_or(())
    .map_err(|_| {
        usbv!("usb: {}: timeout waiting for transfer event\n", what);
    })?;

    Ok(trb_cc(&evt))
}

async fn fetch_first_langid(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
) -> Option<u16> {
    // String descriptor 0 returns supported LANGIDs.
    let (buf_phys, buf_virt) = dma::alloc(256, 64)?;
    unsafe { write_bytes(buf_virt, 0, 256) };
    let res = control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_string_descriptor(0, 0, 255),
        buf_phys,
        255,
        "get-str-langid",
        400,
    )
    .await;

    let lang = match res {
        Ok((_, transferred)) => unsafe {
            let n = (transferred as usize).min(256);
            let desc = core::slice::from_raw_parts(buf_virt, n);
            if desc.len() < 4 || desc[1] != 3 {
                None
            } else {
                Some(u16::from_le_bytes([desc[2], desc[3]]))
            }
        },
        Err(()) => None,
    };

    dma::dealloc(buf_virt, 256);
    lang
}

pub(crate) async fn fetch_string_ascii<const N: usize>(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    string_index: u8,
) -> String<N> {
    let mut out: String<N> = String::new();
    if string_index == 0 {
        return out;
    }

    let langid = fetch_first_langid(ctx, ep0_ring, slot_id)
        .await
        .unwrap_or(0x0409);

    let (buf_phys, buf_virt) = match dma::alloc(256, 64) {
        Some(p) => p,
        None => return out,
    };
    unsafe { write_bytes(buf_virt, 0, 256) };

    let res = control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_string_descriptor(string_index, langid, 255),
        buf_phys,
        255,
        "get-str",
        800,
    )
    .await;

    if let Ok((_, transferred)) = res {
        unsafe {
            let n = (transferred as usize).min(256);
            let desc = core::slice::from_raw_parts(buf_virt, n);
            if desc.len() >= 2 && desc[1] == 3 {
                let total = (desc[0] as usize).min(desc.len());
                let mut idx = 2usize;
                while idx + 1 < total {
                    let ch = u16::from_le_bytes([desc[idx], desc[idx + 1]]);
                    let ch = if ch <= 0x7F { ch as u8 as char } else { '?' };
                    if out.push(ch).is_err() {
                        break;
                    }
                    idx += 2;
                }
            }
        }
    }

    dma::dealloc(buf_virt, 256);
    out
}
