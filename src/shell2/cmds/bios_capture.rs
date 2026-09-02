use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::mem::size_of;

use spin::Mutex;

use crate::efi::EfiGuid;
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 8;
const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const STATUS_MAGIC: [u8; 8] = *b"TRSTAT1\0";
const VERSION: u16 = 1;

const SEC_STATUS: u32 = 1;
const SEC_HII: u32 = 2;
const SEC_CONFIG: u32 = 3;
const HII_FORMS: u8 = 0x02;
const HII_STRINGS: u8 = 0x04;

const TRUEOS_BIOS_CATALOG_GUID: EfiGuid = EfiGuid {
    data1: 0x184d_a5de,
    data2: 0xfa77,
    data3: 0x4a1f,
    data4: [0xb4, 0x27, 0xd4, 0xdb, 0xfc, 0xe6, 0xd7, 0xf7],
};

static REPORT_CACHE: Mutex<Option<String>> = Mutex::new(None);

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

#[derive(Clone, Copy)]
struct HiiSummary {
    lists: u32,
    packages: u32,
    forms: u32,
    form_bytes: u64,
    strings: u32,
    string_bytes: u64,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (None, None) | (Some("status" | "sections"), None) => emit(io, &cached_report()),
        (Some("help" | "-h" | "--help"), None) => usage(io),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "bios: usage `bios capture [status|sections]`");
}

fn emit(io: &'static dyn ShellBackend2, text: &str) {
    for line in text.lines() {
        print_shell_line(io, line.trim_end_matches('\r'));
    }
}

fn cached_report() -> String {
    let mut cache = REPORT_CACHE.lock();
    if let Some(report) = cache.as_ref() {
        return report.clone();
    }
    let report = build_report();
    *cache = Some(report.clone());
    report
}

fn build_report() -> String {
    let mut out = String::new();
    writeln!(out, "=== TRUEOS Preboot HII Capture ===").unwrap();
    writeln!(
        out,
        "policy=read-only decoder; no Runtime Service, HII protocol, variable, storage, USB, capsule, reset, or flash write"
    )
    .unwrap();
    writeln!(
        out,
        "privacy=config content=redacted; output is bounded metadata/status/count/CRC only"
    )
    .unwrap();
    if let Err(error) = append_catalog(&mut out) {
        writeln!(out, "capture_state=unavailable detail=\"{}\"", error).unwrap();
        writeln!(out, "capture_ready_for_ifr_parser=no").unwrap();
    }
    out
}

/// TRPAY1 payload handed off directly by a patched Limine (see
/// `t4ce/Limine`'s `common/lib/trueos_hii.c`), if the kernel's request was
/// answered. Bounded and mapped the same way as the FirmwareScout/TRBIOS1
/// payload below, just without an outer catalog wrapper to unwrap first.
fn trueos_hii_payload() -> Option<&'static [u8]> {
    let response = crate::limine::trueos_hii_capture_response()?;
    let len = usize::try_from(response.size).ok()?;
    if len == 0 || len > MAX_PAYLOAD_BYTES {
        return None;
    }
    let phys = crate::limine::try_as_phys_addr(response.address)?;
    require_range(phys, len, "limine hii capture payload").ok()?;
    let mapping = crate::pci::mmio::map_mmio_region_exact(phys, len).ok()?;
    Some(unsafe { core::slice::from_raw_parts(mapping.as_ptr(), len) })
}

fn append_catalog(out: &mut String) -> Result<(), String> {
    if let Some(payload) = trueos_hii_payload() {
        writeln!(
            out,
            "fallback_preboot_catalog=valid source=limine-experimental-hii-capture payload_bytes={}",
            payload.len()
        )
        .unwrap();
        return append_payload_sections(out, payload, None);
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| alloc::format!("configuration tables: {error:?}"))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| String::from("TRBIOS1 absent; boot through FirmwareScout first"))?;
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
        .ok_or_else(|| String::from("payload pointer is not mappable"))?;
    require_range(payload_phys, payload_len, "catalog payload")?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| alloc::format!("payload map: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    let payload_crc = crc32fast::hash(payload);
    if payload_crc != catalog.payload_crc32 {
        return Err(alloc::format!(
            "payload CRC mismatch stored=0x{:08X} computed=0x{:08X}",
            catalog.payload_crc32,
            payload_crc
        ));
    }

    writeln!(
        out,
        "fallback_preboot_catalog=valid source=firmware-scout-trbios1 table_phys=0x{:016X} payload_phys=0x{:016X} payload_bytes={} crc_valid=yes",
        catalog_phys,
        payload_phys,
        payload_len
    )
    .unwrap();
    writeln!(
        out,
        "catalog flags=0x{:08X} package_lists={} formsets={} questions={} aggregate_crc32=0x{:08X}",
        catalog.flags,
        catalog.package_list_count,
        catalog.formset_count,
        catalog.question_count,
        catalog.payload_crc32
    )
    .unwrap();

    append_payload_sections(out, payload, Some(catalog.package_list_count))
}

