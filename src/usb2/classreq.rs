use super::super::control;
use super::super::syscall::{control_in_sync, control_out_sync};
use super::super::xhci::{Trb, TrbRing, XhciContext, trb_type, xhc_list};
use core::ptr::write_bytes;
use dma_api::{DArray, DeviceDma, DmaDirection};
use heapless::Vec;

const SYNC_REPORT_MAX: usize = 256;
const USB_DMA_MASK: u64 = 0xFFFF_FFFF;

struct UsbDmaBuf {
    data: DArray<u8>,
}

impl UsbDmaBuf {
    fn alloc(size: usize, align: usize) -> Option<Self> {
        usb_dma()
            .array_zero_with_align::<u8>(size, align, DmaDirection::Bidirectional)
            .ok()
            .map(|data| Self { data })
    }

    fn phys(&self) -> u64 {
        self.data.dma_addr().as_u64()
    }

    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr().as_ptr()
    }

    unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        self.data.as_mut_slice()
    }
}

fn usb_dma() -> DeviceDma {
    DeviceDma::new(USB_DMA_MASK, &super::super::crabusb_service::CRABUSB_KERNEL)
}

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
    let mut buf = UsbDmaBuf::alloc(1, 64).ok_or(())?;
    let phys = buf.phys();

    let setup = setup_class_in(0x03, 0, iface as u16, 1);
    let res = control::control_in(ctx, ep0_ring, slot_id, setup, phys, 1, "hid-get-proto", 800)
        .await
        .map(|(_, xfer)| xfer);

    let out = match res {
        Ok(xfer) if xfer >= 1 => unsafe { buf.as_mut_slice()[0] },
        _ => return Err(()),
    };

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
    let mut buf = UsbDmaBuf::alloc(1, 64).ok_or(())?;
    let phys = buf.phys();

    let value = (report_id as u16) << 8;
    let setup = setup_class_in(0x02, value, iface as u16, 1);
    let res = control::control_in(ctx, ep0_ring, slot_id, setup, phys, 1, "hid-get-idle", 800)
        .await
        .map(|(_, xfer)| xfer);

    let out = match res {
        Ok(xfer) if xfer >= 1 => unsafe { buf.as_mut_slice()[0] },
        _ => return Err(()),
    };

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
    let mut buf = UsbDmaBuf::alloc(want_len, 64).ok_or(())?;
    let phys = buf.phys();

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
        Err(()) => return Err(()),
    };

    let n = core::cmp::min(transferred, want_len);
    unsafe {
        let src = buf.as_mut_slice();
        out[..n].copy_from_slice(&src[..n]);
    }

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
    let mut buf = UsbDmaBuf::alloc(want_len, 64).ok_or(())?;
    let phys = buf.phys();
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), buf.as_ptr(), want_len);
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
    let mut buf = UsbDmaBuf::alloc(1, 64)?;
    let phys = buf.phys();

    let setup = setup_class_in(0x03, 0, iface as u16, 1);
    let out = match control_in_sync(&ctx, &mut ep0_ring, slot_id, setup, phys, 1, timeout_ms) {
        Ok((_, transferred)) if transferred >= 1 => unsafe { buf.as_mut_slice()[0] },
        _ => {
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

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
    let mut buf = UsbDmaBuf::alloc(1, 64)?;
    let phys = buf.phys();

    let value = (report_id as u16) << 8;
    let setup = setup_class_in(0x02, value, iface as u16, 1);
    let out = match control_in_sync(&ctx, &mut ep0_ring, slot_id, setup, phys, 1, timeout_ms) {
        Ok((_, transferred)) if transferred >= 1 => unsafe { buf.as_mut_slice()[0] },
        _ => {
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

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
    let mut buf = UsbDmaBuf::alloc(want_len, 64)?;
    let phys = buf.phys();

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
            super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
            return None;
        }
    };

    let mut out = Vec::new();
    unsafe {
        let src = buf.as_mut_slice();
        let _ = out.extend_from_slice(&src[..core::cmp::min(transferred, want_len)]);
    }

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
        let mut buf = UsbDmaBuf::alloc(want_len, 64)?;
        let phys = buf.phys();
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), buf.as_ptr(), want_len);
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
        out
    };

    super::set_ep0_state_for_slot(controller_id, slot_id, ep0_ring.snapshot());
    res
}
