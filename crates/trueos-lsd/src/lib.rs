#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use trueos_io::{self as io, ErrorKind};
use v::vfs as api;
use v::vio::kfs;

const MAX_ENTRIES: usize = 4096;
const DEFAULT_GRID_WIDTH: usize = 96;
const MIN_CELL_WIDTH: usize = 18;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    long: bool,
    tree: bool,
    width: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    path: String,
    name: String,
    kind: kfs::FsEntryKind,
    depth: usize,
    len: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableListing {
    pub path: String,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    pub mode: &'static str,
    pub owner: &'static str,
    pub group: &'static str,
    pub size: String,
    pub date: &'static str,
    pub kind: &'static str,
    pub name: String,
}

impl Options {
    const fn new() -> Self {
        Self {
            long: false,
            tree: false,
            width: DEFAULT_GRID_WIDTH,
        }
    }
}

fn attached_line(line: &str) {
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

fn entry_size(path: &str) -> Option<u64> {
    api::stat(path.as_bytes()).ok().map(|stat| stat.len)
}

fn is_under_prefix(entry_path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || entry_path == prefix
        || entry_path
            .strip_prefix(prefix)
            .map(|rest| rest.starts_with('/'))
            .unwrap_or(false)
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        String::from(child)
    } else {
        format!("{parent}/{child}")
    }
}

fn path_depth(path: &str) -> usize {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

fn is_directory_marker(name: &str) -> bool {
    name == ".keep"
}

fn immediate_entries(prefix: &str) -> io::Result<Vec<Entry>> {
    let snapshot = kfs::tree(MAX_ENTRIES).map_err(|rc| {
        io::Error::new(trueos_io::status_kind(rc), "TRUEOSFS index unavailable for lsd")
    })?;
    let mut children = BTreeMap::<String, Entry>::new();

    for raw in snapshot.entries.into_iter() {
        if !is_under_prefix(raw.path.as_str(), prefix) || raw.path == prefix {
            continue;
        }

        let rest = if prefix.is_empty() {
            raw.path.as_str()
        } else {
            raw.path
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or("")
        };
        let Some(name) = rest.split('/').next().filter(|name| !name.is_empty()) else {
            continue;
        };
        if is_directory_marker(name) {
            continue;
        }

        let path = join_path(prefix, name);
        let kind = if rest.contains('/') {
            kfs::FsEntryKind::Dir
        } else {
            raw.kind
        };
        let depth = path_depth(path.as_str());

        children
            .entry(path.clone())
            .and_modify(|entry| {
                if matches!(kind, kfs::FsEntryKind::Dir) {
                    entry.kind = kfs::FsEntryKind::Dir;
                }
            })
            .or_insert_with(|| Entry {
                path,
                name: String::from(name),
                kind,
                depth,
                len: None,
            });
    }

    Ok(children
        .into_values()
        .map(|mut entry| {
            entry.len = entry_size(entry.path.as_str());
            entry
        })
        .collect())
}

fn tree_entries(prefix: &str) -> io::Result<Vec<Entry>> {
    let snapshot = kfs::tree(MAX_ENTRIES).map_err(|rc| {
        io::Error::new(trueos_io::status_kind(rc), "TRUEOSFS index unavailable for lsd")
    })?;
    let mut entries = BTreeMap::<String, Entry>::new();

    for raw in snapshot.entries.into_iter() {
        if !is_under_prefix(raw.path.as_str(), prefix) || raw.path == prefix {
            continue;
        }

        let mut current = String::new();
        for segment in raw.path.split('/').filter(|segment| !segment.is_empty()) {
            current = join_path(current.as_str(), segment);
            if !is_under_prefix(current.as_str(), prefix) || current == prefix {
                continue;
            }

            let is_leaf = current == raw.path;
            if is_leaf && is_directory_marker(segment) {
                continue;
            }
            let kind = if is_leaf {
                raw.kind
            } else {
                kfs::FsEntryKind::Dir
            };
            let depth = path_depth(current.as_str());
            entries
                .entry(current.clone())
                .and_modify(|entry| {
                    if matches!(kind, kfs::FsEntryKind::Dir) {
                        entry.kind = kfs::FsEntryKind::Dir;
                    }
                })
                .or_insert_with(|| Entry {
                    path: current.clone(),
                    name: String::from(segment),
                    kind,
                    depth,
                    len: None,
                });
        }
    }

    Ok(entries
        .into_values()
        .map(|mut entry| {
            entry.len = entry_size(entry.path.as_str());
            entry
        })
        .collect())
}

fn colorize(text: &str, kind: kfs::FsEntryKind) -> String {
    match kind {
        kfs::FsEntryKind::Dir => format!("\x1b[1;38;5;33m{text}\x1b[0m"),
        kfs::FsEntryKind::File => format!("\x1b[38;5;230m{text}\x1b[0m"),
        kfs::FsEntryKind::Other => format!("\x1b[38;5;245m{text}\x1b[0m"),
    }
}

fn display_name(entry: &Entry) -> String {
    let suffix = if matches!(entry.kind, kfs::FsEntryKind::Dir) {
        "/"
    } else {
        ""
    };
    format!("{}{suffix}", entry.name)
}

fn pad_visible(mut text: String, visible_width: usize, target_width: usize) -> String {
    if visible_width < target_width {
        text.push_str(" ".repeat(target_width - visible_width).as_str());
    }
    text
}

fn human_size(len: Option<u64>, is_dir: bool) -> String {
    if is_dir {
        return String::from("-");
    }
    let Some(bytes) = len else {
        return String::from("?");
    };
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{} K", (bytes + KB / 2) / KB)
    } else if bytes < GB {
        format!("{} M", (bytes + MB / 2) / MB)
    } else {
        format!("{} G", (bytes + GB / 2) / GB)
    }
}

