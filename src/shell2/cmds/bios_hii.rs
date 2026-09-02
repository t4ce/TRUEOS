use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

use crate::efi::EfiGuid;

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 8;
const MAX_PACKAGE_LISTS: usize = 256;
const MAX_PACKAGES: usize = 4096;
const MAX_FORM_PACKAGES: usize = 256;
const MAX_STRING_TABLES: usize = 512;
const MAX_STRINGS: usize = 65_535;
const MAX_DECODED_STRING_BYTES: usize = 4 * 1024 * 1024;
const MAX_STRING_UNITS: usize = 8192;
const MAX_DIAGNOSTICS: usize = 32;

const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const VERSION: u16 = 1;

const SEC_HII: u32 = 2;
const SEC_CONFIG: u32 = 3;
const HII_FORMS: u8 = 0x02;
const HII_STRINGS: u8 = 0x04;

const SIBT_END: u8 = 0x00;
const SIBT_STRING_SCSU: u8 = 0x10;
const SIBT_STRING_SCSU_FONT: u8 = 0x11;
const SIBT_STRINGS_SCSU: u8 = 0x12;
const SIBT_STRINGS_SCSU_FONT: u8 = 0x13;
const SIBT_STRING_UCS2: u8 = 0x14;
const SIBT_STRING_UCS2_FONT: u8 = 0x15;
const SIBT_STRINGS_UCS2: u8 = 0x16;
const SIBT_STRINGS_UCS2_FONT: u8 = 0x17;
const SIBT_DUPLICATE: u8 = 0x20;
const SIBT_SKIP2: u8 = 0x21;
const SIBT_SKIP1: u8 = 0x22;
const SIBT_EXT1: u8 = 0x30;
const SIBT_EXT2: u8 = 0x31;
const SIBT_EXT4: u8 = 0x32;

const TRUEOS_BIOS_CATALOG_GUID: EfiGuid = EfiGuid {
    data1: 0x184d_a5de,
    data2: 0xfa77,
    data3: 0x4a1f,
    data4: [0xb4, 0x27, 0xd4, 0xdb, 0xfc, 0xe6, 0xd7, 0xf7],
};

#[repr(C)]
#[derive(Clone, Copy)]
struct CatalogHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    payload_crc32: u32,
    reserved: u32,
    payload_phys: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PayloadHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    section_entry_bytes: u16,
    reserved0: u16,
    section_count: u32,
    total_bytes: u32,
    capture_flags: u32,
    reserved1: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SectionEntry {
    kind: u32,
    flags: u32,
    offset: u32,
    length: u32,
    crc32: u32,
    reserved: u32,
    status: u64,
}

pub(crate) struct PackageListInfo {
    pub guid: EfiGuid,
    pub bytes: usize,
    pub packages: u32,
    pub form_packages: u32,
    pub string_packages: u32,
    pub package_types: BTreeMap<u8, u32>,
}

pub(crate) struct StringTable {
    pub list_index: usize,
    pub package_index: u32,
    pub language: String,
    pub language_name_id: u16,
    pub strings: BTreeMap<u16, String>,
    pub unsupported_blocks: u32,
    pub duplicate_misses: u32,
}

pub(crate) struct FormPackage {
    pub list_index: usize,
    pub package_index: u32,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct HiiDiagnostics {
    pub package_errors: u32,
    pub string_package_errors: u32,
    pub unsupported_string_blocks: u32,
    pub duplicate_misses: u32,
    pub messages: Vec<String>,
}

#[derive(Default)]
pub(crate) struct HiiStats {
    pub package_lists: u32,
    pub packages: u32,
    pub form_packages: u32,
    pub string_packages: u32,
    pub decoded_strings: u32,
    pub decoded_string_bytes: usize,
}

pub(crate) struct HiiIndex {
    pub source: &'static str,
    pub payload_bytes: usize,
    pub hii_bytes: usize,
    pub config_bytes: usize,
    pub config_route_headers: usize,
    pub package_lists: Vec<PackageListInfo>,
    pub string_tables: Vec<StringTable>,
    pub form_packages: Vec<FormPackage>,
    pub stats: HiiStats,
    pub diagnostics: HiiDiagnostics,
}

impl HiiIndex {
    pub fn resolve_string(&self, list_index: usize, string_id: u16) -> Option<&str> {
        if string_id == 0 {
            return None;
        }
        let mut best: Option<(u8, &str)> = None;
        for table in self
            .string_tables
            .iter()
            .filter(|table| table.list_index == list_index)
        {
            let Some(text) = table.strings.get(&string_id) else {
                continue;
            };
            let score = language_score(&table.language);
            if best.map(|(old, _)| score < old).unwrap_or(true) {
                best = Some((score, text.as_str()));
            }
        }
        best.map(|(_, text)| text)
    }

