use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

use spin::Mutex;

use crate::efi::EfiGuid;

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 8;
const MAX_PACKAGE_LISTS: usize = 4_096;
const MAX_PACKAGES: usize = 65_536;
const MAX_STRING_BLOCKS: usize = 262_144;
const MAX_STRING_UNITS: usize = 4_096;
const MAX_DECODED_STRING_BYTES: usize = 8 * 1024 * 1024;

const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const STATUS_MAGIC: [u8; 8] = *b"TRSTAT1\0";
const VERSION: u16 = 1;

const SEC_STATUS: u32 = 1;
const SEC_HII: u32 = 2;

pub(crate) const HII_PACKAGE_FORMS: u8 = 0x02;
pub(crate) const HII_PACKAGE_STRINGS: u8 = 0x04;

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

static CATALOGUE_CACHE: Mutex<Option<Result<BiosCatalogue, String>>> = Mutex::new(None);

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

#[repr(C)]
#[derive(Clone, Copy)]
struct CaptureStatus {
    magic: [u8; 8],
    version: u16,
    bytes: u16,
    flags: u32,
    hii_database_locate_status: u64,
    hii_export_query_status: u64,
    hii_export_status: u64,
    hii_parse_status: u64,
    hii_bytes: u32,
    package_lists: u32,
    form_packages: u32,
    string_packages: u32,
    config_routing_locate_status: u64,
    config_export_status: u64,
    config_bytes: u32,
    reserved: u32,
}

pub(crate) struct BiosCatalogue {
    pub source: &'static str,
    pub payload_bytes: u32,
    pub hii_bytes: u32,
    pub capture_flags: u32,
    pub section_count: u32,
    pub status_receipt_valid: bool,
    pub package_count: u32,
    pub form_package_count: u32,
    pub string_package_count: u32,
    pub malformed_packages: u32,
    pub unknown_sections: u32,
    pub package_lists: Vec<PackageListRecord>,
    pub string_stats: StringStats,
}

pub(crate) struct PackageListRecord {
    pub index: usize,
    pub guid: EfiGuid,
    pub offset: u32,
    pub bytes: u32,
    pub packages: Vec<PackageRecord>,
    pub strings: Vec<StringTable>,
    pub forms: Vec<FormPackage>,
}

pub(crate) struct PackageRecord {
    pub index: usize,
    pub package_type: u8,
    pub offset: u32,
    pub bytes: u32,
    pub decoded: bool,
}

pub(crate) struct StringTable {
    pub package_index: usize,
    pub language: String,
    pub language_name_id: u16,
    pub language_name: Option<String>,
    pub strings: BTreeMap<u16, String>,
}

pub(crate) struct FormPackage {
    pub package_list_index: usize,
    pub package_index: usize,
    pub package_offset: u32,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct StringStats {
    pub decoded_strings: u32,
    pub ucs2_strings: u32,
    pub scsu_ascii_strings: u32,
    pub duplicate_strings: u32,
    pub unresolved_duplicates: u32,
    pub skipped_ids: u32,
    pub extension_blocks: u32,
    pub opaque_blocks: u32,
    pub opaque_strings: u32,
    pub truncated_strings: u32,
}

#[derive(Default)]
struct CaptureCounts {
    package_lists: Option<u32>,
    form_packages: Option<u32>,
    string_packages: Option<u32>,
    receipt_valid: bool,
}

struct ParsedStringPackage {
    table: StringTable,
    stats: StringStats,
}

pub(crate) fn with_catalogue<R>(
    f: impl FnOnce(&BiosCatalogue) -> R,
) -> Result<R, String> {
    let mut cache = CATALOGUE_CACHE.lock();
    if cache.is_none() {
        *cache = Some(load_catalogue());
    }
    match cache.as_ref().expect("catalogue cache initialized") {
        Ok(catalogue) => Ok(f(catalogue)),
        Err(error) => Err(error.clone()),
    }
}

impl BiosCatalogue {
    pub(crate) fn resolve_string(&self, package_list_index: usize, id: u16) -> Option<&str> {
        if id == 0 {
            return None;
        }
        let list = self.package_lists.get(package_list_index)?;
        for table in &list.strings {
            if is_preferred_english(&table.language) {
                if let Some(text) = table.strings.get(&id) {
                    return Some(text.as_str());
                }
            }
        }
        for table in &list.strings {
            if let Some(text) = table.strings.get(&id) {
                return Some(text.as_str());
            }
        }
        None
    }
}

pub(crate) const fn package_type_name(package_type: u8) -> &'static str {
    match package_type {
        0x00 => "all",
        0x01 => "guid",
        HII_PACKAGE_FORMS => "forms",
        HII_PACKAGE_STRINGS => "strings",
        0x05 => "fonts",
        0x06 => "images",
        0x07 => "simple-fonts",
        0x08 => "device-path",
        0x09 => "keyboard-layout",
        0x0a => "animations",
        0xdf => "end",
        0xe0..=0xff => "system",
        _ => "unknown",
    }
}

