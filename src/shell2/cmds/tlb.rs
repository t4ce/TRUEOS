use core::fmt::Write;
use core::str::SplitWhitespace;

use acpi::sdt::fadt::Fadt;
use acpi::sdt::mcfg::Mcfg;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use embassy_executor::Spawner;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) const DUMP_FILE_PATH: &str = "trueos/pci/tlb.txt";

const TLB_USAGE: &str = "tlb: usage `tlb [pci|pcibar|mem|cpu|turbo|ucode|pmu|rapl|acpi [sig [index]]|aml [ec|symbol <path>|prefix <path>]|facp|madt|hpet|mcfg|ssdt|uefi|x2apic|usb [probe]|dump]`";
const TLB_ACPI_USAGE: &str = "tlb: usage `tlb acpi [sig [index]]`";
const TLB_AML_USAGE: &str = "tlb: usage `tlb aml [ec|symbol <path>|prefix <path>]`";
const ACPI_HEXDUMP_MAX_BYTES: usize = 512;
const ACPI_HEXDUMP_ROW_BYTES: usize = 16;
const ACPI_AML_DUMP_MAX_BYTES: usize = 1024;
const TLB_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const TLB_MENU_ROWS: [(&str, &str); 19] = [
    ("pci", "List PCI devices"),
    ("pcibar", "List PCI BAR windows"),
    ("mem", "List memory map"),
    ("cpu", "List CPU cores"),
    ("turbo", "List CPU turbo state and all-core verify stats"),
    ("ucode", "Show Intel microcode loader snapshot"),
    ("pmu", "Show architectural PMU/perf snapshot"),
    ("rapl", "Show latest Intel RAPL energy snapshot"),
    ("acpi", "List ACPI tables or dump one (`tlb acpi SSDT 3`)"),
    ("aml", "Inspect parsed AML namespace (`tlb aml ec|symbol|prefix`)"),
    ("facp", "Show FACP/FADT details"),
    ("madt", "Show MADT details"),
    ("hpet", "Show HPET details"),
    ("mcfg", "Show MCFG details"),
    ("ssdt", "Show SSDT details"),
    ("uefi", "List UEFI tables"),
    ("x2apic", "List x2APIC topology"),
    ("usb", "List USB controllers and ports (`tlb usb probe` for live state)"),
    ("dump", "Write all tables to trueos/pci/tlb.txt"),
];

#[derive(Clone, Copy)]
struct Column {
    header: &'static str,
    width: usize,
}

#[derive(Clone, Copy)]
struct PciBarDecoded {
    kind: &'static str,
    width: &'static str,
    prefetch: &'static str,
    base: u64,
    is_64: bool,
}

struct PciBarRow {
    addr: String,
    vid: String,
    pid: String,
    bar: String,
    kind: &'static str,
    width: &'static str,
    prefetch: &'static str,
    base: String,
    size: String,
    raw: String,
}

struct PciDeviceRow {
    name: String,
    addr: String,
    vid: String,
    pid: String,
}

struct AmlTableRecord {
    label: String,
    phys: usize,
    bytes: &'static [u8],
}

struct AmlNamespaceEntry {
    path: String,
    handle: aml::AmlHandle,
}

struct AmlDefinitionHit {
    table_label: String,
    table_phys: usize,
    aml_offset: usize,
}

struct AmlMethodReferenceHit {
    method_path: String,
    offsets: Vec<usize>,
}

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn blank(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "");
}

fn multiline(io: &'static dyn ShellBackend2, text: &str) {
    for line_text in text.lines() {
        line(io, line_text.trim_end_matches('\r'));
    }
}

fn parse_acpi_signature(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() != 4 {
        return None;
    }

    let upper = trimmed.to_ascii_uppercase();
    if !upper
        .as_bytes()
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }

    Some(upper)
}

fn format_acpi_text_field(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        let ch = if byte.is_ascii_graphic() || byte == b' ' {
            byte as char
        } else {
            '.'
        };
        out.push(ch);
    }

    while out.ends_with(' ') {
        out.pop();
    }

    if out.is_empty() {
        out.push('-');
    }

    out
}

fn emit_hex_dump(io: &'static dyn ShellBackend2, bytes: &[u8]) {
    for (row, chunk) in bytes.chunks(ACPI_HEXDUMP_ROW_BYTES).enumerate() {
        let offset = row.saturating_mul(ACPI_HEXDUMP_ROW_BYTES);
        let mut hex = String::new();
        let mut ascii = String::new();
        for index in 0..ACPI_HEXDUMP_ROW_BYTES {
            if index < chunk.len() {
                let byte = chunk[index];
                if index != 0 {
                    hex.push(' ');
                }
                hex.push_str(alloc::format!("{:02X}", byte).as_str());
                ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                });
            } else {
                if index != 0 {
                    hex.push(' ');
                }
                hex.push_str("  ");
                ascii.push(' ');
            }
        }
        line(io, alloc::format!("0x{:04X}  {}  |{}|", offset, hex, ascii).as_str());
    }
}

fn emit_table_header_details(io: &'static dyn ShellBackend2, bytes: &[u8]) {
    if bytes.len() < crate::efi::acpi::SDT_HEADER_LEN {
        line(io, "  Header: unavailable (short table)");
        return;
    }

    let signature_text = format_acpi_text_field(&bytes[0..4]);
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let revision = bytes[8];
    let checksum = bytes[9];
    let oem_id = format_acpi_text_field(&bytes[10..16]);
    let table_id = format_acpi_text_field(&bytes[16..24]);
    let oem_revision = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let creator_id = format_acpi_text_field(&bytes[28..32]);
    let creator_revision = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);

    line(io, alloc::format!("  Signature: {}", signature_text).as_str());
    line(io, alloc::format!("  Length: {} bytes (0x{:X})", length, length).as_str());
    line(io, alloc::format!("  Revision: {}", revision).as_str());
    line(io, alloc::format!("  Checksum: 0x{:02X}", checksum).as_str());
    line(io, alloc::format!("  OEM ID: {}", oem_id).as_str());
    line(io, alloc::format!("  Table ID: {}", table_id).as_str());
    line(io, alloc::format!("  OEM Revision: 0x{:08X}", oem_revision).as_str());
    line(io, alloc::format!("  Creator ID: {}", creator_id).as_str());
    line(io, alloc::format!("  Creator Revision: 0x{:08X}", creator_revision).as_str());
}

fn emit_aml_dump(io: &'static dyn ShellBackend2, bytes: &[u8], max_bytes: usize) {
    if bytes.len() <= crate::efi::acpi::SDT_HEADER_LEN {
        line(io, "  AML payload: empty");
        return;
    }

    let aml = &bytes[crate::efi::acpi::SDT_HEADER_LEN..];
    let shown = aml.len().min(max_bytes);
    line(io, alloc::format!("  AML dump: showing {} of {} bytes", shown, aml.len()).as_str());
    emit_hex_dump(io, &aml[..shown]);
    if shown < aml.len() {
        line(
            io,
            alloc::format!("  ... truncated, {} AML bytes not shown", aml.len() - shown).as_str(),
        );
    }
}

fn emit_acpi_table_dump(
    io: &'static dyn ShellBackend2,
    label: &str,
    phys: usize,
    bytes: &[u8],
    dump_aml: bool,
    max_bytes: usize,
) {
    line(io, alloc::format!("{} @ 0x{:016X}", label, phys).as_str());
    emit_table_header_details(io, bytes);
    if dump_aml {
        emit_aml_dump(io, bytes, max_bytes);
    } else {
        let shown = bytes.len().min(max_bytes);
        line(io, alloc::format!("  Raw dump: showing {} of {} bytes", shown, bytes.len()).as_str());
        emit_hex_dump(io, &bytes[..shown]);
        if shown < bytes.len() {
            line(
                io,
                alloc::format!("  ... truncated, {} bytes not shown", bytes.len() - shown).as_str(),
            );
        }
    }
    blank(io);
}

#[derive(Clone, Copy)]
struct TlbAmlRuntimeHandler;

impl TlbAmlRuntimeHandler {
    #[inline(always)]
    fn map_ptr(&self, phys_addr: usize, size: usize) -> core::ptr::NonNull<u8> {
        crate::pci::mmio::map_mmio_region(phys_addr as u64, size)
            .unwrap_or_else(|err| panic!("AML map {:x} size {} failed: {:?}", phys_addr, size, err))
    }

    #[inline(always)]
    unsafe fn read_phys<T: Copy>(&self, phys_addr: usize) -> T {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        core::ptr::read_unaligned(ptr.as_ptr() as *const T)
    }

    #[inline(always)]
    unsafe fn write_phys<T>(&self, phys_addr: usize, value: T) {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        core::ptr::write_volatile(ptr.as_ptr() as *mut T, value);
    }
}

impl aml::Handler for TlbAmlRuntimeHandler {
    fn read_u8(&self, address: usize) -> u8 {
        unsafe { self.read_phys::<u8>(address) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        unsafe { self.read_phys::<u16>(address) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        unsafe { self.read_phys::<u32>(address) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        unsafe { self.read_phys::<u64>(address) }
    }

    fn write_u8(&mut self, address: usize, value: u8) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u16(&mut self, address: usize, value: u16) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u32(&mut self, address: usize, value: u32) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u64(&mut self, address: usize, value: u64) {
        unsafe { self.write_phys(address, value) };
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        unsafe { crate::inb(port) }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        unsafe { crate::inw(port) }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        unsafe { crate::inl(port) }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        unsafe { crate::outb(port, value) };
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        unsafe { crate::outw(port, value) };
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        unsafe { crate::outl(port, value) };
    }

    fn read_pci_u8(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
        if segment != 0 {
            return 0xFF;
        }
        crate::pci::config_read_u8(bus, device, function, offset)
    }

    fn read_pci_u16(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
        if segment != 0 {
            return 0xFFFF;
        }
        crate::pci::config_read_u16(bus, device, function, offset)
    }

    fn read_pci_u32(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
        if segment != 0 {
            return 0xFFFF_FFFF;
        }
        crate::pci::config_read_u32(bus, device, function, offset)
    }

    fn write_pci_u8(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u8,
    ) {
        if segment == 0 {
            crate::pci::config_write_u8(bus, device, function, offset, value);
        }
    }

    fn write_pci_u16(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u16,
    ) {
        if segment == 0 {
            crate::pci::config_write_u16(bus, device, function, offset, value);
        }
    }

    fn write_pci_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        if segment == 0 {
            crate::pci::config_write_u32(bus, device, function, offset, value);
        }
    }
}

fn build_aml_context() -> Result<(aml::AmlContext, Vec<AmlTableRecord>), &'static str> {
    let tables = crate::efi::acpi::ensure_tables().ok_or("ACPI tables not found")?;
    let fadt = tables.find_table::<Fadt>().ok_or("FADT/FACP not found")?;
    let fadt_ref = unsafe { fadt.virtual_start.as_ref() };
    let dsdt_phys = fadt_ref
        .dsdt_address()
        .map_err(|_| "DSDT address unavailable from FADT")?;
    let dsdt_bytes = crate::efi::acpi::map_table_bytes(dsdt_phys).ok_or("Failed to map DSDT")?;

    let mut records = Vec::new();
    records.push(AmlTableRecord {
        label: String::from("DSDT"),
        phys: dsdt_phys,
        bytes: dsdt_bytes,
    });

    for (index, (phys, _hdr)) in tables
        .table_headers()
        .filter(|(_, hdr)| hdr.signature.as_str() == "SSDT")
        .enumerate()
    {
        if let Some(bytes) = crate::efi::acpi::map_table_bytes(phys) {
            records.push(AmlTableRecord {
                label: alloc::format!("SSDT#{}", index + 1),
                phys,
                bytes,
            });
        }
    }

    let mut ctx = aml::AmlContext::new(Box::new(TlbAmlRuntimeHandler), aml::DebugVerbosity::None);
    for record in &records {
        if record.bytes.len() < crate::efi::acpi::SDT_HEADER_LEN {
            return Err("ACPI table shorter than SDT header");
        }
        ctx.parse_table(&record.bytes[crate::efi::acpi::SDT_HEADER_LEN..])
            .map_err(|_| "AML parse failed")?;
    }

    Ok((ctx, records))
}

fn collect_aml_namespace_entries(
    ctx: &mut aml::AmlContext,
) -> Result<Vec<AmlNamespaceEntry>, aml::AmlError> {
    let mut entries = Vec::new();
    ctx.namespace.traverse(|scope, level| {
        for (seg, handle) in &level.values {
            let path = aml::AmlName::from_name_seg(*seg).resolve(scope)?;
            entries.push(AmlNamespaceEntry {
                path: path.as_string(),
                handle: *handle,
            });
        }
        Ok(true)
    })?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn simplify_aml_lookup(text: &str) -> String {
    text.trim().to_ascii_uppercase().replace('.', "")
}

fn find_aml_entries<'a>(
    entries: &'a [AmlNamespaceEntry],
    query: &str,
) -> Vec<&'a AmlNamespaceEntry> {
    let query_key = simplify_aml_lookup(query);
    let query_key_no_root = query_key.trim_start_matches('\\');

    let mut exact = Vec::new();
    for entry in entries {
        let entry_key = simplify_aml_lookup(&entry.path);
        if entry_key == query_key || entry_key.trim_start_matches('\\') == query_key_no_root {
            exact.push(entry);
        }
    }
    if !exact.is_empty() {
        return exact;
    }

    entries
        .iter()
        .filter(|entry| {
            let entry_key = simplify_aml_lookup(&entry.path);
            entry_key.ends_with(&query_key)
                || entry_key
                    .trim_start_matches('\\')
                    .ends_with(query_key_no_root)
        })
        .collect()
}

fn aml_handle_paths(entries: &[AmlNamespaceEntry]) -> BTreeMap<aml::AmlHandle, Vec<String>> {
    let mut out: BTreeMap<aml::AmlHandle, Vec<String>> = BTreeMap::new();
    for entry in entries {
        out.entry(entry.handle)
            .or_default()
            .push(entry.path.clone());
    }
    out
}