    pub fn preferred_language(&self, list_index: usize) -> Option<&str> {
        self.string_tables
            .iter()
            .filter(|table| table.list_index == list_index)
            .min_by_key(|table| language_score(&table.language))
            .map(|table| table.language.as_str())
    }

    pub fn languages_for(&self, list_index: usize) -> Vec<&str> {
        let mut languages = Vec::new();
        for table in self
            .string_tables
            .iter()
            .filter(|table| table.list_index == list_index)
        {
            if !languages
                .iter()
                .any(|language| *language == table.language.as_str())
            {
                languages.push(table.language.as_str());
            }
        }
        languages
    }
}

struct CapturedSections {
    source: &'static str,
    payload_bytes: usize,
    hii: Vec<u8>,
    config_bytes: usize,
    config_route_headers: usize,
}

pub(crate) fn load_hii_index() -> Result<HiiIndex, String> {
    parse_hii_database(load_capture_sections()?)
}

fn load_capture_sections() -> Result<CapturedSections, String> {
    if let Some(response) = crate::limine::trueos_hii_capture_response() {
        let len = usize::try_from(response.size)
            .map_err(|_| String::from("Limine HII payload length does not fit usize"))?;
        if len == 0 || len > MAX_PAYLOAD_BYTES {
            return Err(alloc::format!("Limine HII payload bytes={} outside bound", len));
        }
        let phys = crate::limine::try_as_phys_addr(response.address)
            .ok_or_else(|| String::from("Limine HII payload pointer is not mappable"))?;
        require_range(phys, len, "Limine HII payload")?;
        let mapping = crate::pci::mmio::map_mmio_region_exact(phys, len)
            .map_err(|error| alloc::format!("Limine HII payload map: {error:?}"))?;
        let payload = unsafe { core::slice::from_raw_parts(mapping.as_ptr(), len) };
        return extract_payload_sections("limine-experimental-hii-capture", payload);
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| alloc::format!("configuration tables: {error:?}"))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| String::from("HII handoff absent"))?;
    if entry.vendor_table == 0 {
        return Err(String::from("TRBIOS1 table pointer is zero"));
    }

    let catalog_phys = crate::limine::try_as_phys_addr(entry.vendor_table as u64)
        .ok_or_else(|| String::from("TRBIOS1 table pointer is not mappable"))?;
    require_range(catalog_phys, size_of::<CatalogHeader>(), "TRBIOS1 header")?;
    let mapping = crate::pci::mmio::map_limine_struct::<CatalogHeader>(catalog_phys)
        .map_err(|error| alloc::format!("TRBIOS1 map: {error:?}"))?;
    let catalog = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    if catalog.magic != CATALOG_MAGIC || catalog.version != VERSION {
        return Err(String::from("unsupported TRBIOS1 magic/version"));
    }
    if usize::from(catalog.header_bytes) < size_of::<CatalogHeader>() {
        return Err(String::from("TRBIOS1 header is too small"));
    }
    let payload_len = catalog.payload_bytes as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!("TRBIOS1 payload bytes={} outside bound", payload_len));
    }
    let payload_phys = crate::limine::try_as_phys_addr(catalog.payload_phys)
        .ok_or_else(|| String::from("TRBIOS1 payload pointer is not mappable"))?;
    require_range(payload_phys, payload_len, "TRBIOS1 payload")?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| alloc::format!("TRBIOS1 payload map: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    let computed = crc32fast::hash(payload);
    if computed != catalog.payload_crc32 {
        return Err(alloc::format!(
            "TRBIOS1 payload CRC mismatch stored=0x{:08X} computed=0x{:08X}",
            catalog.payload_crc32,
            computed
        ));
    }
    extract_payload_sections("firmware-scout-trbios1", payload)
}

