#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use trueos_io::{self as io, ErrorKind};
use v::vfs as api;
use v::vio::kfs;

const MAX_ENTRIES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    long: bool,
    tree: bool,
}

impl Options {
    const fn new() -> Self {
        Self {
            long: false,
            tree: false,
        }
    }
}

fn write_line(line: &str) {
    let _ = v::vshell::attached_write(line.as_bytes());
    let _ = v::vshell::attached_write(b"\n");
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_matches('/').to_string()
}

fn display_path(path: &str) -> &str {
    if path.is_empty() {
        "."
    } else {
        path
    }
}

fn entry_kind_text(kind: kfs::FsEntryKind) -> &'static str {
    match kind {
        kfs::FsEntryKind::File => "file",
        kfs::FsEntryKind::Dir => "dir ",
        kfs::FsEntryKind::Other => "node",
    }
}

fn entry_size(path: &str) -> Option<u64> {
    api::stat(path.as_bytes()).ok().map(|stat| stat.len)
}

fn render_entry(entry: &kfs::FsTreeEntry, options: Options, base_depth: usize) {
    let depth = entry.depth.saturating_sub(base_depth);
    let indent = "  ".repeat(depth);
    let suffix = if matches!(entry.kind, kfs::FsEntryKind::Dir) {
        "/"
    } else {
        ""
    };

    if options.long {
        let size = entry_size(entry.path.as_str())
            .map(|size| format!("{size:>8}"))
            .unwrap_or_else(|| String::from("       -"));
        write_line(
            format!("{}{} {}{}", entry_kind_text(entry.kind), size, indent, entry.name).as_str(),
        );
    } else {
        write_line(format!("{indent}{}{suffix}", entry.name).as_str());
    }
}

fn list_one(path: &str, options: Options) -> io::Result<()> {
    let normalized = normalize_path(path);
    let stat = if normalized.is_empty() {
        None
    } else {
        Some(api::stat(normalized.as_bytes()).map_err(trueos_io::status_error)?)
    };

    if let Some(stat) = stat {
        if matches!(stat.kind, api::FsNodeKind::File) {
            if options.long {
                write_line(format!("file{size:>8} {}", normalized, size = stat.len).as_str());
            } else {
                write_line(normalized.as_str());
            }
            return Ok(());
        }
    }

    let entries = if options.tree {
        kfs::walk_entries(normalized.as_str(), MAX_ENTRIES)
    } else {
        kfs::list_dir(normalized.as_str(), MAX_ENTRIES)
    }
    .map_err(trueos_io::status_error)?;

    if entries.is_empty() {
        write_line(format!("{}: empty", display_path(normalized.as_str())).as_str());
        return Ok(());
    }

    let base_depth = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count();

    for entry in entries.iter() {
        render_entry(entry, options, base_depth);
    }

    Ok(())
}

fn print_usage() {
    write_line("lsd: usage `lsd [--version] [-l] [-R|--tree] [path ...]`");
}

pub fn run(args: &[String]) -> io::Result<()> {
    let mut options = Options::new();
    let mut paths = Vec::new();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--version" => {
                write_line(concat!("lsd ", env!("CARGO_PKG_VERSION")));
                return Ok(());
            }
            "-l" | "--long" => options.long = true,
            "-R" | "--tree" => options.tree = true,
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            raw if raw.starts_with('-') => {
                print_usage();
                return Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd flag"));
            }
            path => paths.push(String::from(path)),
        }
    }

    if paths.is_empty() {
        paths.push(String::from("."));
    }

    let multiple = paths.len() > 1;
    for (idx, path) in paths.iter().enumerate() {
        if multiple {
            if idx > 0 {
                write_line("");
            }
            write_line(format!("{}:", path).as_str());
        }
        list_one(path.as_str(), options)?;
    }

    Ok(())
}
