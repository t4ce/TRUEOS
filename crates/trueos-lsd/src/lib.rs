#![no_std]

extern crate alloc;

mod glob;
mod runtime_config;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use trueos_io::{self as io, ErrorKind};
use v::vfs as api;
use v::vio::kfs;

const MAX_ENTRIES: usize = 1024;
const DEFAULT_GRID_WIDTH: usize = 96;
const MIN_CELL_WIDTH: usize = 18;

#[derive(Clone, Debug, Eq, PartialEq)]
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
    hyperlink: HyperlinkStyle,
    ignore_globs: Vec<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HyperlinkStyle {
    Always,
    Auto,
    Never,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    id: u64,
    path: String,
    name: String,
    kind: kfs::FsEntryKind,
    depth: usize,
    len: Option<u64>,
    record_key: trueos_fs::RecordKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableListing {
    pub path: String,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    pub key: String,
    pub id: String,
    pub size: String,
    pub kind: &'static str,
    pub name: String,
}

impl Options {
    fn new(config: Option<&runtime_config::RuntimeConfig>) -> Self {
        let mut options = Self {
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
            hyperlink: HyperlinkStyle::Never,
            ignore_globs: Vec::new(),
        };

        if let Some(config) = config {
            options.apply_config(config);
        }
        options
    }

    fn apply_config(&mut self, config: &runtime_config::RuntimeConfig) {
        if let Some(value) = config
            .scalar("color.when")
            .or_else(|| config.scalar("color"))
        {
            match value {
                "always" | "auto" => self.color = true,
                "never" => self.color = false,
                _ => {}
            }
        }
        if let Some(value) = config.scalar("display")
            && value == "directory-only"
        {
            self.directory_only = true;
        }
        if let Some(value) = config.scalar("indicators").and_then(parse_bool) {
            self.classify = value;
        }
        if let Some(value) = config.scalar("header").and_then(parse_bool) {
            self.header = value;
        }
        if let Some(value) = config.scalar("layout") {
            match value {
                "tree" => self.select_tree(),
                "oneline" | "one-line" => self.select_oneline(),
                "grid" => {
                    self.tree = false;
                    self.oneline = false;
                }
                _ => {}
            }
        }
        if config.scalar("recursion.enabled").and_then(parse_bool) == Some(true) {
            self.select_tree();
        }
        if let Some(depth) = config
            .scalar("recursion.depth")
            .and_then(|value| value.parse::<usize>().ok())
        {
            self.depth = Some(depth);
            self.select_tree();
        }
        if let Some(value) = config.scalar("size")
            && let Ok(size) = parse_size(value)
        {
            self.size = size;
        }
        if let Some(value) = config.scalar("permission")
            && let Ok(permission) = parse_permission(value)
        {
            self.permission = permission;
        }
        if let Some(value) = config.scalar("sorting.column")
            && let Ok(sort) = parse_sort(value)
        {
            self.sort = sort;
        }
        if let Some(value) = config.scalar("sorting.reverse").and_then(parse_bool) {
            self.reverse = value;
        }
        if let Some(value) = config.scalar("sorting.dir-grouping")
            && let Ok(group_dirs) = parse_group_dirs(value)
        {
            self.group_dirs = group_dirs;
        }
        if let Some(value) = config.scalar("hyperlink")
            && let Ok(hyperlink) = parse_hyperlink(value)
        {
            self.hyperlink = hyperlink;
        }
        if let Some(patterns) = config.list("ignore-globs") {
            self.ignore_globs.extend(patterns.iter().cloned());
        }
        if config.scalar("classic").and_then(parse_bool) == Some(true) {
            self.apply_classic();
        }
    }

    fn apply_classic(&mut self) {
        self.color = false;
        self.group_dirs = DirGrouping::None;
        self.hyperlink = HyperlinkStyle::Never;
    }

    fn select_tree(&mut self) {
        self.tree = true;
        self.oneline = false;
    }

    fn select_oneline(&mut self) {
        self.oneline = true;
        self.tree = false;
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
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

fn indexed_record(path: &str) -> (u64, trueos_fs::RecordKey) {
    kfs::tree(MAX_ENTRIES)
        .ok()
        .and_then(|snapshot| {
            snapshot
                .entries
                .into_iter()
                .filter(|entry| entry.path == path)
                .max_by_key(|entry| entry.id)
        })
        .map(|entry| (entry.id, entry.record_key))
        .unwrap_or((0, trueos_fs::RecordKey::Ffa))
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
    let truncated = snapshot.truncated;
    let mut children = BTreeMap::<String, Entry>::new();

    for raw in snapshot.entries.into_iter() {
        if raw.path == "..." {
            continue;
        }
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
        let record_key = if rest.contains('/') {
            trueos_fs::RecordKey::Ffa
        } else {
            raw.record_key
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
                record_key,
            });
    }

    let mut entries: Vec<Entry> = children
        .into_values()
        .map(|mut entry| {
            entry.len = entry_size(entry.path.as_str());
            entry
        })
        .collect();
    if truncated {
        entries.push(more_entry(prefix));
    }
    Ok(entries)
}

fn tree_entries(prefix: &str) -> io::Result<Vec<Entry>> {
    let snapshot = kfs::tree(MAX_ENTRIES).map_err(|rc| {
        io::Error::new(trueos_io::status_kind(rc), "TRUEOSFS index unavailable for lsd")
    })?;
    let truncated = snapshot.truncated;
    let mut entries = BTreeMap::<String, Entry>::new();

    for raw in snapshot.entries.into_iter() {
        if raw.path == "..." {
            continue;
        }
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
            let record_key = if is_leaf {
                raw.record_key
            } else {
                trueos_fs::RecordKey::Ffa
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
                    record_key,
                });
        }
    }

    let mut entries: Vec<Entry> = entries
        .into_values()
        .map(|mut entry| {
            entry.len = entry_size(entry.path.as_str());
            entry
        })
        .collect();
    if truncated {
        entries.push(more_entry(prefix));
    }
    Ok(entries)
}