fn load_catalogue() -> Result<BiosCatalogue, String> {
    if let Some(payload) = limine_payload()? {
        return parse_payload(
            "limine-experimental-hii-capture",
            payload,
            None,
        );
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| alloc::format!("configuration tables: {error:?}"))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| String::from("TRPAY1 handoff absent"))?;
    if entry.vendor_table == 0 {
        return Err(String::from("TRBIOS1 table pointer is zero"));
    }

    let catalog_phys = crate::limine::try_as_phys_addr(entry.vendor_table as u64)
        .ok_or_else(|| String::from("TRBIOS1 table pointer is not mappable"))?;
    require_range(catalog_phys, size_of::<CatalogHeader>(), "catalog header")?;
    let mapping = crate::pci::mmio::map_limine_struct::<CatalogHeader>(catalog_phys)
        .map_err(|error| alloc::format!("catalog map: {error:?}"))?;
    let catalog = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    if catalog.magic != CATALOG_MAGIC || catalog.version != VERSION {
        return Err(alloc::format!(
            "unsupported catalog magic_ok={} version={}",
            yes_no(catalog.magic == CATALOG_MAGIC),
            catalog.version
        ));
    }
    if usize::from(catalog.header_bytes) < size_of::<CatalogHeader>() {
        return Err(String::from("catalog header_bytes is too small"));
    }
    let payload_len = catalog.payload_bytes as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!("payload bytes={} outside bound", payload_len));
    }
    let payload_phys = crate::limine::try_as_phys_addr(catalog.payload_phys)
        .ok_or_else(|| String::from("catalog payload pointer is not mappable"))?;
    require_range(payload_phys, payload_len, "catalog payload")?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| alloc::format!("payload map: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    let computed = crc32fast::hash(payload);
    if computed != catalog.payload_crc32 {
        return Err(alloc::format!(
            "payload CRC mismatch stored=0x{:08X} computed=0x{:08X}",
            catalog.payload_crc32,
            computed
        ));
    }

    parse_payload(
        "firmware-scout-trbios1",
        payload,
        Some(catalog.package_list_count),
    )
}

fn limine_payload() -> Result<Option<&'static [u8]>, String> {
    let Some(response) = crate::limine::trueos_hii_capture_response() else {
        return Ok(None);
    };
    let len = usize::try_from(response.size)
        .map_err(|_| String::from("limine HII payload length is not representable"))?;
    if len == 0 || len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!("limine HII payload bytes={} outside bound", len));
    }
    let phys = crate::limine::try_as_phys_addr(response.address)
        .ok_or_else(|| String::from("limine HII payload pointer is not mappable"))?;
    require_range(phys, len, "limine HII payload")?;
    let mapping = crate::pci::mmio::map_mmio_region_exact(phys, len)
        .map_err(|error| alloc::format!("limine HII payload map: {error:?}"))?;
    Ok(Some(unsafe {
        core::slice::from_raw_parts(mapping.as_ptr(), len)
    }))
}

