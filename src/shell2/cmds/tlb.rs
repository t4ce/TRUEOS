use core::fmt::Write;
use core::str::SplitWhitespace;

use acpi::sdt::fadt::Fadt;
use acpi::sdt::madt::Madt;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) const DUMP_FILE_PATH: &str = "trueos/pci/tlb.txt";

const TLB_USAGE: &str = "tlb: usage `tlb [pci|pciids|pcibar|mem|cpu|acpi|facp|madt|hpet|mcfg|ssdt|uefi|x2apic|usb|dump]`";
const TLB_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const TLB_MENU_ROWS: [(&str, &str); 15] = [
    ("pci", "List PCI devices"),
    ("pciids", "Download pci.ids once"),
    ("pcibar", "List PCI BAR windows"),
    ("mem", "List memory map"),
    ("cpu", "List CPU cores"),
    ("acpi", "List ACPI tables"),
    ("facp", "Show FACP/FADT details"),
    ("madt", "Show MADT details"),
    ("hpet", "Show HPET details"),
    ("mcfg", "Show MCFG details"),
    ("ssdt", "Show SSDT details"),
    ("uefi", "List UEFI tables"),
    ("x2apic", "List x2APIC topology"),
    ("usb", "List USB controllers and ports"),
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
    emit_table_row(
        io,
        cols,
        &cols.iter().map(|col| col.header).collect::<Vec<_>>(),
    );
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
    let table = TlbTable::with_width(
        &TLB_MENU_HEADERS,
        line_width_for_backend(io).saturating_sub(2),
    );

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

    let cols = [
        Column {
            header: "Name",
            width: 40,
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

fn cmd_tlb_pciids(io: &'static dyn ShellBackend2) {
    crate::pci::pciids::download_once_detached();
    line(io, "tlb pciids: scheduled background download");
    line(
        io,
        "tlb pciids: check global log for success/timeout/failure",
    );
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
        let ty = crate::limine::memmap_type_name(entry.entry_type);
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

fn cmd_tlb_acpi(io: &'static dyn ShellBackend2) {
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

    for (phys, hdr) in tables.table_headers() {
        let addr = alloc::format!("0x{:08X}", phys);
        let length = hdr.length;
        let revision = hdr.revision;
        let len = alloc::format!("0x{:X}", length);
        let rev = alloc::format!("{}", revision);
        let oem = core::str::from_utf8(&hdr.oem_id).unwrap_or("      ");
        let table_id = core::str::from_utf8(&hdr.oem_table_id).unwrap_or("        ");
        emit_table_row(
            io,
            &cols,
            &[hdr.signature.as_str(), &addr, &len, &rev, oem, table_id],
        );
    }
}

fn cmd_tlb_facp(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb facp: no tables found");
        return;
    };

    if let Some(fadt) = tables.find_table::<Fadt>() {
        line(
            io,
            alloc::format!("FACP/FADT Found @ 0x{:X}", fadt.physical_start).as_str(),
        );
        multiline(
            io,
            alloc::format!("{:#?}", unsafe { fadt.virtual_start.as_ref() }).as_str(),
        );
    } else {
        line(io, "FACP: Not found");
    }
}

fn cmd_tlb_madt(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb madt: no tables found");
        return;
    };

    if let Some(madt) = tables.find_table::<Madt>() {
        line(
            io,
            alloc::format!("MADT Found @ 0x{:X}", madt.physical_start).as_str(),
        );
        multiline(
            io,
            alloc::format!("{:#?}", unsafe { madt.virtual_start.as_ref() }).as_str(),
        );
    } else {
        line(io, "MADT: Not found");
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
    line(io, "MCFG: Command disabled due to compilation error");
}

fn cmd_tlb_ssdt(io: &'static dyn ShellBackend2) {
    let Some(tables) = crate::efi::acpi::ensure_tables() else {
        line(io, "tlb ssdt: no tables found");
        return;
    };

    line(
        io,
        "Scanning for SSDT tables (Secondary System Description Table)...",
    );
    blank(io);

    let mut count = 0;
    for (phys, hdr) in tables.table_headers() {
        if hdr.signature.as_str() == "SSDT" {
            count += 1;
            let length = hdr.length;
            let revision = hdr.revision;
            line(
                io,
                alloc::format!("SSDT #{} @ 0x{:08X}", count, phys).as_str(),
            );
            line(io, alloc::format!("  Length: {} bytes", length).as_str());
            line(io, alloc::format!("  Revision: {}", revision).as_str());
            line(
                io,
                alloc::format!(
                    "  OEM ID: {}",
                    core::str::from_utf8(&hdr.oem_id).unwrap_or("      ")
                )
                .as_str(),
            );
            line(
                io,
                alloc::format!(
                    "  Table ID: {}",
                    core::str::from_utf8(&hdr.oem_table_id).unwrap_or("        ")
                )
                .as_str(),
            );
            line(
                io,
                "  (Raw AML content not dumped/parsed in 'best effort' mode)",
            );
            blank(io);
        }
    }

    if count == 0 {
        line(io, "No SSDT tables found.");
    } else {
        line(io, alloc::format!("Found {} SSDT tables.", count).as_str());
    }
}

fn cmd_tlb_uefi(io: &'static dyn ShellBackend2) {
    let Some(st) = crate::efi::system_table() else {
        line(
            io,
            "tlb uefi: system table not found (not booted via UEFI?)",
        );
        return;
    };

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
    emit_table_row(
        io,
        &summary_cols,
        &["Revision", &alloc::format!("0x{:08X}", st_revision)],
    );
    emit_table_row(
        io,
        &summary_cols,
        &["Header Size", &alloc::format!("0x{:X}", st_header_size)],
    );
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

    if let Some(phys) = crate::limine::try_as_phys_addr(cfg_addr)
        && let Ok((cfg_ptr, _)) =
            crate::pci::mmio::map_limine_slice::<crate::efi::EfiConfigurationTable>(phys, entries)
    {
        let slice = unsafe { core::slice::from_raw_parts(cfg_ptr.as_ptr(), entries) };
        for (index, entry) in slice.iter().enumerate() {
            let idx = alloc::format!("{}", index);
            let name = crate::efi::cfg_guid_name(&entry.vendor_guid).unwrap_or("Unknown");
            let guid = entry.vendor_guid.fmt_canonical();
            let ptr = alloc::format!("0x{:016X}", entry.vendor_table as u64);
            emit_table_row(io, &cfg_cols, &[&idx, &guid, name, &ptr]);
        }
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

fn cmd_tlb_usb(io: &'static dyn ShellBackend2) {
    let snapshot = crate::usb2::tlb_snapshot();
    if snapshot.controllers.is_empty() {
        line(io, "tlb usb: no xhci controllers found");
        return;
    }

    // The shell renders System lines newest-first (each line() inserts at top of scroll
    // area). Collect all output first, then emit in reverse so the first logical line
    // ends up being the last thing written → lands at the top of the screen.
    let mut out: Vec<String> = Vec::new();

    for ctrl_info in snapshot.controllers.iter() {
        let bdf = alloc::format!(
            "{:02X}:{:02X}.{}",
            ctrl_info.bus,
            ctrl_info.slot,
            ctrl_info.function
        );
        let diag = alloc::format!(
            "ev={} rp={} empty={}",
            ctrl_info.event_ready as u8,
            ctrl_info.root_port_change_seen as u8,
            ctrl_info.empty_probe_streak
        );
        out.push(alloc::format!(
            "xhci ctrl={} bdf={} diag={} vidpid={:04X}:{:04X} mmio=0x{:X}",
            ctrl_info.index,
            bdf,
            diag,
            ctrl_info.vendor_id,
            ctrl_info.device_id,
            ctrl_info.mmio_base.as_ptr() as usize
        ));

        let mut emitted_any = false;
        for dev in snapshot
            .devices
            .iter()
            .filter(|dev| dev.controller_index == ctrl_info.index)
        {
            emitted_any = true;
            out.push(alloc::format!(
                "  dev slot={} vidpid={:04X}:{:04X} dev={:02X}/{:02X}/{:02X} cfgs={} ep0_mps={}",
                dev.slot_id,
                dev.vendor_id,
                dev.product_id,
                dev.class,
                dev.subclass,
                dev.protocol,
                dev.num_configurations,
                dev.max_packet_size_0
            ));

            for cfg in dev.configurations.iter() {
                out.push(alloc::format!(
                    "    cfg value={} attrs=0x{:02X} max_power={} ifs={}",
                    cfg.configuration_value,
                    cfg.attributes,
                    cfg.max_power,
                    cfg.interfaces.len()
                ));

                for iface in cfg.interfaces.iter() {
                    out.push(alloc::format!(
                        "      if num={} alt={} class={:02X}/{:02X}/{:02X} eps={}",
                        iface.interface_number,
                        iface.alternate_setting,
                        iface.class,
                        iface.subclass,
                        iface.protocol,
                        iface.endpoints.len()
                    ));

                    for ep in iface.endpoints.iter() {
                        let direction = if (ep.address & 0x80) != 0 {
                            "in"
                        } else {
                            "out"
                        };
                        out.push(alloc::format!(
                            "        ep addr=0x{:02X} {} {} mps={} interval={}",
                            ep.address,
                            ep.transfer_type,
                            direction,
                            ep.max_packet_size,
                            ep.interval
                        ));
                    }
                }
            }
        }

        if !emitted_any {
            out.push(alloc::format!("  (no leaf devices cached)"));
        }

        if let Some(err) = snapshot.probe_error {
            out.push(alloc::format!("  probe_error={}", err));
        }
        if let Some(n) = snapshot.probe_device_count {
            out.push(alloc::format!("  probe_device_count={}", n));
        }
    }

    for l in out.into_iter().rev() {
        line(io, l.as_str());
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
                crate::limine::memmap_type_name(entry.entry_type)
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
    writeln!(
        out,
        "{:30}  {:10}  {:6}  {:6}",
        "Name", "Address", "VID", "PID"
    )
    .unwrap();
    writeln!(out, "{:-<30}  {:-<10}  {:-<6}  {:-<6}", "", "", "", "").unwrap();
    for row in pci_device_rows(db.as_deref()) {
        let name_disp = if row.name.chars().count() > 30 {
            let mut s: String = row.name.chars().take(29).collect();
            s.push('…');
            s
        } else {
            row.name
        };
        writeln!(
            out,
            "{:30}  {:10}  {:6}  {:6}",
            name_disp, row.addr, row.vid, row.pid
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    write_pci_bar_dump(&mut out);

    writeln!(out, "=== CPU Cores ===").unwrap();
    if !crate::smp::is_init() {
        writeln!(out, "SMP not initialized").unwrap();
    } else {
        writeln!(
            out,
            "{:6}  {:6}  {:8}  {:10}",
            "Slot", "APIC", "Role", "State"
        )
        .unwrap();
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
                writeln!(
                    out,
                    "{:6}  {:<6}  {:<8}  {:<10}",
                    slot, lapic_id, role, state
                )
                .unwrap();
            }
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "=== ACPI Tables ===").unwrap();
    if let Some(tables) = crate::efi::acpi::ensure_tables() {
        writeln!(out, "{:10}  {:18}  {:10}", "Signature", "Address", "Length").unwrap();
        writeln!(out, "{:-<10}  {:-<18}  {:-<10}", "", "", "").unwrap();
        for (phys, hdr) in tables.table_headers() {
            let length = hdr.length;
            writeln!(
                out,
                "{:10}  0x{:016X}  0x{:X}",
                hdr.signature.as_str(),
                phys,
                length
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    } else {
        writeln!(out, "No tables found").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== UEFI Tables ===").unwrap();
    if let Some(st) = crate::efi::system_table() {
        let st_revision = st.hdr.revision;
        writeln!(out, "Signature: EFI SYSTEM TABLE").unwrap();
        writeln!(out, "Revision: 0x{:08X}", st_revision).unwrap();
        writeln!(
            out,
            "Runtime Services: 0x{:016X}",
            st.runtime_services as u64
        )
        .unwrap();
        writeln!(out, "Boot Services: 0x{:016X}", st.boot_services as u64).unwrap();
        writeln!(out).unwrap();

        let entries = st.number_of_table_entries;
        let cfg_addr = st.configuration_table as u64;
        writeln!(
            out,
            "{:6}  {:40}  {:24}  {:18}",
            "Index", "GUID", "Name", "Table Ptr"
        )
        .unwrap();
        writeln!(out, "{:-<6}  {:-<40}  {:-<24}  {:-<18}", "", "", "", "").unwrap();

        if let Some(phys) = crate::limine::try_as_phys_addr(cfg_addr)
            && let Ok((cfg_ptr, _)) = crate::pci::mmio::map_limine_slice::<
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
        }
    } else {
        writeln!(out, "No UEFI system table found").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== x2APIC Topology ===").unwrap();
    let topo = crate::x2apic::detect_x2apic_topology();
    writeln!(
        out,
        "Leaf=0x{:X} SMT_Bits={} Core_Bits={}",
        topo.leaf, topo.smt_bits, topo.core_bits
    )
    .unwrap();
    writeln!(
        out,
        "{:6}  {:10}  {:6}  {:6}  {:6}",
        "Slot", "APIC ID", "Pkg", "Core", "SMT"
    )
    .unwrap();
    writeln!(
        out,
        "{:-<6}  {:-<10}  {:-<6}  {:-<6}  {:-<6}",
        "", "", "", "", ""
    )
    .unwrap();
    let count = crate::smp::cpu_count();
    let slots = crate::percpu::cpu_slots();
    for slot in 0..count {
        let lapic_id = slots
            .iter()
            .find(|s| s.slot == slot as u32)
            .map(|s| s.lapic_id)
            .unwrap_or(0xFFFF_FFFF);
        if lapic_id == 0xFFFF_FFFF {
            writeln!(
                out,
                "{:6}  {:10}  {:6}  {:6}  {:6}",
                slot, "?", "?", "?", "?"
            )
            .unwrap();
            continue;
        }
        let (pkg, core_id, smt) = topo.decode(lapic_id);
        writeln!(
            out,
            "{:6}  0x{:<8X}  {:<6}  {:<6}  {:<6}",
            slot, lapic_id, pkg, core_id, smt
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "=== USB Devices ===").unwrap();
    let snapshot = crate::usb2::tlb_snapshot();
    if snapshot.controllers.is_empty() {
        writeln!(out, "No XHCI controllers found").unwrap();
    } else {
        writeln!(
            out,
            "{:4}  {:10}  {:8}  {:12}  {:11}  {:10}  {:12}",
            "Ctrl", "BDF", "Probe", "Device", "VID:PID", "Class", "Cfg/If"
        )
        .unwrap();
        writeln!(
            out,
            "{:-<4}  {:-<10}  {:-<8}  {:-<12}  {:-<11}  {:-<10}  {:-<12}",
            "", "", "", "", "", "", ""
        )
        .unwrap();
        for ctrl_info in snapshot.controllers.iter() {
            let probe = if let Some(err) = snapshot.probe_error {
                err
            } else if snapshot.devices.is_empty() {
                "empty"
            } else {
                "ok"
            };
            let mut emitted = false;
            for dev in snapshot
                .devices
                .iter()
                .filter(|dev| dev.controller_index == ctrl_info.index)
            {
                let vidpid = alloc::format!("{:04X}:{:04X}", dev.vendor_id, dev.product_id);
                let class = alloc::format!(
                    "{:02X}/{:02X}/{:02X}",
                    dev.class,
                    dev.subclass,
                    dev.protocol
                );
                let interface_count: usize = dev
                    .configurations
                    .iter()
                    .map(|cfg| cfg.interfaces.len())
                    .sum();
                let cfg_if = alloc::format!("{}/{}", dev.configurations.len(), interface_count);
                writeln!(
                    out,
                    "{:<4}  {:02X}:{:02X}.{}  {:<8}  {:<12}  {:<11}  {:<10}  {:<12}",
                    ctrl_info.index,
                    ctrl_info.bus,
                    ctrl_info.slot,
                    ctrl_info.function,
                    probe,
                    "descriptor",
                    vidpid,
                    class,
                    cfg_if
                )
                .unwrap();
                emitted = true;
            }

            if !emitted {
                let vidpid =
                    alloc::format!("{:04X}:{:04X}", ctrl_info.vendor_id, ctrl_info.device_id);
                let mmio = alloc::format!("mmio=0x{:X}", ctrl_info.mmio_base.as_ptr() as usize);
                writeln!(
                    out,
                    "{:<4}  {:02X}:{:02X}.{}  {:<8}  {:<12}  {:<11}  {:<10}  {:<12}",
                    ctrl_info.index,
                    ctrl_info.bus,
                    ctrl_info.slot,
                    ctrl_info.function,
                    probe,
                    "-",
                    vidpid,
                    "xhci",
                    mmio
                )
                .unwrap();
            }
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "=== Network Interfaces ===").unwrap();
    let net_count = crate::net::device_count();
    if net_count == 0 {
        writeln!(out, "No network interfaces found").unwrap();
    } else {
        writeln!(
            out,
            "{:4}  {:20}  {:17}  {:10}",
            "Idx", "Name", "MAC Address", "Primary"
        )
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
            writeln!(
                out,
                "{:<4}  {:<20}  {:<17}  {:<10}",
                index, name, mac, primary_mark
            )
            .unwrap();
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
    line(
        io,
        alloc::format!("Writing {} bytes to {}...", out.len(), DUMP_FILE_PATH).as_str(),
    );

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
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None => print_menu(io),
        Some("pci") if ensure_no_args(io, args, "tlb: usage `tlb pci`") => cmd_tlb_pci(io),
        Some("pciids") if ensure_no_args(io, args, "tlb: usage `tlb pciids`") => cmd_tlb_pciids(io),
        Some("pcibar") if ensure_no_args(io, args, "tlb: usage `tlb pcibar`") => {
            cmd_tlb_pci_bar(io)
        }
        Some("mem") if ensure_no_args(io, args, "tlb: usage `tlb mem`") => cmd_tlb_mem(io),
        Some("cpu") if ensure_no_args(io, args, "tlb: usage `tlb cpu`") => cmd_tlb_cpu(io),
        Some("acpi") if ensure_no_args(io, args, "tlb: usage `tlb acpi`") => cmd_tlb_acpi(io),
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