fn aml_object_kind(value: &aml::value::AmlValue, alias: bool) -> &'static str {
    if alias {
        return "Alias";
    }

    match value {
        aml::value::AmlValue::Method { .. } => "Method",
        aml::value::AmlValue::Field { .. } => "OperationRegion field",
        aml::value::AmlValue::BufferField { .. } => "Field",
        _ => "Name",
    }
}

fn find_all_subslice_offsets(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    let mut offsets = Vec::new();
    for offset in 0..=haystack.len() - needle.len() {
        if &haystack[offset..offset + needle.len()] == needle {
            offsets.push(offset);
        }
    }
    offsets
}

fn aml_leaf_bytes(path: &str) -> Option<[u8; 4]> {
    let leaf = path.rsplit('.').next()?.trim_start_matches('\\');
    if leaf.len() != 4 {
        return None;
    }
    let bytes = leaf.as_bytes();
    Some([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn find_aml_definition_hits(path: &str, tables: &[AmlTableRecord]) -> Vec<AmlDefinitionHit> {
    let Some(leaf) = aml_leaf_bytes(path) else {
        return Vec::new();
    };

    let mut hits = Vec::new();
    for table in tables {
        if table.bytes.len() <= crate::efi::acpi::SDT_HEADER_LEN {
            continue;
        }

        let aml = &table.bytes[crate::efi::acpi::SDT_HEADER_LEN..];
        for aml_offset in find_all_subslice_offsets(aml, &leaf) {
            hits.push(AmlDefinitionHit {
                table_label: table.label.clone(),
                table_phys: table.phys,
                aml_offset,
            });
        }
    }
    hits
}

fn find_aml_method_refs(
    ctx: &aml::AmlContext,
    entries: &[AmlNamespaceEntry],
    path: &str,
) -> Vec<AmlMethodReferenceHit> {
    let Some(leaf) = aml_leaf_bytes(path) else {
        return Vec::new();
    };

    let mut refs = Vec::new();
    for entry in entries {
        let Ok(value) = ctx.namespace.get(entry.handle) else {
            continue;
        };
        let aml::value::AmlValue::Method { code, .. } = value else {
            continue;
        };
        let aml::value::MethodCode::Aml(code) = code else {
            continue;
        };
        let offsets = find_all_subslice_offsets(code, &leaf);
        if !offsets.is_empty() {
            refs.push(AmlMethodReferenceHit {
                method_path: entry.path.clone(),
                offsets,
            });
        }
    }
    refs
}

fn append_hex_dump(out: &mut String, bytes: &[u8]) {
    for (row, chunk) in bytes.chunks(ACPI_HEXDUMP_ROW_BYTES).enumerate() {
        let offset = row.saturating_mul(ACPI_HEXDUMP_ROW_BYTES);
        let mut hex = String::new();
        let mut ascii = String::new();
        for index in 0..ACPI_HEXDUMP_ROW_BYTES {
            if index < chunk.len() {
                let byte = chunk[index];
                if index != 0 {
                    hex.push(' ');
                }
                write!(hex, "{:02X}", byte).unwrap();
                ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                });
            } else {
                if index != 0 {
                    hex.push(' ');
                }
                hex.push_str("  ");
                ascii.push(' ');
            }
        }
        writeln!(out, "0x{:04X}  {}  |{}|", offset, hex, ascii).unwrap();
    }
}

fn append_table_header_details(out: &mut String, bytes: &[u8]) {
    if bytes.len() < crate::efi::acpi::SDT_HEADER_LEN {
        writeln!(out, "  Header: unavailable (short table)").unwrap();
        return;
    }

    let signature_text = format_acpi_text_field(&bytes[0..4]);
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let revision = bytes[8];
    let checksum = bytes[9];
    let oem_id = format_acpi_text_field(&bytes[10..16]);
    let table_id = format_acpi_text_field(&bytes[16..24]);
    let oem_revision = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let creator_id = format_acpi_text_field(&bytes[28..32]);
    let creator_revision = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);

    writeln!(out, "  Signature: {}", signature_text).unwrap();
    writeln!(out, "  Length: {} bytes (0x{:X})", length, length).unwrap();
    writeln!(out, "  Revision: {}", revision).unwrap();
    writeln!(out, "  Checksum: 0x{:02X}", checksum).unwrap();
    writeln!(out, "  OEM ID: {}", oem_id).unwrap();
    writeln!(out, "  Table ID: {}", table_id).unwrap();
    writeln!(out, "  OEM Revision: 0x{:08X}", oem_revision).unwrap();
    writeln!(out, "  Creator ID: {}", creator_id).unwrap();
    writeln!(out, "  Creator Revision: 0x{:08X}", creator_revision).unwrap();
}

fn append_aml_dump(out: &mut String, bytes: &[u8], max_bytes: usize) {
    if bytes.len() <= crate::efi::acpi::SDT_HEADER_LEN {
        writeln!(out, "  AML payload: empty").unwrap();
        return;
    }

    let aml = &bytes[crate::efi::acpi::SDT_HEADER_LEN..];
    let shown = aml.len().min(max_bytes);
    writeln!(out, "  AML dump: showing {} of {} bytes", shown, aml.len()).unwrap();
    append_hex_dump(out, &aml[..shown]);
    if shown < aml.len() {
        writeln!(out, "  ... truncated, {} AML bytes not shown", aml.len() - shown).unwrap();
    }
}

fn append_acpi_table_dump(
    out: &mut String,
    label: &str,
    phys: usize,
    bytes: &[u8],
    dump_aml: bool,
    max_bytes: usize,
) {
    writeln!(out, "{} @ 0x{:016X}", label, phys).unwrap();
    append_table_header_details(out, bytes);
    if dump_aml {
        append_aml_dump(out, bytes, max_bytes);
    } else {
        let shown = bytes.len().min(max_bytes);
        writeln!(out, "  Raw dump: showing {} of {} bytes", shown, bytes.len()).unwrap();
        append_hex_dump(out, &bytes[..shown]);
        if shown < bytes.len() {
            writeln!(out, "  ... truncated, {} bytes not shown", bytes.len() - shown).unwrap();
        }
    }
    writeln!(out).unwrap();
}

fn append_ssdt_dump_text(out: &mut String) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        writeln!(out, "tlb ssdt: no tables found").unwrap();
        return;
    };

    if let Some(fadt_mapping) = tables.find_table::<Fadt>() {
        let fadt_ref = unsafe { fadt_mapping.virtual_start.as_ref() };
        writeln!(out, "FADT/FACP @ 0x{:016X}", fadt_mapping.physical_start).unwrap();
        match fadt_ref.dsdt_address() {
            Ok(dsdt_phys) => {
                writeln!(out, "  DSDT address: 0x{:016X}", dsdt_phys).unwrap();
                if let Some(bytes) = crate::efi::acpi::map_table_bytes(dsdt_phys) {
                    writeln!(out).unwrap();
                    append_acpi_table_dump(
                        out,
                        "DSDT (from FADT)",
                        dsdt_phys,
                        bytes,
                        true,
                        ACPI_AML_DUMP_MAX_BYTES,
                    );
                } else {
                    writeln!(out, "  DSDT map failed").unwrap();
                    writeln!(out).unwrap();
                }
            }
            Err(err) => {
                writeln!(out, "  DSDT address unavailable: {:?}", err).unwrap();
                writeln!(out).unwrap();
            }
        }
    } else {
        writeln!(out, "FADT/FACP not found; cannot resolve DSDT from FADT").unwrap();
        writeln!(out).unwrap();
    }

    writeln!(out, "Scanning for SSDT tables (Secondary System Description Table)...").unwrap();
    writeln!(out).unwrap();

    let mut count = 0usize;
    for (phys, hdr) in tables.table_headers() {
        if hdr.signature.as_str() != "SSDT" {
            continue;
        }

        count = count.saturating_add(1);
        if let Some(bytes) = crate::efi::acpi::map_table_bytes(phys) {
            append_acpi_table_dump(
                out,
                alloc::format!("SSDT #{}", count).as_str(),
                phys,
                bytes,
                true,
                ACPI_AML_DUMP_MAX_BYTES,
            );
        } else {
            writeln!(out, "SSDT #{} @ 0x{:016X}", count, phys).unwrap();
            writeln!(out, "  Map failed").unwrap();
            writeln!(out).unwrap();
        }
    }

    if count == 0 {
        writeln!(out, "No SSDT tables found.").unwrap();
    } else {
        writeln!(out, "Found {} SSDT tables.", count).unwrap();
    }
}

fn truncate_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let chars = text.chars().count();
    if chars <= width {
        let mut out = String::from(text);
        for _ in 0..(width - chars) {
            out.push(' ');
        }
        return out;
    }

    if width <= 3 {
        return text.chars().take(width).collect();
    }

    let mut out = String::new();
    for ch in text.chars().take(width - 3) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn emit_table_header(io: &'static dyn ShellBackend2, cols: &[Column]) {
    emit_table_row(io, cols, &cols.iter().map(|col| col.header).collect::<Vec<_>>());
    let sep = cols
        .iter()
        .map(|col| "-".repeat(col.width))
        .collect::<Vec<_>>();
    let sep_refs = sep.iter().map(String::as_str).collect::<Vec<_>>();
    emit_table_row(io, cols, &sep_refs);
}

fn emit_table_row(io: &'static dyn ShellBackend2, cols: &[Column], cells: &[&str]) {
    let mut out = String::new();
    for (index, col) in cols.iter().enumerate() {
        if index > 0 {
            out.push_str("  ");
        }
        out.push_str(truncate_cell(cells.get(index).copied().unwrap_or(""), col.width).as_str());
    }
    line(io, out.as_str());
}

fn usb_port_speed_text(portsc: u32) -> String {
    match (portsc >> 10) & 0xF {
        0 => String::from("-"),
        1 => String::from("full"),
        2 => String::from("low"),
        3 => String::from("high"),
        4 => String::from("super"),
        5 => String::from("super+"),
        n => alloc::format!("sp{}", n),
    }
}

#[inline]
fn ensure_pci_devices_enumerated() {
    let mut len: usize = 0;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_impl();
    }
}

#[inline]
fn decode_pci_bar(bar_lo: u32, bar_hi: Option<u32>) -> PciBarDecoded {
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF {
        return PciBarDecoded {
            kind: "None",
            width: "-",
            prefetch: "-",
            base: 0,
            is_64: false,
        };
    }

    if (bar_lo & 0x1) != 0 {
        return PciBarDecoded {
            kind: "IO",
            width: "-",
            prefetch: "-",
            base: (bar_lo & !0x3) as u64,
            is_64: false,
        };
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let prefetch = if (bar_lo & 0x8) != 0 { "Y" } else { "N" };
    let base = if is_64 {
        (((bar_hi.unwrap_or(0) as u64) << 32) | (bar_lo as u64)) & !0xFu64
    } else {
        (bar_lo as u64) & !0xFu64
    };

    PciBarDecoded {
        kind: "MMIO",
        width: if is_64 { "64" } else { "32" },
        prefetch,
        base,
        is_64,
    }
}

#[inline]
fn format_bar_raw(bar_lo: u32, bar_hi: Option<u32>) -> String {
    if let Some(hi) = bar_hi {
        alloc::format!("0x{:08X}:{:08X}", hi, bar_lo)
    } else {
        alloc::format!("0x{:08X}", bar_lo)
    }
}

fn pci_bar_rows() -> Vec<PciBarRow> {
    let mut rows = Vec::new();

    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            let addr = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let vid = alloc::format!("{:04X}", dev.vendor);
            let pid = alloc::format!("{:04X}", dev.device);

            let mut bar_idx: u8 = 0;
            while bar_idx < 6 {
                let (bar_lo, bar_hi) =
                    crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar_idx);
                let decoded = decode_pci_bar(bar_lo, bar_hi);
                let size = if decoded.kind == "None" {
                    String::from("-")
                } else if let Some(sz) =
                    crate::pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_idx)
                {
                    alloc::format!("0x{:X}", sz)
                } else {
                    String::from("-")
                };
                let base = if decoded.kind == "None" {
                    String::from("-")
                } else {
                    alloc::format!("0x{:016X}", decoded.base)
                };

                rows.push(PciBarRow {
                    addr: addr.clone(),
                    vid: vid.clone(),
                    pid: pid.clone(),
                    bar: alloc::format!("BAR{}", bar_idx),
                    kind: decoded.kind,
                    width: decoded.width,
                    prefetch: decoded.prefetch,
                    base,
                    size,
                    raw: format_bar_raw(bar_lo, bar_hi),
                });

                bar_idx += if decoded.is_64 { 2 } else { 1 };
            }
        }
    });

    rows
}

fn pci_device_rows(db: Option<&[u8]>) -> Vec<PciDeviceRow> {
    let mut rows = Vec::new();

    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            let addr = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let vid = alloc::format!("{:04X}", dev.vendor);
            let pid = alloc::format!("{:04X}", dev.device);

            let name = if let Some(db) = db {
                if let Some((vendor, device)) =
                    crate::pci::pciids::lookup_vendor_device_from_db(db, dev.vendor, dev.device)
                {
                    let vendor_s = String::from_utf8_lossy(vendor).trim().to_string();
                    let device_s = String::from_utf8_lossy(device).trim().to_string();
                    alloc::format!("{} {}", vendor_s, device_s)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            rows.push(PciDeviceRow {
                name,
                addr,
                vid,
                pid,
            });
        }
    });

    rows
}

fn write_pci_bar_dump(out: &mut String) {
    writeln!(out, "=== PCI BARs ===").unwrap();
    writeln!(
        out,
        "{:10}  {:6}  {:6}  {:4}  {:5}  {:2}  {:1}  {:18}  {:12}  {:19}",
        "Address", "VID", "PID", "BAR", "Kind", "W", "P", "Base", "Size", "Raw"
    )
    .unwrap();
    writeln!(
        out,
        "{:-<10}  {:-<6}  {:-<6}  {:-<4}  {:-<5}  {:-<2}  {:-<1}  {:-<18}  {:-<12}  {:-<19}",
        "", "", "", "", "", "", "", "", "", ""
    )
    .unwrap();

    for row in pci_bar_rows() {
        writeln!(
            out,
            "{:10}  {:6}  {:6}  {:4}  {:5}  {:2}  {:1}  {:18}  {:12}  {:19}",
            row.addr,
            row.vid,
            row.pid,
            row.bar,
            row.kind,
            row.width,
            row.prefetch,
            row.base,
            row.size,
            row.raw
        )
        .unwrap();
    }

    writeln!(out).unwrap();
}