fn parse_payload(
    source: &'static str,
    payload: &[u8],
    outer_package_list_count: Option<u32>,
) -> Result<BiosCatalogue, String> {
    let header = read_struct::<PayloadHeader>(payload, 0)?;
    if header.magic != PAYLOAD_MAGIC || header.version != VERSION {
        return Err(alloc::format!(
            "unsupported payload magic_ok={} version={}",
            yes_no(header.magic == PAYLOAD_MAGIC),
            header.version
        ));
    }
    if usize::from(header.header_bytes) < size_of::<PayloadHeader>()
        || usize::from(header.section_entry_bytes) < size_of::<SectionEntry>()
    {
        return Err(String::from("TRPAY1 header or entry size is too small"));
    }
    let section_count = header.section_count as usize;
    if section_count == 0
        || section_count > MAX_SECTIONS
        || header.total_bytes as usize != payload.len()
    {
        return Err(alloc::format!(
            "TRPAY1 shape invalid sections={} total_bytes={}",
            section_count,
            header.total_bytes
        ));
    }
    let entry_bytes = header.section_entry_bytes as usize;
    let directory_end = usize::from(header.header_bytes)
        .checked_add(
            section_count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("section directory overflow"))?,
        )
        .ok_or_else(|| String::from("section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("section directory is truncated"));
    }

    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(section_count);
    let mut hii_range = None;
    let mut counts = CaptureCounts::default();
    let mut unknown_sections = 0u32;
    for index in 0..section_count {
        let entry_offset = usize::from(header.header_bytes)
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("section entry overflow"))?,
            )
            .ok_or_else(|| String::from("section entry overflow"))?;
        let entry = read_struct::<SectionEntry>(payload, entry_offset)?;
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| String::from("section range overflow"))?;
        if entry.length == 0 || start < directory_end || end > payload.len() {
            return Err(alloc::format!("section {} range invalid", index));
        }
        if ranges
            .iter()
            .any(|&(left, right)| start < right && end > left)
        {
            return Err(alloc::format!("section {} overlaps another", index));
        }
        ranges.push((start, end));
        let section = &payload[start..end];
        let computed = crc32fast::hash(section);
        if computed != entry.crc32 {
            return Err(alloc::format!(
                "section {} CRC mismatch stored=0x{:08X} computed=0x{:08X}",
                index,
                entry.crc32,
                computed
            ));
        }
        match entry.kind {
            SEC_STATUS => parse_capture_counts(section, &mut counts),
            SEC_HII => {
                if hii_range.replace((start, end)).is_some() {
                    return Err(String::from("multiple HII sections in TRPAY1"));
                }
            }
            _ => unknown_sections = unknown_sections.saturating_add(1),
        }
    }

    let (hii_start, hii_end) = hii_range.ok_or_else(|| String::from("HII section absent"))?;
    let expected_lists = outer_package_list_count.or(counts.package_lists);
    let mut catalogue = parse_hii_export(
        source,
        &payload[hii_start..hii_end],
        header.capture_flags,
        section_count as u32,
        counts.receipt_valid,
        unknown_sections,
    )?;
    catalogue.payload_bytes = payload.len() as u32;

    if let Some(expected) = expected_lists {
        if catalogue.package_lists.len() as u32 != expected {
            return Err(alloc::format!(
                "package-list count mismatch expected={} parsed={}",
                expected,
                catalogue.package_lists.len()
            ));
        }
    }
    if let Some(expected) = counts.form_packages {
        if catalogue.form_package_count != expected {
            return Err(alloc::format!(
                "form-package count mismatch expected={} parsed={}",
                expected,
                catalogue.form_package_count
            ));
        }
    }
    if let Some(expected) = counts.string_packages {
        if catalogue.string_package_count != expected {
            return Err(alloc::format!(
                "string-package count mismatch expected={} parsed={}",
                expected,
                catalogue.string_package_count
            ));
        }
    }
    Ok(catalogue)
}

fn parse_capture_counts(bytes: &[u8], counts: &mut CaptureCounts) {
    let Ok(status) = read_struct::<CaptureStatus>(bytes, 0) else {
        return;
    };
    if status.magic != STATUS_MAGIC
        || status.version != VERSION
        || usize::from(status.bytes) < size_of::<CaptureStatus>()
        || usize::from(status.bytes) > bytes.len()
    {
        return;
    }
    counts.receipt_valid = true;
    counts.package_lists = Some(status.package_lists);
    counts.form_packages = Some(status.form_packages);
    counts.string_packages = Some(status.string_packages);
}

