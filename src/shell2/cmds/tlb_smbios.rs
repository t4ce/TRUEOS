use core::fmt::Write;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

use crate::efi::smbios::{self, Structure};

pub(crate) fn append_dump(out: &mut String) {
    writeln!(out, "=== SMBIOS Hardware Inventory ===").unwrap();
    let entry_points = crate::limine::smbios_entry_point_addresses();
    if let Some((entry_64, entry_32)) = entry_points {
        writeln!(
            out,
            "Limine entry points: smbios3=0x{:016X} smbios2=0x{:016X}",
            entry_64, entry_32
        )
        .unwrap();
    } else {
        writeln!(out, "Limine entry points: response unavailable").unwrap();
    }

    let table = match smbios::discover() {
        Ok(table) => table,
        Err(error) => {
            writeln!(out, "SMBIOS unavailable: reason={} detail={:?}", error.label(), error)
                .unwrap();
            writeln!(out).unwrap();
            return;
        }
    };

    writeln!(out, "Entry kind: {}", table.kind.label()).unwrap();
    writeln!(
        out,
        "Version: {}.{} doc_revision={} entry_point_revision=0x{:02X}",
        table.major,
        table.minor,
        table
            .doc_revision
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-")),
        table.entry_point_revision
    )
    .unwrap();
    writeln!(
        out,
        "Entry point: raw=0x{:016X} phys=0x{:016X} bytes=0x{:X} anchor=ok checksum=ok",
        table.entry_point_raw, table.entry_point_phys, table.entry_point_len
    )
    .unwrap();
    writeln!(
        out,
        "Structure table: phys=0x{:016X} declared_or_max_bytes=0x{:X} crc32=0x{:08X} declared_structures={}",
        table.table_phys,
        table.table_declared_bytes,
        crc32fast::hash(table.bytes()),
        table
            .declared_structure_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    writeln!(out).unwrap();

    let mut structures = table.structures();
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut parsed_count = 0usize;
    let mut consumed_bytes = 0usize;
    let mut end_seen = false;
    let mut parse_error = None;

    loop {
        match structures.next_structure() {
            Ok(Some(structure)) => {
                parsed_count = parsed_count.saturating_add(1);
                consumed_bytes = structure
                    .table_offset
                    .saturating_add(structure.total_bytes());
                *counts.entry(structure.type_id).or_insert(0) += 1;
                append_structure(out, structure);
                if structure.type_id == 127 {
                    end_seen = true;
                }
            }
            Ok(None) => break,
            Err(error) => {
                parse_error = Some(error);
                writeln!(out, "SMBIOS parse stopped: {:?}", error).unwrap();
                break;
            }
        }
    }

    writeln!(out, "SMBIOS summary").unwrap();
    writeln!(out, "  Parsed structures: {}", parsed_count).unwrap();
    writeln!(out, "  Consumed bytes: 0x{:X}", consumed_bytes).unwrap();
    writeln!(out, "  Declared/max bytes: 0x{:X}", table.table_declared_bytes).unwrap();
    writeln!(out, "  End-of-table marker: {}", yes_no(end_seen)).unwrap();
    writeln!(
        out,
        "  Parse status: {}",
        if parse_error.is_none() {
            "complete"
        } else {
            "failed"
        }
    )
    .unwrap();
    if let Some(declared) = table.declared_structure_count {
        writeln!(
            out,
            "  Structure-count match: {} (declared={} parsed={})",
            yes_no(usize::from(declared) == parsed_count),
            declared,
            parsed_count
        )
        .unwrap();
    }
    writeln!(out, "  Type counts:").unwrap();
    for (type_id, count) in counts {
        writeln!(out, "    {:3} {:5} {}", type_id, count, smbios::structure_type_name(type_id))
            .unwrap();
    }
    writeln!(out).unwrap();
}

fn append_structure(out: &mut String, structure: Structure<'_>) {
    writeln!(
        out,
        "[SMBIOS @0x{:06X}] type={} ({}) handle=0x{:04X} formatted=0x{:X} total=0x{:X}",
        structure.table_offset,
        structure.type_id,
        structure.type_name(),
        structure.handle,
        structure.formatted_len(),
        structure.total_bytes()
    )
    .unwrap();
    append_decoded(out, structure);
    append_formatted_bytes(out, structure);
    append_strings(out, structure);
    writeln!(out).unwrap();
}

fn append_decoded(out: &mut String, structure: Structure<'_>) {
    match structure.type_id {
        0 => {
            text_field(out, structure, "Firmware vendor", 0x04);
            text_field(out, structure, "Firmware version", 0x05);
            text_field(out, structure, "Firmware release date", 0x08);
            hex_u16_field(out, structure, "Firmware start segment", 0x06);
            hex_u64_field(out, structure, "Firmware characteristics", 0x0A);
            if let Some(size) = platform_firmware_rom_bytes(structure) {
                value_field(out, "Firmware ROM bytes", size);
            }
            version_pair_field(out, structure, "System firmware version", 0x14, 0x15);
            version_pair_field(out, structure, "Embedded controller version", 0x16, 0x17);
        }
        1 => {
            text_field(out, structure, "System manufacturer", 0x04);
            text_field(out, structure, "System product", 0x05);
            text_field(out, structure, "System version", 0x06);
            text_field(out, structure, "System serial", 0x07);
            if let Some(uuid) = structure.bytes(0x08, 16) {
                writeln!(out, "  System UUID: {}", format_uuid(uuid)).unwrap();
            }
            if let Some(value) = structure.byte(0x18) {
                writeln!(out, "  Wake-up type: 0x{:02X} ({})", value, wake_up_type(value)).unwrap();
            }
            text_field(out, structure, "System SKU", 0x19);
            text_field(out, structure, "System family", 0x1A);
        }
        2 => {
            text_field(out, structure, "Baseboard manufacturer", 0x04);
            text_field(out, structure, "Baseboard product", 0x05);
            text_field(out, structure, "Baseboard version", 0x06);
            text_field(out, structure, "Baseboard serial", 0x07);
            text_field(out, structure, "Baseboard asset tag", 0x08);
            hex_u8_field(out, structure, "Baseboard feature flags", 0x09);
            text_field(out, structure, "Baseboard chassis location", 0x0A);
            hex_u16_field(out, structure, "Baseboard chassis handle", 0x0B);
            if let Some(value) = structure.byte(0x0D) {
                writeln!(out, "  Baseboard type: 0x{:02X} ({})", value, baseboard_type(value))
                    .unwrap();
            }
            decimal_u8_field(out, structure, "Contained object handles", 0x0E);
        }
        3 => {
            text_field(out, structure, "Chassis manufacturer", 0x04);
            if let Some(value) = structure.byte(0x05) {
                writeln!(
                    out,
                    "  Chassis type: 0x{:02X} ({}) lock_present={}",
                    value & 0x7F,
                    chassis_type(value & 0x7F),
                    yes_no(value & 0x80 != 0)
                )
                .unwrap();
            }
            text_field(out, structure, "Chassis version", 0x06);
            text_field(out, structure, "Chassis serial", 0x07);
            text_field(out, structure, "Chassis asset tag", 0x08);
            hex_u8_field(out, structure, "Chassis boot-up state", 0x09);
            hex_u8_field(out, structure, "Chassis power-supply state", 0x0A);
            hex_u8_field(out, structure, "Chassis thermal state", 0x0B);
            hex_u8_field(out, structure, "Chassis security status", 0x0C);
            hex_u32_field(out, structure, "Chassis OEM-defined", 0x0D);
            decimal_u8_field(out, structure, "Chassis height (U)", 0x11);
            decimal_u8_field(out, structure, "Chassis power cords", 0x12);
            decimal_u8_field(out, structure, "Chassis contained element count", 0x13);
            decimal_u8_field(out, structure, "Chassis contained element record length", 0x14);
            if let (Some(count), Some(record_len)) = (structure.byte(0x13), structure.byte(0x14)) {
                let sku_offset = 0x15usize
                    .checked_add(usize::from(count).saturating_mul(usize::from(record_len)));
                if let Some(sku_offset) = sku_offset {
                    text_field(out, structure, "Chassis SKU", sku_offset);
                }
            }
        }
        4 => {
            text_field(out, structure, "Processor socket", 0x04);
            hex_u8_field(out, structure, "Processor type", 0x05);
            hex_u8_field(out, structure, "Processor family", 0x06);
            text_field(out, structure, "Processor manufacturer", 0x07);
            hex_u64_field(out, structure, "Processor ID", 0x08);
            text_field(out, structure, "Processor version", 0x10);
            decimal_u16_field(out, structure, "External clock MHz", 0x12);
            decimal_u16_field(out, structure, "Maximum speed MHz", 0x14);
            decimal_u16_field(out, structure, "Current speed MHz", 0x16);
            if let Some(value) = structure.byte(0x18) {
                writeln!(
                    out,
                    "  Processor status: 0x{:02X} socket_populated={} state={}",
                    value,
                    yes_no(value & 0x40 != 0),
                    processor_status(value & 0x07)
                )
                .unwrap();
            }
            hex_u8_field(out, structure, "Processor upgrade", 0x19);
            hex_u16_field(out, structure, "L1 cache handle", 0x1A);
            hex_u16_field(out, structure, "L2 cache handle", 0x1C);
            hex_u16_field(out, structure, "L3 cache handle", 0x1E);
            text_field(out, structure, "Processor serial", 0x20);
            text_field(out, structure, "Processor asset tag", 0x21);
            text_field(out, structure, "Processor part number", 0x22);
            processor_count_field(out, structure, "Core count", 0x23, 0x2A);
            processor_count_field(out, structure, "Enabled cores", 0x24, 0x2C);
            processor_count_field(out, structure, "Thread count", 0x25, 0x2E);
            hex_u16_field(out, structure, "Processor characteristics", 0x26);
            hex_u16_field(out, structure, "Processor family 2", 0x28);
        }
        7 => {
            text_field(out, structure, "Cache socket designation", 0x04);
            hex_u16_field(out, structure, "Cache configuration", 0x05);
            hex_u16_field(out, structure, "Maximum cache size raw", 0x07);
            hex_u16_field(out, structure, "Installed cache size raw", 0x09);
            hex_u16_field(out, structure, "Supported SRAM type", 0x0B);
            hex_u16_field(out, structure, "Current SRAM type", 0x0D);
            decimal_u8_field(out, structure, "Cache speed ns", 0x0F);
            hex_u8_field(out, structure, "Cache error correction", 0x10);
            hex_u8_field(out, structure, "System cache type", 0x11);
            hex_u8_field(out, structure, "Cache associativity", 0x12);
            hex_u32_field(out, structure, "Maximum cache size 2 raw", 0x13);
            hex_u32_field(out, structure, "Installed cache size 2 raw", 0x17);
        }
        8 => {
            text_field(out, structure, "Internal connector", 0x04);
            hex_u8_field(out, structure, "Internal connector type", 0x05);
            text_field(out, structure, "External connector", 0x06);
            hex_u8_field(out, structure, "External connector type", 0x07);
            hex_u8_field(out, structure, "Port type", 0x08);
        }
        9 => {
            text_field(out, structure, "Slot designation", 0x04);
            hex_u8_field(out, structure, "Slot type", 0x05);
            hex_u8_field(out, structure, "Slot data-bus width", 0x06);
            if let Some(value) = structure.byte(0x07) {
                writeln!(out, "  Slot usage: 0x{:02X} ({})", value, slot_usage(value)).unwrap();
            }
            hex_u8_field(out, structure, "Slot length", 0x08);
            hex_u16_field(out, structure, "Slot ID", 0x09);
            hex_u8_field(out, structure, "Slot characteristics 1", 0x0B);
            hex_u8_field(out, structure, "Slot characteristics 2", 0x0C);
            decimal_u16_field(out, structure, "PCI segment", 0x0D);
            decimal_u8_field(out, structure, "PCI bus", 0x0F);
            if let Some(value) = structure.byte(0x10) {
                writeln!(out, "  PCI device/function: {:02X}:{:X}", value >> 3, value & 0x07)
                    .unwrap();
            }
            hex_u8_field(out, structure, "Slot physical width", 0x11);
            decimal_u16_field(out, structure, "Slot pitch", 0x12);
            hex_u8_field(out, structure, "Slot height", 0x14);
        }
        11 => decimal_u8_field(out, structure, "OEM string count", 0x04),
        12 => decimal_u8_field(out, structure, "Configuration string count", 0x04),
        13 => {
            decimal_u8_field(out, structure, "Installable language count", 0x04);
            hex_u8_field(out, structure, "Language flags", 0x05);
            text_field(out, structure, "Current language", 0x15);
        }
        16 => {
            hex_u8_field(out, structure, "Memory-array location", 0x04);
            hex_u8_field(out, structure, "Memory-array use", 0x05);
            hex_u8_field(out, structure, "Memory-array error correction", 0x06);
            if let Some(kib) = structure.u32(0x07) {
                if kib == 0x8000_0000 {
                    if let Some(bytes) = structure.u64(0x0F) {
                        value_field(out, "Maximum memory capacity bytes", bytes);
                    }
                } else {
                    value_field(out, "Maximum memory capacity bytes", u64::from(kib) * 1024);
                }
            }
            hex_u16_field(out, structure, "Memory error handle", 0x0B);
            decimal_u16_field(out, structure, "Memory device count", 0x0D);
        }
        17 => append_memory_device(out, structure),
        22 => {
            text_field(out, structure, "Battery location", 0x04);
            text_field(out, structure, "Battery manufacturer", 0x05);
            text_field(out, structure, "Battery manufacture date", 0x06);
            text_field(out, structure, "Battery serial", 0x07);
            text_field(out, structure, "Battery device name", 0x08);
            hex_u8_field(out, structure, "Battery chemistry", 0x09);
            decimal_u16_field(out, structure, "Battery design capacity", 0x0A);
            decimal_u16_field(out, structure, "Battery design voltage mV", 0x0C);
            text_field(out, structure, "Battery SBDS version", 0x0E);
            decimal_u8_field(out, structure, "Battery capacity multiplier", 0x15);
            hex_u32_field(out, structure, "Battery OEM-specific", 0x16);
        }
        23 => {
            hex_u8_field(out, structure, "Reset capabilities", 0x04);
            hex_u16_field(out, structure, "Reset count", 0x05);
            hex_u16_field(out, structure, "Reset limit", 0x07);
            hex_u16_field(out, structure, "Reset timer interval", 0x09);
            hex_u16_field(out, structure, "Reset timeout", 0x0B);
        }
        24 => hex_u8_field(out, structure, "Hardware security settings", 0x04),
        32 => {
            if let Some(value) = structure.byte(0x0A) {
                writeln!(out, "  Last boot status: 0x{:02X} ({})", value, boot_status(value))
                    .unwrap();
            }
        }
        38 => {
            hex_u8_field(out, structure, "IPMI interface type", 0x04);
            version_pair_field(out, structure, "IPMI specification", 0x05, 0x05);
            hex_u8_field(out, structure, "IPMI I2C slave address", 0x06);
            hex_u8_field(out, structure, "IPMI NV storage address", 0x07);
            hex_u64_field(out, structure, "IPMI base address", 0x08);
            hex_u8_field(out, structure, "IPMI base modifiers", 0x10);
            decimal_u8_field(out, structure, "IPMI interrupt", 0x11);
        }
        39 => {
            decimal_u8_field(out, structure, "Power-supply group", 0x04);
            text_field(out, structure, "Power-supply location", 0x05);
            text_field(out, structure, "Power-supply device name", 0x06);
            text_field(out, structure, "Power-supply manufacturer", 0x07);
            text_field(out, structure, "Power-supply serial", 0x08);
            text_field(out, structure, "Power-supply asset tag", 0x09);
            text_field(out, structure, "Power-supply model", 0x0A);
            text_field(out, structure, "Power-supply revision", 0x0B);
            decimal_u16_field(out, structure, "Power-supply max watts", 0x0C);
            hex_u16_field(out, structure, "Power-supply characteristics", 0x0E);
        }
        41 => {
            text_field(out, structure, "Onboard device designation", 0x04);
            hex_u8_field(out, structure, "Onboard device type/enable", 0x05);
            decimal_u8_field(out, structure, "Onboard device instance", 0x06);
            decimal_u16_field(out, structure, "Onboard PCI segment", 0x07);
            decimal_u8_field(out, structure, "Onboard PCI bus", 0x09);
            if let Some(value) = structure.byte(0x0A) {
                writeln!(
                    out,
                    "  Onboard PCI device/function: {:02X}:{:X}",
                    value >> 3,
                    value & 0x07
                )
                .unwrap();
            }
        }
        42 => {
            hex_u8_field(out, structure, "Management host-interface type", 0x04);
            decimal_u8_field(out, structure, "Interface-specific bytes", 0x05);
        }
        43 => append_tpm_device(out, structure),
        44 => {
            hex_u16_field(out, structure, "Referenced processor handle", 0x04);
            decimal_u8_field(out, structure, "Processor-specific block length", 0x06);
            hex_u8_field(out, structure, "Processor architecture type", 0x07);
        }
        45 => append_firmware_inventory(out, structure),
        46 => {
            hex_u16_field(out, structure, "String property ID", 0x04);
            text_field(out, structure, "String property value", 0x06);
            hex_u16_field(out, structure, "String property parent handle", 0x07);
        }
        _ => {}
    }
}

fn append_memory_device(out: &mut String, structure: Structure<'_>) {
    hex_u16_field(out, structure, "Physical-memory-array handle", 0x04);
    hex_u16_field(out, structure, "Memory-error handle", 0x06);
    decimal_u16_field(out, structure, "Total width bits", 0x08);
    decimal_u16_field(out, structure, "Data width bits", 0x0A);
    if let Some(raw) = structure.u16(0x0C) {
        let description = match raw {
            0 => String::from("not installed"),
            0xFFFF => String::from("unknown"),
            0x7FFF => structure
                .u32(0x1C)
                .map(|mib| alloc::format!("{} MiB (extended)", mib))
                .unwrap_or_else(|| String::from("extended-size field missing")),
            value if value & 0x8000 != 0 => {
                alloc::format!("{} KiB", value & 0x7FFF)
            }
            value => alloc::format!("{} MiB", value),
        };
        writeln!(out, "  Memory size: 0x{:04X} ({})", raw, description).unwrap();
    }
    hex_u8_field(out, structure, "Memory form factor", 0x0E);
    hex_u8_field(out, structure, "Memory device set", 0x0F);
    text_field(out, structure, "Memory device locator", 0x10);
    text_field(out, structure, "Memory bank locator", 0x11);
    hex_u8_field(out, structure, "Memory type", 0x12);
    hex_u16_field(out, structure, "Memory type detail", 0x13);
    decimal_u16_field(out, structure, "Memory speed MT/s", 0x15);
    text_field(out, structure, "Memory manufacturer", 0x17);
    text_field(out, structure, "Memory serial", 0x18);
    text_field(out, structure, "Memory asset tag", 0x19);
    text_field(out, structure, "Memory part number", 0x1A);
    hex_u8_field(out, structure, "Memory attributes", 0x1B);
    decimal_u16_field(out, structure, "Configured memory speed MT/s", 0x20);
    decimal_u16_field(out, structure, "Minimum voltage mV", 0x22);
    decimal_u16_field(out, structure, "Maximum voltage mV", 0x24);
    decimal_u16_field(out, structure, "Configured voltage mV", 0x26);
    hex_u8_field(out, structure, "Memory technology", 0x28);
    hex_u16_field(out, structure, "Memory operating-mode capability", 0x29);
    text_field(out, structure, "Memory firmware version", 0x2B);
    hex_u16_field(out, structure, "Module manufacturer ID", 0x2C);
    hex_u16_field(out, structure, "Module product ID", 0x2E);
    hex_u16_field(out, structure, "Memory-controller manufacturer ID", 0x30);
    hex_u16_field(out, structure, "Memory-controller product ID", 0x32);
    if let Some(value) = structure.u64(0x34) {
        value_field(out, "Non-volatile memory bytes", value);
    }
    if let Some(value) = structure.u64(0x3C) {
        value_field(out, "Volatile memory bytes", value);
    }
    if let Some(value) = structure.u64(0x44) {
        value_field(out, "Memory cache bytes", value);
    }
    if let Some(value) = structure.u64(0x4C) {
        value_field(out, "Logical memory bytes", value);
    }
    decimal_u32_field(out, structure, "Extended speed MT/s", 0x54);
    decimal_u32_field(out, structure, "Extended configured speed MT/s", 0x58);
}

fn append_tpm_device(out: &mut String, structure: Structure<'_>) {
    if let Some(vendor) = structure.bytes(0x04, 4) {
        writeln!(out, "  TPM vendor ID: {}", firmware_text(vendor)).unwrap();
    }
    version_pair_field(out, structure, "TPM specification", 0x08, 0x09);
    hex_u32_field(out, structure, "TPM firmware revision 1", 0x0A);
    hex_u32_field(out, structure, "TPM firmware revision 2", 0x0E);
    text_field(out, structure, "TPM description", 0x12);
    hex_u64_field(out, structure, "TPM characteristics", 0x13);
    hex_u32_field(out, structure, "TPM OEM-defined", 0x1B);
}

fn append_firmware_inventory(out: &mut String, structure: Structure<'_>) {
    text_field(out, structure, "Firmware component name", 0x04);
    text_field(out, structure, "Firmware version", 0x05);
    hex_u8_field(out, structure, "Firmware version format", 0x06);
    text_field(out, structure, "Firmware ID", 0x07);
    hex_u8_field(out, structure, "Firmware ID format", 0x08);
    text_field(out, structure, "Firmware component release date", 0x09);
    text_field(out, structure, "Firmware component manufacturer", 0x0A);
    text_field(out, structure, "Lowest supported firmware version", 0x0B);
    if let Some(value) = structure.u64(0x0C) {
        value_field(out, "Firmware image size bytes", value);
    }
    hex_u16_field(out, structure, "Firmware characteristics", 0x14);
    hex_u8_field(out, structure, "Firmware state", 0x16);
    decimal_u8_field(out, structure, "Associated component count", 0x17);
}

fn append_formatted_bytes(out: &mut String, structure: Structure<'_>) {
    writeln!(out, "  Formatted bytes:").unwrap();
    for (row, chunk) in structure.formatted().chunks(16).enumerate() {
        write!(out, "    {:04X}:", row * 16).unwrap();
        for byte in chunk {
            write!(out, " {:02X}", byte).unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn append_strings(out: &mut String, structure: Structure<'_>) {
    let mut count = 0usize;
    for (index, bytes) in structure.strings().enumerate() {
        count += 1;
        write!(out, "  String[{}]: \"{}\" raw=", index + 1, firmware_text(bytes)).unwrap();
        if bytes.is_empty() {
            write!(out, "-").unwrap();
        } else {
            for byte in bytes {
                write!(out, "{:02X}", byte).unwrap();
            }
        }
        writeln!(out).unwrap();
    }
    if count == 0 {
        writeln!(out, "  Strings: none").unwrap();
    }
}

fn firmware_text(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for ch in decoded.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            value if value.is_control() => {
                write!(out, "\\u{{{:X}}}", value as u32).unwrap();
            }
            value => out.push(value),
        }
    }
    out
}

fn text_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    let Some(index) = structure.byte(offset) else {
        return;
    };
    let value = structure
        .string_bytes(index)
        .map(firmware_text)
        .unwrap_or_else(|| {
            if index == 0 {
                String::from("-")
            } else {
                alloc::format!("<missing string {}>", index)
            }
        });
    writeln!(out, "  {}: {} [string={}]", name, value, index).unwrap();
}

fn value_field(out: &mut String, name: &str, value: u64) {
    writeln!(out, "  {}: {} (0x{:X})", name, value, value).unwrap();
}

fn hex_u8_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.byte(offset) {
        writeln!(out, "  {}: 0x{:02X}", name, value).unwrap();
    }
}

