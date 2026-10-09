use alloc::{string::String, vec::Vec};
use core::{fmt::Write, str::SplitWhitespace};

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

#[derive(Clone, Copy)]
enum Selection {
    All,
    Pmm,
    Host,
    Vm(u8),
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "ram: usage `ram [pmm|host|vm-id]`");
}

fn format_bytes(bytes: u64) -> alloc::string::String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    let (unit, suffix) = if bytes >= GIB {
        (GIB, "GiB")
    } else if bytes >= MIB {
        (MIB, "MiB")
    } else if bytes >= KIB {
        (KIB, "KiB")
    } else {
        return alloc::format!("{bytes}B");
    };
    let whole = bytes / unit;
    let tenth = bytes
        .saturating_sub(whole.saturating_mul(unit))
        .saturating_mul(10)
        / unit;
    if tenth == 0 {
        alloc::format!("{whole}{suffix}")
    } else {
        alloc::format!("{whole}.{tenth}{suffix}")
    }
}

fn vm_scope(vm_id: u8) -> alloc::string::String {
    let archive = crate::hv::app_vm_archive(vm_id);
    match archive {
        Some(archive) if !archive.is_empty() => alloc::format!("vm{vm_id}:{archive}"),
        _ => alloc::format!("vm{vm_id}"),
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
                let _ = write!(out, "\\u{{{:X}}}", value as u32);
            }
            value => out.push(value),
        }
    }
    out
}

fn module_label(device: &crate::efi::smbios::MemoryDevice<'_>) -> String {
    let locator = device
        .locator
        .map(firmware_text)
        .filter(|text| !text.is_empty());
    let bank = device
        .bank_locator
        .map(firmware_text)
        .filter(|text| !text.is_empty());
    match (locator, bank) {
        (Some(locator), Some(bank)) if locator != bank => alloc::format!("{locator}/{bank}"),
        (Some(locator), _) => locator,
        (_, Some(bank)) => bank,
        _ => alloc::format!("handle:{:04X}", device.handle),
    }
}

fn memory_speed(device: &crate::efi::smbios::MemoryDevice<'_>) -> String {
    device
        .configured_speed_mt_s
        .or(device.speed_mt_s)
        .map(|speed| alloc::format!("{speed}MT/s"))
        .unwrap_or_else(|| String::from("unknown"))
}