fn colorize(text: &str, kind: kfs::FsEntryKind, options: &Options) -> String {
    if !options.color {
        return text.to_string();
    }

    match kind {
        kfs::FsEntryKind::Dir => format!("\x1b[1;38;5;33m{text}\x1b[0m"),
        kfs::FsEntryKind::File => format!("\x1b[38;5;230m{text}\x1b[0m"),
        kfs::FsEntryKind::Other => format!("\x1b[38;5;245m{text}\x1b[0m"),
    }
}

fn display_name(entry: &Entry, options: &Options) -> String {
    let suffix = if options.classify || matches!(entry.kind, kfs::FsEntryKind::Dir) {
        "/"
    } else {
        ""
    };
    format!("{}{suffix}", entry.name)
}

fn hyperlink_name(entry: &Entry, options: &Options) -> String {
    let name = display_name(entry, options);
    if matches!(options.hyperlink, HyperlinkStyle::Never) {
        return name;
    }

    let uri = trueosfs_file_uri(entry.path.as_str());
    format!("\x1b]8;;{uri}\x1b\\{name}\x1b]8;;\x1b\\")
}

fn trueosfs_file_uri(path: &str) -> String {
    let mut uri = String::from("file:///");
    for byte in path.as_bytes().iter().copied() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            uri.push('%');
            uri.push(HEX[(byte >> 4) as usize] as char);
            uri.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    uri
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

fn push_hex_byte(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0f) as usize] as char);
}

