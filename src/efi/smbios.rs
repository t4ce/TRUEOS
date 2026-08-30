//! Bounded, read-only SMBIOS discovery and structure-table walking.
//!
//! Firmware tables are treated as untrusted input. Entry-point anchors,
//! checksums, lengths and complete physical ranges are validated before any
//! structure data is exposed to diagnostics.

use crate::{limine, pci::mmio};

const SMBIOS2_ENTRY_MIN_BYTES: usize = 0x1F;
const SMBIOS3_ENTRY_MIN_BYTES: usize = 0x18;
const SMBIOS_ENTRY_MAX_BYTES: usize = 0x40;
const SMBIOS_TABLE_MAX_BYTES: usize = 16 * 1024 * 1024;
const SMBIOS_STRUCTURE_MAX_COUNT: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryPointKind {
    Smbios3,
    Smbios2,
}

impl EntryPointKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Smbios3 => "SMBIOS 3.x (64-bit entry point)",
            Self::Smbios2 => "SMBIOS 2.x (32-bit entry point)",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoverError {
    NoBootloaderResponse,
    NoEntryPoint,
    EntryPointAddressInvalid,
    EntryPointRangeInvalid,
    EntryPointMapFailed,
    EntryPointAnchorInvalid,
    EntryPointLengthInvalid,
    EntryPointChecksumInvalid,
    IntermediateAnchorInvalid,
    IntermediateChecksumInvalid,
    TableAddressInvalid,
    TableLengthInvalid,
    TableRangeInvalid,
    TableMapFailed,
}

impl DiscoverError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoBootloaderResponse => "bootloader-did-not-return-smbios",
            Self::NoEntryPoint => "bootloader-returned-no-smbios-entry-point",
            Self::EntryPointAddressInvalid => "entry-point-address-not-in-memory-map",
            Self::EntryPointRangeInvalid => "entry-point-range-not-contained",
            Self::EntryPointMapFailed => "entry-point-map-failed",
            Self::EntryPointAnchorInvalid => "entry-point-anchor-invalid",
            Self::EntryPointLengthInvalid => "entry-point-length-invalid",
            Self::EntryPointChecksumInvalid => "entry-point-checksum-invalid",
            Self::IntermediateAnchorInvalid => "intermediate-anchor-invalid",
            Self::IntermediateChecksumInvalid => "intermediate-checksum-invalid",
            Self::TableAddressInvalid => "structure-table-address-invalid",
            Self::TableLengthInvalid => "structure-table-length-invalid",
            Self::TableRangeInvalid => "structure-table-range-not-contained",
            Self::TableMapFailed => "structure-table-map-failed",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Table {
    pub kind: EntryPointKind,
    pub entry_point_raw: u64,
    pub entry_point_phys: u64,
    pub entry_point_len: usize,
    pub major: u8,
    pub minor: u8,
    pub doc_revision: Option<u8>,
    pub entry_point_revision: u8,
    pub table_phys: u64,
    pub table_declared_bytes: usize,
    pub declared_structure_count: Option<u16>,
    bytes: &'static [u8],
}

impl Table {
    pub fn bytes(self) -> &'static [u8] {
        self.bytes
    }

    pub fn structures(self) -> Structures<'static> {
        Structures {
            bytes: self.bytes,
            offset: 0,
            count: 0,
            finished: false,
        }
    }
}