fn parse_hii_export(
    source: &'static str,
    bytes: &[u8],
    capture_flags: u32,
    section_count: u32,
    status_receipt_valid: bool,
    unknown_sections: u32,
) -> Result<BiosCatalogue, String> {
    const LIST_HEADER_BYTES: usize = 20;
    const PACKAGE_HEADER_BYTES: usize = 4;

    if bytes.len() < LIST_HEADER_BYTES {
        return Err(String::from("short HII package-list export"));
    }
    let mut package_lists = Vec::new();
    let mut package_count = 0u32;
    let mut form_package_count = 0u32;
    let mut string_package_count = 0u32;
    let mut malformed_packages = 0u32;
    let mut string_stats = StringStats::default();
    let mut decoded_budget = MAX_DECODED_STRING_BYTES;
    let mut list_offset = 0usize;

    while list_offset < bytes.len() {
        if package_lists.len() >= MAX_PACKAGE_LISTS {
            return Err(String::from("HII package-list count exceeds bound"));
        }
        let list_header_end = list_offset
            .checked_add(LIST_HEADER_BYTES)
            .ok_or_else(|| String::from("package-list header overflow"))?;
        if list_header_end > bytes.len() {
            return Err(String::from("truncated package-list header"));
        }
        let list_len = read_u32(bytes, list_offset + 16)? as usize;
        let list_end = list_offset
            .checked_add(list_len)
            .ok_or_else(|| String::from("package-list range overflow"))?;
        if list_len < LIST_HEADER_BYTES || list_end > bytes.len() {
            return Err(alloc::format!(
                "invalid package-list length offset={} bytes={}",
                list_offset,
                list_len
            ));
        }
        let mut guid_bytes = [0u8; 16];
        guid_bytes.copy_from_slice(&bytes[list_offset..list_offset + 16]);
        let mut list = PackageListRecord {
            index: package_lists.len(),
            guid: EfiGuid::from_uefi_bytes(guid_bytes),
            offset: list_offset as u32,
            bytes: list_len as u32,
            packages: Vec::new(),
            strings: Vec::new(),
            forms: Vec::new(),
        };

        let mut package_offset = list_header_end;
        while package_offset < list_end {
            if package_count as usize >= MAX_PACKAGES {
                return Err(String::from("HII package count exceeds bound"));
            }
            if package_offset
                .checked_add(PACKAGE_HEADER_BYTES)
                .unwrap_or(usize::MAX)
                > list_end
            {
                return Err(String::from("truncated HII package header"));
            }
            let raw = read_u32(bytes, package_offset)?;
            let package_len = (raw & 0x00ff_ffff) as usize;
            let package_type = (raw >> 24) as u8;
            let package_end = package_offset
                .checked_add(package_len)
                .ok_or_else(|| String::from("HII package range overflow"))?;
            if package_len < PACKAGE_HEADER_BYTES || package_end > list_end {
                return Err(alloc::format!(
                    "invalid HII package length list={} offset={} bytes={}",
                    list.index,
                    package_offset,
                    package_len
                ));
            }

            let package_index = list.packages.len();
            let package = &bytes[package_offset..package_end];
            let mut decoded = false;
            match package_type {
                HII_PACKAGE_FORMS => {
                    form_package_count = form_package_count.saturating_add(1);
                    list.forms.push(FormPackage {
                        package_list_index: list.index,
                        package_index,
                        package_offset: package_offset as u32,
                        bytes: package[PACKAGE_HEADER_BYTES..].to_vec(),
                    });
                    decoded = true;
                }
                HII_PACKAGE_STRINGS => {
                    string_package_count = string_package_count.saturating_add(1);
                    match parse_string_package(package, package_index, &mut decoded_budget) {
                        Ok(mut parsed) => {
                            if parsed.table.language_name_id != 0 {
                                parsed.table.language_name = parsed
                                    .table
                                    .strings
                                    .get(&parsed.table.language_name_id)
                                    .cloned();
                            }
                            merge_string_stats(&mut string_stats, &parsed.stats);
                            list.strings.push(parsed.table);
                            decoded = true;
                        }
                        Err(_) => {
                            malformed_packages = malformed_packages.saturating_add(1);
                        }
                    }
                }
                _ => {}
            }
            list.packages.push(PackageRecord {
                index: package_index,
                package_type,
                offset: package_offset as u32,
                bytes: package_len as u32,
                decoded,
            });
            package_count = package_count.saturating_add(1);
            package_offset = package_end;
        }
        if package_offset != list_end {
            return Err(String::from("package list ended off a package boundary"));
        }
        package_lists.push(list);
        list_offset = list_end;
    }
    if list_offset != bytes.len() || package_lists.is_empty() {
        return Err(String::from("HII export ended off a package-list boundary"));
    }

    Ok(BiosCatalogue {
        source,
        payload_bytes: 0,
        hii_bytes: bytes.len() as u32,
        capture_flags,
        section_count,
        status_receipt_valid,
        package_count,
        form_package_count,
        string_package_count,
        malformed_packages,
        unknown_sections,
        package_lists,
        string_stats,
    })
}