fn print_menu(io: &'static dyn ShellBackend2) {
    let table =
        TlbTable::with_width(&TLB_MENU_HEADERS, line_width_for_backend(io).saturating_sub(2));

    table.emit_header(|text| line(io, text));
    for (cmd, desc) in TLB_MENU_ROWS {
        table.emit_row(&[cmd, desc], |text| line(io, text));
    }
    table.emit_footer(|text| line(io, text));
}

fn cmd_tlb_pci(io: &'static dyn ShellBackend2) {
    ensure_pci_devices_enumerated();

    let db = if crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
        crate::pci::pciids::load_sanitized_from_root_blocking()
            .ok()
            .flatten()
    } else {
        line(io, "tlb pci: no filesystem readiness");
        None
    };

    let shell_width = line_width_for_backend(io);
    let fixed_width = 10 + 6 + 6;
    let separator_width = 2 * 3;
    let min_name_width = 16usize.max("Name".chars().count());
    let name_width = shell_width
        .saturating_sub(fixed_width + separator_width)
        .max(min_name_width);

    let cols = [
        Column {
            header: "Name",
            width: name_width,
        },
        Column {
            header: "Address",
            width: 10,
        },
        Column {
            header: "VID",
            width: 6,
        },
        Column {
            header: "PID",
            width: 6,
        },
    ];
    emit_table_header(io, &cols);

    for row in pci_device_rows(db.as_deref()) {
        emit_table_row(io, &cols, &[&row.name, &row.addr, &row.vid, &row.pid]);
    }
}

fn cmd_tlb_pci_bar(io: &'static dyn ShellBackend2) {
    ensure_pci_devices_enumerated();

    let cols = [
        Column {
            header: "Address",
            width: 10,
        },
        Column {
            header: "VID",
            width: 6,
        },
        Column {
            header: "PID",
            width: 6,
        },
        Column {
            header: "BAR",
            width: 4,
        },
        Column {
            header: "Kind",
            width: 5,
        },
        Column {
            header: "W",
            width: 2,
        },
        Column {
            header: "P",
            width: 1,
        },
        Column {
            header: "Base",
            width: 18,
        },
        Column {
            header: "Size",
            width: 12,
        },
        Column {
            header: "Raw",
            width: 19,
        },
    ];
    emit_table_header(io, &cols);

    for row in pci_bar_rows() {
        emit_table_row(
            io,
            &cols,
            &[
                &row.addr,
                &row.vid,
                &row.pid,
                &row.bar,
                row.kind,
                row.width,
                row.prefetch,
                &row.base,
                &row.size,
                &row.raw,
            ],
        );
    }
}

fn cmd_tlb_mem(io: &'static dyn ShellBackend2) {
    let memmap = crate::limine::memmap_entries().unwrap_or(&[]);
    if memmap.is_empty() {
        line(io, "tlb mem: no memory map available");
        return;
    }

    let cols = [
        Column {
            header: "Base",
            width: 18,
        },
        Column {
            header: "Length",
            width: 18,
        },
        Column {
            header: "Type",
            width: 24,
        },
    ];
    emit_table_header(io, &cols);

    for entry in memmap {
        let base = alloc::format!("0x{:016X}", entry.base);
        let len = alloc::format!("0x{:016X}", entry.length);
        let ty = crate::limine::memmap_type_name(entry.type_);
        emit_table_row(io, &cols, &[&base, &len, ty]);
    }
}

fn cmd_tlb_cpu(io: &'static dyn ShellBackend2) {
    if !crate::smp::is_init() {
        line(io, "tlb cpu: smp not initialized");
        return;
    }

    let cols = [
        Column {
            header: "Slot",
            width: 6,
        },
        Column {
            header: "APIC",
            width: 10,
        },
        Column {
            header: "Role",
            width: 8,
        },
        Column {
            header: "State",
            width: 10,
        },
        Column {
            header: "Seq",
            width: 6,
        },
    ];
    emit_table_header(io, &cols);

    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();

    for slot in 0..count {
        if let Some(info) = crate::smp::read(slot) {
            let slot_s = alloc::format!("{}", slot);
            let lapic_id = slots
                .iter()
                .find(|s| s.slot == slot as u32)
                .map(|s| s.lapic_id)
                .unwrap_or(0xFFFF_FFFF);
            let apic = alloc::format!("{}", lapic_id);
            let role = if slot == 0 { "BSP" } else { "AP" };
            let state = match info.state {
                crate::smp::STATE_IDLE => "Idle",
                crate::smp::STATE_PENDING => "Pending",
                crate::smp::STATE_RUNNING => "Running",
                crate::smp::STATE_DONE => "Done",
                _ => "Unknown",
            };
            let seq = alloc::format!("{}", info.seq);
            emit_table_row(io, &cols, &[&slot_s, &apic, role, state, &seq]);
        }
    }
}

fn fmt_opt_u64_hex(value: Option<u64>) -> String {
    match value {
        Some(value) => alloc::format!("0x{:016X}", value),
        None => String::from("-"),
    }
}

fn cmd_tlb_ucode(io: &'static dyn ShellBackend2) {
    let snapshot = crate::microcode::snapshot();
    const HEADERS: [&str; 2] = ["Field", "Value"];
    let table = TlbTable::with_width(&HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[24, 0]);
    table.emit_header(|text| line(io, text));

    let intel = if snapshot.intel { "yes" } else { "no" };
    table.emit_row(&["intel", intel], |text| line(io, text));
    table.emit_row(&["target", snapshot.target_name], |text| line(io, text));
    table.emit_row(
        &[
            "signature",
            alloc::format!("0x{:08X}", snapshot.signature).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "family-model-step",
            alloc::format!("{}", snapshot.fms).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "platform-mask",
            alloc::format!("0x{:02X}", snapshot.platform_mask).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "current-revision",
            alloc::format!("0x{:08X}", snapshot.current_revision).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "selected-revision",
            alloc::format!("0x{:08X}", snapshot.selected_revision).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "selected-len",
            alloc::format!("0x{:X}", snapshot.selected_len).as_str(),
        ],
        |text| line(io, text),
    );
    for source in snapshot.embedded_sources {
        table.emit_row(
            &[
                "embedded",
                alloc::format!("{} len=0x{:X}", source.name, source.len).as_str(),
            ],
            |text| line(io, text),
        );
    }
    table.emit_footer(|text| line(io, text));
}