/// Shared TRPAY1 section decoder for both payload sources: the Limine
/// experimental HII-capture handoff and the FirmwareScout TRBIOS1 catalog.
/// `expected_package_list_count` is only available from the TRBIOS1 outer
/// header; the Limine source has no separate outer header to cross-check.
fn append_payload_sections(
    out: &mut String,
    payload: &[u8],
    expected_package_list_count: Option<u32>,
) -> Result<(), String> {
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
    let count = header.section_count as usize;
    if count == 0 || count > MAX_SECTIONS || header.total_bytes as usize != payload.len() {
        return Err(alloc::format!(
            "TRPAY1 shape invalid sections={} total_bytes={}",
            count,
            header.total_bytes
        ));
    }
    let entry_bytes = header.section_entry_bytes as usize;
    let directory_end = usize::from(header.header_bytes)
        .checked_add(
            count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("section directory overflow"))?,
        )
        .ok_or_else(|| String::from("section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("section directory is truncated"));
    }

    writeln!(
        out,
        "payload_format=TRPAY1 version={} sections={} capture_flags=0x{:08X}",
        header.version,
        count,
        header.capture_flags
    )
    .unwrap();

    let mut entries = Vec::with_capacity(count);
    let mut ranges = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::from(header.header_bytes)
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("section entry overflow"))?,
            )
            .ok_or_else(|| String::from("section entry overflow"))?;
        let entry = read_struct::<SectionEntry>(payload, offset)?;
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
        entries.push(entry);
    }

    let mut receipt_valid = false;
    let mut hii_ready = false;
    let mut config_valid = false;
    for (index, entry) in entries.iter().enumerate() {
        let start = entry.offset as usize;
        let end = start + entry.length as usize;
        let bytes = &payload[start..end];
        let computed = crc32fast::hash(bytes);
        let crc_ok = computed == entry.crc32;
        writeln!(
            out,
            "section={} kind={}({}) offset={} bytes={} flags=0x{:08X} status={} raw=0x{:016X} crc_valid={}",
            index,
            entry.kind,
            kind_name(entry.kind),
            entry.offset,
            entry.length,
            entry.flags,
            status_name(entry.status),
            entry.status,
            yes_no(crc_ok)
        )
        .unwrap();
        if !crc_ok {
            continue;
        }
        match entry.kind {
            SEC_STATUS => receipt_valid = append_receipt(out, bytes)?,
            SEC_HII => match summarize_hii(bytes) {
                Ok(summary) => {
                    let catalog_count_match = match expected_package_list_count {
                        Some(expected) => yes_no(summary.lists == expected),
                        None => "n/a",
                    };
                    writeln!(
                        out,
                        "  hii lists={} packages={} forms={} form_bytes={} strings={} string_bytes={} catalog_count_match={}",
                        summary.lists,
                        summary.packages,
                        summary.forms,
                        summary.form_bytes,
                        summary.strings,
                        summary.string_bytes,
                        catalog_count_match
                    )
                    .unwrap();
                    hii_ready = summary.forms != 0 && summary.strings != 0;
                }
                Err(error) => writeln!(out, "  hii_parse=invalid detail=\"{}\"", error).unwrap(),
            },
            SEC_CONFIG => config_valid = append_config(out, bytes)?,
            _ => writeln!(out, "  decoder=unknown-section-kept-opaque").unwrap(),
        }
    }

    writeln!(
        out,
        "capture_status_receipt_valid={} current_config_captured={} capture_ready_for_ifr_parser={}",
        yes_no(receipt_valid),
        yes_no(config_valid),
        yes_no(hii_ready)
    )
    .unwrap();
    writeln!(
        out,
        "next_schema_step=parse form/string packages into formsets/questions/varstores/options/defaults/suppression/reset metadata; active_write_path=none"
    )
    .unwrap();
    Ok(())
}

