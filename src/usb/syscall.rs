use super::control::{setup_get_descriptor, setup_get_descriptor_interface};
use super::xhci::{self, Trb, TrbRing, XhciContext, hi, lo, trb_type};
use crate::pci::dma;
use core::ptr::{write_bytes, write_volatile};
use heapless::Vec;

const DESC_MAX: usize = 256;

/// Trigger a port reset on `port_idx` of the given controller.
/// Returns 0 on success, -1 if the controller was not found.
pub fn port_reset(controller_id: usize, port_idx: usize) -> i32 {
    let controllers = xhci::xhc_list();
    let info = match controllers
        .iter()
        .find(|c| c.controller_id == controller_id)
    {
        Some(c) => *c,
        None => return -1,
    };
    unsafe { XhciContext::new(info).reset_port(port_idx) };
    0
}

/// Synchronous GET_DESCRIPTOR control transfer on EP0 for an unclaimed device.
/// Returns up to 256 bytes of descriptor data, or `None` on failure.
pub fn control_get_descriptor(
    controller_id: usize,
    slot_id: u32,
    desc_type: u8,
    desc_index: u8,
    length: u16,
    timeout_ms: u64,
) -> Option<Vec<u8, DESC_MAX>> {
    let ring_state = super::ep0_ring_state_for_slot(controller_id, slot_id)?;

    let controllers = xhci::xhc_list();
    let info = controllers
        .iter()
        .find(|c| c.controller_id == controller_id)
        .copied()?;
    let ctx = unsafe { XhciContext::new(info) };

    let clamped = (length as usize).min(DESC_MAX);
    let (buf_phys, buf_virt) = dma::alloc(clamped, 64)?;
    unsafe { write_bytes(buf_virt, 0, clamped) };

    let mut state = ring_state;
    state.pending = 0; // treat any pre-syscall enqueued TRBs as already retired
    let mut ep0_ring = unsafe { TrbRing::from_state(state) };

    let setup = setup_get_descriptor(desc_type, desc_index, clamped as u16);
    let result = control_in_sync(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup,
        buf_phys,
        clamped as u16,
        timeout_ms,
    );

    super::update_ep0_ring_state(controller_id, slot_id, ep0_ring.snapshot());

    let (_, transferred) = match result {
        Ok(r) => r,
        Err(()) => {
            dma::dealloc(buf_virt, clamped);
            return None;
        }
    };

    let data = unsafe { core::slice::from_raw_parts(buf_virt, transferred as usize) };
    let mut out: Vec<u8, DESC_MAX> = Vec::new();
    for &b in data {
        let _ = out.push(b);
    }
    dma::dealloc(buf_virt, clamped);
    Some(out)
}

/// Synchronous GET_DESCRIPTOR control transfer on EP0 with an interface-scoped
/// request type (bmRequestType=0x81) and caller-provided wIndex.
/// Returns up to 256 bytes of descriptor data, or `None` on failure.
pub fn control_get_descriptor_interface(
    controller_id: usize,
    slot_id: u32,
    desc_type: u8,
    desc_index: u8,
    interface_number: u16,
    length: u16,
    timeout_ms: u64,
) -> Option<Vec<u8, DESC_MAX>> {
    let ring_state = super::ep0_ring_state_for_slot(controller_id, slot_id)?;

    let controllers = xhci::xhc_list();
    let info = controllers
        .iter()
        .find(|c| c.controller_id == controller_id)
        .copied()?;
    let ctx = unsafe { XhciContext::new(info) };

    let clamped = (length as usize).min(DESC_MAX);
    let (buf_phys, buf_virt) = dma::alloc(clamped, 64)?;
    unsafe { write_bytes(buf_virt, 0, clamped) };

    let mut state = ring_state;
    state.pending = 0;
    let mut ep0_ring = unsafe { TrbRing::from_state(state) };

    let setup =
        setup_get_descriptor_interface(desc_type, desc_index, interface_number, clamped as u16);
    let result = control_in_sync(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup,
        buf_phys,
        clamped as u16,
        timeout_ms,
    );

    super::update_ep0_ring_state(controller_id, slot_id, ep0_ring.snapshot());

    let (_, transferred) = match result {
        Ok(r) => r,
        Err(()) => {
            dma::dealloc(buf_virt, clamped);
            return None;
        }
    };

    let data = unsafe { core::slice::from_raw_parts(buf_virt, transferred as usize) };
    let mut out: Vec<u8, DESC_MAX> = Vec::new();
    for &b in data {
        let _ = out.push(b);
    }
    dma::dealloc(buf_virt, clamped);
    Some(out)
}