fn extract_payload_sections(
    source: &'static str,
    payload: &[u8],
) -> Result<CapturedSections, String> {
    let header = read_struct::<PayloadHeader>(payload, 0)?;
    if header.magic != PAYLOAD_MAGIC || header.version != VERSION {
        return Err(String::from("unsupported TRPAY1 magic/version"));
    }
    if usize::from(header.header_bytes) < size_of::<PayloadHeader>()
        || usize::from(header.section_entry_bytes) < size_of::<SectionEntry>()
    {
        return Err(String::from("TRPAY1 header or entry size is too small"));
    }
    let count = header.section_count as usize;
    if count == 0 || count > MAX_SECTIONS || header.total_bytes as usize != payload.len() {
        return Err(String::from("TRPAY1 shape is invalid"));
    }
    let entry_bytes = header.section_entry_bytes as usize;
    let directory_end = usize::from(header.header_bytes)
        .checked_add(
            count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("TRPAY1 directory overflow"))?,
        )
        .ok_or_else(|| String::from("TRPAY1 directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("TRPAY1 directory is truncated"));
    }

    let mut ranges = Vec::with_capacity(count);
    let mut hii: Option<Vec<u8>> = None;
    let mut config_bytes = 0usize;
    let mut config_route_headers = 0usize;
    for index in 0..count {
        let entry_offset = usize::from(header.header_bytes)
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("TRPAY1 entry overflow"))?,
            )
            .ok_or_else(|| String::from("TRPAY1 entry overflow"))?;
        let entry = read_struct::<SectionEntry>(payload, entry_offset)?;
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| String::from("TRPAY1 section range overflow"))?;
        if entry.length == 0 || start < directory_end || end > payload.len() {
            return Err(alloc::format!("TRPAY1 section {} range is invalid", index));
        }
        if ranges
            .iter()
            .any(|&(left, right)| start < right && end > left)
        {
            return Err(alloc::format!("TRPAY1 section {} overlaps another", index));
        }
        ranges.push((start, end));
        let bytes = &payload[start..end];
        if crc32fast::hash(bytes) != entry.crc32 {
            return Err(alloc::format!("TRPAY1 section {} CRC mismatch", index));
        }
        match entry.kind {
            SEC_HII => {
                if hii.is_some() {
                    return Err(String::from("TRPAY1 contains multiple HII sections"));
                }
                hii = Some(bytes.to_vec());
            }
            SEC_CONFIG => {
                if bytes.len() >= 2
                    && bytes.len() % 2 == 0
                    && read_u16(bytes, bytes.len() - 2)? == 0
                {
                    config_bytes = bytes.len();
                    config_route_headers = count_utf16_ascii(bytes, b"GUID=");
                }
            }
            _ => {}
        }
    }

    let hii = hii.ok_or_else(|| String::from("TRPAY1 HII section is absent"))?;
    Ok(CapturedSections {
        source,
        payload_bytes: payload.len(),
        hii,
        config_bytes,
        config_route_headers,
    })
}