fn parse_string_package(
    package: &[u8],
    package_index: usize,
    decoded_budget: &mut usize,
) -> Result<ParsedStringPackage, String> {
    const LANGUAGE_OFFSET: usize = 46;
    if package.len() <= LANGUAGE_OFFSET {
        return Err(String::from("short string package header"));
    }
    let header_size = read_u32(package, 4)? as usize;
    let string_info_offset = read_u32(package, 8)? as usize;
    if header_size <= LANGUAGE_OFFSET
        || header_size > package.len()
        || string_info_offset < header_size
        || string_info_offset >= package.len()
    {
        return Err(String::from("invalid string package header offsets"));
    }
    let language_end = package[LANGUAGE_OFFSET..header_size]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| LANGUAGE_OFFSET + relative)
        .ok_or_else(|| String::from("string package language is not terminated"))?;
    let language = core::str::from_utf8(&package[LANGUAGE_OFFSET..language_end])
        .map_err(|_| String::from("string package language is not ASCII/UTF-8"))?;
    if language.is_empty() {
        return Err(String::from("string package language is empty"));
    }

    let mut table = StringTable {
        package_index,
        language: String::from(language),
        language_name_id: read_u16(package, 44)?,
        language_name: None,
        strings: BTreeMap::new(),
    };
    let mut stats = StringStats::default();
    let mut current_id = 1u32;
    let mut offset = string_info_offset;
    let mut block_count = 0usize;
    let mut saw_end = false;

    while offset < package.len() {
        block_count = block_count.saturating_add(1);
        if block_count > MAX_STRING_BLOCKS {
            return Err(String::from("string block count exceeds bound"));
        }
        let block_type = package[offset];
        match block_type {
            SIBT_END => {
                saw_end = true;
                break;
            }
            SIBT_STRING_SCSU | SIBT_STRING_SCSU_FONT => {
                let text_offset = offset
                    .checked_add(if block_type == SIBT_STRING_SCSU { 1 } else { 2 })
                    .ok_or_else(|| String::from("SCSU string offset overflow"))?;
                let (text, next) = read_nul_bytes(package, text_offset)?;
                append_scsu_string(
                    &mut table,
                    &mut stats,
                    &mut current_id,
                    text,
                    decoded_budget,
                )?;
                offset = next;
            }
            SIBT_STRINGS_SCSU | SIBT_STRINGS_SCSU_FONT => {
                let font_bytes = if block_type == SIBT_STRINGS_SCSU_FONT { 1 } else { 0 };
                let count_offset = offset
                    .checked_add(1 + font_bytes)
                    .ok_or_else(|| String::from("SCSU strings offset overflow"))?;
                let count = read_u16(package, count_offset)? as usize;
                let mut text_offset = count_offset + 2;
                for _ in 0..count {
                    let (text, next) = read_nul_bytes(package, text_offset)?;
                    append_scsu_string(
                        &mut table,
                        &mut stats,
                        &mut current_id,
                        text,
                        decoded_budget,
                    )?;
                    text_offset = next;
                }
                offset = text_offset;
            }
            SIBT_STRING_UCS2 | SIBT_STRING_UCS2_FONT => {
                let text_offset = offset
                    .checked_add(if block_type == SIBT_STRING_UCS2 { 1 } else { 2 })
                    .ok_or_else(|| String::from("UCS-2 string offset overflow"))?;
                let (start, end, next) = read_nul_utf16_bounds(package, text_offset)?;
                append_ucs2_string(
                    &mut table,
                    &mut stats,
                    &mut current_id,
                    &package[start..end],
                    decoded_budget,
                )?;
                offset = next;
            }
            SIBT_STRINGS_UCS2 | SIBT_STRINGS_UCS2_FONT => {
                let font_bytes = if block_type == SIBT_STRINGS_UCS2_FONT { 1 } else { 0 };
                let count_offset = offset
                    .checked_add(1 + font_bytes)
                    .ok_or_else(|| String::from("UCS-2 strings offset overflow"))?;
                let count = read_u16(package, count_offset)? as usize;
                let mut text_offset = count_offset + 2;
                for _ in 0..count {
                    let (start, end, next) = read_nul_utf16_bounds(package, text_offset)?;
                    append_ucs2_string(
                        &mut table,
                        &mut stats,
                        &mut current_id,
                        &package[start..end],
                        decoded_budget,
                    )?;
                    text_offset = next;
                }
                offset = text_offset;
            }
            SIBT_DUPLICATE => {
                let source_id = read_u16(package, offset + 1)?;
                let id = take_string_id(&mut current_id)?;
                stats.duplicate_strings = stats.duplicate_strings.saturating_add(1);
                if let Some(text) = table.strings.get(&source_id).cloned() {
                    if reserve_decoded_bytes(decoded_budget, text.len()) {
                        table.strings.insert(id, text);
                        stats.decoded_strings = stats.decoded_strings.saturating_add(1);
                    } else {
                        stats.truncated_strings = stats.truncated_strings.saturating_add(1);
                    }
                } else {
                    stats.unresolved_duplicates = stats.unresolved_duplicates.saturating_add(1);
                }
                offset = offset
                    .checked_add(3)
                    .ok_or_else(|| String::from("duplicate block overflow"))?;
                if offset > package.len() {
                    return Err(String::from("truncated duplicate block"));
                }
            }
            SIBT_SKIP1 => {
                let skip = *package
                    .get(offset + 1)
                    .ok_or_else(|| String::from("truncated skip1 block"))?
                    as u32;
                advance_string_ids(&mut current_id, skip)?;
                stats.skipped_ids = stats.skipped_ids.saturating_add(skip);
                offset += 2;
            }
            SIBT_SKIP2 => {
                let skip = read_u16(package, offset + 1)? as u32;
                advance_string_ids(&mut current_id, skip)?;
                stats.skipped_ids = stats.skipped_ids.saturating_add(skip);
                offset += 3;
            }
            SIBT_EXT1 | SIBT_EXT2 | SIBT_EXT4 => {
                let (minimum, length) = match block_type {
                    SIBT_EXT1 => (3usize, *package.get(offset + 2).ok_or_else(|| {
                        String::from("truncated extension-1 block")
                    })? as usize),
                    SIBT_EXT2 => (4usize, read_u16(package, offset + 2)? as usize),
                    _ => (6usize, read_u32(package, offset + 2)? as usize),
                };
                let end = offset
                    .checked_add(length)
                    .ok_or_else(|| String::from("extension block overflow"))?;
                if length < minimum || end > package.len() {
                    return Err(String::from("invalid extension block length"));
                }
                stats.extension_blocks = stats.extension_blocks.saturating_add(1);
                offset = end;
            }
            _ => {
                stats.opaque_blocks = stats.opaque_blocks.saturating_add(1);
                break;
            }
        }
    }
    if !saw_end && stats.opaque_blocks == 0 {
        return Err(String::from("string package has no END block"));
    }

    Ok(ParsedStringPackage { table, stats })
}