fn memory_speed_summary(devices: &[crate::efi::smbios::MemoryDevice<'_>]) -> String {
    let mut minimum = u32::MAX;
    let mut maximum = 0u32;
    for device in devices {
        if matches!(device.size, crate::efi::smbios::MemoryDeviceSize::NotInstalled) {
            continue;
        }
        let Some(speed) = device.configured_speed_mt_s.or(device.speed_mt_s) else {
            continue;
        };
        minimum = minimum.min(speed);
        maximum = maximum.max(speed);
    }
    match (minimum, maximum) {
        (u32::MAX, _) => String::from("unknown"),
        (minimum, maximum) if minimum == maximum => alloc::format!("{minimum}MT/s"),
        (minimum, maximum) => alloc::format!("{minimum}-{maximum}MT/s"),
    }
}

fn collect_memory_modules(width: usize, mut emit: impl FnMut(&str)) {
    let smbios = match crate::efi::smbios::discover() {
        Ok(table) => table,
        Err(error) => {
            emit(
                alloc::format!("ram: physical_modules=unavailable | smbios={}", error.label())
                    .as_str(),
            );
            return;
        }
    };
    let mut structures = smbios.structures();
    let mut devices = Vec::new();
    loop {
        match structures.next_structure() {
            Ok(Some(structure)) => {
                if let Some(device) = structure.memory_device() {
                    devices.push(device);
                }
            }
            Ok(None) => break,
            Err(error) => {
                emit(
                    alloc::format!("ram: physical_modules=incomplete | smbios={error:?}").as_str(),
                );
                break;
            }
        }
    }
    if devices.is_empty() {
        emit("ram: physical_modules=unavailable | smbios=no_type17_records");
        return;
    }

    let installed_bytes = devices
        .iter()
        .filter_map(|device| match device.size {
            crate::efi::smbios::MemoryDeviceSize::Bytes(bytes) => Some(bytes),
            _ => None,
        })
        .fold(0u64, u64::saturating_add);
    let installed_count = devices
        .iter()
        .filter(|device| !matches!(device.size, crate::efi::smbios::MemoryDeviceSize::NotInstalled))
        .count();
    let capacity_unknown = devices
        .iter()
        .any(|device| matches!(device.size, crate::efi::smbios::MemoryDeviceSize::Unknown));
    let installed = if capacity_unknown {
        alloc::format!(">={}", format_bytes(installed_bytes))
    } else {
        format_bytes(installed_bytes)
    };
    emit(
        alloc::format!(
            "ram: memory_speed={} | physical_modules={}/{} | installed={}",
            memory_speed_summary(devices.as_slice()),
            installed_count,
            devices.len(),
            installed
        )
        .as_str(),
    );

    const HEADERS: [&str; 4] = ["physical module", "size", "speed", "installed share"];
    let table = TlbTable::with_width(&HEADERS, width.saturating_sub(2))
        .with_max_col_widths(&[28, 9, 12, 0]);
    table.emit_header(|text| emit(text));
    for device in &devices {
        let label = module_label(device);
        let (size, share) = match device.size {
            crate::efi::smbios::MemoryDeviceSize::NotInstalled => (
                String::from("empty"),
                alloc::format!("  0% {}", crate::ram_usage::bar_text(0, 10)),
            ),
            crate::efi::smbios::MemoryDeviceSize::Unknown => {
                (String::from("unknown"), String::from("unknown"))
            }
            crate::efi::smbios::MemoryDeviceSize::Bytes(bytes) => {
                let percent = crate::ram_usage::use_percent(bytes, installed_bytes);
                (
                    format_bytes(bytes),
                    alloc::format!("{percent:>3}% {}", crate::ram_usage::bar_text(percent, 10)),
                )
            }
        };
        let speed = if matches!(device.size, crate::efi::smbios::MemoryDeviceSize::NotInstalled) {
            String::from("-")
        } else {
            memory_speed(device)
        };
        let row = [label, size, speed, share];
        table.emit_row(&row, |text| emit(text));
    }
    table.emit_footer(|text| emit(text));
}

pub(crate) fn memory_modules_snapshot(width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    collect_memory_modules(width, |line| lines.push(line.into()));
    lines
}

fn pmm_row() -> Option<[String; 7]> {
    let stats = crate::phys::pmm_stats()?;
    let used = stats.total_bytes.saturating_sub(stats.free_bytes);
    let percent = crate::ram_usage::use_percent(used, stats.total_bytes);
    Some([
        String::from("pmm"),
        format_bytes(used),
        format_bytes(stats.total_bytes),
        format_bytes(stats.free_bytes),
        format_bytes(stats.largest_free_region),
        alloc::format!("{}", stats.free_regions),
        crate::ram_usage::chart_text(percent, &crate::ram_usage::pmm_history_text()),
    ])
}
fn heap_row(scope: String, stats: crate::allocators::HeapStats, history: String) -> [String; 7] {
    let total = stats.usable_total as u64;
    let free = stats.free_bytes as u64;
    let used = total.saturating_sub(free);
    let percent = crate::ram_usage::use_percent(used, total);
    [
        scope,
        format_bytes(used),
        format_bytes(total),
        format_bytes(free),
        format_bytes(stats.largest_free_block as u64),
        alloc::format!("{}", stats.free_blocks),
        crate::ram_usage::chart_text(percent, &history),
    ]
}
fn emit_pmm_row(table: &TlbTable<'_>, io: &'static dyn ShellBackend2) -> bool {
    let Some(row) = pmm_row() else {
        return false;
    };
    table.emit_row(&row, |text| print_shell_line(io, text));
    true
}
fn emit_heap_row(
    table: &TlbTable<'_>,
    io: &'static dyn ShellBackend2,
    scope: String,
    stats: crate::allocators::HeapStats,
    history: String,
) {
    table.emit_row(&heap_row(scope, stats, history), |text| print_shell_line(io, text));
}
fn usage_rows() -> Vec<[String; 7]> {
    let mut rows = Vec::new();
    if let Some(row) = pmm_row() {
        rows.push(row);
    }
    let host = crate::allocators::heap_stats();
    if host.initialized && host.usable_total != 0 {
        rows.push(heap_row(String::from("host"), host, crate::ram_usage::host_history_text()));
    }
    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        if let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id as u8) {
            rows.push(heap_row(
                vm_scope(vm_id as u8),
                stats,
                crate::ram_usage::vm_history_text(vm_id as u8),
            ));
        }
    }
    rows
}
const USAGE_HEADERS: [&str; 7] = [
    "scope",
    "used",
    "total",
    "free",
    "largest",
    "chunks",
    "use / recent",
];
fn usage_table(width: usize) -> TlbTable<'static> {
    TlbTable::with_width(&USAGE_HEADERS, width.saturating_sub(2))
        .with_max_col_widths(&[18, 9, 9, 9, 9, 6, 0])
}
/// Read the live counters and existing history without adding an extra sample.
pub(crate) fn usage_snapshot(width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(alloc::format!(
        "PMM = reserved physical; heap use is nested. History: {} × {} ms; samples={}",
        crate::ram_usage::HISTORY_LEN,
        crate::ram_usage::SAMPLE_MS,
        crate::ram_usage::sample_count()
    ));
    let table = usage_table(width);
    table.emit_header(|line| lines.push(line.into()));
    for row in usage_rows() {
        table.emit_row(&row, |line| lines.push(line.into()));
    }
    table.emit_footer(|line| lines.push(line.into()));
    lines
}

