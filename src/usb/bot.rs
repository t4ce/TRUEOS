use super::scsi;
use super::xhci::{self, hi, lo, trb_type, Trb, TrbRing, XhciContext};
use crate::pci::dma;
use core::ptr::{write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;

const CBW_SIGNATURE: u32 = 0x4342_5355; // 'USBC'
const CSW_SIGNATURE: u32 = 0x5342_5355; // 'USBS'

const CBW_LEN: usize = 31;
const CSW_LEN: usize = 13;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BotStatus {
    Passed,
    Failed,
    PhaseError,
    Unknown(u8),
}

impl BotStatus {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => BotStatus::Passed,
            1 => BotStatus::Failed,
            2 => BotStatus::PhaseError,
            other => BotStatus::Unknown(other),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Csw {
    pub residue: u32,
    pub status: BotStatus,
}

fn build_cbw(tag: u32, data_len: u32, dir_in: bool, lun: u8, cdb: &[u8]) -> [u8; CBW_LEN] {
    // USB Mass Storage Bulk-Only Transport CBW (31 bytes)
    // Offsets:
    // 0..4  dCBWSignature
    // 4..8  dCBWTag
    // 8..12 dCBWDataTransferLength
    // 12    bmCBWFlags (bit7: IN)
    // 13    bCBWLUN
    // 14    bCBWCBLength
    // 15..31 CBWCB (16 bytes)

    let mut out = [0u8; CBW_LEN];
    out[0..4].copy_from_slice(&CBW_SIGNATURE.to_le_bytes());
    out[4..8].copy_from_slice(&tag.to_le_bytes());
    out[8..12].copy_from_slice(&data_len.to_le_bytes());
    out[12] = if dir_in { 0x80 } else { 0x00 };
    out[13] = lun & 0x0F;

    let cb_len = cdb.len().min(16);
    out[14] = cb_len as u8;
    out[15..15 + cb_len].copy_from_slice(&cdb[..cb_len]);

    out
}

fn parse_csw(buf: &[u8]) -> Option<(u32, Csw)> {
    if buf.len() < CSW_LEN {
        return None;
    }
    let sig = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if sig != CSW_SIGNATURE {
        return None;
    }
    let tag = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let residue = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let status = BotStatus::from_u8(buf[12]);
    Some((tag, Csw { residue, status }))
}

async fn bulk_xfer(
    ctx: &XhciContext,
    ring: &mut TrbRing,
    slot_id: u32,
    ep_target: u32,
    buf_phys: u64,
    length: u32,
    what: &'static str,
    timeout_iters: usize,
) -> Result<(u32, u32), ()> {
    let trb = Trb {
        d0: lo(buf_phys),
        d1: hi(buf_phys),
        d2: length,
        d3: trb_type(1) | (1 << 5), // Normal TRB, IOC
    };

    let Some(trb_phys) = ring.push_with_phys(trb) else {
        crate::log!("usb: {}: bulk ring full\n", what);
        return Err(());
    };

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target) };

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
            let evt_ep_target = (evt.d3 >> 16) & 0x1F;
            if evt_ep_target != ep_target {
                return false;
            }
            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (trb_phys & !0xFu64)
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
    )
    .await
    .ok_or(())
    .map_err(|_| {
        xhci::debug_event_buffer_summary(ctx.controller_id);
        xhci::debug_peek_transfer_events(ctx.controller_id, slot_id, ep_target, 4);
        xhci::debug_peek_transfer_events_for_slot(ctx.controller_id, slot_id, 4);
        crate::log!("usb: {}: timeout waiting for bulk transfer\n", what);
    })?;

    let completion = (evt.d2 >> 24) & 0xFF;
    let residual = (evt.d2 & 0x00FF_FFFF) as u32;

    let requested = length;
    let transferred = requested.saturating_sub(residual.min(requested));

    Ok((completion, transferred))
}

