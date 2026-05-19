#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use trueos_io::{self as io, ErrorKind};
use v::vfs as api;
use v::vio::kfs;

const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    long: bool,
    tree: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    path: String,
    name: String,
    kind: kfs::FsEntryKind,
    depth: usize,
}

impl Options {
    const fn new() -> Self {
        Self {
            long: false,
            tree: false,
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
            });
    }

    Ok(children.into_values().collect())
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
                });
        }
    }

    Ok(entries.into_values().collect())
}

fn render_entry<W>(entry: &Entry, options: Options, base_depth: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
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

fn list_one<W>(path: &str, options: Options, write_line: &mut W) -> io::Result<()>
where
    W: FnMut(&str),
{
    let normalized = normalize_path(path);

    if !normalized.is_empty() {
        match api::stat(normalized.as_bytes()) {
            Ok(stat) if matches!(stat.kind, api::FsNodeKind::File) => {
                if options.long {
                    write_line(format!("file{size:>8} {}", normalized, size = stat.len).as_str());
                } else {
                    write_line(normalized.as_str());
                }
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

    for entry in entries.iter() {
        render_entry(entry, options, base_depth, write_line);
    }

    Ok(())
}

fn print_usage<W>(write_line: &mut W)
where
    W: FnMut(&str),
{
    write_line("lsd: usage `lsd [path ...]`");
    write_line("     flags: -l/--long  -R/--tree  --version  help");
    write_line("     paths: / and . both mean the TRUEOSFS root");
}

pub fn run_with_writer<W>(args: &[String], mut write_line: W) -> io::Result<()>
where
    W: FnMut(&str),
{
    let mut options = Options::new();
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

pub fn run(args: &[String]) -> io::Result<()> {
    run_with_writer(args, attached_line)
}
