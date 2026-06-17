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
    oneline: bool,
    width: usize,
    color: bool,
    directory_only: bool,
    classify: bool,
    header: bool,
    depth: Option<usize>,
    size: SizeStyle,
    permission: PermissionStyle,
    sort: SortColumn,
    reverse: bool,
    group_dirs: DirGrouping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SizeStyle {
    Default,
    Short,
    Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PermissionStyle {
    Rwx,
    Octal,
    Attributes,
    Disable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SortColumn {
    Name,
    Size,
    Extension,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirGrouping {
    None,
    First,
    Last,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    id: u64,
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
    pub id: String,
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
            oneline: false,
            width: DEFAULT_GRID_WIDTH,
            color: true,
            directory_only: false,
            classify: false,
            header: false,
            depth: None,
            size: SizeStyle::Default,
            permission: PermissionStyle::Rwx,
            sort: SortColumn::Name,
            reverse: false,
            group_dirs: DirGrouping::None,
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

fn indexed_id(path: &str) -> u64 {
    kfs::tree(MAX_ENTRIES)
        .ok()
        .and_then(|snapshot| {
            snapshot
                .entries
                .into_iter()
                .filter(|entry| entry.path == path)
                .map(|entry| entry.id)
                .max()
        })
        .unwrap_or(0)
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

fn basename(path: &str) -> &str {
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(".")
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
                entry.id = entry.id.max(raw.id);
                if matches!(kind, kfs::FsEntryKind::Dir) {
                    entry.kind = kfs::FsEntryKind::Dir;
                }
            })
            .or_insert_with(|| Entry {
                id: raw.id,
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
                    entry.id = entry.id.max(raw.id);
                    if matches!(kind, kfs::FsEntryKind::Dir) {
                        entry.kind = kfs::FsEntryKind::Dir;
                    }
                })
                .or_insert_with(|| Entry {
                    id: raw.id,
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

fn colorize(text: &str, kind: kfs::FsEntryKind, options: Options) -> String {
    if !options.color {
        return text.to_string();
    }

    match kind {
        kfs::FsEntryKind::Dir => format!("\x1b[1;38;5;33m{text}\x1b[0m"),
        kfs::FsEntryKind::File => format!("\x1b[38;5;230m{text}\x1b[0m"),
        kfs::FsEntryKind::Other => format!("\x1b[38;5;245m{text}\x1b[0m"),
    }
}

fn display_name(entry: &Entry, options: Options) -> String {
    let suffix = if options.classify || matches!(entry.kind, kfs::FsEntryKind::Dir) {
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

fn human_size(len: Option<u64>, is_dir: bool, style: SizeStyle) -> String {
    if is_dir {
        return String::from("-");
    }
    let Some(bytes) = len else {
        return String::from("?");
    };
    if matches!(style, SizeStyle::Bytes) {
        return bytes.to_string();
    }

    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes < KB {
        match style {
            SizeStyle::Short => format!("{bytes}B"),
            SizeStyle::Default | SizeStyle::Bytes => format!("{bytes} B"),
        }
    } else if bytes < MB {
        let value = (bytes + KB / 2) / KB;
        match style {
            SizeStyle::Short => format!("{value}K"),
            SizeStyle::Default | SizeStyle::Bytes => format!("{value} K"),
        }
    } else if bytes < GB {
        let value = (bytes + MB / 2) / MB;
        match style {
            SizeStyle::Short => format!("{value}M"),
            SizeStyle::Default | SizeStyle::Bytes => format!("{value} M"),
        }
    } else {
        let value = (bytes + GB / 2) / GB;
        match style {
            SizeStyle::Short => format!("{value}G"),
            SizeStyle::Default | SizeStyle::Bytes => format!("{value} G"),
        }
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

fn permissions(entry: &Entry, style: PermissionStyle) -> &'static str {
    match style {
        PermissionStyle::Disable => "-",
        PermissionStyle::Octal => match entry.kind {
            kfs::FsEntryKind::Dir => "770",
            kfs::FsEntryKind::File => "660",
            kfs::FsEntryKind::Other => "000",
        },
        PermissionStyle::Rwx | PermissionStyle::Attributes => match entry.kind {
            kfs::FsEntryKind::Dir => "drwxrwx---",
            kfs::FsEntryKind::File => "-rw-rw----",
            kfs::FsEntryKind::Other => "?---------",
        },
    }
}

fn kind_text(kind: kfs::FsEntryKind) -> &'static str {
    match kind {
        kfs::FsEntryKind::Dir => "dir",
        kfs::FsEntryKind::File => "file",
        kfs::FsEntryKind::Other => "other",
    }
}

fn entry_id(entry: &Entry) -> String {
    if entry.id == 0 {
        String::from("-")
    } else {
        format!("{:08x}", entry.id)
    }
}

fn table_row(entry: &Entry, base_depth: usize, options: Options) -> TableRow {
    let (owner, group) = authority(entry.path.as_str());
    let is_dir = matches!(entry.kind, kfs::FsEntryKind::Dir);
    let depth = if options.tree {
        entry.depth.saturating_sub(base_depth.saturating_add(1))
    } else {
        0
    };
    TableRow {
        mode: permissions(entry, options.permission),
        id: entry_id(entry),
        owner,
        group,
        size: human_size(entry.len, is_dir, options.size),
        date: "-",
        kind: kind_text(entry.kind),
        name: format!("{}{}", "  ".repeat(depth), display_name(entry, options)),
    }
}

fn render_grid_entry(entry: &Entry, cell_width: usize, options: Options) -> String {
    let label = format!("{} {}", entry_id(entry), display_name(entry, options));
    let visible = label.len();
    pad_visible(colorize(label.as_str(), entry.kind, options), visible, cell_width)
}

fn render_grid<W>(entries: &[Entry], options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    let max_name = entries
        .iter()
        .map(|entry| entry_id(entry).len() + 1 + display_name(entry, options).len())
        .max()
        .unwrap_or(MIN_CELL_WIDTH);
    let cell_width = core::cmp::max(max_name.saturating_add(3), MIN_CELL_WIDTH);
    let columns = core::cmp::max(1, options.width.max(MIN_CELL_WIDTH) / cell_width);

    for row in entries.chunks(columns) {
        let mut line = String::new();
        for entry in row {
            line.push_str(render_grid_entry(entry, cell_width, options).as_str());
        }
        write_line(line.trim_end());
    }
}

fn render_oneline<W>(entries: &[Entry], options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let label = format!("{} {}", entry_id(entry), display_name(entry, options));
        write_line(colorize(label.as_str(), entry.kind, options).as_str());
    }
}

fn render_long_header<W>(write_line: &mut W)
where
    W: FnMut(&str),
{
    write_line("FileID   Mode       Owner   Group      Size       Date Name");
}

fn render_long<W>(entries: &[Entry], options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    if options.header {
        render_long_header(write_line);
    }
    for entry in entries {
        render_long_entry(entry, String::new(), options, write_line);
    }
}

fn render_long_entry<W>(entry: &Entry, name_prefix: String, options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    let (owner, group) = authority(entry.path.as_str());
    let size = human_size(entry.len, matches!(entry.kind, kfs::FsEntryKind::Dir), options.size);
    let name = colorize(
        format!("{name_prefix}{}", display_name(entry, options)).as_str(),
        entry.kind,
        options,
    );
    write_line(
        format!(
            "{:<8} {} {:<7} {:<7} {:>7} {:>10} {}",
            entry_id(entry),
            permissions(entry, options.permission),
            owner,
            group,
            size,
            "-",
            name
        )
        .as_str(),
    );
}

fn render_tree<W>(entries: &[Entry], base_depth: usize, options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
        let indent = "  ".repeat(depth);
        let name = colorize(
            format!("{} {}", entry_id(entry), display_name(entry, options)).as_str(),
            entry.kind,
            options,
        );
        write_line(format!("{indent}{name}").as_str());
    }
}

fn render_long_tree<W>(entries: &[Entry], base_depth: usize, options: Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    if options.header {
        render_long_header(write_line);
    }
    for entry in entries {
        let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
        render_long_entry(entry, "  ".repeat(depth), options, write_line);
    }
}

fn relative_depth(entry: &Entry, base_depth: usize) -> usize {
    entry.depth.saturating_sub(base_depth)
}

fn extension(name: &str) -> &str {
    name.rsplit_once('.')
        .filter(|(base, _)| !base.is_empty())
        .map(|(_, ext)| ext)
        .unwrap_or("")
}

fn sort_entries(entries: &mut [Entry], options: Options) {
    entries.sort_by(|a, b| {
        let a_dir = matches!(a.kind, kfs::FsEntryKind::Dir);
        let b_dir = matches!(b.kind, kfs::FsEntryKind::Dir);
        let dir_order = match options.group_dirs {
            DirGrouping::First => b_dir.cmp(&a_dir),
            DirGrouping::Last => a_dir.cmp(&b_dir),
            DirGrouping::None => core::cmp::Ordering::Equal,
        };
        if !dir_order.is_eq() {
            return dir_order;
        }

        let order = match options.sort {
            SortColumn::Name => a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)),
            SortColumn::Size => a
                .len
                .unwrap_or(0)
                .cmp(&b.len.unwrap_or(0))
                .then_with(|| a.name.cmp(&b.name)),
            SortColumn::Extension => extension(a.name.as_str())
                .cmp(extension(b.name.as_str()))
                .then_with(|| a.name.cmp(&b.name)),
            SortColumn::None => core::cmp::Ordering::Equal,
        };
        if options.reverse {
            order.reverse()
        } else {
            order
        }
    });
}

fn apply_listing_options(entries: &mut Vec<Entry>, base_depth: usize, options: Options) {
    if let Some(max_depth) = options.depth {
        entries.retain(|entry| relative_depth(entry, base_depth) <= max_depth);
    }
    if !matches!(options.sort, SortColumn::None)
        || options.reverse
        || options.group_dirs != DirGrouping::None
    {
        sort_entries(entries.as_mut_slice(), options);
    }
}

fn render_entries<W>(entries: &[Entry], options: Options, base_depth: usize, write_line: &mut W)
where
    W: FnMut(&str),
{
    if options.tree && options.long {
        render_long_tree(entries, base_depth, options, write_line);
    } else if options.tree {
        render_tree(entries, base_depth, options, write_line);
    } else if options.long {
        render_long(entries, options, write_line);
    } else if options.oneline {
        render_oneline(entries, options, write_line);
    } else {
        render_grid(entries, options, write_line);
    }
}

fn self_entry(path: &str, kind: kfs::FsEntryKind, len: Option<u64>) -> Entry {
    let normalized = normalize_path(path);
    let name = if normalized.is_empty() {
        String::from(".")
    } else {
        String::from(basename(normalized.as_str()))
    };
    Entry {
        id: indexed_id(normalized.as_str()),
        path: normalized.clone(),
        name,
        kind,
        depth: path_depth(normalized.as_str()),
        len,
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
                let entry = self_entry(normalized.as_str(), kfs::FsEntryKind::File, Some(stat.len));
                render_entries(&[entry], options, 0, write_line);
                return Ok(());
            }
            Ok(stat) if options.directory_only => {
                let entry = self_entry(normalized.as_str(), kfs::FsEntryKind::Dir, Some(stat.len));
                render_entries(&[entry], options, 0, write_line);
                return Ok(());
            }
            Ok(_) => {}
            Err(rc) if trueos_io::status_kind(rc) == ErrorKind::NotFound => {}
            Err(rc) => return Err(trueos_io::status_error(rc)),
        }
    } else if options.directory_only {
        let entry = self_entry(".", kfs::FsEntryKind::Dir, None);
        render_entries(&[entry], options, 0, write_line);
        return Ok(());
    }

    let mut entries = if options.tree {
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
    apply_listing_options(&mut entries, base_depth, options);
    render_entries(entries.as_slice(), options, base_depth, write_line);

    Ok(())
}

fn print_usage<W>(write_line: &mut W)
where
    W: FnMut(&str),
{
    write_line("lsd: usage `lsd [path ...]`");
    write_line("     flags: -l/--long  -R/--tree  -T/--table  -1/--oneline  -d/--directory-only");
    write_line("            --color always|auto|never  --size default|short|bytes");
    write_line(
        "            --permission rwx|octal|attributes|disable  --sort name|size|extension|none",
    );
    write_line(
        "            --reverse  --group-dirs first|last|none  --depth N  --header  --version  help",
    );
    write_line("     paths: / and . both mean the TRUEOSFS root");
}

fn apply_short_flags(flags: &str, options: &mut Options) -> bool {
    for ch in flags.chars() {
        match ch {
            'l' => options.long = true,
            'R' => options.tree = true,
            '1' => options.oneline = true,
            'd' => options.directory_only = true,
            'F' => options.classify = true,
            'N' | 'a' | 'A' | 'i' => {}
            'r' => options.reverse = true,
            'S' => options.sort = SortColumn::Size,
            'X' => options.sort = SortColumn::Extension,
            _ => return false,
        }
    }
    true
}

enum ParseAction {
    Run,
    Help,
    Version,
}

fn parse_usize(value: &str) -> io::Result<usize> {
    value
        .parse::<usize>()
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid lsd number"))
}

fn parse_size(value: &str) -> io::Result<SizeStyle> {
    match value {
        "default" => Ok(SizeStyle::Default),
        "short" => Ok(SizeStyle::Short),
        "bytes" => Ok(SizeStyle::Bytes),
        _ => Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd size")),
    }
}

fn parse_permission(value: &str) -> io::Result<PermissionStyle> {
    match value {
        "rwx" => Ok(PermissionStyle::Rwx),
        "octal" => Ok(PermissionStyle::Octal),
        "attributes" => Ok(PermissionStyle::Attributes),
        "disable" => Ok(PermissionStyle::Disable),
        _ => Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd permission")),
    }
}

fn parse_sort(value: &str) -> io::Result<SortColumn> {
    match value {
        "name" => Ok(SortColumn::Name),
        "size" => Ok(SortColumn::Size),
        "extension" => Ok(SortColumn::Extension),
        "none" => Ok(SortColumn::None),
        _ => Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd sort")),
    }
}

fn parse_group_dirs(value: &str) -> io::Result<DirGrouping> {
    match value {
        "none" => Ok(DirGrouping::None),
        "first" => Ok(DirGrouping::First),
        "last" => Ok(DirGrouping::Last),
        _ => Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd dir grouping")),
    }
}

fn parse_value_arg(args: &[String], idx: &mut usize, inline: Option<&str>) -> io::Result<String> {
    if let Some(value) = inline {
        return Ok(String::from(value));
    }
    *idx += 1;
    args.get(*idx)
        .cloned()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing lsd flag value"))
}

fn parse_args(args: &[String], width: usize) -> io::Result<(Options, Vec<String>, ParseAction)> {
    let mut options = Options::new();
    options.width = width;
    let mut paths = Vec::new();
    let mut idx = 1usize;

    while idx < args.len() {
        let arg = args[idx].as_str();
        match arg {
            "help" | "-help" | "--help" | "-h" => return Ok((options, paths, ParseAction::Help)),
            "--version" => return Ok((options, paths, ParseAction::Version)),
            "-l" | "--long" => options.long = true,
            "-R" | "--tree" | "--recursive" => options.tree = true,
            "-T" | "--table" => options.long = true,
            "-1" | "--oneline" => options.oneline = true,
            "-d" | "--directory-only" => options.directory_only = true,
            "-F" | "--classify" => options.classify = true,
            "-N" | "--literal" | "-a" | "--all" | "-A" | "--almost-all" | "-i" | "--inode" => {}
            "-r" | "--reverse" => options.reverse = true,
            "-S" | "--sizesort" => options.sort = SortColumn::Size,
            "-X" | "--extensionsort" => options.sort = SortColumn::Extension,
            "--header" => options.header = true,
            "--group-directories-first" => options.group_dirs = DirGrouping::First,
            raw if raw == "--color" || raw.starts_with("--color=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--color="))?;
                options.color = match value.as_str() {
                    "always" | "auto" => true,
                    "never" => false,
                    _ => {
                        return Err(io::Error::new(
                            ErrorKind::InvalidInput,
                            "unsupported lsd color",
                        ));
                    }
                };
            }
            raw if raw == "--size" || raw.starts_with("--size=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--size="))?;
                options.size = parse_size(value.as_str())?;
            }
            raw if raw == "--permission" || raw.starts_with("--permission=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--permission="))?;
                options.permission = parse_permission(value.as_str())?;
            }
            raw if raw == "--sort" || raw.starts_with("--sort=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--sort="))?;
                options.sort = parse_sort(value.as_str())?;
            }
            raw if raw == "--group-dirs" || raw.starts_with("--group-dirs=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--group-dirs="))?;
                options.group_dirs = parse_group_dirs(value.as_str())?;
            }
            raw if raw == "--depth" || raw.starts_with("--depth=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--depth="))?;
                options.depth = Some(parse_usize(value.as_str())?);
                options.tree = true;
            }
            raw if raw.starts_with('-') && apply_short_flags(&raw[1..], &mut options) => {}
            raw if raw.starts_with('-') => {
                return Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd flag"));
            }
            path => paths.push(String::from(path)),
        }
        idx += 1;
    }

    Ok((options, paths, ParseAction::Run))
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
    let (options, mut paths, action) = parse_args(args, width)?;

    match action {
        ParseAction::Help => {
            print_usage(&mut write_line);
            return Ok(());
        }
        ParseAction::Version => {
            write_line(concat!("lsd ", env!("CARGO_PKG_VERSION")));
            return Ok(());
        }
        ParseAction::Run => {}
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
    let (options, mut paths, action) = parse_args(args, DEFAULT_GRID_WIDTH)?;

    if !matches!(action, ParseAction::Run) {
        return Ok(Vec::new());
    }

    if paths.is_empty() {
        paths.push(String::from("."));
    }

    let mut listings = Vec::new();
    for path in paths {
        let normalized = normalize_path(path.as_str());
        let mut entries = if !normalized.is_empty() {
            match api::stat(normalized.as_bytes()) {
                Ok(stat) if matches!(stat.kind, api::FsNodeKind::File) => vec![Entry {
                    id: indexed_id(normalized.as_str()),
                    path: normalized.clone(),
                    name: normalized.clone(),
                    kind: kfs::FsEntryKind::File,
                    depth: path_depth(normalized.as_str()),
                    len: Some(stat.len),
                }],
                Ok(stat) if options.directory_only => {
                    vec![self_entry(
                        normalized.as_str(),
                        kfs::FsEntryKind::Dir,
                        Some(stat.len),
                    )]
                }
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
        } else if options.directory_only {
            vec![self_entry(".", kfs::FsEntryKind::Dir, None)]
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
        apply_listing_options(&mut entries, base_depth, options);
        let rows = entries
            .iter()
            .map(|entry| table_row(entry, base_depth, options))
            .collect();
        listings.push(TableListing { path, rows });
    }

    Ok(listings)
}

pub fn run(args: &[String]) -> io::Result<()> {
    run_with_writer(args, attached_line)
}