fn parse_selection(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> Option<Selection> {
    let Some(raw) = args.next() else {
        return Some(Selection::All);
    };
    if args.next().is_some() {
        print_usage(io);
        return None;
    }
    match raw {
        "pmm" => Some(Selection::Pmm),
        "host" => Some(Selection::Host),
        _ => {
            let Ok(vm_id) = raw.parse::<u8>() else {
                print_usage(io);
                return None;
            };
            if vm_id as usize >= crate::allcaps::hv::VM_ID_LIMIT {
                print_usage(io);
                return None;
            }
            Some(Selection::Vm(vm_id))
        }
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(selection) = parse_selection(io, args) else {
        return ParseOutcome::Handled;
    };

    // Include a command-time sample so the dump is useful even if the
    // background system-service sampler has not started yet.
    crate::ram_usage::sample_once();
    print_shell_line(
        io,
        alloc::format!(
            "pmm=reserved_physical | heap rows=nested_allocator_use | recent={}.{:04}s | samples={}",
            (crate::ram_usage::HISTORY_LEN as u64)
                .saturating_mul(crate::ram_usage::SAMPLE_MS)
                / 1_000,
            (crate::ram_usage::HISTORY_LEN as u64)
                .saturating_mul(crate::ram_usage::SAMPLE_MS)
                .rem_euclid(1_000)
                .saturating_mul(10),
            crate::ram_usage::sample_count()
        )
        .as_str(),
    );

    if matches!(selection, Selection::All | Selection::Pmm) {
        for line in memory_modules_snapshot(line_width_for_backend(io)) {
            print_shell_line(io, &line);
        }
    }

    let table = usage_table(line_width_for_backend(io));
    table.emit_header(|text| print_shell_line(io, text));

    match selection {
        Selection::All => {
            for row in usage_rows() {
                table.emit_row(&row, |text| print_shell_line(io, text));
            }
        }
        Selection::Pmm => {
            if !emit_pmm_row(&table, io) {
                print_shell_line(io, "ram: pmm not initialized");
            }
        }
        Selection::Host => emit_heap_row(
            &table,
            io,
            alloc::string::String::from("host"),
            crate::allocators::heap_stats(),
            crate::ram_usage::host_history_text(),
        ),
        Selection::Vm(vm_id) => {
            if let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id) {
                emit_heap_row(
                    &table,
                    io,
                    vm_scope(vm_id),
                    stats,
                    crate::ram_usage::vm_history_text(vm_id),
                );
            } else {
                let row = [
                    vm_scope(vm_id),
                    alloc::string::String::from("-"),
                    alloc::string::String::from("-"),
                    alloc::string::String::from("-"),
                    alloc::string::String::from("-"),
                    alloc::string::String::from("-"),
                    alloc::string::String::from("not configured"),
                ];
                table.emit_row(&row, |text| print_shell_line(io, text));
            }
        }
    }

    table.emit_footer(|text| print_shell_line(io, text));
    ParseOutcome::Handled
}