fn hex_u16_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.u16(offset) {
        writeln!(out, "  {}: 0x{:04X}", name, value).unwrap();
    }
}

fn hex_u32_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.u32(offset) {
        writeln!(out, "  {}: 0x{:08X}", name, value).unwrap();
    }
}

fn hex_u64_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.u64(offset) {
        writeln!(out, "  {}: 0x{:016X}", name, value).unwrap();
    }
}

fn decimal_u8_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.byte(offset) {
        writeln!(out, "  {}: {}", name, value).unwrap();
    }
}

fn decimal_u16_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.u16(offset) {
        writeln!(out, "  {}: {}", name, value).unwrap();
    }
}

fn decimal_u32_field(out: &mut String, structure: Structure<'_>, name: &str, offset: usize) {
    if let Some(value) = structure.u32(offset) {
        writeln!(out, "  {}: {}", name, value).unwrap();
    }
}

fn version_pair_field(
    out: &mut String,
    structure: Structure<'_>,
    name: &str,
    major_offset: usize,
    minor_offset: usize,
) {
    let (Some(major), Some(minor)) = (structure.byte(major_offset), structure.byte(minor_offset))
    else {
        return;
    };
    if major == 0xFF && minor == 0xFF {
        writeln!(out, "  {}: unknown", name).unwrap();
    } else if major_offset == minor_offset {
        writeln!(out, "  {}: {}.{}", name, major >> 4, major & 0x0F).unwrap();
    } else {
        writeln!(out, "  {}: {}.{}", name, major, minor).unwrap();
    }
}