fn cmd_tlb_pmu(io: &'static dyn ShellBackend2) {
    let source_ready = crate::pmu::ensure_liveness_source();
    let snapshot = crate::pmu::snapshot();
    let gpu = crate::intel::gpgpu::activity_snapshot();
    const HEADERS: [&str; 2] = ["Field", "Value"];
    let table = TlbTable::with_width(&HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[24, 0]);
    table.emit_header(|text| line(io, text));

    table.emit_row(
        &[
            "arch-perfmon",
            if snapshot.arch_perfmon { "yes" } else { "no" },
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "live-source",
            if source_ready {
                "fixed-counters armed"
            } else {
                "unavailable"
            },
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "gpgpu-source",
            if gpu.available {
                if gpu.direct_rcs_enabled {
                    "rcs mmio + submit counter"
                } else {
                    "mmio only"
                }
            } else {
                "unavailable"
            },
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "gpgpu-submit-seq",
            alloc::format!("{}", gpu.submit_seq).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "gpgpu-rcs",
            alloc::format!(
                "head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}",
                gpu.ring_head,
                gpu.ring_tail,
                gpu.acthd
            )
            .as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "gpgpu-errors",
            alloc::format!(
                "ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X}",
                gpu.ipeir,
                gpu.ipehr,
                gpu.eir
            )
            .as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(&["version", alloc::format!("{}", snapshot.version).as_str()], |text| {
        line(io, text)
    });
    table.emit_row(
        &[
            "gp-counters",
            alloc::format!("{} x {}b", snapshot.gp_counter_count, snapshot.gp_counter_bits)
                .as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "event-mask-len",
            alloc::format!("{}", snapshot.event_mask_len).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "unavailable-events",
            alloc::format!("0x{:08X}", snapshot.unavailable_events).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "fixed-counters",
            alloc::format!("{} x {}b", snapshot.fixed_counter_count, snapshot.fixed_counter_bits)
                .as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "perf-global-ctrl",
            fmt_opt_u64_hex(snapshot.perf_global_ctrl).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(
        &[
            "fixed-ctr-ctrl",
            fmt_opt_u64_hex(snapshot.fixed_ctr_ctrl).as_str(),
        ],
        |text| line(io, text),
    );
    table.emit_row(&["pmc0", fmt_opt_u64_hex(snapshot.pmc0).as_str()], |text| line(io, text));
    for idx in 0..snapshot.fixed_ctr.len() {
        table.emit_row(
            &[
                alloc::format!("fixed-ctr{}", idx).as_str(),
                fmt_opt_u64_hex(snapshot.fixed_ctr[idx]).as_str(),
            ],
            |text| line(io, text),
        );
    }
    table.emit_footer(|text| line(io, text));
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn rapl_sample_for_domain(
    probe: crate::power::rapl::RaplProbe,
    domain: crate::power::rapl::RaplDomain,
) -> Option<crate::power::rapl::RaplSample> {
    probe
        .samples()
        .iter()
        .copied()
        .find(|sample| sample.domain == domain)
}

fn cmd_tlb_rapl(io: &'static dyn ShellBackend2) {
    crate::power::rapl::init();
    let snapshot = crate::power::rapl::latest_snapshot();
    let caps = crate::power::rapl::caps().copied();
    let interval_ms = if snapshot.sample_valid && snapshot.previous.is_some() {
        Some(snapshot.interval_ms)
    } else {
        None
    };
    const FIELD_HEADERS: [&str; 2] = ["Field", "Value"];
    let field_table =
        TlbTable::with_width(&FIELD_HEADERS, line_width_for_backend(io).saturating_sub(2))
            .with_max_col_widths(&[24, 0]);

    field_table.emit_header(|text| line(io, text));
    field_table.emit_row(
        &[
            "intel-cpuid",
            caps.map(|caps| yes_no(caps.vendor_intel)).unwrap_or("-"),
        ],
        |text| line(io, text),
    );
    field_table.emit_row(
        &[
            "msr-cpuid",
            caps.map(|caps| yes_no(caps.has_msr)).unwrap_or("-"),
        ],
        |text| line(io, text),
    );
    field_table
        .emit_row(&["cpuid-supported", yes_no(snapshot.cpuid_supported)], |text| line(io, text));
    field_table.emit_row(
        &[
            "updates",
            alloc::format!("{}", snapshot.update_count).as_str(),
        ],
        |text| line(io, text),
    );
    field_table.emit_row(
        &[
            "last-update-ms",
            alloc::format!("{}", snapshot.last_update_ms).as_str(),
        ],
        |text| line(io, text),
    );
    field_table.emit_row(&["sample-valid", yes_no(snapshot.sample_valid)], |text| line(io, text));
    field_table.emit_row(
        &[
            "interval-ms",
            interval_ms
                .map(|ms| alloc::format!("{}", ms))
                .unwrap_or_else(|| String::from("-"))
                .as_str(),
        ],
        |text| line(io, text),
    );

    let Some(probe) = snapshot.latest else {
        field_table.emit_footer(|text| line(io, text));
        if snapshot.update_count == 0 {
            line(io, "tlb rapl: no RAPL service snapshot has been published yet");
        } else if snapshot.cpuid_supported {
            line(io, "tlb rapl: no readable RAPL sample in the latest snapshot");
        } else {
            line(io, "tlb rapl: Intel MSR/RAPL path unavailable on this hardware");
        }
        return;
    };

    field_table.emit_row(
        &[
            "units",
            alloc::format!(
                "power=2^-{}W ({:.9}) energy=2^-{}J ({:.9}) time=2^-{}s ({:.9})",
                probe.units.power_raw_shift,
                probe.units.power_watts,
                probe.units.energy_raw_shift,
                probe.units.energy_joules,
                probe.units.time_raw_shift,
                probe.units.time_seconds,
            )
            .as_str(),
        ],
        |text| line(io, text),
    );
    field_table.emit_footer(|text| line(io, text));

    blank(io);

    let interval_seconds = interval_ms.unwrap_or(0) as f64 / 1000.0;
    let previous_probe = snapshot.previous;

    const SAMPLE_HEADERS: [&str; 8] = [
        "Domain",
        "Description",
        "MSR",
        "Raw",
        "Joules",
        "DeltaJ",
        "Watts",
        "State",
    ];
    let sample_table =
        TlbTable::with_width(&SAMPLE_HEADERS, line_width_for_backend(io).saturating_sub(2))
            .with_max_col_widths(&[8, 16, 10, 12, 12, 12, 12, 0]);
    sample_table.emit_header(|text| line(io, text));
    for sample in probe.samples() {
        let previous_sample =
            previous_probe.and_then(|probe| rapl_sample_for_domain(probe, sample.domain));
        let delta_joules =
            previous_sample.and_then(|earlier| sample.delta_joules_since(earlier, probe.units));
        let watts = previous_sample.and_then(|earlier| {
            sample.average_power_watts_since(earlier, probe.units, interval_seconds)
        });
        let state = if sample.raw == 0 {
            "zero/absent?"
        } else if watts.is_some() {
            "active"
        } else {
            "sampled"
        };
        sample_table.emit_row(
            &[
                sample.domain.short_name(),
                sample.domain.description(),
                alloc::format!("0x{:03X}", sample.msr).as_str(),
                alloc::format!("0x{:08X}", sample.raw).as_str(),
                alloc::format!("{:.6}", sample.joules).as_str(),
                delta_joules
                    .map(|delta| alloc::format!("{:.6}", delta))
                    .unwrap_or_else(|| String::from("-"))
                    .as_str(),
                watts
                    .map(|watts| alloc::format!("{:.3}", watts))
                    .unwrap_or_else(|| String::from("-"))
                    .as_str(),
                state,
            ],
            |text| line(io, text),
        );
    }
    sample_table.emit_footer(|text| line(io, text));
}

fn turbo_state_text(state: crate::power::turbo::TurboState) -> &'static str {
    match state {
        crate::power::turbo::TurboState::Turbo => "turbo",
        crate::power::turbo::TurboState::NoTurbo => "noturbo",
    }
}

fn cmd_tlb_turbo(io: &'static dyn ShellBackend2) {
    const VERIFY_SPINS: usize = 200_000;

    let cols = [
        Column {
            header: "Metric",
            width: 18,
        },
        Column {
            header: "Value",
            width: 18,
        },
        Column {
            header: "Details",
            width: line_width_for_backend(io).saturating_sub(42).max(24),
        },
    ];
    emit_table_header(io, &cols);

    let armed = if crate::power::turbo::armed() {
        "yes"
    } else {
        "no"
    };
    emit_table_row(io, &cols, &["write gate", armed, "armed by boot policy"]);

    match crate::power::turbo::local_status() {
        crate::power::turbo::TurboStatus::Unsupported => {
            emit_table_row(
                io,
                &cols,
                &["local state", "unsupported", "intel MSR path unavailable"],
            );
            return;
        }
        crate::power::turbo::TurboStatus::State(state) => {
            emit_table_row(
                io,
                &cols,
                &[
                    "local state",
                    turbo_state_text(state),
                    "BSP IA32_MISC_ENABLE",
                ],
            );
        }
    }

    match crate::power::turbo::verify_all(VERIFY_SPINS) {
        Ok(report) => {
            let total = alloc::format!("{}", report.total_cpus);
            emit_table_row(io, &cols, &["total CPUs", &total, "BSP plus SMP slots"]);

            let aps = alloc::format!("{}/{}", report.completed_aps, report.submitted_aps);
            let ap_detail = alloc::format!(
                "online={} busy={} seq={}{}",
                report.online_aps,
                report.busy_aps,
                report.seq,
                if report.timed_out { " timeout" } else { "" }
            );
            emit_table_row(io, &cols, &["AP verify", &aps, &ap_detail]);

            let turbo = alloc::format!("{}", report.turbo_cpus);
            emit_table_row(io, &cols, &["turbo CPUs", &turbo, "MSR turbo-disable bit clear"]);

            let noturbo = alloc::format!("{}", report.noturbo_cpus);
            emit_table_row(io, &cols, &["noturbo CPUs", &noturbo, "MSR turbo-disable bit set"]);

            let unknown = alloc::format!("{}", report.unknown_cpus);
            emit_table_row(
                io,
                &cols,
                &[
                    "unknown CPUs",
                    &unknown,
                    "unsupported or no completed reply",
                ],
            );
        }
        Err(crate::power::turbo::TurboSetError::Unsupported) => {
            emit_table_row(io, &cols, &["verify", "unsupported", "intel MSR path unavailable"]);
        }
        Err(crate::power::turbo::TurboSetError::Disarmed) => {
            emit_table_row(io, &cols, &["verify", "disarmed", "unexpected for read-only verify"]);
        }
    }
}

fn cmd_tlb_acpi_list(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb acpi: no tables found");
        return;
    };

    let cols = [
        Column {
            header: "Signature",
            width: 10,
        },
        Column {
            header: "Address",
            width: 18,
        },
        Column {
            header: "Length",
            width: 10,
        },
        Column {
            header: "Rev",
            width: 4,
        },
        Column {
            header: "OEM",
            width: 8,
        },
        Column {
            header: "Table ID",
            width: 10,
        },
    ];
    emit_table_header(io, &cols);

    let mut total_bytes: u64 = 0;
    let mut ssdt_count: usize = 0;
    let mut largest_sig = String::new();
    let mut largest_addr: u64 = 0;
    let mut largest_len: u64 = 0;
    let mut sig_stats: Vec<(String, usize, u64)> = Vec::new();

    for (phys, hdr) in tables.table_headers() {
        let sig = hdr.signature.as_str();
        let addr = alloc::format!("0x{:08X}", phys);
        let length = hdr.length;
        let revision = hdr.revision;
        let len = alloc::format!("0x{:X}", length);
        let rev = alloc::format!("{}", revision);
        let oem = core::str::from_utf8(&hdr.oem_id).unwrap_or("      ");
        let table_id = core::str::from_utf8(&hdr.oem_table_id).unwrap_or("        ");
        total_bytes = total_bytes.saturating_add(length as u64);
        if sig == "SSDT" {
            ssdt_count = ssdt_count.saturating_add(1);
        }
        if (length as u64) > largest_len {
            largest_len = length as u64;
            largest_addr = phys as u64;
            largest_sig = sig.to_string();
        }
        if let Some((_, count, bytes)) = sig_stats.iter_mut().find(|(name, _, _)| name == sig) {
            *count = count.saturating_add(1);
            *bytes = bytes.saturating_add(length as u64);
        } else {
            sig_stats.push((sig.to_string(), 1, length as u64));
        }
        emit_table_row(io, &cols, &[sig, &addr, &len, &rev, oem, table_id]);
    }

    blank(io);
    line(
        io,
        alloc::format!(
            "ACPI summary: tables={} total_bytes=0x{:X} ssdt_count={}",
            sig_stats
                .iter()
                .fold(0usize, |acc, (_, count, _)| acc.saturating_add(*count)),
            total_bytes,
            ssdt_count
        )
        .as_str(),
    );
    if largest_len != 0 {
        line(
            io,
            alloc::format!(
                "ACPI largest: {} @ 0x{:08X} len=0x{:X}",
                largest_sig,
                largest_addr as u64,
                largest_len
            )
            .as_str(),
        );
    }

    sig_stats.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let stats_cols = [
        Column {
            header: "Sig",
            width: 10,
        },
        Column {
            header: "Count",
            width: 6,
        },
        Column {
            header: "Bytes",
            width: 12,
        },
    ];
    emit_table_header(io, &stats_cols);
    for (sig, count, bytes) in sig_stats {
        let count_s = alloc::format!("{}", count);
        let bytes_s = alloc::format!("0x{:X}", bytes);
        emit_table_row(io, &stats_cols, &[&sig, &count_s, &bytes_s]);
    }
}

fn cmd_tlb_acpi_dump(io: &'static dyn ShellBackend2, signature: &str, index: usize) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb acpi: no tables found");
        return;
    };

    let mut seen = 0usize;
    for (phys, hdr) in tables.table_headers() {
        if hdr.signature.as_str() != signature {
            continue;
        }

        seen = seen.saturating_add(1);
        if seen != index {
            continue;
        }

        let Some(bytes) = crate::efi::acpi::map_table_bytes(phys) else {
            line(
                io,
                alloc::format!(
                    "tlb acpi: failed to map {} #{} @ 0x{:016X}",
                    signature,
                    index,
                    phys
                )
                .as_str(),
            );
            return;
        };

        if bytes.len() < crate::efi::acpi::SDT_HEADER_LEN {
            line(
                io,
                alloc::format!(
                    "tlb acpi: {} #{} @ 0x{:016X} is shorter than an SDT header",
                    signature,
                    index,
                    phys
                )
                .as_str(),
            );
            return;
        }

        emit_acpi_table_dump(
            io,
            alloc::format!("ACPI {} #{}", signature, index).as_str(),
            phys,
            bytes,
            false,
            ACPI_HEXDUMP_MAX_BYTES,
        );
        return;
    }

    if seen == 0 {
        line(io, alloc::format!("tlb acpi: no {} tables found", signature).as_str());
    } else {
        line(
            io,
            alloc::format!(
                "tlb acpi: {} #{} not found (only {} table(s) matched)",
                signature,
                index,
                seen
            )
            .as_str(),
        );
    }
}

fn cmd_tlb_acpi(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(raw_signature) = args.next() else {
        cmd_tlb_acpi_list(io);
        return;
    };

    let Some(signature) = parse_acpi_signature(raw_signature) else {
        line(io, TLB_ACPI_USAGE);
        return;
    };

    let index = match args.next() {
        None => 1usize,
        Some(raw_index) => match raw_index.parse::<usize>() {
            Ok(value) if value != 0 => value,
            _ => {
                line(io, TLB_ACPI_USAGE);
                return;
            }
        },
    };

    if args.next().is_some() {
        line(io, TLB_ACPI_USAGE);
        return;
    }

    cmd_tlb_acpi_dump(io, signature.as_str(), index);
}

fn cmd_tlb_facp(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb facp: no tables found");
        return;
    };

    if let Some(fadt) = tables.find_table::<Fadt>() {
        line(io, alloc::format!("FACP/FADT Found @ 0x{:X}", fadt.physical_start).as_str());
        multiline(io, alloc::format!("{:#?}", unsafe { fadt.virtual_start.as_ref() }).as_str());
    } else {
        line(io, "FACP: Not found");
    }
}

fn cmd_tlb_madt(io: &'static dyn ShellBackend2) {
    if crate::efi::acpi::ensure_tables().is_none() {
        line(io, "tlb madt: no tables found");
        return;
    }

    let mut out = String::new();
    crate::efi::acpi::madt::walk_subtables(|entry| {
        writeln!(&mut out, "{:#?}", entry).unwrap();
    });
    if out.is_empty() {
        line(io, "MADT: Not found");
    } else {
        multiline(io, out.as_str());
    }
}

fn cmd_tlb_hpet(io: &'static dyn ShellBackend2) {
    if let Some(hpet) = crate::efi::acpi::hpet::ensure() {
        multiline(io, alloc::format!("{:#?}", hpet).as_str());
    } else {
        line(io, "HPET: Not found or initialization failed");
    }
}

fn cmd_tlb_mcfg(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb mcfg: no tables found");
        return;
    };

    let Some(mcfg) = tables.find_table::<Mcfg>() else {
        line(io, "tlb mcfg: MCFG table not found");
        return;
    };

    line(io, alloc::format!("MCFG @ 0x{:X}", mcfg.physical_start).as_str());

    let cols = [
        Column {
            header: "Seg",
            width: 4,
        },
        Column {
            header: "Bus",
            width: 7,
        },
        Column {
            header: "ECAM Base",
            width: 18,
        },
        Column {
            header: "ECAM End",
            width: 18,
        },
        Column {
            header: "Size",
            width: 10,
        },
    ];
    emit_table_header(io, &cols);

    let mut count = 0usize;
    for entry in mcfg.entries() {
        count += 1;
        let segment = entry.pci_segment_group;
        let bus_start = entry.bus_number_start;
        let bus_end = entry.bus_number_end;
        let base_addr = entry.base_address;
        let bus_span = (bus_end as u64)
            .saturating_sub(bus_start as u64)
            .saturating_add(1);
        let bytes = bus_span << 20;
        let end = base_addr.saturating_add(bytes).saturating_sub(1);
        let seg = alloc::format!("{}", segment);
        let bus = alloc::format!("{}-{}", bus_start, bus_end);
        let base = alloc::format!("0x{:016X}", base_addr);
        let end_s = alloc::format!("0x{:016X}", end);
        let size = alloc::format!("0x{:X}", bytes);
        emit_table_row(io, &cols, &[&seg, &bus, &base, &end_s, &size]);
    }

    if count == 0 {
        line(io, "tlb mcfg: no ECAM regions listed in MCFG");
    }
}

fn cmd_tlb_ssdt(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb ssdt: no tables found");
        return;
    };

    let fadt = tables.find_table::<Fadt>();
    if let Some(fadt_mapping) = fadt {
        let fadt_ref = unsafe { fadt_mapping.virtual_start.as_ref() };
        line(io, alloc::format!("FADT/FACP @ 0x{:016X}", fadt_mapping.physical_start).as_str());
        match fadt_ref.dsdt_address() {
            Ok(dsdt_phys) => {
                line(io, alloc::format!("  DSDT address: 0x{:016X}", dsdt_phys).as_str());
                if let Some(bytes) = crate::efi::acpi::map_table_bytes(dsdt_phys) {
                    blank(io);
                    emit_acpi_table_dump(
                        io,
                        "DSDT (from FADT)",
                        dsdt_phys,
                        bytes,
                        true,
                        ACPI_AML_DUMP_MAX_BYTES,
                    );
                } else {
                    line(io, "  DSDT map failed");
                    blank(io);
                }
            }
            Err(err) => {
                line(io, alloc::format!("  DSDT address unavailable: {:?}", err).as_str());
                blank(io);
            }
        }
    } else {
        line(io, "FADT/FACP not found; cannot resolve DSDT from FADT");
        blank(io);
    }

    line(io, "Scanning for SSDT tables (Secondary System Description Table)...");
    blank(io);

    let mut count = 0usize;
    for (phys, hdr) in tables.table_headers() {
        if hdr.signature.as_str() == "SSDT" {
            count = count.saturating_add(1);
            let Some(bytes) = crate::efi::acpi::map_table_bytes(phys) else {
                line(io, alloc::format!("SSDT #{} @ 0x{:016X}", count, phys).as_str());
                line(io, "  Map failed");
                blank(io);
                continue;
            };

            let _ = hdr;
            emit_acpi_table_dump(
                io,
                alloc::format!("SSDT #{}", count).as_str(),
                phys,
                bytes,
                true,
                ACPI_AML_DUMP_MAX_BYTES,
            );
        }
    }

    if count == 0 {
        line(io, "No SSDT tables found.");
    } else {
        line(io, alloc::format!("Found {} SSDT tables.", count).as_str());
    }
}