pub fn discover() -> Result<Table, DiscoverError> {
    let Some((entry_64, entry_32)) = limine::smbios_entry_point_addresses() else {
        return Err(DiscoverError::NoBootloaderResponse);
    };

    let mut first_error = None;
    if entry_64 != 0 {
        match discover_smbios3(entry_64) {
            Ok(table) => return Ok(table),
            Err(error) => first_error = Some(error),
        }
    }
    if entry_32 != 0 {
        match discover_smbios2(entry_32) {
            Ok(table) => return Ok(table),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    Err(first_error.unwrap_or(DiscoverError::NoEntryPoint))
}

fn discover_smbios3(entry_raw: u64) -> Result<Table, DiscoverError> {
    let initial = map_firmware_bytes(entry_raw, SMBIOS3_ENTRY_MIN_BYTES, true)?;
    if initial.get(..5) != Some(b"_SM3_") {
        return Err(DiscoverError::EntryPointAnchorInvalid);
    }
    let entry_len = usize::from(initial[6]);
    if !(SMBIOS3_ENTRY_MIN_BYTES..=SMBIOS_ENTRY_MAX_BYTES).contains(&entry_len) {
        return Err(DiscoverError::EntryPointLengthInvalid);
    }
    let entry = map_firmware_bytes(entry_raw, entry_len, true)?;
    if !checksum_is_zero(entry) {
        return Err(DiscoverError::EntryPointChecksumInvalid);
    }

    let table_len =
        usize::try_from(read_u32(entry, 0x0C).ok_or(DiscoverError::EntryPointLengthInvalid)?)
            .map_err(|_| DiscoverError::TableLengthInvalid)?;
    let table_raw = read_u64(entry, 0x10).ok_or(DiscoverError::EntryPointLengthInvalid)?;
    let bytes = map_structure_table(table_raw, table_len)?;
    let entry_point_phys =
        limine::try_as_phys_addr(entry_raw).ok_or(DiscoverError::EntryPointAddressInvalid)?;
    let table_phys =
        limine::try_as_phys_addr(table_raw).ok_or(DiscoverError::TableAddressInvalid)?;

    Ok(Table {
        kind: EntryPointKind::Smbios3,
        entry_point_raw: entry_raw,
        entry_point_phys,
        entry_point_len: entry_len,
        major: entry[7],
        minor: entry[8],
        doc_revision: Some(entry[9]),
        entry_point_revision: entry[10],
        table_phys,
        table_declared_bytes: table_len,
        declared_structure_count: None,
        bytes,
    })
}

fn discover_smbios2(entry_raw: u64) -> Result<Table, DiscoverError> {
    let initial = map_firmware_bytes(entry_raw, SMBIOS2_ENTRY_MIN_BYTES, true)?;
    if initial.get(..4) != Some(b"_SM_") {
        return Err(DiscoverError::EntryPointAnchorInvalid);
    }
    let entry_len = usize::from(initial[5]);
    if !(SMBIOS2_ENTRY_MIN_BYTES..=SMBIOS_ENTRY_MAX_BYTES).contains(&entry_len) {
        return Err(DiscoverError::EntryPointLengthInvalid);
    }
    let entry = map_firmware_bytes(entry_raw, entry_len, true)?;
    if !checksum_is_zero(entry) {
        return Err(DiscoverError::EntryPointChecksumInvalid);
    }
    if entry.get(0x10..0x15) != Some(b"_DMI_") {
        return Err(DiscoverError::IntermediateAnchorInvalid);
    }
    if !checksum_is_zero(
        entry
            .get(0x10..SMBIOS2_ENTRY_MIN_BYTES)
            .ok_or(DiscoverError::EntryPointLengthInvalid)?,
    ) {
        return Err(DiscoverError::IntermediateChecksumInvalid);
    }

    let table_len =
        usize::from(read_u16(entry, 0x16).ok_or(DiscoverError::EntryPointLengthInvalid)?);
    let table_raw = u64::from(read_u32(entry, 0x18).ok_or(DiscoverError::EntryPointLengthInvalid)?);
    let structure_count = read_u16(entry, 0x1C).ok_or(DiscoverError::EntryPointLengthInvalid)?;
    let bytes = map_structure_table(table_raw, table_len)?;
    let entry_point_phys =
        limine::try_as_phys_addr(entry_raw).ok_or(DiscoverError::EntryPointAddressInvalid)?;
    let table_phys =
        limine::try_as_phys_addr(table_raw).ok_or(DiscoverError::TableAddressInvalid)?;

    Ok(Table {
        kind: EntryPointKind::Smbios2,
        entry_point_raw: entry_raw,
        entry_point_phys,
        entry_point_len: entry_len,
        major: entry[6],
        minor: entry[7],
        doc_revision: None,
        entry_point_revision: entry[10],
        table_phys,
        table_declared_bytes: table_len,
        declared_structure_count: Some(structure_count),
        bytes,
    })
}

fn map_structure_table(raw: u64, byte_len: usize) -> Result<&'static [u8], DiscoverError> {
    if byte_len == 0 || byte_len > SMBIOS_TABLE_MAX_BYTES {
        return Err(DiscoverError::TableLengthInvalid);
    }
    let phys = limine::try_as_phys_addr(raw).ok_or(DiscoverError::TableAddressInvalid)?;
    if !limine::memmap_contains_phys_range(phys, byte_len) {
        return Err(DiscoverError::TableRangeInvalid);
    }
    let ptr =
        mmio::map_mmio_region_exact(phys, byte_len).map_err(|_| DiscoverError::TableMapFailed)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), byte_len) })
}