fn record_key_text(record_key: trueos_fs::RecordKey) -> String {
    match record_key {
        trueos_fs::RecordKey::Ffa => String::from("FFA"),
        trueos_fs::RecordKey::Key(key) => {
            let mut out = String::from("KEY:");
            for byte in key.handle.as_bytes()[..4].iter().copied() {
                push_hex_byte(&mut out, byte);
            }
            out
        }
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

fn table_row(entry: &Entry, base_depth: usize, options: &Options) -> TableRow {
    let is_dir = matches!(entry.kind, kfs::FsEntryKind::Dir);
    let depth = if options.tree {
        entry.depth.saturating_sub(base_depth.saturating_add(1))
    } else {
        0
    };
    TableRow {
        key: record_key_text(entry.record_key),
        id: entry_id(entry),
        size: human_size(entry.len, is_dir, options.size),
        kind: kind_text(entry.kind),
        name: format!("{}{}", "  ".repeat(depth), hyperlink_name(entry, options)),
    }
}

fn render_grid_entry(entry: &Entry, cell_width: usize, options: &Options) -> String {
    let visible = entry_id(entry).len() + 1 + display_name(entry, options).len();
    let label = format!("{} {}", entry_id(entry), hyperlink_name(entry, options));
    pad_visible(colorize(label.as_str(), entry.kind, options), visible, cell_width)
}

fn render_grid<W>(entries: &[Entry], options: &Options, write_line: &mut W)
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

fn render_oneline<W>(entries: &[Entry], options: &Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let label = format!("{} {}", entry_id(entry), hyperlink_name(entry, options));
        write_line(colorize(label.as_str(), entry.kind, options).as_str());
    }
}

fn render_long_header<W>(write_line: &mut W)
where
    W: FnMut(&str),
{
    write_line("FileID   Key               Size  Kind Name");
}

fn render_long<W>(entries: &[Entry], options: &Options, write_line: &mut W)
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

fn render_long_entry<W>(entry: &Entry, name_prefix: String, options: &Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    let size = human_size(entry.len, matches!(entry.kind, kfs::FsEntryKind::Dir), options.size);
    let record_key = record_key_text(entry.record_key);
    let name = colorize(
        format!("{name_prefix}{}", hyperlink_name(entry, options)).as_str(),
        entry.kind,
        options,
    );
    write_line(
        format!(
            "{:<8} {:<12} {:>7} {:<5} {}",
            entry_id(entry),
            record_key,
            size,
            kind_text(entry.kind),
            name
        )
        .as_str(),
    );
}

fn render_tree<W>(entries: &[Entry], base_depth: usize, options: &Options, write_line: &mut W)
where
    W: FnMut(&str),
{
    for entry in entries {
        let depth = entry.depth.saturating_sub(base_depth.saturating_add(1));
        let indent = "  ".repeat(depth);
        let name = colorize(
            format!("{} {}", entry_id(entry), hyperlink_name(entry, options)).as_str(),
            entry.kind,
            options,
        );
        write_line(format!("{indent}{name}").as_str());
    }
}

fn render_long_tree<W>(entries: &[Entry], base_depth: usize, options: &Options, write_line: &mut W)
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

fn sort_entries(entries: &mut [Entry], options: &Options) {
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

fn entry_is_ignored(entry: &Entry, base_depth: usize, options: &Options) -> bool {
    if options.ignore_globs.is_empty() {
        return false;
    }

    entry
        .path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .skip(base_depth)
        .any(|segment| {
            options
                .ignore_globs
                .iter()
                .any(|pattern| glob::matches(pattern.as_str(), segment))
        })
}

fn apply_listing_options(entries: &mut Vec<Entry>, base_depth: usize, options: &Options) {
    if !options.ignore_globs.is_empty() {
        entries.retain(|entry| !entry_is_ignored(entry, base_depth, options));
    }
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

fn render_entries<W>(entries: &[Entry], options: &Options, base_depth: usize, write_line: &mut W)
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
    let (id, record_key) = indexed_record(normalized.as_str());
    let name = if normalized.is_empty() {
        String::from(".")
    } else {
        String::from(basename(normalized.as_str()))
    };
    Entry {
        id,
        path: normalized.clone(),
        name,
        kind,
        depth: path_depth(normalized.as_str()),
        len,
        record_key,
    }
}

fn more_entry(prefix: &str) -> Entry {
    let path = join_path(prefix, "...");
    Entry {
        id: 0,
        depth: path_depth(path.as_str()),
        path,
        name: String::from("..."),
        kind: kfs::FsEntryKind::Other,
        len: None,
        record_key: trueos_fs::RecordKey::Ffa,
    }
}

fn list_one<W>(path: &str, options: &Options, write_line: &mut W) -> io::Result<()>
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
        "            --reverse  --group-dirs first|last|none  --depth N  --header  --classic",
    );
    write_line(
        "            -I/--ignore-glob PATTERN  --hyperlink always|auto|never  --ignore-config",
    );
    write_line("            --version  help");
    write_line("     paths: / and . both mean the TRUEOSFS root");
    write_line("     config: trueos/lsdconf.xdg (latched on first lsd command; reboot to reload)");
}