/// Aggregate fields for the single-line early-boot `bios-handoff` receipt.
/// A trimmed-down parallel of [`append_catalog`] that skips the verbose
/// per-section text the interactive `bios capture` command prints.
pub(crate) struct HandoffSummary {
    pub source: &'static str,
    pub payload_bytes: u32,
    pub status_receipt_valid: bool,
    pub hii_packages: u32,
    pub form_packages: u32,
    pub string_packages: u32,
    pub config_captured: bool,
    pub ready_for_ifr_parser: bool,
}

/// Distinguishes "never handed off" from "handoff present but malformed" so
/// the receipt line can tell the two failure modes apart.
pub(crate) enum HandoffError {
    Absent(String),
    Invalid(String),
}

pub(crate) fn handoff_summary() -> Result<HandoffSummary, HandoffError> {
    if let Some(payload) = trueos_hii_payload() {
        return parse_handoff_summary(payload)
            .map(|mut summary| {
                summary.source = "limine-experimental-hii-capture";
                summary
            })
            .map_err(HandoffError::Invalid);
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| HandoffError::Absent(alloc::format!("configuration tables: {error:?}")))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| {
            HandoffError::Absent(String::from("TRBIOS1 absent; boot through FirmwareScout first"))
        })?;
    if entry.vendor_table == 0 {
        return Err(HandoffError::Absent(String::from("TRBIOS1 table pointer is zero")));
    }

    let catalog_phys = crate::limine::try_as_phys_addr(entry.vendor_table as u64)
        .ok_or_else(|| HandoffError::Absent(String::from("TRBIOS1 table pointer is not mappable")))?;
    require_range(catalog_phys, size_of::<CatalogHeader>(), "catalog header")
        .map_err(HandoffError::Invalid)?;
    let mapping = crate::pci::mmio::map_limine_struct::<CatalogHeader>(catalog_phys)
        .map_err(|error| HandoffError::Invalid(alloc::format!("catalog map: {error:?}")))?;
    let catalog = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    if catalog.magic != CATALOG_MAGIC || catalog.version != VERSION {
        return Err(HandoffError::Invalid(String::from("unsupported catalog magic/version")));
    }
    if usize::from(catalog.header_bytes) < size_of::<CatalogHeader>() {
        return Err(HandoffError::Invalid(String::from("catalog header_bytes is too small")));
    }
    let payload_len = catalog.payload_bytes as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(HandoffError::Invalid(alloc::format!(
            "payload bytes={} outside bound",
            payload_len
        )));
    }
    let payload_phys = crate::limine::try_as_phys_addr(catalog.payload_phys)
        .ok_or_else(|| HandoffError::Invalid(String::from("payload pointer is not mappable")))?;
    require_range(payload_phys, payload_len, "catalog payload").map_err(HandoffError::Invalid)?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| HandoffError::Invalid(alloc::format!("payload map: {error:?}")))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    if crc32fast::hash(payload) != catalog.payload_crc32 {
        return Err(HandoffError::Invalid(String::from("payload CRC mismatch")));
    }

    parse_handoff_summary(payload)
        .map(|mut summary| {
            summary.source = "firmware-scout-trbios1";
            summary
        })
        .map_err(HandoffError::Invalid)
}

