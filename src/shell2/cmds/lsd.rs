use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::super::{
    ShellBackend2, line_width_for_backend, matrix_target_for_backend, print_native_line,
    print_shell_line,
};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

const TABLE_HEADERS: [&str; 7] = ["Mode", "Owner", "Group", "Size", "Date", "Kind", "Name"];

fn run_lsd(io: &'static dyn ShellBackend2, args: Vec<String>) -> trueos_io::Result<()> {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io).saturating_sub(2);
    crate::r::io::env::with_launch_context_console_and_fs_root(
        args.clone(),
        BTreeMap::new(),
        Some(target),
        None,
        || {
            trueos_lsd::run_with_writer_and_width(args.as_slice(), width, |line| {
                print_native_line(io, line)
            })
        },
    )
}

fn strip_table_flag(rest: &str) -> (bool, Vec<String>) {
    let mut table = false;
    let mut args = vec![String::from("lsd")];

    for raw in rest.split_whitespace() {
        if raw == "-T" || raw == "--table" {
            table = true;
            continue;
        }

        if let Some(flags) = raw.strip_prefix('-')
            && !flags.starts_with('-')
            && flags.chars().any(|ch| ch == 'T')
        {
            table = true;
            let mut kept = String::from("-");
            kept.extend(flags.chars().filter(|ch| *ch != 'T'));
            if kept.len() > 1 {
                args.push(kept);
            }
            continue;
        }

        args.push(String::from(raw));
    }

    (table, args)
}

fn run_lsd_table(io: &'static dyn ShellBackend2, args: Vec<String>) -> trueos_io::Result<()> {
    let listings = trueos_lsd::table_listings(args.as_slice())?;
    let multiple = listings.len() > 1;
    let width = line_width_for_backend(io).saturating_sub(2);
    let table =
        TlbTable::with_width(&TABLE_HEADERS, width).with_max_col_widths(&[10, 7, 7, 8, 10, 5, 0]);

    for (idx, listing) in listings.iter().enumerate() {
        if multiple {
            if idx > 0 {
                print_shell_line(io, "");
            }
            print_shell_line(io, alloc::format!("{}:", listing.path).as_str());
        }

        table.emit_header(|text| print_shell_line(io, text));
        for row in listing.rows.iter() {
            let cells = [
                row.mode,
                row.owner,
                row.group,
                row.size.as_str(),
                row.date,
                row.kind,
                row.name.as_str(),
            ];
            table.emit_row(&cells, |text| print_shell_line(io, text));
        }
        table.emit_footer(|text| print_shell_line(io, text));
    }

    Ok(())
}

fn first_path_arg(args: &[String]) -> Option<String> {
    args.iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .cloned()
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let (table, args) = strip_table_flag(rest);
    let display_path = first_path_arg(args.as_slice());

    let result = if table {
        run_lsd_table(io, args)
    } else {
        run_lsd(io, args)
    };

    if let Err(err) = result {
        if err.kind() == trueos_io::ErrorKind::NotFound {
            let path = display_path.as_deref().unwrap_or(".");
            print_shell_line(io, alloc::format!("lsd: {path}: not found").as_str());
        } else {
            print_shell_line(io, alloc::format!("lsd: {}", err).as_str());
        }
    }

    ParseOutcome::Handled
}