fn apply_short_flags(flags: &str, options: &mut Options) -> bool {
    for ch in flags.chars() {
        match ch {
            'l' => options.long = true,
            'R' => options.select_tree(),
            '1' => options.select_oneline(),
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

fn parse_hyperlink(value: &str) -> io::Result<HyperlinkStyle> {
    match value {
        "always" => Ok(HyperlinkStyle::Always),
        "auto" => Ok(HyperlinkStyle::Auto),
        "never" => Ok(HyperlinkStyle::Never),
        _ => Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd hyperlink")),
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
    parse_args_with_config(args, width, runtime_config::latched())
}

fn parse_args_with_config(
    args: &[String],
    width: usize,
    config: &runtime_config::RuntimeConfig,
) -> io::Result<(Options, Vec<String>, ParseAction)> {
    let ignore_config = args.iter().skip(1).any(|arg| arg == "--ignore-config");
    let mut options = Options::new((!ignore_config).then_some(config));
    options.width = width;
    let mut paths = Vec::new();
    let mut idx = 1usize;
    let mut cli_ignore_globs = false;
    let mut cli_classic = false;

    while idx < args.len() {
        let arg = args[idx].as_str();
        match arg {
            "help" | "-help" | "--help" | "-h" => return Ok((options, paths, ParseAction::Help)),
            "--version" => return Ok((options, paths, ParseAction::Version)),
            "-l" | "--long" => options.long = true,
            "-R" | "--tree" | "--recursive" => options.select_tree(),
            "-T" | "--table" => options.long = true,
            "-1" | "--oneline" => options.select_oneline(),
            "-d" | "--directory-only" => options.directory_only = true,
            "-F" | "--classify" => options.classify = true,
            "-N" | "--literal" | "-a" | "--all" | "-A" | "--almost-all" | "-i" | "--inode" => {}
            "--ignore-config" => {}
            "--classic" => cli_classic = true,
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
                options.select_tree();
            }
            raw if raw == "--hyperlink" || raw.starts_with("--hyperlink=") => {
                let value = parse_value_arg(args, &mut idx, raw.strip_prefix("--hyperlink="))?;
                options.hyperlink = parse_hyperlink(value.as_str())?;
            }
            raw if raw == "-I"
                || raw == "--ignore-glob"
                || raw.starts_with("--ignore-glob=")
                || (raw.starts_with("-I") && raw.len() > 2) =>
            {
                let inline = raw
                    .strip_prefix("--ignore-glob=")
                    .or_else(|| raw.strip_prefix("-I").filter(|value| !value.is_empty()));
                let value = parse_value_arg(args, &mut idx, inline)?;
                if !cli_ignore_globs {
                    options.ignore_globs.clear();
                    cli_ignore_globs = true;
                }
                options.ignore_globs.push(value);
            }
            raw if raw.starts_with('-') && apply_short_flags(&raw[1..], &mut options) => {}
            raw if raw.starts_with('-') => {
                return Err(io::Error::new(ErrorKind::InvalidInput, "unsupported lsd flag"));
            }
            path => paths.push(String::from(path)),
        }
        idx += 1;
    }

    if cli_classic {
        options.apply_classic();
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
        list_one(path.as_str(), &options, &mut write_line)?;
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
                Ok(stat) if matches!(stat.kind, api::FsNodeKind::File) => {
                    let (id, record_key) = indexed_record(normalized.as_str());
                    vec![Entry {
                        id,
                        path: normalized.clone(),
                        name: normalized.clone(),
                        kind: kfs::FsEntryKind::File,
                        depth: path_depth(normalized.as_str()),
                        len: Some(stat.len),
                        record_key,
                    }]
                }
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
        apply_listing_options(&mut entries, base_depth, &options);
        let rows = entries
            .iter()
            .map(|entry| table_row(entry, base_depth, &options))
            .collect();
        listings.push(TableListing { path, rows });
    }

    Ok(listings)
}

pub fn run(args: &[String]) -> io::Result<()> {
    run_with_writer(args, attached_line)
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{
        DEFAULT_GRID_WIDTH, Entry, HyperlinkStyle, Options, SortColumn, entry_is_ignored,
        hyperlink_name, parse_args_with_config, record_key_text, runtime_config,
    };
    use v::vio::kfs;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    #[test]
    fn native_record_key_labels_replace_unix_permissions() {
        assert_eq!(record_key_text(trueos_fs::RecordKey::Ffa), "FFA");

        let key = trueos_crypto::KeyRef::new(
            trueos_crypto::ProviderId::new([0x11; 16]).unwrap(),
            trueos_crypto::KeyHandle::new([0xab; 32]).unwrap(),
        );
        assert_eq!(record_key_text(trueos_fs::RecordKey::Key(key)), "KEY:abababab");
    }

    #[test]
    fn command_line_ignore_globs_replace_config_globs() {
        let config = runtime_config::RuntimeConfig::parse(
            r#"
ignore-globs:
  - .git
hyperlink: auto
"#,
        );
        let args = argv(&[
            "lsd",
            "--ignore-glob",
            "*.tmp",
            "-I*.bak",
            "--hyperlink=always",
        ]);
        let (options, _, _) = parse_args_with_config(&args, DEFAULT_GRID_WIDTH, &config).unwrap();

        assert_eq!(options.ignore_globs, vec![String::from("*.tmp"), String::from("*.bak")]);
        assert_eq!(options.hyperlink, HyperlinkStyle::Always);
    }

    #[test]
    fn ignore_config_bypasses_latched_values() {
        let config = runtime_config::RuntimeConfig::parse(
            r#"
sorting:
  column: extension
ignore-globs: [.git]
hyperlink: always
"#,
        );
        let args = argv(&["lsd", "--ignore-config"]);
        let (options, _, _) = parse_args_with_config(&args, DEFAULT_GRID_WIDTH, &config).unwrap();

        assert_eq!(options.sort, SortColumn::Name);
        assert!(options.ignore_globs.is_empty());
        assert_eq!(options.hyperlink, HyperlinkStyle::Never);
    }

    #[test]
    fn command_line_layout_overrides_config_layout() {
        let config = runtime_config::RuntimeConfig::parse("layout: tree");
        let args = argv(&["lsd", "--oneline"]);
        let (options, _, _) = parse_args_with_config(&args, DEFAULT_GRID_WIDTH, &config).unwrap();

        assert!(options.oneline);
        assert!(!options.tree);
    }

    #[test]
    fn ignored_tree_component_prunes_descendants() {
        let config = runtime_config::RuntimeConfig::parse("ignore-globs: [.git, '*.tmp']");
        let options = Options::new(Some(&config));
        let git_entry = Entry {
            id: 1,
            path: String::from("apps/demo/.git/objects/one"),
            name: String::from("one"),
            kind: kfs::FsEntryKind::File,
            depth: 5,
            len: Some(1),
            record_key: trueos_fs::RecordKey::Ffa,
        };
        let tmp_entry = Entry {
            id: 2,
            path: String::from("apps/demo/scratch.tmp"),
            name: String::from("scratch.tmp"),
            kind: kfs::FsEntryKind::File,
            depth: 3,
            len: Some(1),
            record_key: trueos_fs::RecordKey::Ffa,
        };

        assert!(entry_is_ignored(&git_entry, 2, &options));
        assert!(entry_is_ignored(&tmp_entry, 2, &options));
    }

    #[test]
    fn hyperlink_uses_percent_encoded_trueosfs_file_uri() {
        let config = runtime_config::RuntimeConfig::parse("hyperlink: always");
        let options = Options::new(Some(&config));
        let entry = Entry {
            id: 1,
            path: String::from("docs/my notes#1.txt"),
            name: String::from("my notes#1.txt"),
            kind: kfs::FsEntryKind::File,
            depth: 2,
            len: Some(1),
            record_key: trueos_fs::RecordKey::Ffa,
        };

        assert_eq!(
            hyperlink_name(&entry, &options),
            "\x1b]8;;file:///docs/my%20notes%231.txt\x1b\\my notes#1.txt\x1b]8;;\x1b\\"
        );
    }
}