/// Shared TRPAY1 parser for [`HandoffSummary`], used by both the Limine
/// experimental HII-capture handoff and the FirmwareScout TRBIOS1 catalog.
fn parse_handoff_summary(payload: &[u8]) -> Result<HandoffSummary, String> {
    let header = read_struct::<PayloadHeader>(payload, 0)?;
    if header.magic != PAYLOAD_MAGIC || header.version != VERSION {
        return Err(String::from("unsupported payload magic/version"));
    }
    if usize::from(header.header_bytes) < size_of::<PayloadHeader>()
        || usize::from(header.section_entry_bytes) < size_of::<SectionEntry>()
    {
        return Err(String::from("TRPAY1 header or entry size is too small"));
    }
    let count = header.section_count as usize;
    if count == 0 || count > MAX_SECTIONS || header.total_bytes as usize != payload.len() {
        return Err(String::from("TRPAY1 shape invalid"));
    }
    let entry_bytes = header.section_entry_bytes as usize;
    let directory_end = usize::from(header.header_bytes)
        .checked_add(
            count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("section directory overflow"))?,
        )
        .ok_or_else(|| String::from("section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("section directory is truncated"));
    }

    let mut summary = HandoffSummary {
        source: "",
        payload_bytes: payload.len() as u32,
        status_receipt_valid: false,
        hii_packages: 0,
        form_packages: 0,
        string_packages: 0,
        config_captured: false,
        ready_for_ifr_parser: false,
    };
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::from(header.header_bytes)
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("section entry overflow"))?,
            )
            .ok_or_else(|| String::from("section entry overflow"))?;
        let section = read_struct::<SectionEntry>(payload, offset)?;
        let start = section.offset as usize;
        let end = start
            .checked_add(section.length as usize)
            .ok_or_else(|| String::from("section range overflow"))?;
        if section.length == 0 || start < directory_end || end > payload.len() {
            return Err(alloc::format!("section {} range invalid", index));
        }
        if ranges.iter().any(|&(left, right)| start < right && end > left) {
            return Err(alloc::format!("section {} overlaps another", index));
        }
        ranges.push((start, end));

        let bytes = &payload[start..end];
        if crc32fast::hash(bytes) != section.crc32 {
            continue;
        }
        match section.kind {
            SEC_STATUS => {
                if let Ok(status) = read_struct::<CaptureStatus>(bytes, 0) {
                    summary.status_receipt_valid = status.magic == STATUS_MAGIC
                        && status.version == VERSION
                        && usize::from(status.bytes) >= size_of::<CaptureStatus>()
                        && usize::from(status.bytes) <= bytes.len();
                }
            }
            SEC_HII => {
                if let Ok(hii) = summarize_hii(bytes) {
                    summary.hii_packages = hii.packages;
                    summary.form_packages = hii.forms;
                    summary.string_packages = hii.strings;
                    summary.ready_for_ifr_parser = hii.forms != 0 && hii.strings != 0;
                }
            }
            SEC_CONFIG => {
                summary.config_captured = bytes.len() >= 2
                    && bytes.len() % 2 == 0
                    && read_u16(bytes, bytes.len() - 2).unwrap_or(1) == 0;
            }
            _ => {}
        }
    }
    Ok(summary)
}

/// One-line high-salience `bios-handoff` receipt for the ordinary bare-metal
/// boot log, so hardware acceptance no longer needs an interactive
/// `bios capture` or a photo of the preboot screen.
pub(crate) fn important_receipt_line() -> String {
    match handoff_summary() {
        Ok(summary) => alloc::format!(
            "bios-handoff: source={} payload=TRPAY1 payload_bytes={} aggregate_crc=yes status_receipt={} hii_packages={} form_packages={} string_packages={} config_captured={} ready_for_ifr_parser={}",
            summary.source,
            summary.payload_bytes,
            yes_no(summary.status_receipt_valid),
            summary.hii_packages,
            summary.form_packages,
            summary.string_packages,
            yes_no(summary.config_captured),
            yes_no(summary.ready_for_ifr_parser)
        ),
        Err(HandoffError::Absent(_)) => String::from(
            "bios-handoff: trbios1=absent booted_through_firmware_scout=not-evidenced",
        ),
        Err(HandoffError::Invalid(detail)) => {
            alloc::format!("bios-handoff: trbios1=invalid detail=\"{}\"", detail)
        }
    }
}

fn append_receipt(out: &mut String, bytes: &[u8]) -> Result<bool, String> {
    let status = read_struct::<CaptureStatus>(bytes, 0)?;
    if status.magic != STATUS_MAGIC
        || status.version != VERSION
        || usize::from(status.bytes) < size_of::<CaptureStatus>()
        || usize::from(status.bytes) > bytes.len()
    {
        writeln!(out, "  capture_receipt=invalid").unwrap();
        return Ok(false);
    }
    writeln!(
        out,
        "  capture_receipt=TRSTAT1 flags=0x{:08X} hii_bytes={} package_lists={} forms={} strings={} config_bytes={}",
        status.flags,
        status.hii_bytes,
        status.package_lists,
        status.form_packages,
        status.string_packages,
        status.config_bytes
    )
    .unwrap();
    writeln!(
        out,
        "  hii_status locate={} query={} export={} parse={} config_status locate={} export={}",
        status_name(status.hii_database_locate_status),
        status_name(status.hii_export_query_status),
        status_name(status.hii_export_status),
        status_name(status.hii_parse_status),
        status_name(status.config_routing_locate_status),
        status_name(status.config_export_status)
    )
    .unwrap();
    Ok(true)
}