fn processor_count_field(
    out: &mut String,
    structure: Structure<'_>,
    name: &str,
    legacy_offset: usize,
    extended_offset: usize,
) {
    let Some(legacy) = structure.byte(legacy_offset) else {
        return;
    };
    let value = if legacy == 0xFF {
        structure.u16(extended_offset).map(u32::from)
    } else {
        Some(u32::from(legacy))
    };
    if let Some(value) = value {
        writeln!(out, "  {}: {}", name, value).unwrap();
    }
}

fn platform_firmware_rom_bytes(structure: Structure<'_>) -> Option<u64> {
    let legacy = structure.byte(0x09)?;
    if legacy != 0xFF {
        return Some((u64::from(legacy) + 1) * 64 * 1024);
    }
    let extended = structure.u16(0x18)?;
    let magnitude = u64::from(extended & 0x3FFF);
    let unit = match extended >> 14 {
        0 => 1024 * 1024,
        1 => 1024 * 1024 * 1024,
        _ => return None,
    };
    Some(magnitude * unit)
}

fn format_uuid(bytes: &[u8]) -> String {
    if bytes.len() != 16 {
        return String::from("<invalid>");
    }
    alloc::format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[3],
        bytes[2],
        bytes[1],
        bytes[0],
        bytes[5],
        bytes[4],
        bytes[7],
        bytes[6],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

const fn wake_up_type(value: u8) -> &'static str {
    match value {
        0 => "reserved",
        1 => "other",
        2 => "unknown",
        3 => "APM timer",
        4 => "modem ring",
        5 => "LAN remote",
        6 => "power switch",
        7 => "PCI PME",
        8 => "AC power restored",
        _ => "reserved",
    }
}

