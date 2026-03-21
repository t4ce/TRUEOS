// Transport-agnostic SCSI helpers.
//
// This module is the shared home for command builders and small response
// parsers. The active BOT/crab-usb execution path lives in `mass.rs`.

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