fn bulk_xfer_sync(
    ctx: &XhciContext,
    ring: &mut TrbRing,
    slot_id: u32,
    ep_target: u32,
    buf_phys: u64,
    length: u32,
    what: &'static str,
    timeout_ms: u64,
) -> Result<(u32, u32), ()> {
    let trb = Trb {
        d0: lo(buf_phys),
        d1: hi(buf_phys),
        d2: length,
        d3: trb_type(1) | (1 << 5), // Normal TRB, IOC
    };

    let Some(trb_phys) = ring.push_with_phys(trb) else {
        crate::log!("usb: {}: bulk ring full\n", what);
        return Err(());
    };

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target) };

    let evt = xhci::wait_for_event_spin_ms(
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
            let evt_ep_target = (evt.d3 >> 16) & 0x1F;
            if evt_ep_target != ep_target {
                return false;
            }
            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (trb_phys & !0xFu64)
        },
        timeout_ms,
    )
    .ok_or(())
    .map_err(|_| {
        xhci::debug_event_buffer_summary(ctx.controller_id);
        xhci::debug_peek_transfer_events(ctx.controller_id, slot_id, ep_target, 4);
        xhci::debug_peek_transfer_events_for_slot(ctx.controller_id, slot_id, 4);
        crate::log!("usb: {}: timeout waiting for bulk transfer\n", what);
    })?;

    let completion = (evt.d2 >> 24) & 0xFF;
    let residual = (evt.d2 & 0x00FF_FFFF) as u32;

    let requested = length;
    let transferred = requested.saturating_sub(residual.min(requested));

    Ok((completion, transferred))
}

