use super::bot::{self, BotStatus, Csw};
use super::xhci::{TrbRing, XhciContext};

// SCSI command-set helpers plus the current BOT-backed execution helpers.
//
// The pure SCSI pieces in this module are transport-agnostic: CDB builders and
// response parsers. The `*_via_bot`-style helpers are the current concrete
// integration layer used by TRUEOS today, keeping the SCSI-facing API together
// while `bot.rs` remains focused on Bulk-Only Transport mechanics.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SenseKey {
    NoSense,
    RecoveredError,
    NotReady,
    MediumError,
    HardwareError,
    IllegalRequest,
    UnitAttention,
    DataProtect,
    BlankCheck,
    VendorSpecific,
    CopyAborted,
    AbortedCommand,
    VolumeOverflow,
    Miscompare,
    Completed,
    Unknown(u8),
}

impl SenseKey {
    pub fn from_u8(v: u8) -> Self {
        match v & 0x0F {
            0x0 => SenseKey::NoSense,
            0x1 => SenseKey::RecoveredError,
            0x2 => SenseKey::NotReady,
            0x3 => SenseKey::MediumError,
            0x4 => SenseKey::HardwareError,
            0x5 => SenseKey::IllegalRequest,
            0x6 => SenseKey::UnitAttention,
            0x7 => SenseKey::DataProtect,
            0x8 => SenseKey::BlankCheck,
            0x9 => SenseKey::VendorSpecific,
            0xA => SenseKey::CopyAborted,
            0xB => SenseKey::AbortedCommand,
            0xD => SenseKey::VolumeOverflow,
            0xE => SenseKey::Miscompare,
            0xF => SenseKey::Completed,
            other => SenseKey::Unknown(other),
        }
    }
}

pub const OP_TEST_UNIT_READY: u8 = 0x00;
pub const OP_REQUEST_SENSE: u8 = 0x03;
pub const OP_INQUIRY: u8 = 0x12;
pub const OP_READ_CAPACITY_10: u8 = 0x25;
pub const OP_READ_10: u8 = 0x28;
pub const OP_WRITE_10: u8 = 0x2A;
pub const OP_SYNCHRONIZE_CACHE_10: u8 = 0x35;

pub fn cdb_test_unit_ready() -> [u8; 6] {
    [OP_TEST_UNIT_READY, 0, 0, 0, 0, 0]
}

pub fn cdb_request_sense(allocation_len: u8) -> [u8; 6] {
    [OP_REQUEST_SENSE, 0, 0, 0, allocation_len, 0]
}

pub fn cdb_inquiry(allocation_len: u16) -> [u8; 6] {
    // EVPD=0, PageCode=0
    let len = allocation_len.min(0xFF) as u8;
    [OP_INQUIRY, 0, 0, 0, len, 0]
}