fn map_firmware_bytes(
    raw: u64,
    byte_len: usize,
    entry_point: bool,
) -> Result<&'static [u8], DiscoverError> {
    let phys = limine::try_as_phys_addr(raw).ok_or(if entry_point {
        DiscoverError::EntryPointAddressInvalid
    } else {
        DiscoverError::TableAddressInvalid
    })?;
    if !limine::memmap_contains_phys_range(phys, byte_len) {
        return Err(if entry_point {
            DiscoverError::EntryPointRangeInvalid
        } else {
            DiscoverError::TableRangeInvalid
        });
    }
    let ptr = mmio::map_mmio_region_exact(phys, byte_len).map_err(|_| {
        if entry_point {
            DiscoverError::EntryPointMapFailed
        } else {
            DiscoverError::TableMapFailed
        }
    })?;
    Ok(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), byte_len) })
}

fn checksum_is_zero(bytes: &[u8]) -> bool {
    bytes.iter().copied().fold(0u8, u8::wrapping_add) == 0
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    StructureLimit,
    TruncatedHeader(usize),
    InvalidFormattedLength(usize),
    TruncatedFormattedArea(usize),
    UnterminatedStringSet(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Structure<'a> {
    pub table_offset: usize,
    pub type_id: u8,
    pub handle: u16,
    formatted: &'a [u8],
    strings: &'a [u8],
    total_bytes: usize,
}

impl<'a> Structure<'a> {
    pub fn formatted(self) -> &'a [u8] {
        self.formatted
    }

    pub fn formatted_len(self) -> usize {
        self.formatted.len()
    }

    pub fn total_bytes(self) -> usize {
        self.total_bytes
    }

    pub fn byte(self, offset: usize) -> Option<u8> {
        self.formatted.get(offset).copied()
    }

    pub fn u16(self, offset: usize) -> Option<u16> {
        read_u16(self.formatted, offset)
    }

    pub fn u32(self, offset: usize) -> Option<u32> {
        read_u32(self.formatted, offset)
    }

    pub fn u64(self, offset: usize) -> Option<u64> {
        read_u64(self.formatted, offset)
    }

    pub fn bytes(self, offset: usize, byte_len: usize) -> Option<&'a [u8]> {
        self.formatted.get(offset..offset.checked_add(byte_len)?)
    }

    pub fn string_bytes(self, index: u8) -> Option<&'a [u8]> {
        if index == 0 {
            return None;
        }
        self.strings().nth(usize::from(index) - 1)
    }

    pub fn strings(self) -> Strings<'a> {
        Strings {
            bytes: self.strings,
            offset: 0,
        }
    }

    pub const fn type_name(self) -> &'static str {
        structure_type_name(self.type_id)
    }

    /// Decode the stable, user-facing subset of an SMBIOS Type 17 record.
    ///
    /// The raw structure remains available to detailed diagnostics. This
    /// helper gives concise consumers such as Shell2's `ram` command one
    /// shared interpretation of module capacity, slot labels, and speed.
    pub fn memory_device(self) -> Option<MemoryDevice<'a>> {
        if self.type_id != 17 {
            return None;
        }

        let size = match self.u16(0x0C)? {
            0 => MemoryDeviceSize::NotInstalled,
            0xFFFF => MemoryDeviceSize::Unknown,
            0x7FFF => match self.u32(0x1C).map(|mib| mib & 0x7FFF_FFFF) {
                Some(0) | None => MemoryDeviceSize::Unknown,
                Some(mib) => MemoryDeviceSize::Bytes(u64::from(mib) * 1024 * 1024),
            },
            value if value & 0x8000 != 0 => {
                MemoryDeviceSize::Bytes(u64::from(value & 0x7FFF) * 1024)
            }
            value => MemoryDeviceSize::Bytes(u64::from(value) * 1024 * 1024),
        };

        Some(MemoryDevice {
            handle: self.handle,
            locator: self.byte(0x10).and_then(|index| self.string_bytes(index)),
            bank_locator: self.byte(0x11).and_then(|index| self.string_bytes(index)),
            size,
            speed_mt_s: memory_speed(self, 0x15, 0x54),
            configured_speed_mt_s: memory_speed(self, 0x20, 0x58),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryDeviceSize {
    NotInstalled,
    Unknown,
    Bytes(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryDevice<'a> {
    pub handle: u16,
    pub locator: Option<&'a [u8]>,
    pub bank_locator: Option<&'a [u8]>,
    pub size: MemoryDeviceSize,
    pub speed_mt_s: Option<u32>,
    pub configured_speed_mt_s: Option<u32>,
}

fn memory_speed(
    structure: Structure<'_>,
    legacy_offset: usize,
    extended_offset: usize,
) -> Option<u32> {
    match structure.u16(legacy_offset)? {
        0 => None,
        0xFFFF => structure.u32(extended_offset).filter(|speed| *speed != 0),
        speed => Some(u32::from(speed)),
    }
}

pub struct Strings<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Iterator for Strings<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let start = self.offset;
        let end = self.bytes[start..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|relative| start + relative)
            .unwrap_or(self.bytes.len());
        self.offset = if end < self.bytes.len() {
            end.saturating_add(1)
        } else {
            end
        };
        Some(&self.bytes[start..end])
    }
}

pub struct Structures<'a> {
    bytes: &'a [u8],
    offset: usize,
    count: usize,
    finished: bool,
}

