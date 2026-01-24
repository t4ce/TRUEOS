use super::cdc_acm;
use super::xhci;
use super::xhci::{hi, lo, trb_type, Trb, TrbRing, XhciContext};
use crate::pci::dma;
use core::ptr::{write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;

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

pub(crate) fn setup_set_address(address: u8) -> Trb {
    // bmRequestType=0x00 (OUT|Standard|Device), bRequest=0x05 (SET_ADDRESS)
    Trb {
        d0: (0x00u32) | (0x05u32 << 8) | ((address as u32) << 16),
        d1: 0,
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

    if !ep0_ring.push(setup) || !ep0_ring.push(data) {
        usbv!("usb: {}: ep0 ring full\n", what);
        return Err(());
    }

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

            // Match the specific status-stage TRB we rang the doorbell for.
            // Transfer Event TRB Pointer is 16-byte aligned; ignore low 4 bits.
            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (status_trb_phys & !0xFu64)
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
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
        Ok((completion, transferred))
    } else {
        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        Err(())
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
    if !ep0_ring.push(setup) {
        usbv!("usb: {}: ep0 ring full (setup)\n", what);
        return Err(());
    }

    if let Some(phys) = buf_phys {
        let data = Trb {
            d0: lo(phys),
            d1: hi(phys),
            d2: length as u32,
            d3: trb_type(3),
        };
        if !ep0_ring.push(data) {
            usbv!("usb: {}: ep0 ring full (data)\n", what);
            return Err(());
        }
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

            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (status_trb_phys & !0xFu64)
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
    )
    .await
    .ok_or(())
    .map_err(|_| {
        usbv!("usb: {}: timeout waiting for transfer event\n", what);
    })?;

    let completion = trb_cc(&evt);
    if completion == 1 {
        Ok(())
    } else {
        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        Err(())
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
    if !ep0_ring.push(setup) {
        usbv!("usb: {}: ep0 ring full (setup)\n", what);
        return Err(());
    }

    if let Some(phys) = buf_phys {
        let data = Trb {
            d0: lo(phys),
            d1: hi(phys),
            d2: length as u32,
            d3: trb_type(3),
        };
        if !ep0_ring.push(data) {
            usbv!("usb: {}: ep0 ring full (data)\n", what);
            return Err(());
        }
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

            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (status_trb_phys & !0xFu64)
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
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
        Ok((_cc, transferred)) => unsafe {
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

pub(crate) async fn fetch_serial_string(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    serial_index: u8,
) -> cdc_acm::UsbSerial {
    if serial_index == 0 {
        return cdc_acm::UsbSerial::none();
    }

    let langid = fetch_first_langid(ctx, ep0_ring, slot_id)
        .await
        .unwrap_or(0x0409);

    let (buf_phys, buf_virt) = match dma::alloc(256, 64) {
        Some(p) => p,
        None => return cdc_acm::UsbSerial::none(),
    };
    unsafe { write_bytes(buf_virt, 0, 256) };

    let res = control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_string_descriptor(serial_index, langid, 255),
        buf_phys,
        255,
        "get-serial",
        800,
    )
    .await;

    let mut out: heapless::Vec<u8, 64> = heapless::Vec::new();
    if let Ok((_cc, transferred)) = res {
        unsafe {
            let n = (transferred as usize).min(256);
            let desc = core::slice::from_raw_parts(buf_virt, n);
            if desc.len() >= 2 && desc[1] == 3 {
                let total = (desc[0] as usize).min(desc.len());
                let mut idx = 2usize;
                while idx + 1 < total {
                    let ch = u16::from_le_bytes([desc[idx], desc[idx + 1]]);
                    let b = if ch <= 0x7F { ch as u8 } else { b'?' };
                    let _ = out.push(b);
                    idx += 2;
                }
            }
        }
    }

    dma::dealloc(buf_virt, 256);
    cdc_acm::UsbSerial::from_bytes(out.as_slice())
}