fn parse_hii_database(capture: CapturedSections) -> Result<HiiIndex, String> {
    const LIST_HEADER_BYTES: usize = 20;
    const PACKAGE_HEADER_BYTES: usize = 4;

    if capture.hii.len() < LIST_HEADER_BYTES {
        return Err(String::from("HII package-list export is too short"));
    }

    let mut index = HiiIndex {
        source: capture.source,
        payload_bytes: capture.payload_bytes,
        hii_bytes: capture.hii.len(),
        config_bytes: capture.config_bytes,
        config_route_headers: capture.config_route_headers,
        package_lists: Vec::new(),
        string_tables: Vec::new(),
        form_packages: Vec::new(),
        stats: HiiStats::default(),
        diagnostics: HiiDiagnostics::default(),
    };
    let mut list_offset = 0usize;
    let mut total_strings = 0usize;
    let mut total_string_bytes = 0usize;

    while list_offset < capture.hii.len() {
        if index.package_lists.len() >= MAX_PACKAGE_LISTS {
            return Err(String::from("HII package-list count exceeds bound"));
        }
        let list_header_end = checked_end(
            list_offset,
            LIST_HEADER_BYTES,
            capture.hii.len(),
            "HII list header",
        )?;
        let guid = read_guid(&capture.hii, list_offset)?;
        let list_len = read_u32(&capture.hii, list_offset + 16)? as usize;
        if list_len < LIST_HEADER_BYTES {
            return Err(String::from("HII package-list length is too small"));
        }
        let list_end = checked_end(
            list_offset,
            list_len,
            capture.hii.len(),
            "HII package list",
        )?;
        let list_index = index.package_lists.len();
        index.package_lists.push(PackageListInfo {
            guid,
            bytes: list_len,
            packages: 0,
            form_packages: 0,
            string_packages: 0,
            package_types: BTreeMap::new(),
        });
        index.stats.package_lists = index.stats.package_lists.saturating_add(1);

        let mut package_offset = list_header_end;
        let mut package_index = 0u32;
        while package_offset < list_end {
            if index.stats.packages as usize >= MAX_PACKAGES {
                return Err(String::from("HII package count exceeds bound"));
            }
            checked_end(
                package_offset,
                PACKAGE_HEADER_BYTES,
                list_end,
                "HII package header",
            )?;
            let raw = read_u32(&capture.hii, package_offset)?;
            let package_len = (raw & 0x00ff_ffff) as usize;
            let package_type = (raw >> 24) as u8;
            if package_len < PACKAGE_HEADER_BYTES {
                return Err(String::from("HII package length is too small"));
            }
            let package_end = checked_end(
                package_offset,
                package_len,
                list_end,
                "HII package",
            )?;
            let package = &capture.hii[package_offset..package_end];

            let list = &mut index.package_lists[list_index];
            list.packages = list.packages.saturating_add(1);
            *list.package_types.entry(package_type).or_insert(0) += 1;
            index.stats.packages = index.stats.packages.saturating_add(1);

            match package_type {
                HII_FORMS => {
                    list.form_packages = list.form_packages.saturating_add(1);
                    index.stats.form_packages = index.stats.form_packages.saturating_add(1);
                    if index.form_packages.len() >= MAX_FORM_PACKAGES {
                        return Err(String::from("HII form-package count exceeds bound"));
                    }
                    index.form_packages.push(FormPackage {
                        list_index,
                        package_index,
                        bytes: package.to_vec(),
                    });
                }
                HII_STRINGS => {
                    list.string_packages = list.string_packages.saturating_add(1);
                    index.stats.string_packages = index.stats.string_packages.saturating_add(1);
                    if index.string_tables.len() >= MAX_STRING_TABLES {
                        return Err(String::from("HII string-package count exceeds bound"));
                    }
                    match parse_string_package(
                        list_index,
                        package_index,
                        package,
                        &mut total_strings,
                        &mut total_string_bytes,
                    ) {
                        Ok(table) => {
                            index.diagnostics.unsupported_string_blocks = index
                                .diagnostics
                                .unsupported_string_blocks
                                .saturating_add(table.unsupported_blocks);
                            index.diagnostics.duplicate_misses = index
                                .diagnostics
                                .duplicate_misses
                                .saturating_add(table.duplicate_misses);
                            index.string_tables.push(table);
                        }
                        Err(error) => {
                            index.diagnostics.string_package_errors = index
                                .diagnostics
                                .string_package_errors
                                .saturating_add(1);
                            push_diagnostic(
                                &mut index.diagnostics,
                                alloc::format!(
                                    "list={} package={} string parse: {}",
                                    list_index,
                                    package_index,
                                    error
                                ),
                            );
                        }
                    }
                }
                _ => {}
            }

            package_offset = package_end;
            package_index = package_index.saturating_add(1);
        }
        if package_offset != list_end {
            return Err(String::from("HII package list ended off a package boundary"));
        }
        list_offset = list_end;
    }
    if list_offset != capture.hii.len() || index.package_lists.is_empty() {
        return Err(String::from("HII export ended off a package-list boundary"));
    }
    index.stats.decoded_strings = total_strings as u32;
    index.stats.decoded_string_bytes = total_string_bytes;
    Ok(index)
}