fn authority(path: &str) -> (&'static str, &'static str) {
    if path == "apps/common" || path.starts_with("apps/common/") {
        ("common", "vmx")
    } else if path.starts_with("apps/") {
        ("vm", "vmx")
    } else {
        ("kernel", "system")
    }
}

fn permissions(entry: &Entry) -> &'static str {
    match entry.kind {
        kfs::FsEntryKind::Dir => "drwxrwx---",
        kfs::FsEntryKind::File => "-rw-rw----",
        kfs::FsEntryKind::Other => "?---------",
    }
}

fn kind_text(kind: kfs::FsEntryKind) -> &'static str {
    match kind {
        kfs::FsEntryKind::Dir => "dir",
        kfs::FsEntryKind::File => "file",
        kfs::FsEntryKind::Other => "other",
    }
}

fn table_row(entry: &Entry, base_depth: usize, tree: bool) -> TableRow {
    let (owner, group) = authority(entry.path.as_str());
    let is_dir = matches!(entry.kind, kfs::FsEntryKind::Dir);
    let depth = if tree {
        entry.depth.saturating_sub(base_depth.saturating_add(1))
    } else {
        0
    };
    TableRow {
        mode: permissions(entry),
        owner,
        group,
        size: human_size(entry.len, is_dir),
        date: "-",
        kind: kind_text(entry.kind),
        name: format!("{}{}", "  ".repeat(depth), display_name(entry)),
    }
}

fn render_grid_entry(entry: &Entry, cell_width: usize) -> String {
    let name = display_name(entry);
    let visible = name.len();
    pad_visible(colorize(name.as_str(), entry.kind), visible, cell_width)
}

fn render_grid<W>(entries: &[Entry], width: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    let max_name = entries
        .iter()
        .map(|entry| display_name(entry).len())
        .max()
        .unwrap_or(MIN_CELL_WIDTH);
    let cell_width = core::cmp::max(max_name.saturating_add(3), MIN_CELL_WIDTH);
    let columns = core::cmp::max(1, width.max(MIN_CELL_WIDTH) / cell_width);

    for row in entries.chunks(columns) {
        let mut line = String::new();
        for entry in row {
            line.push_str(render_grid_entry(entry, cell_width).as_str());
        }
        write_line(line.trim_end());
    }
}

fn render_long<W>(entries: &[Entry], write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        render_long_entry(entry, String::new(), write_line);
    }
}

fn render_long_entry<W>(entry: &Entry, name_prefix: String, write_line: &mut W)
where
    W: FnMut(&str),
{
    let (owner, group) = authority(entry.path.as_str());
    let size = human_size(entry.len, matches!(entry.kind, kfs::FsEntryKind::Dir));
    let name = colorize(format!("{name_prefix}{}", display_name(entry)).as_str(), entry.kind);
    write_line(
        format!(
            "{} {:<7} {:<7} {:>7} {:>10} {}",
            permissions(entry),
            owner,
            group,
            size,
            "-",
            name
        )
        .as_str(),
    );
}

fn render_tree<W>(entries: &[Entry], base_depth: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
        let indent = "  ".repeat(depth);
        let name = colorize(display_name(entry).as_str(), entry.kind);
        write_line(format!("{indent}{name}").as_str());
    }
}

fn render_long_tree<W>(entries: &[Entry], base_depth: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
        render_long_entry(entry, "  ".repeat(depth), write_line);
    }
}

fn render_entries<W>(entries: &[Entry], options: Options, base_depth: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    if options.tree && options.long {
        render_long_tree(entries, base_depth, write_line);
    } else if options.tree {
        render_tree(entries, base_depth, write_line);
    } else if options.long {
        render_long(entries, write_line);
    } else {
        render_grid(entries, options.width, write_line);
    }
}