/// Convenience helper for HID descriptor reads (`desc_type=0x21`).
pub fn control_get_hid_descriptor(
    controller_id: usize,
    slot_id: u32,
    interface_number: u16,
    length: u16,
    timeout_ms: u64,
) -> Option<Vec<u8, DESC_MAX>> {
    control_get_descriptor_interface(
        controller_id,
        slot_id,
        0x21,
        0,
        interface_number,
        length,
        timeout_ms,
    )
}

/// Convenience helper for HID report descriptor reads (`desc_type=0x22`).
pub fn control_get_hid_report_descriptor(
    controller_id: usize,
    slot_id: u32,
    interface_number: u16,
    length: u16,
    timeout_ms: u64,
) -> Option<Vec<u8, DESC_MAX>> {
    control_get_descriptor_interface(
        controller_id,
        slot_id,
        0x22,
        0,
        interface_number,
        length,
        timeout_ms,
    )
}

/// Scan the software-side transfer-event buffer for an event matching
/// `slot_id` + `ep_target`. Returns `(completion_code, residual_bytes)` or `None`.
pub fn read_transfer_event(
    controller_id: usize,
    slot_id: u32,
    ep_target: u32,
) -> Option<(u32, u32)> {
    let evt = xhci::try_take_buffered_event(controller_id, &mut |evt: &Trb| {
        let evt_type = (evt.d3 >> 10) & 0x3F;
        let evt_slot = (evt.d3 >> 24) & 0xFF;
        let evt_ep = (evt.d3 >> 16) & 0x1F;
        evt_type == 32 && evt_slot == slot_id && evt_ep == ep_target
    })?;
    let cc = (evt.d2 >> 24) & 0xFF;
    let residual = evt.d2 & 0x00FF_FFFF;
    Some((cc, residual))
}

pub(crate) fn control_in_sync(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: u64,
    length: u16,
    timeout_ms: u64,
) -> Result<(u32, u16), ()> {
    let data = Trb {
        d0: lo(buf_phys),
        d1: hi(buf_phys),
        d2: length as u32,
        d3: trb_type(3) | (1 << 16), // Data Stage, DIR=IN
    };
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5), // Status Stage, IOC
    };

    let setup_phys = ep0_ring.push_with_phys(setup).ok_or(())?;
    let data_phys = ep0_ring.push_with_phys(data).ok_or(())?;
    let status_phys = ep0_ring.push_with_phys(status).ok_or(())?;

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let evt = xhci::wait_for_event_spin_ms(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            let evt_ep = (evt.d3 >> 16) & 0x1F;
            if evt_type != 32 || evt_slot != slot_id || evt_ep != 1 {
                return false;
            }
            let evt_ptr = ((evt.d0 as u64) | ((evt.d1 as u64) << 32)) & !0xF;
            evt_ptr == (setup_phys & !0xF)
                || evt_ptr == (data_phys & !0xF)
                || evt_ptr == (status_phys & !0xF)
        },
        timeout_ms,
    )
    .ok_or(())?;

    ep0_ring.release_completed(3);

    let cc = (evt.d2 >> 24) & 0xFF;
    let remaining = evt.d2 & 0x00FF_FFFF;
    let transferred = (length as u32).saturating_sub(remaining).min(length as u32) as u16;

    if cc == 1 || cc == 13 {
        Ok((cc, transferred))
    } else {
        Err(())
    }
}

pub(crate) fn control_out_sync(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: Option<u64>,
    length: u16,
    timeout_ms: u64,
) -> Result<u32, ()> {
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16), // Status Stage, IOC, DIR=IN
    };

    let setup_phys = ep0_ring.push_with_phys(setup).ok_or(())?;
    let data_phys = if let Some(buf_phys) = buf_phys {
        let data = Trb {
            d0: lo(buf_phys),
            d1: hi(buf_phys),
            d2: length as u32,
            d3: trb_type(3), // Data Stage, DIR=OUT
        };
        Some(ep0_ring.push_with_phys(data).ok_or(())?)
    } else {
        None
    };
    let status_phys = ep0_ring.push_with_phys(status).ok_or(())?;

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let evt = xhci::wait_for_event_spin_ms(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            let evt_ep = (evt.d3 >> 16) & 0x1F;
            if evt_type != 32 || evt_slot != slot_id || evt_ep != 1 {
                return false;
            }
            let evt_ptr = ((evt.d0 as u64) | ((evt.d1 as u64) << 32)) & !0xF;
            evt_ptr == (setup_phys & !0xF)
                || data_phys
                    .map(|phys| evt_ptr == (phys & !0xF))
                    .unwrap_or(false)
                || evt_ptr == (status_phys & !0xF)
        },
        timeout_ms,
    )
    .ok_or(())?;

    ep0_ring.release_completed(2 + data_phys.is_some() as usize);

    let cc = (evt.d2 >> 24) & 0xFF;
    if cc == 1 || cc == 13 { Ok(cc) } else { Err(()) }
}