fn parse_string_package(
    list_index: usize,
    package_index: u32,
    package: &[u8],
    total_strings: &mut usize,
    total_string_bytes: &mut usize,
) -> Result<StringTable, String> {
    const STRING_HEADER_FIXED_BYTES: usize = 46;
    if package.len() <= STRING_HEADER_FIXED_BYTES {
        return Err(String::from("string package is shorter than its fixed header"));
    }
    let header_bytes = read_u32(package, 4)? as usize;
    let string_info_offset = read_u32(package, 8)? as usize;
    if header_bytes < STRING_HEADER_FIXED_BYTES + 1 || header_bytes > package.len() {
        return Err(String::from("string package header size is invalid"));
    }
    if string_info_offset < header_bytes || string_info_offset >= package.len() {
        return Err(String::from("string package block offset is invalid"));
    }
    let language_name_id = read_u16(package, 44)?;
    let language_end = package[STRING_HEADER_FIXED_BYTES..header_bytes]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| STRING_HEADER_FIXED_BYTES + relative)
        .ok_or_else(|| String::from("string package language is not terminated"))?;
    let language = decode_byte_string(&package[STRING_HEADER_FIXED_BYTES..language_end]);

    let mut table = StringTable {
        list_index,
        package_index,
        language: if language.is_empty() {
            String::from("und")
        } else {
            language
        },
        language_name_id,
        strings: BTreeMap::new(),
        unsupported_blocks: 0,
        duplicate_misses: 0,
    };
    let mut current_id = 1u32;
    let mut offset = string_info_offset;
    let mut ended = false;

    while offset < package.len() {
        let block = package[offset];
        match block {
            SIBT_END => {
                ended = true;
                break;
            }
            SIBT_STRING_SCSU => {
                let (text, next) = read_nul_byte_string(package, offset + 1)?;
                insert_string(
                    &mut table,
                    &mut current_id,
                    text,
                    total_strings,
                    total_string_bytes,
                )?;
                offset = next;
            }
            SIBT_STRING_SCSU_FONT => {
                checked_end(offset, 2, package.len(), "SCSU font string header")?;
                let (text, next) = read_nul_byte_string(package, offset + 2)?;
                insert_string(
                    &mut table,
                    &mut current_id,
                    text,
                    total_strings,
                    total_string_bytes,
                )?;
                offset = next;
            }
            SIBT_STRINGS_SCSU | SIBT_STRINGS_SCSU_FONT => {
                let count_offset = if block == SIBT_STRINGS_SCSU {
                    offset + 1
                } else {
                    offset + 2
                };
                let count = read_u16(package, count_offset)? as usize;
                let mut cursor = count_offset + 2;
                for _ in 0..count {
                    let (text, next) = read_nul_byte_string(package, cursor)?;
                    insert_string(
                        &mut table,
                        &mut current_id,
                        text,
                        total_strings,
                        total_string_bytes,
                    )?;
                    cursor = next;
                }
                offset = cursor;
            }
            SIBT_STRING_UCS2 => {
                let (text, next) = read_nul_utf16(package, offset + 1)?;
                insert_string(
                    &mut table,
                    &mut current_id,
                    text,
                    total_strings,
                    total_string_bytes,
                )?;
                offset = next;
            }
            SIBT_STRING_UCS2_FONT => {
                checked_end(offset, 2, package.len(), "UCS2 font string header")?;
                let (text, next) = read_nul_utf16(package, offset + 2)?;
                insert_string(
                    &mut table,
                    &mut current_id,
                    text,
                    total_strings,
                    total_string_bytes,
                )?;
                offset = next;
            }
            SIBT_STRINGS_UCS2 | SIBT_STRINGS_UCS2_FONT => {
                let count_offset = if block == SIBT_STRINGS_UCS2 {
                    offset + 1
                } else {
                    offset + 2
                };
                let count = read_u16(package, count_offset)? as usize;
                let mut cursor = count_offset + 2;
                for _ in 0..count {
                    let (text, next) = read_nul_utf16(package, cursor)?;
                    insert_string(
                        &mut table,
                        &mut current_id,
                        text,
                        total_strings,
                        total_string_bytes,
                    )?;
                    cursor = next;
                }
                offset = cursor;
            }
            SIBT_DUPLICATE => {
                let source = read_u16(package, offset + 1)?;
                let text = table.strings.get(&source).cloned();
                if let Some(text) = text {
                    insert_string(
                        &mut table,
                        &mut current_id,
                        text,
                        total_strings,
                        total_string_bytes,
                    )?;
                } else {
                    table.duplicate_misses = table.duplicate_misses.saturating_add(1);
                    advance_string_id(&mut current_id, 1)?;
                }
                offset = checked_end(offset, 3, package.len(), "duplicate string block")?;
            }
            SIBT_SKIP1 => {
                let count = *package
                    .get(offset + 1)
                    .ok_or_else(|| String::from("skip1 block is truncated"))?
                    as u32;
                advance_string_id(&mut current_id, count)?;
                offset = checked_end(offset, 2, package.len(), "skip1 string block")?;
            }
            SIBT_SKIP2 => {
                let count = read_u16(package, offset + 1)? as u32;
                advance_string_id(&mut current_id, count)?;
                offset = checked_end(offset, 3, package.len(), "skip2 string block")?;
            }
            SIBT_EXT1 => {
                let length = *package
                    .get(offset + 2)
                    .ok_or_else(|| String::from("ext1 string block is truncated"))?
                    as usize;
                if length < 3 {
                    return Err(String::from("ext1 string block length is too small"));
                }
                table.unsupported_blocks = table.unsupported_blocks.saturating_add(1);
                offset = checked_end(offset, length, package.len(), "ext1 string block")?;
            }
            SIBT_EXT2 => {
                let length = read_u16(package, offset + 2)? as usize;
                if length < 4 {
                    return Err(String::from("ext2 string block length is too small"));
                }
                table.unsupported_blocks = table.unsupported_blocks.saturating_add(1);
                offset = checked_end(offset, length, package.len(), "ext2 string block")?;
            }
            SIBT_EXT4 => {
                let length = read_u32(package, offset + 2)? as usize;
                if length < 6 {
                    return Err(String::from("ext4 string block length is too small"));
                }
                table.unsupported_blocks = table.unsupported_blocks.saturating_add(1);
                offset = checked_end(offset, length, package.len(), "ext4 string block")?;
            }
            _ => {
                return Err(alloc::format!(
                    "unsupported unbounded string block 0x{:02X} at offset 0x{:X}",
                    block,
                    offset
                ));
            }
        }
    }
    if !ended {
        return Err(String::from("string package has no END block"));
    }
    Ok(table)
}