impl<'a> Structures<'a> {
    pub fn next_structure(&mut self) -> Result<Option<Structure<'a>>, ParseError> {
        if self.finished {
            return Ok(None);
        }
        if self.count >= SMBIOS_STRUCTURE_MAX_COUNT {
            return Err(ParseError::StructureLimit);
        }
        if self.offset >= self.bytes.len() {
            self.finished = true;
            return Ok(None);
        }
        let start = self.offset;
        let header = self
            .bytes
            .get(start..start.saturating_add(4))
            .ok_or(ParseError::TruncatedHeader(start))?;
        let type_id = header[0];
        let formatted_len = usize::from(header[1]);
        if formatted_len < 4 {
            return Err(ParseError::InvalidFormattedLength(start));
        }
        let formatted_end = start
            .checked_add(formatted_len)
            .ok_or(ParseError::TruncatedFormattedArea(start))?;
        let formatted = self
            .bytes
            .get(start..formatted_end)
            .ok_or(ParseError::TruncatedFormattedArea(start))?;

        let mut terminator = formatted_end;
        while terminator.saturating_add(1) < self.bytes.len() {
            if self.bytes[terminator] == 0 && self.bytes[terminator + 1] == 0 {
                break;
            }
            terminator += 1;
        }
        if terminator.saturating_add(1) >= self.bytes.len() {
            return Err(ParseError::UnterminatedStringSet(start));
        }

        let strings = &self.bytes[formatted_end..terminator];
        let next = terminator + 2;
        let structure = Structure {
            table_offset: start,
            type_id,
            handle: u16::from_le_bytes([header[2], header[3]]),
            formatted,
            strings,
            total_bytes: next - start,
        };
        self.offset = next;
        self.count += 1;
        if type_id == 127 {
            self.finished = true;
        }
        Ok(Some(structure))
    }
}