pub fn cdb_read_capacity_10() -> [u8; 10] {
    [OP_READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}

pub fn cdb_read_10(lba: u32, blocks: u16) -> [u8; 10] {
    let lba_be = lba.to_be_bytes();
    let blocks_be = blocks.to_be_bytes();
    [
        OP_READ_10,
        0,
        lba_be[0],
        lba_be[1],
        lba_be[2],
        lba_be[3],
        0,
        blocks_be[0],
        blocks_be[1],
        0,
    ]
}

pub fn cdb_write_10(lba: u32, blocks: u16) -> [u8; 10] {
    let lba_be = lba.to_be_bytes();
    let blocks_be = blocks.to_be_bytes();
    [
        OP_WRITE_10,
        0,
        lba_be[0],
        lba_be[1],
        lba_be[2],
        lba_be[3],
        0,
        blocks_be[0],
        blocks_be[1],
        0,
    ]
}

pub fn cdb_synchronize_cache_10() -> [u8; 10] {
    // SYNCHRONIZE CACHE(10): request device to flush volatile caches.
    // We do not set IMMED; we want the command to complete when durable.
    [OP_SYNCHRONIZE_CACHE_10, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct InquiryBasic {
    pub peripheral_type: u8,
    pub removable: bool,
    pub vendor: [u8; 8],
    pub product: [u8; 16],
    pub revision: [u8; 4],
}

pub fn parse_inquiry_basic(buf: &[u8]) -> Option<InquiryBasic> {
    if buf.len() < 36 {
        return None;
    }

    let peripheral_type = buf[0] & 0x1F;
    let removable = (buf[1] & 0x80) != 0;

    let mut vendor = [0u8; 8];
    vendor.copy_from_slice(&buf[8..16]);

    let mut product = [0u8; 16];
    product.copy_from_slice(&buf[16..32]);

    let mut revision = [0u8; 4];
    revision.copy_from_slice(&buf[32..36]);

    Some(InquiryBasic {
        peripheral_type,
        removable,
        vendor,
        product,
        revision,
    })
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Capacity10 {
    pub last_lba: u32,
    pub block_size: u32,
}

pub fn parse_read_capacity_10(buf: &[u8]) -> Option<Capacity10> {
    if buf.len() < 8 {
        return None;
    }

    let last_lba = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let block_size = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Some(Capacity10 {
        last_lba,
        block_size,
    })
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SenseFixed {
    pub response_code: u8,
    pub sense_key: SenseKey,
    pub asc: u8,
    pub ascq: u8,
}

pub fn parse_request_sense_fixed(buf: &[u8]) -> Option<SenseFixed> {
    // SPC fixed format sense data is typically at least 18 bytes.
    if buf.len() < 14 {
        return None;
    }

    let response_code = buf[0] & 0x7F;
    let sense_key = SenseKey::from_u8(buf[2]);
    let asc = buf[12];
    let ascq = buf[13];

    Some(SenseFixed {
        response_code,
        sense_key,
        asc,
        ascq,
    })
}

fn log_request_sense(prefix: &str, sense: &SenseFixed) {
    crate::log!(
        "usb: scsi: {} sense rc={:#x} key={:?} asc={:#x} ascq={:#x}\n",
        prefix,
        sense.response_code,
        sense.sense_key,
        sense.asc,
        sense.ascq
    );
}

pub async fn request_sense_fixed_async(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Option<SenseFixed> {
    let cdb = cdb_request_sense(18);
    let mut data = [0u8; 18];

    let csw = bot::command_in(
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

    parse_request_sense_fixed(&data)
}

pub fn request_sense_fixed_sync(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Option<SenseFixed> {
    let cdb = cdb_request_sense(18);
    let mut data = [0u8; 18];

    let csw = bot::command_in_sync(
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

    parse_request_sense_fixed(&data)
}

pub async fn test_unit_ready(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<(), ()> {
    let cdb = cdb_test_unit_ready();
    let csw = bot::command_in(
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
        if let Some(sense) = request_sense_fixed_async(
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
            crate::log!("usb: scsi: test-unit-ready request-sense failed\n");
        }
        return Err(());
    }

    Ok(())
}

pub async fn inquiry_basic(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<InquiryBasic, ()> {
    let cdb = cdb_inquiry(36);
    let mut data = [0u8; 64];

    let csw = bot::command_in(
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
            "usb: scsi: inquiry failed status={:?} residue={}\n",
            csw.status,
            csw.residue
        );
        if let Some(sense) = request_sense_fixed_async(
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
            crate::log!("usb: scsi: inquiry request-sense failed\n");
        }
        return Err(());
    }

    parse_inquiry_basic(&data).ok_or(())
}

pub async fn read_capacity_10(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<Capacity10, ()> {
    let cdb = cdb_read_capacity_10();
    let mut data = [0u8; 16];

    let csw = bot::command_in(
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
            "usb: scsi: read-capacity failed status={:?} residue={}\n",
            csw.status,
            csw.residue
        );
        if let Some(sense) = request_sense_fixed_async(
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
            crate::log!("usb: scsi: read-capacity request-sense failed\n");
        }
        return Err(());
    }

    parse_read_capacity_10(&data).ok_or(())
}

pub async fn read_10(
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
    let cdb = cdb_read_10(lba, blocks);
    bot::command_in(
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

pub async fn write_10(
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
    let cdb = cdb_write_10(lba, blocks);
    bot::command_out(
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

pub fn synchronize_cache_10_sync(
    ctx: &XhciContext,
    ring_out: &mut TrbRing,
    ring_in: &mut TrbRing,
    slot_id: u32,
    ep_out_target: u32,
    ep_in_target: u32,
    tag: u32,
) -> Result<Csw, ()> {
    let cdb = cdb_synchronize_cache_10();
    bot::command_in_sync(
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