async fn bot_command(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
    cdb: &[u8],
    data_in: Option<&mut [u8]>,
) -> Result<Csw, ()> {
    // Allocate DMA for CBW + CSW.
    let (cbw_phys, cbw_virt) = dma::alloc(CBW_LEN, 64).ok_or(())?;
    let (csw_phys, csw_virt) = match dma::alloc(CSW_LEN, 64) {
        Some(p) => p,
        None => {
            dma::dealloc(cbw_virt, CBW_LEN);
            return Err(());
        }
    };

    unsafe {
        write_bytes(cbw_virt, 0, CBW_LEN);
        write_bytes(csw_virt, 0, CSW_LEN);
    }

    // If we have a data stage, allocate DMA buffer for it.
    let mut data_phys: u64 = 0;
    let mut data_virt: *mut u8 = core::ptr::null_mut();
    let mut data_len: usize = 0;
    if let Some(data) = data_in.as_deref() {
        data_len = data.len();
        let (p, v) = match dma::alloc(data_len, 64) {
            Some(p) => p,
            None => {
                dma::dealloc(csw_virt, CSW_LEN);
                dma::dealloc(cbw_virt, CBW_LEN);
                return Err(());
            }
        };
        data_phys = p;
        data_virt = v;
        unsafe { write_bytes(data_virt, 0, data_len) };
    }

    // Build and write CBW.
    let cbw = build_cbw(tag, data_len as u32, data_in.is_some(), 0, cdb);
    unsafe {
        core::ptr::copy_nonoverlapping(cbw.as_ptr(), cbw_virt, CBW_LEN);
    }

    // Stage 1: CBW on bulk-out.
    let (cc_cbw, _xfer_cbw) = bulk_xfer(
        ctx,
        ring_out,
        slot_id,
        ep_out_target,
        cbw_phys,
        CBW_LEN as u32,
        "bot-cbw",
        800,
    )
    .await?;
    if cc_cbw != 1 {
        crate::log!("usb: bot: cbw cc={}\n", cc_cbw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    // Stage 2: optional data stage.
    if let Some(data) = data_in {
        let (cc_data, xfer) = bulk_xfer(
            ctx,
            ring_in,
            slot_id,
            ep_in_target,
            data_phys,
            data_len as u32,
            "bot-data-in",
            1200,
        )
        .await?;

        // CC=13 (short packet) is common/acceptable for some reads.
        if cc_data != 1 && cc_data != 13 {
            crate::log!("usb: bot: data-in cc={}\n", cc_data);
            goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
            return Err(());
        }

        let n = (xfer as usize).min(data.len());
        unsafe {
            let src = core::slice::from_raw_parts(data_virt, n);
            data[..n].copy_from_slice(src);
        }
    }

    // Stage 3: CSW on bulk-in.
    let (cc_csw, xfer_csw) = bulk_xfer(
        ctx,
        ring_in,
        slot_id,
        ep_in_target,
        csw_phys,
        CSW_LEN as u32,
        "bot-csw",
        1200,
    )
    .await?;
    if cc_csw != 1 && cc_csw != 13 {
        crate::log!("usb: bot: csw cc={}\n", cc_csw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    let csw_buf = unsafe {
        let n = (xfer_csw as usize).min(CSW_LEN);
        core::slice::from_raw_parts(csw_virt, n)
    };

    let Some((csw_tag, csw)) = parse_csw(csw_buf) else {
        crate::log!("usb: bot: invalid csw (len={})\n", xfer_csw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    };

    if csw_tag != tag {
        crate::log!("usb: bot: csw tag mismatch {} != {}\n", csw_tag, tag);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
    Ok(csw)
}

async fn bot_command_out(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
    cdb: &[u8],
    data_out: &[u8],
) -> Result<Csw, ()> {
    // Allocate DMA for CBW + CSW + OUT data.
    let (cbw_phys, cbw_virt) = dma::alloc(CBW_LEN, 64).ok_or(())?;
    let (csw_phys, csw_virt) = match dma::alloc(CSW_LEN, 64) {
        Some(p) => p,
        None => {
            dma::dealloc(cbw_virt, CBW_LEN);
            return Err(());
        }
    };

    unsafe {
        write_bytes(cbw_virt, 0, CBW_LEN);
        write_bytes(csw_virt, 0, CSW_LEN);
    }

    let data_len = data_out.len();
    let (data_phys, data_virt) = match dma::alloc(data_len, 64) {
        Some(p) => p,
        None => {
            dma::dealloc(csw_virt, CSW_LEN);
            dma::dealloc(cbw_virt, CBW_LEN);
            return Err(());
        }
    };
    unsafe {
        core::ptr::copy_nonoverlapping(data_out.as_ptr(), data_virt, data_len);
    }

    let cbw = build_cbw(tag, data_len as u32, false, 0, cdb);
    unsafe {
        core::ptr::copy_nonoverlapping(cbw.as_ptr(), cbw_virt, CBW_LEN);
    }

    // Stage 1: CBW on bulk-out.
    let (cc_cbw, _xfer_cbw) = bulk_xfer(
        ctx,
        ring_out,
        slot_id,
        ep_out_target,
        cbw_phys,
        CBW_LEN as u32,
        "bot-cbw",
        800,
    )
    .await?;
    if cc_cbw != 1 {
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    // Stage 2: OUT data stage on bulk-out.
    let (cc_data, _xfer) = bulk_xfer(
        ctx,
        ring_out,
        slot_id,
        ep_out_target,
        data_phys,
        data_len as u32,
        "bot-data-out",
        1200,
    )
    .await?;
    if cc_data != 1 {
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    // Stage 3: CSW on bulk-in.
    let (cc_csw, xfer_csw) = bulk_xfer(
        ctx,
        ring_in,
        slot_id,
        ep_in_target,
        csw_phys,
        CSW_LEN as u32,
        "bot-csw",
        1200,
    )
    .await?;
    if cc_csw != 1 && cc_csw != 13 {
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    let csw_buf = unsafe {
        let n = (xfer_csw as usize).min(CSW_LEN);
        core::slice::from_raw_parts(csw_virt, n)
    };

    let Some((csw_tag, csw)) = parse_csw(csw_buf) else {
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    };
    if csw_tag != tag {
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
    Ok(csw)
}

fn bot_command_sync(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
    cdb: &[u8],
    data_in: Option<&mut [u8]>,
) -> Result<Csw, ()> {
    let (cbw_phys, cbw_virt) = dma::alloc(CBW_LEN, 64).ok_or(())?;
    let (csw_phys, csw_virt) = match dma::alloc(CSW_LEN, 64) {
        Some(p) => p,
        None => {
            dma::dealloc(cbw_virt, CBW_LEN);
            return Err(());
        }
    };

    unsafe {
        write_bytes(cbw_virt, 0, CBW_LEN);
        write_bytes(csw_virt, 0, CSW_LEN);
    }

    let mut data_phys: u64 = 0;
    let mut data_virt: *mut u8 = core::ptr::null_mut();
    let mut data_len: usize = 0;
    if let Some(data) = data_in.as_deref() {
        data_len = data.len();
        let (p, v) = match dma::alloc(data_len, 64) {
            Some(p) => p,
            None => {
                dma::dealloc(csw_virt, CSW_LEN);
                dma::dealloc(cbw_virt, CBW_LEN);
                return Err(());
            }
        };
        data_phys = p;
        data_virt = v;
        unsafe { write_bytes(data_virt, 0, data_len) };
    }

    let cbw = build_cbw(tag, data_len as u32, data_in.is_some(), 0, cdb);
    unsafe {
        core::ptr::copy_nonoverlapping(cbw.as_ptr(), cbw_virt, CBW_LEN);
    }

    let (cc_cbw, _xfer_cbw) = bulk_xfer_sync(
        ctx,
        ring_out,
        slot_id,
        ep_out_target,
        cbw_phys,
        CBW_LEN as u32,
        "bot-cbw",
        500,
    )?;
    if cc_cbw != 1 {
        crate::log!("usb: bot: cbw cc={}\n", cc_cbw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    if let Some(data) = data_in {
        let (cc_data, xfer) = bulk_xfer_sync(
            ctx,
            ring_in,
            slot_id,
            ep_in_target,
            data_phys,
            data_len as u32,
            "bot-data-in",
            5000,
        )?;

        if cc_data != 1 && cc_data != 13 {
            crate::log!("usb: bot: data-in cc={}\n", cc_data);
            goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
            return Err(());
        }

        let n = (xfer as usize).min(data.len());
        unsafe {
            let src = core::slice::from_raw_parts(data_virt, n);
            data[..n].copy_from_slice(src);
        }
    }

    let (cc_csw, xfer_csw) = bulk_xfer_sync(
        ctx,
        ring_in,
        slot_id,
        ep_in_target,
        csw_phys,
        CSW_LEN as u32,
        "bot-csw",
        2000,
    )?;
    if cc_csw != 1 && cc_csw != 13 {
        crate::log!("usb: bot: csw cc={}\n", cc_csw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    let csw_buf = unsafe {
        let n = (xfer_csw as usize).min(CSW_LEN);
        core::slice::from_raw_parts(csw_virt, n)
    };

    let Some((csw_tag, csw)) = parse_csw(csw_buf) else {
        crate::log!("usb: bot: invalid csw (len={})\n", xfer_csw);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    };

    if csw_tag != tag {
        crate::log!("usb: bot: csw tag mismatch {} != {}\n", csw_tag, tag);
        goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
        return Err(());
    }

    goto_cleanup(cbw_virt, csw_virt, data_virt, data_len);
    Ok(csw)
}

fn goto_cleanup(cbw_virt: *mut u8, csw_virt: *mut u8, data_virt: *mut u8, data_len: usize) {
    if !data_virt.is_null() && data_len != 0 {
        dma::dealloc(data_virt, data_len);
    }
    dma::dealloc(csw_virt, CSW_LEN);
    dma::dealloc(cbw_virt, CBW_LEN);
}

fn log_request_sense(prefix: &str, sense: &scsi::SenseFixed) {
    crate::log!(
        "usb: bot: {} sense rc={:#x} key={:?} asc={:#x} ascq={:#x}\n",
        prefix,
        sense.response_code,
        sense.sense_key,
        sense.asc,
        sense.ascq
    );
}

pub async fn scsi_request_sense_fixed_async(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Option<scsi::SenseFixed> {
    let cdb = scsi::cdb_request_sense(18);
    let mut data = [0u8; 18];

    let csw = bot_command(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        Some(&mut data),
    )
    .await
    .ok()?;

    if csw.status != BotStatus::Passed {
        return None;
    }

    scsi::parse_request_sense_fixed(&data)
}

pub fn scsi_request_sense_fixed_sync(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Option<scsi::SenseFixed> {
    let cdb = scsi::cdb_request_sense(18);
    let mut data = [0u8; 18];

    let csw = bot_command_sync(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        Some(&mut data),
    )
    .ok()?;

    if csw.status != BotStatus::Passed {
        return None;
    }

    scsi::parse_request_sense_fixed(&data)
}

pub async fn scsi_test_unit_ready(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<(), ()> {
    let cdb = scsi::cdb_test_unit_ready();
    let csw = bot_command(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        None,
    )
    .await?;

    if csw.status != BotStatus::Passed {
        if let Some(sense) = scsi_request_sense_fixed_async(
            ctx,
            ring_out,
            ring_in,
            slot_id,
            ep_out_target,
            ep_in_target,
            tag.wrapping_add(1),
        )
        .await
        {
            log_request_sense("test-unit-ready", &sense);
        } else {
            crate::log!("usb: bot: test-unit-ready request-sense failed\n");
        }
        return Err(());
    }

    Ok(())
}

pub async fn scsi_inquiry_basic(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<scsi::InquiryBasic, ()> {
    let cdb = scsi::cdb_inquiry(36);
    let mut data = [0u8; 64];

    let csw = bot_command(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        Some(&mut data),
    )
    .await?;

    if csw.status != BotStatus::Passed {
        crate::log!(
            "usb: bot: inquiry failed status={:?} residue={}\n",
            csw.status,
            csw.residue
        );
        if let Some(sense) = scsi_request_sense_fixed_async(
            ctx,
            ring_out,
            ring_in,
            slot_id,
            ep_out_target,
            ep_in_target,
            tag.wrapping_add(1),
        )
        .await
        {
            log_request_sense("inquiry", &sense);
        } else {
            crate::log!("usb: bot: inquiry request-sense failed\n");
        }
        return Err(());
    }

    scsi::parse_inquiry_basic(&data).ok_or(())
}

pub async fn scsi_read_capacity_10(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<scsi::Capacity10, ()> {
    let cdb = scsi::cdb_read_capacity_10();
    let mut data = [0u8; 16];

    let csw = bot_command(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        Some(&mut data),
    )
    .await?;

    if csw.status != BotStatus::Passed {
        crate::log!(
            "usb: bot: read-capacity failed status={:?} residue={}\n",
            csw.status,
            csw.residue
        );
        if let Some(sense) = scsi_request_sense_fixed_async(
            ctx,
            ring_out,
            ring_in,
            slot_id,
            ep_out_target,
            ep_in_target,
            tag.wrapping_add(1),
        )
        .await
        {
            log_request_sense("read-capacity", &sense);
        } else {
            crate::log!("usb: bot: read-capacity request-sense failed\n");
        }
        return Err(());
    }

    scsi::parse_read_capacity_10(&data).ok_or(())
}

pub async fn scsi_read_10(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
    lba: u32,
    blocks: u16,
    out: &mut [u8],
) -> Result<Csw, ()> {
    let cdb = scsi::cdb_read_10(lba, blocks);
    bot_command(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        Some(out),
    )
    .await
}

pub async fn scsi_write_10(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
    lba: u32,
    blocks: u16,
    data: &[u8],
) -> Result<Csw, ()> {
    let cdb = scsi::cdb_write_10(lba, blocks);
    bot_command_out(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        data,
    )
    .await
}

pub fn scsi_synchronize_cache_10_sync(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<Csw, ()> {
    let cdb = scsi::cdb_synchronize_cache_10();
    bot_command_sync(
        ctx,
        ring_out,
        ring_in,
        slot_id,
        ep_out_target,
        ep_in_target,
        tag,
        &cdb,
        None,
    )
}