pub const fn structure_type_name(type_id: u8) -> &'static str {
    match type_id {
        0 => "Platform Firmware Information",
        1 => "System Information",
        2 => "Baseboard or Module Information",
        3 => "System Enclosure or Chassis",
        4 => "Processor Information",
        5 => "Memory Controller Information (obsolete)",
        6 => "Memory Module Information (obsolete)",
        7 => "Cache Information",
        8 => "Port Connector Information",
        9 => "System Slots",
        10 => "Onboard Devices Information (obsolete)",
        11 => "OEM Strings",
        12 => "System Configuration Options",
        13 => "Firmware Language Information",
        14 => "Group Associations",
        15 => "System Event Log",
        16 => "Physical Memory Array",
        17 => "Memory Device",
        18 => "32-bit Memory Error Information",
        19 => "Memory Array Mapped Address",
        20 => "Memory Device Mapped Address",
        21 => "Built-in Pointing Device",
        22 => "Portable Battery",
        23 => "System Reset",
        24 => "Hardware Security",
        25 => "System Power Controls",
        26 => "Voltage Probe",
        27 => "Cooling Device",
        28 => "Temperature Probe",
        29 => "Electrical Current Probe",
        30 => "Out-of-Band Remote Access",
        31 => "Boot Integrity Services Entry Point",
        32 => "System Boot Information",
        33 => "64-bit Memory Error Information",
        34 => "Management Device",
        35 => "Management Device Component",
        36 => "Management Device Threshold Data",
        37 => "Memory Channel",
        38 => "IPMI Device Information",
        39 => "System Power Supply",
        40 => "Additional Information",
        41 => "Onboard Devices Extended Information",
        42 => "Management Controller Host Interface",
        43 => "TPM Device",
        44 => "Processor Additional Information",
        45 => "Firmware Inventory Information",
        46 => "String Property",
        126 => "Inactive",
        127 => "End of Table",
        128..=255 => "OEM-specific",
        _ => "Reserved",
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryDeviceSize, ParseError, Structure, Structures, checksum_is_zero};

    fn structures(bytes: &[u8]) -> Structures<'_> {
        Structures {
            bytes,
            offset: 0,
            count: 0,
            finished: false,
        }
    }

    #[test]
    fn walks_strings_and_end_marker_without_overread() {
        let bytes = [
            1, 0x1B, 0x34, 0x12, // Type 1 header
            1, 2, 3, 4, // manufacturer/product/version/serial
            0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE, // UUID
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, // UUID
            6, 5, 6, // wake-up/SKU/family
            b'A', b'c', b'm', b'e', 0, // string 1
            b'B', b'o', b'a', b'r', b'd', 0, // string 2
            b'v', b'1', 0, // string 3
            b'S', b'E', b'R', b'1', b'2', b'3', 0, // string 4
            b'S', b'K', b'U', 0, // string 5
            b'F', b'a', b'm', b'i', b'l', b'y', 0, 0, // string 6 + set terminator
            127, 4, 0xFF, 0xFF, 0, 0, // End-of-table structure
        ];
        let mut walker = structures(&bytes);

        let system = walker.next_structure().unwrap().unwrap();
        assert_eq!(system.type_id, 1);
        assert_eq!(system.handle, 0x1234);
        assert_eq!(system.formatted_len(), 0x1B);
        assert_eq!(system.string_bytes(1), Some(&b"Acme"[..]));
        assert_eq!(system.string_bytes(4), Some(&b"SER123"[..]));
        assert_eq!(system.string_bytes(6), Some(&b"Family"[..]));
        assert_eq!(system.string_bytes(7), None);

        let end = walker.next_structure().unwrap().unwrap();
        assert_eq!(end.type_id, 127);
        assert_eq!(end.total_bytes(), 6);
        assert!(walker.next_structure().unwrap().is_none());
    }

    #[test]
    fn rejects_missing_double_nul_terminator() {
        let bytes = [1, 4, 0, 0, b'x', 0];
        let error = structures(&bytes).next_structure().unwrap_err();
        assert_eq!(error, ParseError::UnterminatedStringSet(0));
    }

    #[test]
    fn rejects_invalid_or_truncated_formatted_area() {
        let invalid_length = [1, 3, 0, 0, 0, 0];
        assert_eq!(
            structures(&invalid_length).next_structure().unwrap_err(),
            ParseError::InvalidFormattedLength(0)
        );

        let truncated = [1, 8, 0, 0, 0, 0];
        assert_eq!(
            structures(&truncated).next_structure().unwrap_err(),
            ParseError::TruncatedFormattedArea(0)
        );
    }

    #[test]
    fn checksum_requires_wrapping_sum_zero() {
        assert!(checksum_is_zero(&[0x5A, 0xA6]));
        assert!(!checksum_is_zero(&[0x5A, 0xA5]));
    }

    #[test]
    fn decodes_installed_memory_device() {
        let mut formatted = [0u8; 0x22];
        formatted[0] = 17;
        formatted[1] = formatted.len() as u8;
        formatted[0x0C..0x0E].copy_from_slice(&16_384u16.to_le_bytes());
        formatted[0x10] = 1;
        formatted[0x11] = 2;
        formatted[0x15..0x17].copy_from_slice(&3_200u16.to_le_bytes());
        formatted[0x20..0x22].copy_from_slice(&2_933u16.to_le_bytes());
        let structure = Structure {
            table_offset: 0,
            type_id: 17,
            handle: 0x1234,
            formatted: &formatted,
            strings: b"DIMM_A1\0BANK 0",
            total_bytes: formatted.len() + 16,
        };

        let device = structure.memory_device().unwrap();
        assert_eq!(device.handle, 0x1234);
        assert_eq!(device.locator, Some(&b"DIMM_A1"[..]));
        assert_eq!(device.bank_locator, Some(&b"BANK 0"[..]));
        assert_eq!(device.size, MemoryDeviceSize::Bytes(16 * 1024 * 1024 * 1024));
        assert_eq!(device.speed_mt_s, Some(3_200));
        assert_eq!(device.configured_speed_mt_s, Some(2_933));
    }

    #[test]
    fn decodes_extended_memory_device_fields_and_empty_slot() {
        let mut formatted = [0u8; 0x5C];
        formatted[0] = 17;
        formatted[1] = formatted.len() as u8;
        formatted[0x0C..0x0E].copy_from_slice(&0x7FFFu16.to_le_bytes());
        formatted[0x1C..0x20].copy_from_slice(&32_768u32.to_le_bytes());
        formatted[0x15..0x17].copy_from_slice(&0xFFFFu16.to_le_bytes());
        formatted[0x20..0x22].copy_from_slice(&0xFFFFu16.to_le_bytes());
        formatted[0x54..0x58].copy_from_slice(&6_400u32.to_le_bytes());
        formatted[0x58..0x5C].copy_from_slice(&5_600u32.to_le_bytes());
        let structure = Structure {
            table_offset: 0,
            type_id: 17,
            handle: 1,
            formatted: &formatted,
            strings: &[],
            total_bytes: formatted.len() + 2,
        };

        let device = structure.memory_device().unwrap();
        assert_eq!(device.size, MemoryDeviceSize::Bytes(32 * 1024 * 1024 * 1024));
        assert_eq!(device.speed_mt_s, Some(6_400));
        assert_eq!(device.configured_speed_mt_s, Some(5_600));

        let mut empty_formatted = formatted;
        empty_formatted[0x0C..0x0E].copy_from_slice(&0u16.to_le_bytes());
        let empty = Structure {
            formatted: &empty_formatted,
            ..structure
        }
        .memory_device()
        .unwrap();
        assert_eq!(empty.size, MemoryDeviceSize::NotInstalled);
    }
}
