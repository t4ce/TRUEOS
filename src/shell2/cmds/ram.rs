use core::str::SplitWhitespace;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

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

fn emit_pmm_row(table: &TlbTable<'_>, io: &'static dyn ShellBackend2) -> bool {
    let Some(stats) = crate::phys::pmm_stats() else {
        return false;
    };
    let used = stats.total_bytes.saturating_sub(stats.free_bytes);
    let percent = crate::ram_usage::use_percent(used, stats.total_bytes);
    let row = [
        alloc::string::String::from("pmm"),
        format_bytes(used),
        format_bytes(stats.total_bytes),
        format_bytes(stats.free_bytes),
        format_bytes(stats.largest_free_region),
        alloc::format!("{}", stats.free_regions),
        crate::ram_usage::chart_text(percent, crate::ram_usage::pmm_history_text().as_str()),
    ];
    table.emit_row(&row, |text| print_shell_line(io, text));
    true
}

fn emit_heap_row(
    table: &TlbTable<'_>,
    io: &'static dyn ShellBackend2,
    scope: alloc::string::String,
    stats: crate::allocators::HeapStats,
    history: alloc::string::String,
) {
    let total = stats.usable_total as u64;
    let free = stats.free_bytes as u64;
    let used = total.saturating_sub(free);
    let percent = crate::ram_usage::use_percent(used, total);
    let row = [
        scope,
        format_bytes(used),
        format_bytes(total),
        format_bytes(free),
        format_bytes(stats.largest_free_block as u64),
        alloc::format!("{}", stats.free_blocks),
        crate::ram_usage::chart_text(percent, history.as_str()),
    ];
    table.emit_row(&row, |text| print_shell_line(io, text));
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
            "ram: recent={}x{}ms samples={} (oldest->newest, row-relative)",
            crate::ram_usage::HISTORY_LEN,
            crate::ram_usage::SAMPLE_MS,
            crate::ram_usage::sample_count()
        )
        .as_str(),
    );
    print_shell_line(io, "ram: best effort; pmm=reserved physical, heap rows=nested allocator use");

    const HEADERS: [&str; 7] = [
        "scope",
        "used",
        "total",
        "free",
        "largest",
        "chunks",
        "use / recent",
    ];
    let table = TlbTable::with_width(&HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[18, 9, 9, 9, 9, 6, 0]);
    table.emit_header(|text| print_shell_line(io, text));

    match selection {
        Selection::All => {
            let _ = emit_pmm_row(&table, io);
            let host = crate::allocators::heap_stats();
            if host.initialized && host.usable_total != 0 {
                emit_heap_row(
                    &table,
                    io,
                    alloc::string::String::from("host"),
                    host,
                    crate::ram_usage::host_history_text(),
                );
            }
            for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
                let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id as u8)
                else {
                    continue;
                };
                emit_heap_row(
                    &table,
                    io,
                    vm_scope(vm_id as u8),
                    stats,
                    crate::ram_usage::vm_history_text(vm_id as u8),
                );
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