fn cmd_tlb_aml_prefix(io: &'static dyn ShellBackend2, prefix: &str) {
    let Ok((mut ctx, _tables)) = build_aml_context() else {
        line(io, "tlb aml: failed to build AML namespace");
        return;
    };
    let Ok(entries) = collect_aml_namespace_entries(&mut ctx) else {
        line(io, "tlb aml: failed to traverse AML namespace");
        return;
    };

    let prefix_key = simplify_aml_lookup(prefix);
    let matches: Vec<_> = entries
        .iter()
        .filter(|entry| simplify_aml_lookup(&entry.path).starts_with(&prefix_key))
        .collect();

    if matches.is_empty() {
        line(io, alloc::format!("tlb aml prefix: no symbols matched {}", prefix).as_str());
        return;
    }

    let handle_paths = aml_handle_paths(&entries);
    for entry in matches {
        let alias = handle_paths
            .get(&entry.handle)
            .map(|paths| {
                paths.len() > 1 && paths.first().map(String::as_str) != Some(entry.path.as_str())
            })
            .unwrap_or(false);
        let kind = ctx
            .namespace
            .get(entry.handle)
            .map(|value| aml_object_kind(value, alias))
            .unwrap_or("Name");
        line(io, alloc::format!("{} [{}]", entry.path, kind).as_str());
    }
}

fn cmd_tlb_aml_symbol(io: &'static dyn ShellBackend2, query: &str) {
    let Ok((mut ctx, tables)) = build_aml_context() else {
        line(io, "tlb aml: failed to build AML namespace");
        return;
    };
    let Ok(entries) = collect_aml_namespace_entries(&mut ctx) else {
        line(io, "tlb aml: failed to traverse AML namespace");
        return;
    };

    let matches = find_aml_entries(&entries, query);
    if matches.is_empty() {
        line(io, alloc::format!("tlb aml symbol: no symbol matched {}", query).as_str());
        return;
    }
    if matches.len() > 1 {
        line(io, alloc::format!("tlb aml symbol: {} was ambiguous", query).as_str());
        for entry in matches {
            line(io, alloc::format!("  {}", entry.path).as_str());
        }
        return;
    }

    let entry = matches[0];
    let handle_paths = aml_handle_paths(&entries);
    let alias_paths = handle_paths.get(&entry.handle).cloned().unwrap_or_default();
    let alias = alias_paths.len() > 1
        && alias_paths.first().map(String::as_str) != Some(entry.path.as_str());
    let Ok(value) = ctx.namespace.get(entry.handle) else {
        line(io, "tlb aml symbol: failed to read AML object");
        return;
    };

    let kind = aml_object_kind(value, alias);
    let enclosing_scope = aml::AmlName::from_str(&entry.path)
        .ok()
        .and_then(|path| path.parent().ok())
        .map(|path| path.as_string())
        .unwrap_or_else(|| String::from("\\"));

    line(io, alloc::format!("Symbol: {}", entry.path).as_str());
    line(io, alloc::format!("  Object type: {}", kind).as_str());
    line(io, alloc::format!("  Enclosing scope: {}", enclosing_scope).as_str());

    let def_hits = find_aml_definition_hits(&entry.path, &tables);
    if def_hits.is_empty() {
        line(io, "  AML offset: no obvious definition hit found");
    } else {
        for hit in def_hits.iter().take(8) {
            line(
                io,
                alloc::format!(
                    "  AML offset candidate: {} @ 0x{:016X} + 0x{:X}",
                    hit.table_label,
                    hit.table_phys,
                    hit.aml_offset
                )
                .as_str(),
            );
        }
        if def_hits.len() > 8 {
            line(io, alloc::format!("  ... {} more candidate hits", def_hits.len() - 8).as_str());
        }
    }

    if alias_paths.len() > 1 {
        line(io, "  Aliases / shared handle:");
        for path in alias_paths {
            line(io, alloc::format!("    {}", path).as_str());
        }
    }

    let refs = find_aml_method_refs(&ctx, &entries, &entry.path);
    if refs.is_empty() {
        line(io, "  Methods that reference it: none found by raw-byte scan");
    } else {
        line(io, "  Methods that reference it:");
        for method_ref in refs {
            let offsets = method_ref
                .offsets
                .iter()
                .map(|offset| alloc::format!("0x{:X}", offset))
                .collect::<Vec<_>>()
                .join(", ");
            line(io, alloc::format!("    {} @ {}", method_ref.method_path, offsets).as_str());
        }
    }
}

fn cmd_tlb_aml_ec(io: &'static dyn ShellBackend2) {
    const PREFIX: &str = "\\_SB.PC00.LPCBH_EC";
    const TARGETS: [&str; 4] = [
        "\\_SB.PC00.LPCBH_ECECAV",
        "\\_SB.PC00.LPCBH_ECECMD",
        "\\_SB.PC00.LPCBH_ECECWT",
        "\\_SB.PC00.LPCBH_ECECRD",
    ];

    line(io, alloc::format!("AML prefix scan for {}", PREFIX).as_str());
    cmd_tlb_aml_prefix(io, PREFIX);
    blank(io);
    for target in TARGETS {
        cmd_tlb_aml_symbol(io, target);
        blank(io);
    }
}

fn cmd_tlb_aml(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    match args.next() {
        Some("ec") if ensure_no_args(io, args, "tlb: usage `tlb aml ec`") => cmd_tlb_aml_ec(io),
        Some("symbol") => {
            let Some(path) = args.next() else {
                line(io, TLB_AML_USAGE);
                return;
            };
            if args.next().is_some() {
                line(io, TLB_AML_USAGE);
                return;
            }
            cmd_tlb_aml_symbol(io, path);
        }
        Some("prefix") => {
            let Some(prefix) = args.next() else {
                line(io, TLB_AML_USAGE);
                return;
            };
            if args.next().is_some() {
                line(io, TLB_AML_USAGE);
                return;
            }
            cmd_tlb_aml_prefix(io, prefix);
        }
        _ => line(io, TLB_AML_USAGE),
    }
}

fn cmd_tlb_uefi(io: &'static dyn ShellBackend2) {
    let Some(st) = crate::efi::system_table() else {
        line(io, "tlb uefi: system table not found (not booted via UEFI?)");
        return;
    };
    let limine_st = crate::limine::efi_system_table_response();

    let summary_cols = [
        Column {
            header: "Field",
            width: 20,
        },
        Column {
            header: "Value",
            width: 44,
        },
    ];
    emit_table_header(io, &summary_cols);
    let st_revision = st.hdr.revision;
    let st_header_size = st.hdr.header_size;
    emit_table_row(io, &summary_cols, &["Signature", "EFI SYSTEM TABLE"]);
    if let Some(resp) = limine_st {
        let limine_ptr = alloc::format!("0x{:016X}", resp.address as u64);
        emit_table_row(io, &summary_cols, &["Limine ST Ptr", &limine_ptr]);

        if let Some(phys) = crate::limine::try_as_phys_addr(resp.address as u64) {
            let phys_text = alloc::format!("0x{:016X}", phys);
            emit_table_row(io, &summary_cols, &["Mapped ST Phys", &phys_text]);
        }
    }
    emit_table_row(io, &summary_cols, &["Revision", &alloc::format!("0x{:08X}", st_revision)]);
    emit_table_row(io, &summary_cols, &["Header Size", &alloc::format!("0x{:X}", st_header_size)]);
    emit_table_row(
        io,
        &summary_cols,
        &[
            "Runtime Services",
            &alloc::format!("0x{:016X}", st.runtime_services as u64),
        ],
    );
    emit_table_row(
        io,
        &summary_cols,
        &[
            "Boot Services",
            &alloc::format!("0x{:016X}", st.boot_services as u64),
        ],
    );
    emit_table_row(
        io,
        &summary_cols,
        &[
            "Config Tables",
            &alloc::format!("{}", st.number_of_table_entries),
        ],
    );
    blank(io);

    let cfg_cols = [
        Column {
            header: "Index",
            width: 6,
        },
        Column {
            header: "GUID",
            width: 36,
        },
        Column {
            header: "Name",
            width: 24,
        },
        Column {
            header: "Table Ptr",
            width: 18,
        },
    ];
    emit_table_header(io, &cfg_cols);

    let entries = st.number_of_table_entries;
    let cfg_addr = st.configuration_table as u64;

    if entries == 0 {
        line(io, "No UEFI configuration tables reported.");
        return;
    }

    let Some(phys) = crate::limine::try_as_phys_addr(cfg_addr) else {
        line(io, "Cannot translate UEFI configuration table pointer to physical address.");
        return;
    };

    let Ok((cfg_ptr, _)) =
        crate::pci::mmio::map_limine_slice::<crate::efi::EfiConfigurationTable>(phys, entries)
    else {
        line(io, "Failed to map UEFI configuration table entries.");
        return;
    };

    let slice = unsafe { core::slice::from_raw_parts(cfg_ptr.as_ptr(), entries) };
    for (index, entry) in slice.iter().enumerate() {
        let idx = alloc::format!("{}", index);
        let name = crate::efi::cfg_guid_name(&entry.vendor_guid).unwrap_or("Unknown");
        let guid = entry.vendor_guid.fmt_canonical();
        let ptr = alloc::format!("0x{:016X}", entry.vendor_table as u64);
        emit_table_row(io, &cfg_cols, &[&idx, &guid, name, &ptr]);
    }
}

fn cmd_tlb_x2apic(io: &'static dyn ShellBackend2) {
    let topo = crate::x2apic::detect_x2apic_topology();
    line(
        io,
        alloc::format!(
            "x2APIC Topology Detection: Leaf=0x{:X} SMT_Bits={} Core_Bits={}",
            topo.leaf,
            topo.smt_bits,
            topo.core_bits
        )
        .as_str(),
    );
    blank(io);

    let cols = [
        Column {
            header: "Slot",
            width: 6,
        },
        Column {
            header: "APIC ID",
            width: 10,
        },
        Column {
            header: "Pkg",
            width: 6,
        },
        Column {
            header: "Core",
            width: 6,
        },
        Column {
            header: "SMT",
            width: 6,
        },
    ];
    emit_table_header(io, &cols);

    if !crate::smp::is_init() {
        line(io, "(SMP not initialized, showing BSP only if possible)");
    }

    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();
    for slot in 0..count {
        let lapic_id = slots
            .iter()
            .find(|s| s.slot == slot as u32)
            .map(|s| s.lapic_id)
            .unwrap_or(0xFFFF_FFFF);

        if lapic_id == 0xFFFF_FFFF {
            let slot_s = alloc::format!("{}", slot);
            emit_table_row(io, &cols, &[&slot_s, "?", "?", "?", "?"]);
            continue;
        }

        let (pkg, core_id, smt) = topo.decode(lapic_id);
        let slot_s = alloc::format!("{}", slot);
        let apic = alloc::format!("0x{:X}", lapic_id);
        let pkg_s = alloc::format!("{}", pkg);
        let core_s = alloc::format!("{}", core_id);
        let smt_s = alloc::format!("{}", smt);
        emit_table_row(io, &cols, &[&slot_s, &apic, &pkg_s, &core_s, &smt_s]);
    }
}

