use super::super::control;
use super::super::xhci::{Trb, TrbRing, XhciContext, trb_type, xhc_list};
use crate::pci::dma;
use core::ptr::write_bytes;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HidReportType {
    Input = 1,
    Output = 2,
    Feature = 3,
}

#[inline]
fn setup_class_in(request: u8, value: u16, index: u16, length: u16) -> Trb {
    Trb {
        d0: (0xA1u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: (index as u32) | ((length as u32) << 16),
        d2: 8 | (2u32 << 16),
        d3: trb_type(2) | (1 << 6),
    }
}

#[inline]
fn setup_class_out_nodata(request: u8, value: u16, index: u16) -> Trb {
    Trb {
        d0: (0x21u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: index as u32,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    }
}

#[inline]
fn setup_class_out_data(request: u8, value: u16, index: u16, length: u16) -> Trb {
    Trb {
        d0: (0x21u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: (index as u32) | ((length as u32) << 16),
        d2: 8 | (3u32 << 16),
        d3: trb_type(2) | (1 << 6),
    }
}

pub async fn get_protocol(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
) -> Result<u8, ()> {
    let (phys, virt) = dma::alloc(1, 64).ok_or(())?;
    unsafe { write_bytes(virt, 0, 1) };

    let setup = setup_class_in(0x03, 0, iface as u16, 1);
    let res = control::control_in(ctx, ep0_ring, slot_id, setup, phys, 1, "hid-get-proto", 800)
        .await
        .map(|(_, xfer)| xfer);

    let out = match res {
        Ok(xfer) if xfer >= 1 => unsafe { core::slice::from_raw_parts(virt, 1)[0] },
        _ => {
            dma::dealloc(virt, 1);
            return Err(());
        }
    };

    dma::dealloc(virt, 1);
    Ok(out)
}

pub async fn set_protocol(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    protocol: u8,
) -> Result<(), ()> {
    let value = (protocol as u16) & 0xFF;
    let setup = setup_class_out_nodata(0x0B, value, iface as u16);
    control::control_out(ctx, ep0_ring, slot_id, setup, None, 0, "hid-set-proto", 800).await
}

pub async fn get_idle(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    report_id: u8,
) -> Result<u8, ()> {
    let (phys, virt) = dma::alloc(1, 64).ok_or(())?;
    unsafe { write_bytes(virt, 0, 1) };

    let value = (report_id as u16) << 8;
    let setup = setup_class_in(0x02, value, iface as u16, 1);
    let res = control::control_in(ctx, ep0_ring, slot_id, setup, phys, 1, "hid-get-idle", 800)
        .await
        .map(|(_, xfer)| xfer);

    let out = match res {
        Ok(xfer) if xfer >= 1 => unsafe { core::slice::from_raw_parts(virt, 1)[0] },
        _ => {
            dma::dealloc(virt, 1);
            return Err(());
        }
    };

    dma::dealloc(virt, 1);
    Ok(out)
}

pub async fn set_idle(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    report_id: u8,
    duration_4ms: u8,
) -> Result<(), ()> {
    let value = ((duration_4ms as u16) << 8) | (report_id as u16);
    let setup = setup_class_out_nodata(0x0A, value, iface as u16);
    control::control_out(ctx, ep0_ring, slot_id, setup, None, 0, "hid-set-idle", 800).await
}

pub async fn get_report_into(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    out: &mut [u8],
) -> Result<usize, ()> {
    if out.is_empty() {
        return Ok(0);
    }

    let want_len: usize = out.len();
    let (phys, virt) = dma::alloc(want_len, 64).ok_or(())?;
    unsafe { write_bytes(virt, 0, want_len) };

    let value = ((report_type as u16) << 8) | (report_id as u16);
    let setup = setup_class_in(0x01, value, iface as u16, want_len as u16);

    let transferred = match control::control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        phys,
        want_len as u16,
        "hid-get-report",
        1200,
    )
    .await
    {
        Ok((_, xfer)) => xfer as usize,
        Err(()) => {
            dma::dealloc(virt, want_len);
            return Err(());
        }
    };

    let n = core::cmp::min(transferred, want_len);
    unsafe {
        let src = core::slice::from_raw_parts(virt, want_len);
        out[..n].copy_from_slice(&src[..n]);
    }

    dma::dealloc(virt, want_len);
    Ok(n)
}

pub async fn set_report(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    data: &[u8],
) -> Result<(), ()> {
    let value = ((report_type as u16) << 8) | (report_id as u16);

    if data.is_empty() {
        let setup = setup_class_out_data(0x09, value, iface as u16, 0);
        return control::control_out(
            ctx,
            ep0_ring,
            slot_id,
            setup,
            None,
            0,
            "hid-set-report",
            1200,
        )
        .await;
    }

    let want_len = data.len();
    let (phys, virt) = dma::alloc(want_len, 64).ok_or(())?;
    unsafe {
        write_bytes(virt, 0, want_len);
        core::ptr::copy_nonoverlapping(data.as_ptr(), virt, want_len);
    }

    let setup = setup_class_out_data(0x09, value, iface as u16, want_len as u16);
    let res = control::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        Some(phys),
        want_len as u16,
        "hid-set-report",
        1200,
    )
    .await;

    dma::dealloc(virt, want_len);
    res
}

fn ctx_for_controller(controller_id: usize) -> Result<XhciContext, ()> {
    for info in xhc_list().iter().copied() {
        if info.controller_id == controller_id {
            return Ok(unsafe { XhciContext::new(info) });
        }
    }
    Err(())
}

pub async fn get_protocol_slot(controller_id: usize, slot_id: u32, iface: u8) -> Result<u8, ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = get_protocol(&ctx, &mut ep0_ring, slot_id, iface).await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub async fn set_protocol_slot(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    protocol: u8,
) -> Result<(), ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = set_protocol(&ctx, &mut ep0_ring, slot_id, iface, protocol).await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub async fn get_idle_slot(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_id: u8,
) -> Result<u8, ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = get_idle(&ctx, &mut ep0_ring, slot_id, iface, report_id).await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub async fn set_idle_slot(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_id: u8,
    duration_4ms: u8,
) -> Result<(), ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = set_idle(&ctx, &mut ep0_ring, slot_id, iface, report_id, duration_4ms).await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub async fn get_report_into_slot(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    out: &mut [u8],
) -> Result<usize, ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = get_report_into(
        &ctx,
        &mut ep0_ring,
        slot_id,
        iface,
        report_type,
        report_id,
        out,
    )
    .await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub async fn set_report_slot(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    data: &[u8],
) -> Result<(), ()> {
    let ctx = ctx_for_controller(controller_id)?;
    let st = super::ep0_state_for_slot(controller_id, slot_id).ok_or(())?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let res = set_report(
        &ctx,
        &mut ep0_ring,
        slot_id,
        iface,
        report_type,
        report_id,
        data,
    )
    .await;
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}