fn append_config(out: &mut String, bytes: &[u8]) -> Result<bool, String> {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        writeln!(out, "  config_utf16=invalid").unwrap();
        return Ok(false);
    }
    let terminated = read_u16(bytes, bytes.len() - 2)? == 0;
    writeln!(
        out,
        "  config_utf16 units={} nul_terminated={} route_headers={} content=redacted",
        bytes.len() / 2,
        yes_no(terminated),
        count_utf16_ascii(bytes, b"GUID=")
    )
    .unwrap();
    Ok(terminated)
}

fn summarize_hii(bytes: &[u8]) -> Result<HiiSummary, String> {
    const LIST_HEADER: usize = 20;
    const PACKAGE_HEADER: usize = 4;
    if bytes.len() < LIST_HEADER {
        return Err(String::from("short HII package-list export"));
    }
    let mut result = HiiSummary {
        lists: 0,
        packages: 0,
        forms: 0,
        form_bytes: 0,
        strings: 0,
        string_bytes: 0,
    };
    let mut list = 0usize;
    while list < bytes.len() {
        let list_header_end = list
            .checked_add(LIST_HEADER)
            .ok_or_else(|| String::from("list overflow"))?;
        if list_header_end > bytes.len() {
            return Err(String::from("truncated list header"));
        }
        let list_len = read_u32(bytes, list + 16)? as usize;
        let list_end = list
            .checked_add(list_len)
            .ok_or_else(|| String::from("list overflow"))?;
        if list_len < LIST_HEADER || list_end > bytes.len() {
            return Err(String::from("invalid package-list length"));
        }
        result.lists = result.lists.saturating_add(1);
        let mut package = list_header_end;
        while package < list_end {
            if package.checked_add(PACKAGE_HEADER).unwrap_or(usize::MAX) > list_end {
                return Err(String::from("truncated package header"));
            }
            let raw = read_u32(bytes, package)?;
            let package_len = (raw & 0x00ff_ffff) as usize;
            let package_type = (raw >> 24) as u8;
            let package_end = package
                .checked_add(package_len)
                .ok_or_else(|| String::from("package overflow"))?;
            if package_len < PACKAGE_HEADER || package_end > list_end {
                return Err(String::from("invalid HII package length"));
            }
            result.packages = result.packages.saturating_add(1);
            match package_type {
                HII_FORMS => {
                    result.forms = result.forms.saturating_add(1);
                    result.form_bytes = result.form_bytes.saturating_add(package_len as u64);
                }
                HII_STRINGS => {
                    result.strings = result.strings.saturating_add(1);
                    result.string_bytes = result.string_bytes.saturating_add(package_len as u64);
                }
                _ => {}
            }
            package = package_end;
        }
        list = list_end;
    }
    if list != bytes.len() || result.lists == 0 {
        return Err(String::from("HII export ended off a list boundary"));
    }
    Ok(result)
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

fn kind_name(kind: u32) -> &'static str {
    match kind {
        SEC_STATUS => "capture-status",
        SEC_HII => "hii-package-lists",
        SEC_CONFIG => "hii-config-utf16",
        _ => "unknown",
    }
}

fn status_name(raw: u64) -> &'static str {
    let error = raw & (1u64 << 63) != 0;
    let code = raw & !(1u64 << 63);
    match (error, code) {
        (false, 0) => "success",
        (true, 2) => "invalid-parameter",
        (true, 3) => "unsupported",
        (true, 4) => "bad-buffer-size",
        (true, 5) => "buffer-too-small",
        (true, 7) => "device-error",
        (true, 9) => "out-of-resources",
        (true, 14) => "not-found",
        (true, 19) => "not-started",
        (true, 21) => "aborted",
        (true, 27) => "crc-error",
        (true, 33) => "compromised-data",
        (false, _) => "warning-or-vendor-success",
        (true, _) => "error-or-vendor-status",
    }
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