pub(crate) fn build_dump_text() -> String {
    let mut out = String::new();

    writeln!(out, "=== Memory Map ===").unwrap();
    let memmap = crate::limine::memmap_entries().unwrap_or(&[]);
    if memmap.is_empty() {
        writeln!(out, "No memory map available").unwrap();
    } else {
        writeln!(out, "{:18}  {:18}  Type", "Base", "Length").unwrap();
        writeln!(out, "{:-<18}  {:-<18}  {:-<20}", "", "", "").unwrap();
        for entry in memmap {
            writeln!(
                out,
                "0x{:016X}  0x{:016X}  {}",
                entry.base,
                entry.length,
                crate::limine::memmap_type_name(entry.type_)
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "=== PCI Devices ===").unwrap();
    ensure_pci_devices_enumerated();
    let db = crate::pci::pciids::load_sanitized_from_root_blocking()
        .ok()
        .flatten();
    writeln!(out, "{:30}  {:10}  {:6}  {:6}", "Name", "Address", "VID", "PID").unwrap();
    writeln!(out, "{:-<30}  {:-<10}  {:-<6}  {:-<6}", "", "", "", "").unwrap();
    for row in pci_device_rows(db.as_deref()) {
        let name_disp = if row.name.chars().count() > 30 {
            let mut s: String = row.name.chars().take(29).collect();
            s.push('…');
            s
        } else {
            row.name
        };
        writeln!(out, "{:30}  {:10}  {:6}  {:6}", name_disp, row.addr, row.vid, row.pid).unwrap();
    }
    writeln!(out).unwrap();

    write_pci_bar_dump(&mut out);

    writeln!(out, "=== USB Overview ===").unwrap();
    append_usb_overview_dump(&mut out);

    writeln!(out, "=== USB Raw Enumeration ===").unwrap();
    append_usb_stashed_detail_dump(&mut out);

    writeln!(out, "=== CPU Cores ===").unwrap();
    if !crate::smp::is_init() {
        writeln!(out, "SMP not initialized").unwrap();
    } else {
        writeln!(out, "{:6}  {:6}  {:8}  {:10}", "Slot", "APIC", "Role", "State").unwrap();
        writeln!(out, "{:-<6}  {:-<6}  {:-<8}  {:-<10}", "", "", "", "").unwrap();
        let count = crate::smp::cpu_count();
        let slots = crate::percpu::cpu_slots();
        for slot in 0..count {
            if let Some(info) = crate::smp::read(slot) {
                let lapic_id = slots
                    .iter()
                    .find(|s| s.slot == slot as u32)
                    .map(|s| s.lapic_id)
                    .unwrap_or(0xFFFF_FFFF);
                let role = if slot == 0 { "BSP" } else { "AP" };
                let state = match info.state {
                    crate::smp::STATE_IDLE => "Idle",
                    crate::smp::STATE_PENDING => "Pending",
                    crate::smp::STATE_RUNNING => "Running",
                    crate::smp::STATE_DONE => "Done",
                    _ => "Unknown",
                };
                writeln!(out, "{:6}  {:<6}  {:<8}  {:<10}", slot, lapic_id, role, state).unwrap();
            }
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "=== ACPI Tables ===").unwrap();
    if let Some(tables) = crate::efi::acpi::ensure_tables() {
        writeln!(out, "{:10}  {:18}  {:10}", "Signature", "Address", "Length").unwrap();
        writeln!(out, "{:-<10}  {:-<18}  {:-<10}", "", "", "").unwrap();
        let mut total_bytes: u64 = 0;
        let mut ssdt_count: usize = 0;
        let mut largest_sig = String::new();
        let mut largest_addr: u64 = 0;
        let mut largest_len: u64 = 0;
        let mut sig_stats: Vec<(String, usize, u64)> = Vec::new();
        for (phys, hdr) in tables.table_headers() {
            let sig = hdr.signature.as_str();
            let length = hdr.length;
            total_bytes = total_bytes.saturating_add(length as u64);
            if sig == "SSDT" {
                ssdt_count = ssdt_count.saturating_add(1);
            }
            if (length as u64) > largest_len {
                largest_len = length as u64;
                largest_addr = phys as u64;
                largest_sig = sig.to_string();
            }
            if let Some((_, count, bytes)) = sig_stats.iter_mut().find(|(name, _, _)| name == sig) {
                *count = count.saturating_add(1);
                *bytes = bytes.saturating_add(length as u64);
            } else {
                sig_stats.push((sig.to_string(), 1, length as u64));
            }
            writeln!(out, "{:10}  0x{:016X}  0x{:X}", sig, phys, length).unwrap();
        }
        writeln!(out).unwrap();
        writeln!(
            out,
            "Summary: tables={} total_bytes=0x{:X} ssdt_count={}",
            sig_stats
                .iter()
                .fold(0usize, |acc, (_, count, _)| acc.saturating_add(*count)),
            total_bytes,
            ssdt_count
        )
        .unwrap();
        if largest_len != 0 {
            writeln!(
                out,
                "Largest: {} @ 0x{:016X} len=0x{:X}",
                largest_sig, largest_addr, largest_len
            )
            .unwrap();
        }

        sig_stats.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        writeln!(out, "{:10}  {:6}  {:12}", "Sig", "Count", "Bytes").unwrap();
        writeln!(out, "{:-<10}  {:-<6}  {:-<12}", "", "", "").unwrap();
        for (sig, count, bytes) in sig_stats {
            writeln!(out, "{:10}  {:6}  0x{:X}", sig, count, bytes).unwrap();
        }
        writeln!(out).unwrap();
    } else {
        writeln!(out, "No tables found").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== ACPI AML ===").unwrap();
    append_ssdt_dump_text(&mut out);
    writeln!(out).unwrap();

    writeln!(out, "=== MCFG ===").unwrap();
    if let Some(tables) = crate::efi::acpi::ensure_tables() {
        if let Some(mcfg) = tables.find_table::<Mcfg>() {
            writeln!(out, "MCFG @ 0x{:X}", mcfg.physical_start).unwrap();
            writeln!(
                out,
                "{:4}  {:7}  {:18}  {:18}  {:10}",
                "Seg", "Bus", "ECAM Base", "ECAM End", "Size"
            )
            .unwrap();
            writeln!(out, "{:-<4}  {:-<7}  {:-<18}  {:-<18}  {:-<10}", "", "", "", "", "").unwrap();

            let mut count = 0usize;
            for entry in mcfg.entries() {
                count += 1;
                let segment = entry.pci_segment_group;
                let bus_start = entry.bus_number_start;
                let bus_end = entry.bus_number_end;
                let base_addr = entry.base_address;
                let bus_span = (bus_end as u64)
                    .saturating_sub(bus_start as u64)
                    .saturating_add(1);
                let bytes = bus_span << 20;
                let end = base_addr.saturating_add(bytes).saturating_sub(1);
                writeln!(
                    out,
                    "{:4}  {:3}-{:3}  0x{:016X}  0x{:016X}  0x{:X}",
                    segment, bus_start, bus_end, base_addr, end, bytes
                )
                .unwrap();
            }

            if count == 0 {
                writeln!(out, "No ECAM regions listed in MCFG").unwrap();
            }
        } else {
            writeln!(out, "MCFG table not found").unwrap();
        }
    } else {
        writeln!(out, "No ACPI tables found").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== UEFI Tables ===").unwrap();
    if let Some(st) = crate::efi::system_table() {
        let st_revision = st.hdr.revision;
        writeln!(out, "Signature: EFI SYSTEM TABLE").unwrap();
        writeln!(out, "Revision: 0x{:08X}", st_revision).unwrap();
        writeln!(out, "Runtime Services: 0x{:016X}", st.runtime_services as u64).unwrap();
        writeln!(out, "Boot Services: 0x{:016X}", st.boot_services as u64).unwrap();
        writeln!(out).unwrap();

        let entries = st.number_of_table_entries;
        let cfg_addr = st.configuration_table as u64;
        writeln!(out, "{:6}  {:40}  {:24}  {:18}", "Index", "GUID", "Name", "Table Ptr").unwrap();
        writeln!(out, "{:-<6}  {:-<40}  {:-<24}  {:-<18}", "", "", "", "").unwrap();

        if entries == 0 {
            writeln!(out, "No UEFI configuration tables reported").unwrap();
        } else if let Some(phys) = crate::limine::try_as_phys_addr(cfg_addr) {
            if let Ok((cfg_ptr, _)) = crate::pci::mmio::map_limine_slice::<
                crate::efi::EfiConfigurationTable,
            >(phys, entries)
            {
                let slice = unsafe { core::slice::from_raw_parts(cfg_ptr.as_ptr(), entries) };
                for (index, entry) in slice.iter().enumerate() {
                    let name = crate::efi::cfg_guid_name(&entry.vendor_guid).unwrap_or("Unknown");
                    writeln!(
                        out,
                        "{:6}  {}  {:24}  0x{:016X}",
                        index,
                        entry.vendor_guid.fmt_canonical(),
                        name,
                        entry.vendor_table as u64
                    )
                    .unwrap();
                }
            } else {
                writeln!(out, "Failed to map UEFI configuration tables").unwrap();
            }
        } else {
            writeln!(out, "Cannot translate UEFI configuration table pointer to physical address")
                .unwrap();
        }
    } else {
        writeln!(out, "No UEFI system table found").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== x2APIC Topology ===").unwrap();
    let topo = crate::x2apic::detect_x2apic_topology();
    writeln!(out, "Leaf=0x{:X} SMT_Bits={} Core_Bits={}", topo.leaf, topo.smt_bits, topo.core_bits)
        .unwrap();
    writeln!(out, "{:6}  {:10}  {:6}  {:6}  {:6}", "Slot", "APIC ID", "Pkg", "Core", "SMT")
        .unwrap();
    writeln!(out, "{:-<6}  {:-<10}  {:-<6}  {:-<6}  {:-<6}", "", "", "", "", "").unwrap();
    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();
    for slot in 0..count {
        let lapic_id = slots
            .iter()
            .find(|s| s.slot == slot as u32)
            .map(|s| s.lapic_id)
            .unwrap_or(0xFFFF_FFFF);
        if lapic_id == 0xFFFF_FFFF {
            writeln!(out, "{:6}  {:10}  {:6}  {:6}  {:6}", slot, "?", "?", "?", "?").unwrap();
            continue;
        }
        let (pkg, core_id, smt) = topo.decode(lapic_id);
        writeln!(out, "{:6}  0x{:<8X}  {:<6}  {:<6}  {:<6}", slot, lapic_id, pkg, core_id, smt)
            .unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "=== Network Interfaces ===").unwrap();
    let net_count = crate::net::device_count();
    if net_count == 0 {
        writeln!(out, "No network interfaces found").unwrap();
    } else {
        writeln!(out, "{:4}  {:20}  {:17}  {:10}", "Idx", "Name", "MAC Address", "Primary")
            .unwrap();
        writeln!(out, "{:-<4}  {:-<20}  {:-<17}  {:-<10}", "", "", "", "").unwrap();
        let primary = crate::net::primary_device_index();
        for index in 0..net_count {
            let name = crate::net::device_name_at(index).unwrap_or("Unknown");
            let mac = if let Some(addr) = crate::net::mac_address_at(index) {
                alloc::format!(
                    "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                    addr[0],
                    addr[1],
                    addr[2],
                    addr[3],
                    addr[4],
                    addr[5]
                )
            } else {
                String::from("??:??:??:??:??:??")
            };
            let primary_mark = if index == primary { "*" } else { "" };
            writeln!(out, "{:<4}  {:<20}  {:<17}  {:<10}", index, name, mac, primary_mark).unwrap();
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "=== Block Devices ===").unwrap();
    let devices: alloc::vec::Vec<_> = crate::disc::block::devices()
        .into_iter()
        .filter(|dev| dev.user_visible)
        .collect();
    if devices.is_empty() {
        writeln!(out, "No block devices found").unwrap();
    } else {
        writeln!(
            out,
            "{:8}  {:10}  {:12}  {:10}  {:20}  {:6}  {:8}",
            "ID", "Kind", "Size (MB)", "Blocks", "Label", "R/W", "Parent"
        )
        .unwrap();
        writeln!(
            out,
            "{:-<8}  {:-<10}  {:-<12}  {:-<10}  {:-<20}  {:-<6}  {:-<8}",
            "", "", "", "", "", "", ""
        )
        .unwrap();
        for dev in devices {
            let id = alloc::format!("{}", dev.id);
            let kind = alloc::format!("{:?}", dev.kind);
            let size_mb = dev.capacity_bytes / (1024 * 1024);
            let blocks = dev.block_count;
            let label = dev.label.as_deref().unwrap_or("-");
            let rw = if dev.writable { "RW" } else { "RO" };
            let parent = dev
                .parent
                .map(|value| alloc::format!("{}", value))
                .unwrap_or_else(|| String::from("-"));
            writeln!(
                out,
                "{:<8}  {:<10}  {:<12}  {:<10}  {:<20}  {:<6}  {:<8}",
                id, kind, size_mb, blocks, label, rw, parent
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();

    out
}

pub(crate) async fn write_dump_bytes_to_default_path(
    bytes: &[u8],
) -> Result<(), crate::disc::block::Error> {
    let Some(handle) = crate::r::fs::trueosfs::primary_root_handle() else {
        return Err(crate::disc::block::Error::NotReady);
    };

    match crate::r::fs::trueosfs::file_in_async(handle, DUMP_FILE_PATH, bytes).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(crate::disc::block::Error::Io),
        Err(err) => Err(err),
    }
}

fn cmd_tlb_dump(io: &'static dyn ShellBackend2) {
    let out = build_dump_text();
    line(io, alloc::format!("Writing {} bytes to {}...", out.len(), DUMP_FILE_PATH).as_str());

    let out_bytes = out.into_bytes();
    let result: Result<(), crate::disc::block::Error> =
        crate::wait::spawn_and_wait_local(async move {
            write_dump_bytes_to_default_path(&out_bytes).await
        });

    match result {
        Ok(()) => line(io, "Success."),
        Err(err) => line(io, alloc::format!("Error writing file: {:?}", err).as_str()),
    }
}

fn usb_port_pls_text(portsc: u32) -> &'static str {
    match (portsc >> 5) & 0x0F {
        0 => "U0",
        1 => "U1",
        2 => "U2",
        3 => "U3",
        4 => "Dis",
        5 => "RxD",
        6 => "Ina",
        7 => "Res",
        8 => "Rec",
        9 => "Hot",
        10 => "Cmp",
        11 => "Tst",
        15 => "Rsv",
        _ => "-",
    }
}

fn yn(flag: bool) -> &'static str {
    if flag { "Y" } else { "-" }
}

fn is_usb_mass_storage_label(label: Option<&str>) -> bool {
    matches!(label, Some(text) if text.starts_with("usbms-"))
}

fn usb_interface_class_text(interface: &crate::usb2::TlbUsbInterface) -> String {
    let triple = crate::usb2::class::UsbClassTriple::from_codes(
        interface.class,
        interface.subclass,
        interface.protocol,
    );
    let base = triple.base_class();
    alloc::format!(
        "{:02X}/{:02X}/{:02X} {} {} {} ({})",
        base.code(),
        interface.subclass,
        interface.protocol,
        triple.short_name(),
        base.descriptor_usage().as_str(),
        triple.description(),
        base.description()
    )
}

fn emit_usb_endpoint_rows(
    io: &'static dyn ShellBackend2,
    table: &TlbTable,
    dev: &crate::usb2::TlbUsbDevice,
) {
    for cfg in dev.configurations.iter() {
        for interface in cfg.interfaces.iter() {
            if interface.endpoints.is_empty() {
                let row = [
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    dev.speed.to_string(),
                    String::new(),
                    alloc::format!(
                        "  if{} alt{}",
                        interface.interface_number,
                        interface.alternate_setting
                    ),
                    dev.slot_id.to_string(),
                    alloc::format!("0x{:05X}", dev.route_string),
                    alloc::format!("cfg={}", cfg.configuration_value),
                    usb_interface_class_text(interface),
                    String::from("no-ep"),
                    dev.product.clone().unwrap_or_else(|| String::from("-")),
                    alloc::format!("{:08X}", dev.stable_id),
                ];
                table.emit_row(&row, |text| print_shell_line(io, text));
                continue;
            }

            for endpoint in interface.endpoints.iter() {
                let row = [
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    dev.speed.to_string(),
                    String::new(),
                    alloc::format!(
                        "  if{} alt{}",
                        interface.interface_number,
                        interface.alternate_setting
                    ),
                    dev.slot_id.to_string(),
                    alloc::format!("0x{:05X}", dev.route_string),
                    alloc::format!("ep=0x{:02X}", endpoint.address),
                    usb_interface_class_text(interface),
                    alloc::format!(
                        "{} mps={} intv={} cfg={}",
                        endpoint.transfer_type,
                        endpoint.max_packet_size,
                        endpoint.interval,
                        cfg.configuration_value
                    ),
                    dev.product.clone().unwrap_or_else(|| String::from("-")),
                    alloc::format!("{:08X}", dev.stable_id),
                ];
                table.emit_row(&row, |text| print_shell_line(io, text));
            }
        }
    }
}

fn append_usb_endpoint_rows(out: &mut String, table: &TlbTable, dev: &crate::usb2::TlbUsbDevice) {
    for cfg in dev.configurations.iter() {
        for interface in cfg.interfaces.iter() {
            if interface.endpoints.is_empty() {
                let row = [
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    dev.speed.to_string(),
                    String::new(),
                    alloc::format!(
                        "  if{} alt{}",
                        interface.interface_number,
                        interface.alternate_setting
                    ),
                    dev.slot_id.to_string(),
                    alloc::format!("0x{:05X}", dev.route_string),
                    alloc::format!("cfg={}", cfg.configuration_value),
                    usb_interface_class_text(interface),
                    String::from("no-ep"),
                    dev.product.clone().unwrap_or_else(|| String::from("-")),
                    alloc::format!("{:08X}", dev.stable_id),
                ];
                table.emit_row(&row, |text| {
                    writeln!(out, "{text}").unwrap();
                });
                continue;
            }

            for endpoint in interface.endpoints.iter() {
                let row = [
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    dev.speed.to_string(),
                    String::new(),
                    alloc::format!(
                        "  if{} alt{}",
                        interface.interface_number,
                        interface.alternate_setting
                    ),
                    dev.slot_id.to_string(),
                    alloc::format!("0x{:05X}", dev.route_string),
                    alloc::format!("ep=0x{:02X}", endpoint.address),
                    usb_interface_class_text(interface),
                    alloc::format!(
                        "{} mps={} intv={} cfg={}",
                        endpoint.transfer_type,
                        endpoint.max_packet_size,
                        endpoint.interval,
                        cfg.configuration_value
                    ),
                    dev.product.clone().unwrap_or_else(|| String::from("-")),
                    alloc::format!("{:08X}", dev.stable_id),
                ];
                table.emit_row(&row, |text| {
                    writeln!(out, "{text}").unwrap();
                });
            }
        }
    }
}

fn append_usb_overview_dump(out: &mut String) {
    const USB_DUMP_TABLE_WIDTH: usize = 180;

    let snapshot = crate::usb2::tlb_usb_snapshot();
    let controllers = snapshot.controllers.as_slice();
    if controllers.is_empty() {
        writeln!(out, "No xHCI USB controllers found.").unwrap();
        writeln!(out).unwrap();
        return;
    }

    let mut devices_by_controller_root: BTreeMap<(usize, u8), Vec<crate::usb2::UsbDeviceSummary>> =
        BTreeMap::new();
    let mut detailed_devices_by_controller_root: BTreeMap<
        (usize, u8),
        Vec<crate::usb2::TlbUsbDevice>,
    > = BTreeMap::new();
    for ctrl in controllers.iter() {
        if let Ok(devices) = crate::usb2::crabusb_observed_device_summaries(ctrl.index) {
            for dev in devices {
                devices_by_controller_root
                    .entry((ctrl.index, dev.root_port_id))
                    .or_default()
                    .push(dev);
            }
        }
        if let Ok(devices) = crate::usb2::crabusb_observed_devices(ctrl.index) {
            for dev in devices {
                detailed_devices_by_controller_root
                    .entry((ctrl.index, dev.root_port_id))
                    .or_default()
                    .push(dev);
            }
        }
    }

    let usbms_count = crate::disc::block::devices()
        .into_iter()
        .filter(|dev| {
            dev.user_visible
                && dev.parent.is_none()
                && is_usb_mass_storage_label(dev.label.as_deref())
        })
        .count();
    let controller_list = controllers
        .iter()
        .map(|ctrl| alloc::format!("{}={:04X}:{:04X}", ctrl.index, ctrl.vendor_id, ctrl.device_id))
        .collect::<Vec<_>>()
        .join(" ");
    let first_controller = crate::usb2::discover_first_controller()
        .map(|ctrl| alloc::format!("{}={:04X}:{:04X}", ctrl.index, ctrl.vendor_id, ctrl.device_id))
        .unwrap_or_else(|| String::from("-"));

    writeln!(
        out,
        "USB Overview (usbms registered={} ctrls={} first={} observed={} devices={} topology={} probe_error={})",
        usbms_count,
        controller_list,
        first_controller,
        snapshot.probe_device_count.unwrap_or(0),
        snapshot.devices.len(),
        snapshot.topology.len(),
        snapshot.probe_error.unwrap_or("-")
    )
    .unwrap();
    let headers = [
        "#",
        "BDF",
        "Port",
        "C",
        "E",
        "W",
        "R",
        "Speed",
        "PLS",
        "Dev Port",
        "Slot",
        "Route",
        "Dev VID:PID",
        "Class",
        "Kind",
        "Product",
        "Stable",
    ];
    let table = TlbTable::with_width(&headers, USB_DUMP_TABLE_WIDTH)
        .with_max_col_widths(&[1, 9, 2, 1, 1, 1, 1, 5, 2, 2, 4, 8, 0, 8, 0, 10, 8]);
    table.emit_header(|text| {
        writeln!(out, "{text}").unwrap();
    });

    let mut disconnected_ports_summary: Vec<String> = Vec::new();
    for ctrl in controllers.iter() {
        let bdf = alloc::format!("{:02X}:{:02X}.{}", ctrl.bus, ctrl.slot, ctrl.function);
        let Some(diag) = crate::usb2::controller_mmio_diag(ctrl.index) else {
            let row = [
                ctrl.index.to_string(),
                bdf,
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("no-mmio"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
            ];
            table.emit_row(&row, |text| {
                writeln!(out, "{text}").unwrap();
            });
            continue;
        };
        let runtime = crate::usb2::crabusb_runtime_diag(ctrl.index);
        writeln!(
            out,
            "ctrl {} runtime phase={} lifecycle={} event={} probe_req={} port_change={} empty={} fail={} early_fatal={} last={} probe_new={} recovery(quiescent_before={} q={}ms init={}ms quiet={}ms skip_delayed={})",
            ctrl.index,
            ctrl.controller_phase,
            ctrl.root_hub_lifecycle,
            yn(ctrl.event_ready),
            yn(runtime.probe_requested),
            yn(ctrl.root_port_change_seen),
            ctrl.empty_probe_streak,
            runtime.probe_fail_streak,
            runtime.early_fatal_rebind_streak,
            runtime.last_probe_state,
            runtime.last_probe_device_count,
            yn(runtime.recovery_quiescent_before_bind),
            runtime.recovery_quiescent_ms,
            runtime.recovery_initial_settle_ms,
            runtime.recovery_probe_quiet_ms,
            yn(runtime.recovery_skip_delayed_event_handler)
        )
        .unwrap();
        writeln!(
            out,
            "ctrl {} xhci caplen={} hcs1=0x{:08X} hcc1=0x{:08X} dboff=0x{:X} rtsoff=0x{:X} usbcmd=0x{:08X} usbsts=0x{:08X} crcr=0x{:016X} dcbaap=0x{:016X} config=0x{:08X} iman=0x{:08X} imod=0x{:08X} erstsz={} erstba=0x{:016X} erdp=0x{:016X}",
            ctrl.index,
            diag.caplen,
            diag.hcsparams1,
            diag.hccparams1,
            diag.dboff,
            diag.rtsoff,
            diag.usbcmd,
            diag.usbsts,
            diag.crcr,
            diag.dcbaap,
            diag.config,
            diag.iman,
            diag.imod,
            diag.erstsz,
            diag.erstba,
            diag.erdp
        )
        .unwrap();

        let mut disconnected_ports: Vec<String> = Vec::new();
        for port in diag.ports.iter() {
            let portsc = port.portsc;
            if (portsc & (1 << 0)) == 0 {
                disconnected_ports.push(port.port_id.to_string());
                continue;
            }

            let attached = devices_by_controller_root.get(&(ctrl.index, port.port_id));
            let detailed = detailed_devices_by_controller_root.get(&(ctrl.index, port.port_id));
            if let Some(devices) = attached {
                for dev in devices.iter() {
                    let dev_vidpid = match (dev.vid, dev.pid) {
                        (Some(vid), Some(pid)) => alloc::format!("{:04X}:{:04X}", vid, pid),
                        _ => String::from("-"),
                    };
                    let class = match (dev.class, dev.subclass, dev.protocol) {
                        (Some(class), Some(subclass), Some(protocol)) => {
                            alloc::format!("{:02X}/{:02X}/{:02X}", class, subclass, protocol)
                        }
                        _ => String::from("-"),
                    };
                    let stable = alloc::format!("{:08X}", dev.stable_id);
                    let row = [
                        ctrl.index.to_string(),
                        bdf.clone(),
                        port.port_id.to_string(),
                        yn((portsc & (1 << 0)) != 0).to_string(),
                        yn((portsc & (1 << 1)) != 0).to_string(),
                        yn((portsc & (1 << 9)) != 0).to_string(),
                        yn((portsc & (1 << 4)) != 0).to_string(),
                        usb_port_speed_text(portsc).to_string(),
                        alloc::format!(
                            "{} pmsc={:08X} li={:08X}",
                            usb_port_pls_text(portsc),
                            port.portpmsc,
                            port.portli
                        ),
                        dev.port.to_string(),
                        dev.slot_id.to_string(),
                        alloc::format!("0x{:05X}", dev.route_string),
                        dev_vidpid,
                        class,
                        String::from(dev.kind),
                        dev.product.clone().unwrap_or_else(|| String::from("-")),
                        stable,
                    ];
                    table.emit_row(&row, |text| {
                        writeln!(out, "{text}").unwrap();
                    });

                    if let Some(detailed_devices) = detailed {
                        if let Some(full) = detailed_devices.iter().find(|candidate| {
                            candidate.stable_id == dev.stable_id && candidate.port_id == dev.port
                        }) {
                            append_usb_endpoint_rows(out, &table, full);
                        }
                    }
                }
            } else {
                let row = [
                    ctrl.index.to_string(),
                    bdf.clone(),
                    port.port_id.to_string(),
                    yn((portsc & (1 << 0)) != 0).to_string(),
                    yn((portsc & (1 << 1)) != 0).to_string(),
                    yn((portsc & (1 << 9)) != 0).to_string(),
                    yn((portsc & (1 << 4)) != 0).to_string(),
                    usb_port_speed_text(portsc).to_string(),
                    alloc::format!(
                        "{} pmsc={:08X} li={:08X}",
                        usb_port_pls_text(portsc),
                        port.portpmsc,
                        port.portli
                    ),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                ];
                table.emit_row(&row, |text| {
                    writeln!(out, "{text}").unwrap();
                });
            }
        }

        if !disconnected_ports.is_empty() {
            disconnected_ports_summary.push(alloc::format!(
                "ctrl {}: {}",
                ctrl.index,
                disconnected_ports.join(", ")
            ));
        }
    }

    table.emit_footer(|text| {
        writeln!(out, "{text}").unwrap();
    });
    if !disconnected_ports_summary.is_empty() {
        writeln!(out, "Disconnected ports: {}", disconnected_ports_summary.join(" | ")).unwrap();
    }
    writeln!(out, "Legend: #=controller C=connected E=enabled W=power R=reset PLS=port link state")
        .unwrap();
    writeln!(out).unwrap();
}

fn append_usb_stashed_detail_dump(out: &mut String) {
    let snapshot = crate::usb2::tlb_usb_snapshot();
    let controllers = snapshot.controllers.as_slice();
    if controllers.is_empty() {
        writeln!(out, "No xHCI USB controllers found.").unwrap();
        writeln!(out).unwrap();
        return;
    }

    for ctrl in controllers.iter() {
        let runtime = crate::usb2::crabusb_runtime_diag(ctrl.index);
        writeln!(
            out,
            "Controller {} {:02X}:{:02X}.{} {:04X}:{:04X} phase={} lifecycle={} event={} probe_req={} port_change={} empty={} fail={} early_fatal={} last={} probe_new={} recovery_quiescent_before={}",
            ctrl.index,
            ctrl.bus,
            ctrl.slot,
            ctrl.function,
            ctrl.vendor_id,
            ctrl.device_id,
            ctrl.controller_phase,
            ctrl.root_hub_lifecycle,
            yn(ctrl.event_ready),
            yn(runtime.probe_requested),
            yn(ctrl.root_port_change_seen),
            ctrl.empty_probe_streak,
            runtime.probe_fail_streak,
            runtime.early_fatal_rebind_streak,
            runtime.last_probe_state,
            runtime.last_probe_device_count,
            yn(runtime.recovery_quiescent_before_bind)
        )
        .unwrap();
        for node in snapshot
            .topology
            .iter()
            .filter(|node| node.controller_index == ctrl.index)
        {
            let kind = match node.kind {
                crate::usb2::TlbUsbTopologyNodeKind::RootPort => "root",
                crate::usb2::TlbUsbTopologyNodeKind::Hub => "hub",
                crate::usb2::TlbUsbTopologyNodeKind::Device => "dev",
            };
            writeln!(
                out,
                "  Topology kind={} root={} port={} depth={} slot={} parent={} speed={} vid:pid={} class={}",
                kind,
                node.root_port_id,
                node.port_id,
                node.depth,
                node.slot_id.map(|v| v.to_string()).unwrap_or_else(|| String::from("-")),
                node.parent_slot_id.map(|v| v.to_string()).unwrap_or_else(|| String::from("-")),
                node.speed,
                match (node.vendor_id, node.product_id) {
                    (Some(vid), Some(pid)) => alloc::format!("{:04X}:{:04X}", vid, pid),
                    _ => String::from("-"),
                },
                match (node.class, node.subclass, node.protocol) {
                    (Some(class), Some(subclass), Some(protocol)) => {
                        alloc::format!("{:02X}/{:02X}/{:02X}", class, subclass, protocol)
                    }
                    _ => String::from("-"),
                }
            )
            .unwrap();
        }

        match crate::usb2::crabusb_observed_devices(ctrl.index) {
            Ok(devices) if !devices.is_empty() => {
                for dev in devices {
                    writeln!(
                        out,
                        "  Device stable={:08X} slot={} root_port={} port={} route=0x{:08X} speed={} vid:pid={:04X}:{:04X} class={:02X}/{:02X}/{:02X} cfgs={} mps0={}",
                        dev.stable_id,
                        dev.slot_id,
                        dev.root_port_id,
                        dev.port_id,
                        dev.route_string,
                        dev.speed,
                        dev.vendor_id,
                        dev.product_id,
                        dev.class,
                        dev.subclass,
                        dev.protocol,
                        dev.num_configurations,
                        dev.max_packet_size_0
                    )
                    .unwrap();
                    if dev.manufacturer.is_some() || dev.product.is_some() || dev.serial.is_some() {
                        writeln!(
                            out,
                            "    Strings manufacturer={} product={} serial={}",
                            dev.manufacturer.as_deref().unwrap_or("-"),
                            dev.product.as_deref().unwrap_or("-"),
                            dev.serial.as_deref().unwrap_or("-")
                        )
                        .unwrap();
                    }

                    if !dev.path.is_empty() {
                        writeln!(out, "    Path: {:?}", dev.path.as_slice()).unwrap();
                    }
                    if let Some(parent_hub_slot_id) = dev.parent_hub_slot_id {
                        writeln!(out, "    Parent hub slot: {}", parent_hub_slot_id).unwrap();
                    }
                    if !dev.hub_path.is_empty() {
                        for hop in dev.hub_path.iter() {
                            writeln!(
                                out,
                                "    Hop slot={} port={} depth={} speed={}",
                                hop.slot_id, hop.port_id, hop.hub_depth, hop.speed
                            )
                            .unwrap();
                        }
                    }

                    for cfg in dev.configurations.iter() {
                        writeln!(
                            out,
                            "    Config cfg={} attr=0x{:02X} max_power={}",
                            cfg.configuration_value, cfg.attributes, cfg.max_power
                        )
                        .unwrap();
                        for interface in cfg.interfaces.iter() {
                            writeln!(
                                out,
                                "      Interface if#{} alt={} class={:02X}/{:02X}/{:02X}",
                                interface.interface_number,
                                interface.alternate_setting,
                                interface.class,
                                interface.subclass,
                                interface.protocol
                            )
                            .unwrap();
                            if interface.endpoints.is_empty() {
                                writeln!(out, "        Endpoint: none").unwrap();
                            } else {
                                for endpoint in interface.endpoints.iter() {
                                    writeln!(
                                        out,
                                        "        Endpoint ep=0x{:02X} type={} mps={} interval={}",
                                        endpoint.address,
                                        endpoint.transfer_type,
                                        endpoint.max_packet_size,
                                        endpoint.interval
                                    )
                                    .unwrap();
                                }
                            }
                        }
                    }
                }
            }
            Ok(_) => {
                writeln!(out, "  No observed devices").unwrap();
            }
            Err(err) => {
                writeln!(out, "  Observed device read failed: {}", err).unwrap();
            }
        }
        writeln!(out).unwrap();
    }
}

fn cmd_tlb_usb(io: &'static dyn ShellBackend2) {
    let snapshot = crate::usb2::tlb_usb_snapshot();
    let controllers = snapshot.controllers.as_slice();
    if controllers.is_empty() {
        line(io, "No xHCI USB controllers found.");
        return;
    }

    let mut devices_by_controller_root: BTreeMap<(usize, u8), Vec<crate::usb2::UsbDeviceSummary>> =
        BTreeMap::new();
    let mut detailed_devices_by_controller_root: BTreeMap<
        (usize, u8),
        Vec<crate::usb2::TlbUsbDevice>,
    > = BTreeMap::new();
    for ctrl in controllers.iter() {
        if let Ok(devices) = crate::usb2::crabusb_observed_device_summaries(ctrl.index) {
            for dev in devices {
                devices_by_controller_root
                    .entry((ctrl.index, dev.root_port_id))
                    .or_default()
                    .push(dev);
            }
        }
        if let Ok(devices) = crate::usb2::crabusb_observed_devices(ctrl.index) {
            for dev in devices {
                detailed_devices_by_controller_root
                    .entry((ctrl.index, dev.root_port_id))
                    .or_default()
                    .push(dev);
            }
        }
    }

    let usbms_count = crate::disc::block::devices()
        .into_iter()
        .filter(|dev| {
            dev.user_visible
                && dev.parent.is_none()
                && is_usb_mass_storage_label(dev.label.as_deref())
        })
        .count();
    let controller_list = controllers
        .iter()
        .map(|ctrl| alloc::format!("{}={:04X}:{:04X}", ctrl.index, ctrl.vendor_id, ctrl.device_id))
        .collect::<Vec<_>>()
        .join(" ");
    let first_controller = crate::usb2::discover_first_controller()
        .map(|ctrl| alloc::format!("{}={:04X}:{:04X}", ctrl.index, ctrl.vendor_id, ctrl.device_id))
        .unwrap_or_else(|| String::from("-"));

    line(
        io,
        alloc::format!(
            "USB Overview (usbms registered={} ctrls={} first={} observed={} devices={} topology={} probe_error={})",
            usbms_count,
            controller_list,
            first_controller,
            snapshot.probe_device_count.unwrap_or(0),
            snapshot.devices.len(),
            snapshot.topology.len(),
            snapshot.probe_error.unwrap_or("-")
        )
        .as_str(),
    );
    let headers = [
        "#",
        "BDF",
        "Port",
        "C",
        "E",
        "W",
        "R",
        "Speed",
        "PLS",
        "Dev Port",
        "Slot",
        "Route",
        "Dev VID:PID",
        "Class",
        "Kind",
        "Product",
        "Stable",
    ];
    let table = TlbTable::with_width(&headers, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[1, 9, 2, 1, 1, 1, 1, 5, 2, 2, 4, 8, 0, 8, 0, 10, 8]);
    table.emit_header(|text| print_shell_line(io, text));
    let mut disconnected_ports_summary: Vec<String> = Vec::new();

    for ctrl in controllers.iter() {
        let bdf = alloc::format!("{:02X}:{:02X}.{}", ctrl.bus, ctrl.slot, ctrl.function);
        let Some(diag) = crate::usb2::controller_mmio_diag(ctrl.index) else {
            let row = [
                ctrl.index.to_string(),
                bdf,
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
                String::from("no-mmio"),
                String::from("-"),
                String::from("-"),
                String::from("-"),
            ];
            table.emit_row(&row, |text| print_shell_line(io, text));
            continue;
        };
        let runtime = crate::usb2::crabusb_runtime_diag(ctrl.index);
        line(
            io,
            alloc::format!(
                "ctrl {} runtime phase={} lifecycle={} event={} probe_req={} port_change={} empty={} fail={} early_fatal={} last={} devices={} recovery(quiescent_before={} q={}ms init={}ms quiet={}ms skip_delayed={})",
                ctrl.index,
                ctrl.controller_phase,
                ctrl.root_hub_lifecycle,
                yn(ctrl.event_ready),
                yn(runtime.probe_requested),
                yn(ctrl.root_port_change_seen),
                ctrl.empty_probe_streak,
                runtime.probe_fail_streak,
                runtime.early_fatal_rebind_streak,
                runtime.last_probe_state,
                runtime.last_probe_device_count,
                yn(runtime.recovery_quiescent_before_bind),
                runtime.recovery_quiescent_ms,
                runtime.recovery_initial_settle_ms,
                runtime.recovery_probe_quiet_ms,
                yn(runtime.recovery_skip_delayed_event_handler)
            )
            .as_str(),
        );
        line(
            io,
            alloc::format!(
                "ctrl {} xhci caplen={} hcs1=0x{:08X} hcc1=0x{:08X} dboff=0x{:X} rtsoff=0x{:X} usbcmd=0x{:08X} usbsts=0x{:08X} crcr=0x{:016X} dcbaap=0x{:016X} config=0x{:08X} iman=0x{:08X} imod=0x{:08X} erstsz={} erstba=0x{:016X} erdp=0x{:016X}",
                ctrl.index,
                diag.caplen,
                diag.hcsparams1,
                diag.hccparams1,
                diag.dboff,
                diag.rtsoff,
                diag.usbcmd,
                diag.usbsts,
                diag.crcr,
                diag.dcbaap,
                diag.config,
                diag.iman,
                diag.imod,
                diag.erstsz,
                diag.erstba,
                diag.erdp
            )
            .as_str(),
        );

        let mut disconnected_ports: Vec<String> = Vec::new();

        for port in diag.ports.iter() {
            let portsc = port.portsc;
            if (portsc & (1 << 0)) == 0 {
                disconnected_ports.push(port.port_id.to_string());
                continue;
            }
            let attached = devices_by_controller_root.get(&(ctrl.index, port.port_id));
            let detailed = detailed_devices_by_controller_root.get(&(ctrl.index, port.port_id));
            if let Some(devices) = attached {
                for dev in devices.iter() {
                    let dev_vidpid = match (dev.vid, dev.pid) {
                        (Some(vid), Some(pid)) => alloc::format!("{:04X}:{:04X}", vid, pid),
                        _ => String::from("-"),
                    };
                    let class = match (dev.class, dev.subclass, dev.protocol) {
                        (Some(class), Some(subclass), Some(protocol)) => {
                            alloc::format!("{:02X}/{:02X}/{:02X}", class, subclass, protocol)
                        }
                        _ => String::from("-"),
                    };
                    let stable = alloc::format!("{:08X}", dev.stable_id);
                    let row = [
                        ctrl.index.to_string(),
                        bdf.clone(),
                        port.port_id.to_string(),
                        yn((portsc & (1 << 0)) != 0).to_string(),
                        yn((portsc & (1 << 1)) != 0).to_string(),
                        yn((portsc & (1 << 9)) != 0).to_string(),
                        yn((portsc & (1 << 4)) != 0).to_string(),
                        usb_port_speed_text(portsc).to_string(),
                        alloc::format!(
                            "{} pmsc={:08X} li={:08X}",
                            usb_port_pls_text(portsc),
                            port.portpmsc,
                            port.portli
                        ),
                        dev.port.to_string(),
                        dev.slot_id.to_string(),
                        alloc::format!("0x{:05X}", dev.route_string),
                        dev_vidpid,
                        class,
                        String::from(dev.kind),
                        dev.product.clone().unwrap_or_else(|| String::from("-")),
                        stable,
                    ];
                    table.emit_row(&row, |text| print_shell_line(io, text));

                    if let Some(detailed_devices) = detailed {
                        if let Some(full) = detailed_devices.iter().find(|candidate| {
                            candidate.stable_id == dev.stable_id && candidate.port_id == dev.port
                        }) {
                            emit_usb_endpoint_rows(io, &table, full);
                        }
                    }
                }
            } else {
                let row = [
                    ctrl.index.to_string(),
                    bdf.clone(),
                    port.port_id.to_string(),
                    yn((portsc & (1 << 0)) != 0).to_string(),
                    yn((portsc & (1 << 1)) != 0).to_string(),
                    yn((portsc & (1 << 9)) != 0).to_string(),
                    yn((portsc & (1 << 4)) != 0).to_string(),
                    usb_port_speed_text(portsc).to_string(),
                    alloc::format!(
                        "{} pmsc={:08X} li={:08X}",
                        usb_port_pls_text(portsc),
                        port.portpmsc,
                        port.portli
                    ),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                    String::from("-"),
                ];
                table.emit_row(&row, |text| print_shell_line(io, text));
            }
        }

        if !disconnected_ports.is_empty() {
            disconnected_ports_summary.push(alloc::format!(
                "ctrl {}: {}",
                ctrl.index,
                disconnected_ports.join(", ")
            ));
        }
    }
    table.emit_footer(|text| print_shell_line(io, text));
    if !disconnected_ports_summary.is_empty() {
        line(
            io,
            alloc::format!("Disconnected ports: {}", disconnected_ports_summary.join(" | "))
                .as_str(),
        );
    }
    line(io, "Legend: #=controller C=connected E=enabled W=power R=reset PLS=port link state");
}

fn ensure_no_args(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
    usage: &str,
) -> bool {
    if args.next().is_some() {
        line(io, usage);
        false
    } else {
        true
    }
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None => print_menu(io),
        Some("pci") if ensure_no_args(io, args, "tlb: usage `tlb pci`") => cmd_tlb_pci(io),
        Some("pcibar") if ensure_no_args(io, args, "tlb: usage `tlb pcibar`") => {
            cmd_tlb_pci_bar(io)
        }
        Some("mem") if ensure_no_args(io, args, "tlb: usage `tlb mem`") => cmd_tlb_mem(io),
        Some("cpu") if ensure_no_args(io, args, "tlb: usage `tlb cpu`") => cmd_tlb_cpu(io),
        Some("turbo") if ensure_no_args(io, args, "tlb: usage `tlb turbo`") => cmd_tlb_turbo(io),
        Some("ucode") if ensure_no_args(io, args, "tlb: usage `tlb ucode`") => cmd_tlb_ucode(io),
        Some("pmu") if ensure_no_args(io, args, "tlb: usage `tlb pmu`") => cmd_tlb_pmu(io),
        Some("rapl") if ensure_no_args(io, args, "tlb: usage `tlb rapl`") => cmd_tlb_rapl(io),
        Some("acpi") => cmd_tlb_acpi(io, args),
        Some("aml") => cmd_tlb_aml(io, args),
        Some("facp") if ensure_no_args(io, args, "tlb: usage `tlb facp`") => cmd_tlb_facp(io),
        Some("madt") if ensure_no_args(io, args, "tlb: usage `tlb madt`") => cmd_tlb_madt(io),
        Some("hpet") if ensure_no_args(io, args, "tlb: usage `tlb hpet`") => cmd_tlb_hpet(io),
        Some("mcfg") if ensure_no_args(io, args, "tlb: usage `tlb mcfg`") => cmd_tlb_mcfg(io),
        Some("ssdt") if ensure_no_args(io, args, "tlb: usage `tlb ssdt`") => cmd_tlb_ssdt(io),
        Some("uefi") if ensure_no_args(io, args, "tlb: usage `tlb uefi`") => cmd_tlb_uefi(io),
        Some("x2apic") if ensure_no_args(io, args, "tlb: usage `tlb x2apic`") => cmd_tlb_x2apic(io),
        Some("usb") if ensure_no_args(io, args, "tlb: usage `tlb usb`") => cmd_tlb_usb(io),
        Some("dump") if ensure_no_args(io, args, "tlb: usage `tlb dump`") => cmd_tlb_dump(io),
        Some(_) => line(io, TLB_USAGE),
    }
    ParseOutcome::Handled
}
