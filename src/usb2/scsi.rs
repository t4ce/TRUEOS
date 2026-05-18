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

pub const OP_REQUEST_SENSE: u8 = 0x03;
pub const OP_INQUIRY: u8 = 0x12;
pub const OP_READ_CAPACITY_10: u8 = 0x25;
pub const OP_READ_10: u8 = 0x28;
pub const OP_WRITE_10: u8 = 0x2A;
pub const OP_SYNCHRONIZE_CACHE_10: u8 = 0x35;

#[allow(dead_code)]
pub const CDB6_LEN: usize = 6;
#[allow(dead_code)]
pub const CDB10_LEN: usize = 10;
#[allow(dead_code)]
pub const READ_WRITE_10_MAX_BLOCKS: u32 = u16::MAX as u32;

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CdbBuildError {
    ZeroBlockSize,
    EmptyTransfer,
    MisalignedOffset,
    MisalignedLength,
    TransferTooLarge,
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
    cdb_rw_10(OP_READ_10, lba, blocks)
}

pub fn cdb_write_10(lba: u32, blocks: u16) -> [u8; 10] {
    cdb_rw_10(OP_WRITE_10, lba, blocks)
}

#[allow(dead_code)]
pub fn cdb_read_10_for_bytes(
    offset_bytes: u64,
    transfer_bytes: u32,
    block_size: u32,
) -> Result<[u8; 10], CdbBuildError> {
    let (lba, blocks) = rw_10_lba_blocks(offset_bytes, transfer_bytes, block_size)?;
    Ok(cdb_read_10(lba, blocks))
}

#[allow(dead_code)]
pub fn cdb_write_10_for_bytes(
    offset_bytes: u64,
    transfer_bytes: u32,
    block_size: u32,
) -> Result<[u8; 10], CdbBuildError> {
    let (lba, blocks) = rw_10_lba_blocks(offset_bytes, transfer_bytes, block_size)?;
    Ok(cdb_write_10(lba, blocks))
}

#[allow(dead_code)]
fn rw_10_lba_blocks(
    offset_bytes: u64,
    transfer_bytes: u32,
    block_size: u32,
) -> Result<(u32, u16), CdbBuildError> {
    if block_size == 0 {
        return Err(CdbBuildError::ZeroBlockSize);
    }
    if transfer_bytes == 0 {
        return Err(CdbBuildError::EmptyTransfer);
    }
    let block_size_u64 = u64::from(block_size);
    if offset_bytes % block_size_u64 != 0 {
        return Err(CdbBuildError::MisalignedOffset);
    }
    if transfer_bytes % block_size != 0 {
        return Err(CdbBuildError::MisalignedLength);
    }

    let lba = offset_bytes / block_size_u64;
    let blocks = transfer_bytes / block_size;
    if lba > u64::from(u32::MAX) || blocks == 0 || blocks > READ_WRITE_10_MAX_BLOCKS {
        return Err(CdbBuildError::TransferTooLarge);
    }
    Ok((lba as u32, blocks as u16))
}

fn cdb_rw_10(opcode: u8, lba: u32, blocks: u16) -> [u8; 10] {
    let lba_be = lba.to_be_bytes();
    let blocks_be = blocks.to_be_bytes();
    [
        opcode,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read10_cdb_matches_spc_layout() {
        assert_eq!(cdb_read_10(0x0000_0008, 1), [0x28, 0, 0, 0, 0, 8, 0, 0, 1, 0]);
    }

    #[test]
    fn write10_cdb_matches_spc_layout() {
        assert_eq!(cdb_write_10(0x0000_0008, 1), [0x2A, 0, 0, 0, 0, 8, 0, 0, 1, 0]);
    }

    #[test]
    fn byte_builders_reject_non_block_transfers() {
        assert_eq!(cdb_write_10_for_bytes(1, 512, 512), Err(CdbBuildError::MisalignedOffset));
        assert_eq!(cdb_write_10_for_bytes(0, 513, 512), Err(CdbBuildError::MisalignedLength));
        assert_eq!(cdb_write_10_for_bytes(0, 0, 512), Err(CdbBuildError::EmptyTransfer));
    }

    #[test]
    fn byte_builders_limit_write10_transfer_blocks() {
        assert_eq!(cdb_read_10_for_bytes(0, 65535 * 512, 512).unwrap(), cdb_read_10(0, 65535));
        assert_eq!(
            cdb_read_10_for_bytes(0, 65536 * 512, 512),
            Err(CdbBuildError::TransferTooLarge)
        );
    }

    #[test]
    fn fixed_sense_parser_extracts_key_asc_ascq() {
        let sense = [
            0x70, 0, 0x05, 0, 0, 0, 0, 0x0A, 0, 0, 0, 0, 0x24, 0x00, 0, 0, 0, 0,
        ];
        assert_eq!(
            parse_request_sense_fixed(&sense),
            Some(SenseFixed {
                response_code: 0x70,
                sense_key: SenseKey::IllegalRequest,
                asc: 0x24,
                ascq: 0x00,
            })
        );
    }
}
