use super::super::control;
use super::super::syscall::{control_in_sync, control_out_sync};
use super::super::xhci::{Trb, TrbRing, XhciContext, trb_type, xhc_list};
use crate::dma;
use core::ptr::write_bytes;
use heapless::Vec;

const SYNC_REPORT_MAX: usize = 256;

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

pub fn get_protocol_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    timeout_ms: u64,
) -> Option<u8> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let (phys, virt) = dma::alloc(1, 64)?;
    unsafe { write_bytes(virt, 0, 1) };

    let setup = setup_class_in(0x03, 0, iface as u16, 1);
    let out = match control_in_sync(&ctx, &mut ep0_ring, slot_id, setup, phys, 1, timeout_ms) {
        Ok((_, transferred)) if transferred >= 1 => unsafe {
            core::slice::from_raw_parts(virt, 1)[0]
        },
        _ => {
            dma::dealloc(virt, 1);
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

    dma::dealloc(virt, 1);
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    Some(out)
}

pub fn set_protocol_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    protocol: u8,
    timeout_ms: u64,
) -> Option<u32> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let value = (protocol as u16) & 0xFF;
    let setup = setup_class_out_nodata(0x0B, value, iface as u16);
    let res = control_out_sync(&ctx, &mut ep0_ring, slot_id, setup, None, 0, timeout_ms).ok();
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub fn get_idle_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_id: u8,
    timeout_ms: u64,
) -> Option<u8> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let (phys, virt) = dma::alloc(1, 64)?;
    unsafe { write_bytes(virt, 0, 1) };

    let value = (report_id as u16) << 8;
    let setup = setup_class_in(0x02, value, iface as u16, 1);
    let out = match control_in_sync(&ctx, &mut ep0_ring, slot_id, setup, phys, 1, timeout_ms) {
        Ok((_, transferred)) if transferred >= 1 => unsafe {
            core::slice::from_raw_parts(virt, 1)[0]
        },
        _ => {
            dma::dealloc(virt, 1);
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

    dma::dealloc(virt, 1);
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    Some(out)
}

pub fn set_idle_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_id: u8,
    duration_4ms: u8,
    timeout_ms: u64,
) -> Option<u32> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let value = ((duration_4ms as u16) << 8) | (report_id as u16);
    let setup = setup_class_out_nodata(0x0A, value, iface as u16);
    let res = control_out_sync(&ctx, &mut ep0_ring, slot_id, setup, None, 0, timeout_ms).ok();
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}

pub fn get_report_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    length: usize,
    timeout_ms: u64,
) -> Option<Vec<u8, SYNC_REPORT_MAX>> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let want_len = length.clamp(1, SYNC_REPORT_MAX);
    let (phys, virt) = dma::alloc(want_len, 64)?;
    unsafe { write_bytes(virt, 0, want_len) };

    let value = ((report_type as u16) << 8) | (report_id as u16);
    let setup = setup_class_in(0x01, value, iface as u16, want_len as u16);
    let transferred = match control_in_sync(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup,
        phys,
        want_len as u16,
        timeout_ms,
    ) {
        Ok((_, transferred)) => transferred as usize,
        Err(()) => {
            dma::dealloc(virt, want_len);
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

    let mut out = Vec::new();
    unsafe {
        let src = core::slice::from_raw_parts(virt, want_len);
        let _ = out.extend_from_slice(&src[..core::cmp::min(transferred, want_len)]);
    }

    dma::dealloc(virt, want_len);
    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    Some(out)
}

pub fn set_report_slot_sync(
    controller_id: usize,
    slot_id: u32,
    iface: u8,
    report_type: HidReportType,
    report_id: u8,
    data: &[u8],
    timeout_ms: u64,
) -> Option<u32> {
    let ctx = ctx_for_controller(controller_id).ok()?;
    let st = super::ep0_state_for_slot(controller_id, slot_id)?;
    let mut ep0_ring = unsafe { TrbRing::from_state(st) };
    let value = ((report_type as u16) << 8) | (report_id as u16);

    let res = if data.is_empty() {
        let setup = setup_class_out_data(0x09, value, iface as u16, 0);
        control_out_sync(&ctx, &mut ep0_ring, slot_id, setup, None, 0, timeout_ms).ok()
    } else {
        let want_len = data.len().min(SYNC_REPORT_MAX);
        let (phys, virt) = dma::alloc(want_len, 64)?;
        unsafe {
            write_bytes(virt, 0, want_len);
            core::ptr::copy_nonoverlapping(data.as_ptr(), virt, want_len);
        }
        let setup = setup_class_out_data(0x09, value, iface as u16, want_len as u16);
        let out = control_out_sync(
            &ctx,
            &mut ep0_ring,
            slot_id,
            setup,
            Some(phys),
            want_len as u16,
            timeout_ms,
        )
        .ok();
        dma::dealloc(virt, want_len);
        out
    };

    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}