fn append_scsu_string(
    table: &mut StringTable,
    stats: &mut StringStats,
    current_id: &mut u32,
    bytes: &[u8],
    decoded_budget: &mut usize,
) -> Result<(), String> {
    let id = take_string_id(current_id)?;
    if bytes.len() > MAX_STRING_UNITS {
        stats.truncated_strings = stats.truncated_strings.saturating_add(1);
        return Ok(());
    }
    let plain_ascii = bytes
        .iter()
        .all(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0x20..=0x7e));
    if !plain_ascii {
        stats.opaque_strings = stats.opaque_strings.saturating_add(1);
        return Ok(());
    }
    if !reserve_decoded_bytes(decoded_budget, bytes.len()) {
        stats.truncated_strings = stats.truncated_strings.saturating_add(1);
        return Ok(());
    }
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| String::from("plain SCSU string was not valid UTF-8"))?;
    table.strings.insert(id, text);
    stats.decoded_strings = stats.decoded_strings.saturating_add(1);
    stats.scsu_ascii_strings = stats.scsu_ascii_strings.saturating_add(1);
    Ok(())
}

fn append_ucs2_string(
    table: &mut StringTable,
    stats: &mut StringStats,
    current_id: &mut u32,
    bytes: &[u8],
    decoded_budget: &mut usize,
) -> Result<(), String> {
    let id = take_string_id(current_id)?;
    let units = bytes.len() / 2;
    if units > MAX_STRING_UNITS {
        stats.truncated_strings = stats.truncated_strings.saturating_add(1);
        return Ok(());
    }
    let mut utf16 = Vec::with_capacity(units);
    for pair in bytes.chunks_exact(2) {
        utf16.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    let Ok(text) = String::from_utf16(&utf16) else {
        stats.opaque_strings = stats.opaque_strings.saturating_add(1);
        return Ok(());
    };
    if !reserve_decoded_bytes(decoded_budget, text.len()) {
        stats.truncated_strings = stats.truncated_strings.saturating_add(1);
        return Ok(());
    }
    table.strings.insert(id, text);
    stats.decoded_strings = stats.decoded_strings.saturating_add(1);
    stats.ucs2_strings = stats.ucs2_strings.saturating_add(1);
    Ok(())
}

fn read_nul_bytes(bytes: &[u8], start: usize) -> Result<(&[u8], usize), String> {
    if start >= bytes.len() {
        return Err(String::from("string text starts outside package"));
    }
    let relative = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| String::from("SCSU string is not terminated"))?;
    let end = start + relative;
    Ok((&bytes[start..end], end + 1))
}