fn insert_string(
    table: &mut StringTable,
    current_id: &mut u32,
    text: String,
    total_strings: &mut usize,
    total_string_bytes: &mut usize,
) -> Result<(), String> {
    if *current_id == 0 || *current_id > u16::MAX as u32 {
        return Err(String::from("string ID exceeds UEFI range"));
    }
    if *total_strings >= MAX_STRINGS {
        return Err(String::from("decoded string count exceeds bound"));
    }
    let new_total = total_string_bytes
        .checked_add(text.len())
        .ok_or_else(|| String::from("decoded string byte count overflow"))?;
    if new_total > MAX_DECODED_STRING_BYTES {
        return Err(String::from("decoded string bytes exceed bound"));
    }
    table.strings.insert(*current_id as u16, text);
    *total_strings += 1;
    *total_string_bytes = new_total;
    *current_id += 1;
    Ok(())
}

fn advance_string_id(current_id: &mut u32, count: u32) -> Result<(), String> {
    *current_id = current_id
        .checked_add(count)
        .ok_or_else(|| String::from("string ID overflow"))?;
    if *current_id > u16::MAX as u32 + 1 {
        return Err(String::from("string ID exceeds UEFI range"));
    }
    Ok(())
}

fn read_nul_byte_string(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    if start >= bytes.len() {
        return Err(String::from("byte string starts outside package"));
    }
    let relative = bytes[start..]
        .iter()
        .take(MAX_STRING_UNITS + 1)
        .position(|byte| *byte == 0)
        .ok_or_else(|| String::from("byte string is missing a bounded terminator"))?;
    if relative > MAX_STRING_UNITS {
        return Err(String::from("byte string exceeds length bound"));
    }
    let end = start + relative;
    Ok((decode_byte_string(&bytes[start..end]), end + 1))
}

fn read_nul_utf16(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    if start >= bytes.len() {
        return Err(String::from("UTF-16 string starts outside package"));
    }
    let mut units = Vec::new();
    let mut offset = start;
    while offset + 1 < bytes.len() {
        if units.len() > MAX_STRING_UNITS {
            return Err(String::from("UTF-16 string exceeds length bound"));
        }
        let unit = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;
        if unit == 0 {
            let mut text = String::new();
            for decoded in core::char::decode_utf16(units.into_iter()) {
                text.push(decoded.unwrap_or(core::char::REPLACEMENT_CHARACTER));
            }
            return Ok((text, offset));
        }
        units.push(unit);
    }
    Err(String::from("UTF-16 string is not terminated"))
}

fn decode_byte_string(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for byte in bytes {
        text.push(char::from(*byte));
    }
    text
}

fn language_score(language: &str) -> u8 {
    let lower = language.to_ascii_lowercase();
    if lower == "en-us" {
        0
    } else if lower.starts_with("en-") {
        1
    } else if lower == "en" {
        2
    } else {
        10
    }
}

