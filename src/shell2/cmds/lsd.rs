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

const TABLE_HEADERS: [&str; 8] = [
    "FileID", "Mode", "Owner", "Group", "Size", "Date", "Kind", "Name",
];

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

fn strip_table_flag(rest: &str) -> Result<(bool, Vec<String>), &'static str> {
    let mut table = false;
    let mut args = vec![String::from("lsd")];

    for raw in parse_args(rest)? {
        if raw == "-T" || raw == "--table" {
            table = true;
            continue;
        }

        if let Some(flags) = raw.as_str().strip_prefix('-')
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

        args.push(raw);
    }

    Ok((table, args))
}

fn parse_args(rest: &str) -> Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in rest.trim().chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                args.push(current);
                current = String::new();
            }
            continue;
        }
        current.push(ch);
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

fn run_lsd_table(io: &'static dyn ShellBackend2, args: Vec<String>) -> trueos_io::Result<()> {
    let listings = trueos_lsd::table_listings(args.as_slice())?;
    let multiple = listings.len() > 1;
    let width = line_width_for_backend(io).saturating_sub(2);
    let table = TlbTable::with_width(&TABLE_HEADERS, width)
        .with_max_col_widths(&[8, 10, 7, 7, 8, 10, 5, 0]);

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
                row.id.as_str(),
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
    let mut skip_next = false;
    for arg in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            "--color" | "--size" | "--permission" | "--sort" | "--group-dirs" | "--depth" => {
                skip_next = true;
            }
            raw if raw.starts_with('-') => {}
            _ => return Some(arg.clone()),
        }
    }
    None
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let (mut table, args) = match strip_table_flag(rest) {
        Ok(parsed) => parsed,
        Err(err) => {
            print_shell_line(io, alloc::format!("lsd: {}", err).as_str());
            return ParseOutcome::Handled;
        }
    };
    let display_path = first_path_arg(args.as_slice());
    if args
        .iter()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "help" | "-help" | "--help" | "-h" | "--version"))
    {
        table = false;
    }

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