fn list_one<W>(path: &str, options: Options, write_line: &mut W) -> io::Result<()>
where
    W: FnMut(&str),
{
    let normalized = normalize_path(path);

    if !normalized.is_empty() {
        match api::stat(normalized.as_bytes()) {
            Ok(stat) if matches!(stat.kind, api::FsNodeKind::File) => {
                let entry = Entry {
                    path: normalized.clone(),
                    name: normalized.clone(),
                    kind: kfs::FsEntryKind::File,
                    depth: path_depth(normalized.as_str()),
                    len: Some(stat.len),
                };
                render_entries(&[entry], options, 0, write_line);
                return Ok(());
            }
            Ok(_) => {}
            Err(rc) if trueos_io::status_kind(rc) == ErrorKind::NotFound => {}
            Err(rc) => return Err(trueos_io::status_error(rc)),
        }
    }

    let entries = if options.tree {
        tree_entries(normalized.as_str())?
    } else {
        immediate_entries(normalized.as_str())?
    };

    if entries.is_empty() {
        if normalized.is_empty() {
            write_line(".: empty");
            return Ok(());
        }

        return Err(io::Error::new(ErrorKind::NotFound, "lsd path not found"));
    }

    let base_depth = path_depth(normalized.as_str());
    render_entries(entries.as_slice(), options, base_depth, write_line);

    Ok(())
}

fn print_usage<W>(write_line: &mut W)
where
    W: FnMut(&str),
{
    write_line("lsd: usage `lsd [path ...]`");
    write_line("     flags: -l/--long  -R/--tree  -T/--table  -lR/-Rl  --version  help");
    write_line("     paths: / and . both mean the TRUEOSFS root");
}

fn apply_short_flags(flags: &str, options: &mut Options) -> bool {
    for ch in flags.chars() {
        match ch {
            'l' => options.long = true,
            'R' => options.tree = true,
            _ => return false,
        }
    }
    true
}

pub fn run_with_writer<W>(args: &[String], write_line: W) -> io::Result<()>
where
    W: FnMut(&str),
{
    run_with_writer_and_width(args, DEFAULT_GRID_WIDTH, write_line)
}

pub fn run_with_writer_and_width<W>(
    args: &[String],
    width: usize,
    mut write_line: W,
) -> io::Result<()>
where
    W: FnMut(&str),
{
    let mut options = Options::new();
    options.width = width;
    let mut paths = Vec::new();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "help" | "-help" | "--help" | "-h" => {
                print_usage(&mut write_line);
                return Ok(());
            }
            "--version" => {
                write_line(concat!("lsd ", env!("CARGO_PKG_VERSION")));
                return Ok(());
            }
            "-l" | "--long" => options.long = true,
            "-R" | "--tree" => options.tree = true,
            raw if raw.starts_with('-') && apply_short_flags(&raw[1..], &mut options) => {}
            raw if raw.starts_with('-') => {
                print_usage(&mut write_line);
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
        list_one(path.as_str(), options, &mut write_line)?;
    }

    Ok(())
}

pub fn table_listings(args: &[String]) -> io::Result<Vec<TableListing>> {
    let mut options = Options::new();
    let mut paths = Vec::new();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-l" | "--long" => options.long = true,
            "-R" | "--tree" => options.tree = true,
            raw if raw.starts_with('-') && apply_short_flags(&raw[1..], &mut options) => {}
            raw if raw.starts_with('-') => {
                return Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd flag"));
            }
            path => paths.push(String::from(path)),
        }
    }

    if paths.is_empty() {
        paths.push(String::from("."));
    }

    let mut listings = Vec::new();
    for path in paths {
        let normalized = normalize_path(path.as_str());
        let entries = if !normalized.is_empty() {
            match api::stat(normalized.as_bytes()) {
                Ok(stat) if matches!(stat.kind, api::FsNodeKind::File) => vec![Entry {
                    path: normalized.clone(),
                    name: normalized.clone(),
                    kind: kfs::FsEntryKind::File,
                    depth: path_depth(normalized.as_str()),
                    len: Some(stat.len),
                }],
                Ok(_) if options.tree => tree_entries(normalized.as_str())?,
                Ok(_) => immediate_entries(normalized.as_str())?,
                Err(rc) if trueos_io::status_kind(rc) == ErrorKind::NotFound => {
                    if options.tree {
                        tree_entries(normalized.as_str())?
                    } else {
                        immediate_entries(normalized.as_str())?
                    }
                }
                Err(rc) => return Err(trueos_io::status_error(rc)),
            }
        } else if options.tree {
            tree_entries(normalized.as_str())?
        } else {
            immediate_entries(normalized.as_str())?
        };

        if entries.is_empty() {
            if normalized.is_empty() {
                listings.push(TableListing {
                    path,
                    rows: Vec::new(),
                });
                continue;
            }
            return Err(io::Error::new(ErrorKind::NotFound, "lsd path not found"));
        }

        let base_depth = path_depth(normalized.as_str());
        let rows = entries
            .iter()
            .map(|entry| table_row(entry, base_depth, options.tree))
            .collect();
        listings.push(TableListing { path, rows });
    }

    Ok(listings)
}

pub fn run(args: &[String]) -> io::Result<()> {
    run_with_writer(args, attached_line)
}