fn push_diagnostic(diagnostics: &mut HiiDiagnostics, message: String) {
    diagnostics.package_errors = diagnostics.package_errors.saturating_add(1);
    if diagnostics.messages.len() < MAX_DIAGNOSTICS {
        diagnostics.messages.push(message);
    }
}

fn checked_end(start: usize, length: usize, limit: usize, label: &str) -> Result<usize, String> {
    let end = start
        .checked_add(length)
        .ok_or_else(|| alloc::format!("{} range overflow", label))?;
    if end > limit {
        Err(alloc::format!("{} is truncated", label))
    } else {
        Ok(end)
    }
}

fn read_struct<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    checked_end(offset, size_of::<T>(), bytes.len(), "structure")?;
    Ok(unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
}

fn read_guid(bytes: &[u8], offset: usize) -> Result<EfiGuid, String> {
    let end = checked_end(offset, 16, bytes.len(), "GUID")?;
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[offset..end]);
    Ok(EfiGuid::from_uefi_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| String::from("u32 crosses buffer"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let slice = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| String::from("u16 crosses buffer"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn count_utf16_ascii(bytes: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || bytes.len() % 2 != 0 {
        return 0;
    }
    let units = bytes.len() / 2;
    let mut found = 0usize;
    let mut index = 0usize;
    while index + needle.len() <= units {
        let matches = needle.iter().enumerate().all(|(part, expected)| {
            let offset = (index + part) * 2;
            bytes[offset] == *expected && bytes[offset + 1] == 0
        });
        if matches {
            found = found.saturating_add(1);
            index += needle.len();
        } else {
            index += 1;
        }
    }
    found
}

fn require_range(phys: u64, bytes: usize, label: &str) -> Result<(), String> {
    if crate::limine::memmap_contains_phys_range(phys, bytes) {
        Ok(())
    } else {
        Err(alloc::format!(
            "{} outside one Limine range phys=0x{:X} bytes={}",
            label,
            phys,
            bytes
        ))
    }
}

fn guid_eq(left: &EfiGuid, right: &EfiGuid) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

#[cfg(test)]
pub(crate) fn parse_hii_for_test(bytes: Vec<u8>) -> Result<HiiIndex, String> {
    parse_hii_database(CapturedSections {
        source: "test",
        payload_bytes: bytes.len(),
        hii: bytes,
        config_bytes: 0,
        config_route_headers: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_header(length: usize, package_type: u8) -> [u8; 4] {
        let raw = (length as u32 & 0x00ff_ffff) | ((package_type as u32) << 24);
        raw.to_le_bytes()
    }

    fn fixture() -> Vec<u8> {
        let mut strings = Vec::new();
        strings.extend_from_slice(&[0; 4]);
        strings.extend_from_slice(&52u32.to_le_bytes());
        strings.extend_from_slice(&52u32.to_le_bytes());
        strings.extend_from_slice(&[0; 32]);
        strings.extend_from_slice(&1u16.to_le_bytes());
        strings.extend_from_slice(b"en-US\0");
        strings.push(SIBT_STRING_SCSU);
        strings.extend_from_slice(b"RAID\0");
        strings.push(SIBT_STRING_UCS2);
        for unit in "USB".encode_utf16() {
            strings.extend_from_slice(&unit.to_le_bytes());
        }
        strings.extend_from_slice(&0u16.to_le_bytes());
        strings.push(SIBT_END);
        let strings_len = strings.len();
        strings[0..4].copy_from_slice(&package_header(strings_len, HII_STRINGS));

        let mut forms = vec![0; 4];
        let forms_len = forms.len();
        forms[0..4].copy_from_slice(&package_header(forms_len, HII_FORMS));

        let list_len = 20 + strings.len() + forms.len();
        let mut list = Vec::new();
        list.extend_from_slice(&[0x11; 16]);
        list.extend_from_slice(&(list_len as u32).to_le_bytes());
        list.extend_from_slice(&strings);
        list.extend_from_slice(&forms);
        list
    }

    #[test]
    fn indexes_package_lists_and_strings() {
        let index = parse_hii_for_test(fixture()).unwrap();
        assert_eq!(index.stats.package_lists, 1);
        assert_eq!(index.stats.form_packages, 1);
        assert_eq!(index.stats.string_packages, 1);
        assert_eq!(index.resolve_string(0, 1), Some("RAID"));
        assert_eq!(index.resolve_string(0, 2), Some("USB"));
        assert_eq!(index.preferred_language(0), Some("en-US"));
    }
}