fn read_nul_utf16_bounds(
    bytes: &[u8],
    start: usize,
) -> Result<(usize, usize, usize), String> {
    if start >= bytes.len() {
        return Err(String::from("UCS-2 string starts outside package"));
    }
    let mut offset = start;
    while offset
        .checked_add(2)
        .ok_or_else(|| String::from("UCS-2 string range overflow"))?
        <= bytes.len()
    {
        if read_u16(bytes, offset)? == 0 {
            return Ok((start, offset, offset + 2));
        }
        offset += 2;
    }
    Err(String::from("UCS-2 string is not terminated"))
}

fn take_string_id(current_id: &mut u32) -> Result<u16, String> {
    if *current_id == 0 || *current_id > u16::MAX as u32 {
        return Err(String::from("string ID exceeds 16-bit range"));
    }
    let id = *current_id as u16;
    *current_id = current_id
        .checked_add(1)
        .ok_or_else(|| String::from("string ID overflow"))?;
    Ok(id)
}

fn advance_string_ids(current_id: &mut u32, count: u32) -> Result<(), String> {
    *current_id = current_id
        .checked_add(count)
        .ok_or_else(|| String::from("string skip overflow"))?;
    if *current_id > u16::MAX as u32 + 1 {
        return Err(String::from("string skip exceeds 16-bit range"));
    }
    Ok(())
}

fn reserve_decoded_bytes(budget: &mut usize, bytes: usize) -> bool {
    if bytes > *budget {
        false
    } else {
        *budget -= bytes;
        true
    }
}

fn merge_string_stats(total: &mut StringStats, add: &StringStats) {
    total.decoded_strings = total.decoded_strings.saturating_add(add.decoded_strings);
    total.ucs2_strings = total.ucs2_strings.saturating_add(add.ucs2_strings);
    total.scsu_ascii_strings = total
        .scsu_ascii_strings
        .saturating_add(add.scsu_ascii_strings);
    total.duplicate_strings = total
        .duplicate_strings
        .saturating_add(add.duplicate_strings);
    total.unresolved_duplicates = total
        .unresolved_duplicates
        .saturating_add(add.unresolved_duplicates);
    total.skipped_ids = total.skipped_ids.saturating_add(add.skipped_ids);
    total.extension_blocks = total
        .extension_blocks
        .saturating_add(add.extension_blocks);
    total.opaque_blocks = total.opaque_blocks.saturating_add(add.opaque_blocks);
    total.opaque_strings = total.opaque_strings.saturating_add(add.opaque_strings);
    total.truncated_strings = total
        .truncated_strings
        .saturating_add(add.truncated_strings);
}

fn is_preferred_english(language: &str) -> bool {
    language.split(';').any(|part| {
        part.eq_ignore_ascii_case("en-US")
            || part.eq_ignore_ascii_case("en")
            || part
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("en-"))
    })
}

fn read_struct<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    let end = offset
        .checked_add(size_of::<T>())
        .ok_or_else(|| String::from("structure range overflow"))?;
    if end > bytes.len() {
        return Err(String::from("structure is truncated"));
    }
    Ok(unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
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

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