const fn baseboard_type(value: u8) -> &'static str {
    match value {
        1 => "unknown",
        2 => "other",
        3 => "server blade",
        4 => "connectivity switch",
        5 => "system-management module",
        6 => "processor module",
        7 => "I/O module",
        8 => "memory module",
        9 => "daughterboard",
        10 => "motherboard",
        11 => "processor/memory module",
        12 => "processor/I/O module",
        13 => "interconnect board",
        _ => "reserved",
    }
}

const fn chassis_type(value: u8) -> &'static str {
    match value {
        1 => "other",
        2 => "unknown",
        3 => "desktop",
        4 => "low-profile desktop",
        5 => "pizza box",
        6 => "mini tower",
        7 => "tower",
        8 => "portable",
        9 => "laptop",
        10 => "notebook",
        11 => "handheld",
        12 => "docking station",
        13 => "all in one",
        14 => "sub-notebook",
        15 => "space-saving",
        16 => "lunch box",
        17 => "main server chassis",
        18 => "expansion chassis",
        23 => "rack mount",
        30 => "tablet",
        31 => "convertible",
        32 => "detachable",
        35 => "mini PC",
        36 => "stick PC",
        _ => "reserved/other",
    }
}

const fn processor_status(value: u8) -> &'static str {
    match value {
        0 => "unknown",
        1 => "enabled",
        2 => "disabled by user",
        3 => "disabled by firmware",
        4 => "idle",
        7 => "other",
        _ => "reserved",
    }
}

const fn slot_usage(value: u8) -> &'static str {
    match value {
        1 => "other",
        2 => "unknown",
        3 => "available",
        4 => "in use",
        5 => "unavailable",
        _ => "reserved",
    }
}

const fn boot_status(value: u8) -> &'static str {
    match value {
        0 => "no errors",
        1 => "no bootable media",
        2 => "normal OS failed to load",
        3 => "firmware-detected hardware failure",
        4 => "OS-detected hardware failure",
        5 => "user-requested boot",
        6 => "system security violation",
        7 => "previously requested image",
        8 => "watchdog expired",
        128..=191 => "OEM-specific",
        192..=255 => "product-specific",
        _ => "reserved",
    }
}
