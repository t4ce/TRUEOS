use core::str::SplitWhitespace;

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const MAX_DEPTH: usize = 3;
const MAX_CHILDREN_PER_DIR: usize = 24;
const MAX_LINES_PER_ROOT: usize = 160;
const TRACE_DEPTH: usize = 1;
const RAMDISK_BLOCK_SIZE: u32 = 512;
const DEFAULT_RAMDISC_BYTES: u64 = 128 * 1024 * 1024;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "file: usage `file` | `file format <disk-id>` | `file ramdisc <size>`");
}

fn parse_size_bytes(raw: &str) -> Option<u64> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }

    let digits_len = text.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }

    let number = text[..digits_len].parse::<u64>().ok()?;
    let suffix = text[digits_len..].trim();
    let mul = if suffix.is_empty() {
        1_048_576u64
    } else if suffix.eq_ignore_ascii_case("B") {
        1u64
    } else if suffix.eq_ignore_ascii_case("KB") || suffix.eq_ignore_ascii_case("K") {
        1_000u64
    } else if suffix.eq_ignore_ascii_case("MB") || suffix.eq_ignore_ascii_case("M") {
        1_000_000u64
    } else if suffix.eq_ignore_ascii_case("GB") || suffix.eq_ignore_ascii_case("G") {
        1_000_000_000u64
    } else if suffix.eq_ignore_ascii_case("KIB") {
        1_024u64
    } else if suffix.eq_ignore_ascii_case("MIB") {
        1_048_576u64
    } else if suffix.eq_ignore_ascii_case("GIB") {
        1_073_741_824u64
    } else {
        return None;
    };

    number.checked_mul(mul)
}

async fn tree_child_names_async(
    disk_id: crate::disc::block::DiscId,
    path: &str,
) -> Result<Vec<String>, &'static str> {
    let Some(disk) = crate::disc::block::device_handle(disk_id) else {
        return Err("root handle missing");
    };

    match crate::r::fs::trueosfs::list_dir_async(disk, path).await {
        Ok(Some(text)) => Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect()),
        Ok(None) => Err("not a TRUEOSFS root"),
        Err(_) => Err("list failed"),
    }
}

async fn file_size_bytes_async(disk_id: crate::disc::block::DiscId, path: &str) -> Option<u64> {
    let disk = crate::disc::block::device_handle(disk_id)?;
    match crate::r::fs::trueosfs::file_info_async(disk, path).await {
        Ok(Some(info)) => Some(info.data_len),
        _ => None,
    }
}

fn child_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        return String::from(name);
    }

    let mut out = String::from(parent);
    out.push('/');
    out.push_str(name);
    out
}

fn format_root_header(root: crate::r::fs::trueosfs::RootInfo) -> String {
    let Some(handle) = crate::disc::block::device_handle(root.disk_id) else {
        return alloc::format!("root {} seq={} (device missing)", root.disk_id, root.seq);
    };

    let info = handle.info();
    let label = info.label.as_deref().unwrap_or("-");
    let mode = if info.writable { "rw" } else { "ro" };
    alloc::format!(
        "root {} seq={} kind={:?} label={} {} index={}{}",
        root.disk_id,
        root.seq,
        info.kind,
        label,
        mode,
        if root.index_ready { "ready" } else { "cold" },
        if root.index_building { " building" } else { "" },
    )
}

struct TreeEntry {
    name: String,
    full_path: String,
    is_dir: bool,
    size_bytes: Option<u64>,
}

enum TreeWorkItem {
    PrintLine(String),
    VisitDir { path: String, depth: usize },
}

struct RootRender {
    root: crate::r::fs::trueosfs::RootInfo,
    lines: Result<Vec<String>, &'static str>,
}

fn index_child_names(paths: &[String], parent: &str) -> Vec<String> {
    let prefix = if parent.is_empty() {
        String::new()
    } else {
        let mut p = String::from(parent);
        p.push('/');
        p
    };
    let mut children = BTreeSet::new();

    for path in paths.iter() {
        let rest = if prefix.is_empty() {
            path.as_str()
        } else if let Some(rest) = path.strip_prefix(prefix.as_str()) {
            rest
        } else {
            continue;
        };

        let seg = rest.split('/').next().unwrap_or("");
        if !seg.is_empty() {
            children.insert(String::from(seg));
        }
    }

    children.into_iter().collect()
}

fn index_path_exists(paths: &[String], path: &str) -> bool {
    paths.iter().any(|p| p == path)
}

fn index_has_descendant(paths: &[String], path: &str) -> bool {
    let mut prefix = String::from(path);
    prefix.push('/');
    paths.iter().any(|p| p.starts_with(prefix.as_str()))
}

fn build_root_tree_lines_from_index(
    root: crate::r::fs::trueosfs::RootInfo,
) -> Result<Vec<String>, &'static str> {
    let Some(paths) =
        crate::r::fs::trueosfs::root_index_paths(root.disk_id, MAX_LINES_PER_ROOT * 4)
    else {
        return Err("index unavailable");
    };

    let mut out = Vec::new();
    let mut work = Vec::new();
    work.push(TreeWorkItem::VisitDir {
        path: String::new(),
        depth: 0,
    });

    while let Some(item) = work.pop() {
        if out.len() >= MAX_LINES_PER_ROOT {
            break;
        }

        match item {
            TreeWorkItem::PrintLine(line) => out.push(line),
            TreeWorkItem::VisitDir { path, depth } => {
                if depth >= MAX_DEPTH {
                    continue;
                }

                let children = index_child_names(paths.as_slice(), path.as_str());
                if children.is_empty() {
                    if depth == 0 {
                        out.push(String::from("  (empty)"));
                    }
                    continue;
                }

                let indent = "  ".repeat(depth + 1);
                if children.len() > MAX_CHILDREN_PER_DIR {
                    work.push(TreeWorkItem::PrintLine(alloc::format!(
                        "{}  ... {} more entries",
                        indent,
                        children.len() - MAX_CHILDREN_PER_DIR
                    )));
                }

                for name in children.iter().take(MAX_CHILDREN_PER_DIR).rev() {
                    let full_path = child_path(path.as_str(), name.as_str());
                    let is_dir = index_has_descendant(paths.as_slice(), full_path.as_str())
                        && !index_path_exists(paths.as_slice(), full_path.as_str());
                    let line = if is_dir {
                        alloc::format!("{}+ {}/", indent, name)
                    } else {
                        alloc::format!("{}- {}", indent, name)
                    };

                    if is_dir {
                        work.push(TreeWorkItem::VisitDir {
                            path: full_path,
                            depth: depth + 1,
                        });
                    }
                    work.push(TreeWorkItem::PrintLine(line));
                }
            }
        }
    }

    Ok(out)
}

fn browse_path_label(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

async fn describe_tree_entry_async(
    disk_id: crate::disc::block::DiscId,
    parent: &str,
    name: &str,
) -> TreeEntry {
    let full_path = child_path(parent, name);
    let nested = tree_child_names_async(disk_id, full_path.as_str())
        .await
        .ok();
    let is_dir = nested.as_ref().is_some_and(|entries| !entries.is_empty());
    let size_bytes = if is_dir {
        None
    } else {
        file_size_bytes_async(disk_id, full_path.as_str()).await
    };

    TreeEntry {
        name: String::from(name),
        full_path,
        is_dir,
        size_bytes,
    }
}

async fn build_root_tree_lines_async(
    root: crate::r::fs::trueosfs::RootInfo,
) -> Result<Vec<String>, &'static str> {
    crate::log!(
        "file: browse start root={} seq={} index_ready={} index_building={} depth_limit={} child_limit={} line_limit={}\n",
        root.disk_id,
        root.seq,
        root.index_ready as u8,
        root.index_building as u8,
        MAX_DEPTH,
        MAX_CHILDREN_PER_DIR,
        MAX_LINES_PER_ROOT,
    );

    if crate::disc::block::device_handle(root.disk_id).is_none() {
        crate::log!("file: browse abort root={} err=root handle missing\n", root.disk_id);
        return Err("root handle missing");
    }

    let mut out = Vec::new();
    let mut work = Vec::new();
    work.push(TreeWorkItem::VisitDir {
        path: String::new(),
        depth: 0,
    });

    while let Some(item) = work.pop() {
        if out.len() >= MAX_LINES_PER_ROOT {
            break;
        }

        match item {
            TreeWorkItem::PrintLine(line) => out.push(line),
            TreeWorkItem::VisitDir { path, depth } => {
                if depth >= MAX_DEPTH {
                    continue;
                }

                if depth <= TRACE_DEPTH {
                    crate::log!(
                        "file: browse root={} list path={} depth={} stage=begin\n",
                        root.disk_id,
                        browse_path_label(path.as_str()),
                        depth,
                    );
                }

                let children = match tree_child_names_async(root.disk_id, path.as_str()).await {
                    Ok(children) => {
                        if depth <= TRACE_DEPTH {
                            crate::log!(
                                "file: browse root={} list path={} depth={} stage=ok children={}\n",
                                root.disk_id,
                                browse_path_label(path.as_str()),
                                depth,
                                children.len(),
                            );
                        }
                        children
                    }
                    Err(err) => {
                        crate::log!(
                            "file: browse root={} list path={} depth={} stage=err err={}\n",
                            root.disk_id,
                            browse_path_label(path.as_str()),
                            depth,
                            err,
                        );
                        if depth == 0 {
                            return Err(err);
                        }
                        let indent = "  ".repeat(depth + 1);
                        work.push(TreeWorkItem::PrintLine(alloc::format!("{}[{}]", indent, err)));
                        continue;
                    }
                };

                if children.is_empty() {
                    if depth == 0 {
                        out.push(String::from("  (empty)"));
                    }
                    continue;
                }

                let mut entries = Vec::new();
                for name in children.iter().take(MAX_CHILDREN_PER_DIR) {
                    if out.len().saturating_add(entries.len()) >= MAX_LINES_PER_ROOT {
                        break;
                    }
                    entries
                        .push(describe_tree_entry_async(root.disk_id, path.as_str(), name).await);
                }

                let indent = "  ".repeat(depth + 1);
                if children.len() > MAX_CHILDREN_PER_DIR {
                    work.push(TreeWorkItem::PrintLine(alloc::format!(
                        "{}  ... {} more entries",
                        indent,
                        children.len() - MAX_CHILDREN_PER_DIR
                    )));
                }

                for entry in entries.into_iter().rev() {
                    let line = if entry.is_dir {
                        alloc::format!("{}+ {}/", indent, entry.name)
                    } else if let Some(size) = entry.size_bytes {
                        alloc::format!("{}- {} ({} bytes)", indent, entry.name, size)
                    } else {
                        alloc::format!("{}- {}", indent, entry.name)
                    };

                    if entry.is_dir {
                        work.push(TreeWorkItem::VisitDir {
                            path: entry.full_path,
                            depth: depth + 1,
                        });
                    }
                    work.push(TreeWorkItem::PrintLine(line));
                }
            }
        }
    }

    crate::log!(
        "file: browse done root={} lines={} truncated={}\n",
        root.disk_id,
        out.len(),
        (out.len() >= MAX_LINES_PER_ROOT) as u8,
    );

    Ok(out)
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        Some("ramdisc") | Some("ramdisk") => {
            let size_arg = args.next();
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }

            let size_bytes = match size_arg {
                Some(raw) => match parse_size_bytes(raw) {
                    Some(v) => v,
                    None => {
                        print_shell_line(
                            io,
                            "file ramdisc: invalid size (examples: 512MB, 1GB, 1024MiB)",
                        );
                        return ParseOutcome::Handled;
                    }
                },
                None => DEFAULT_RAMDISC_BYTES,
            };

            if size_arg.is_none() {
                print_shell_line(io, "file ramdisc: using default size 128MiB");
            }

            let label = alloc::format!("ramdisc-{}mb", size_bytes / (1024 * 1024));
            let out = crate::wait::spawn_and_wait_local(async move {
                let disk = crate::r::disc::ramdisk::create_trueos_public(
                    size_bytes,
                    RAMDISK_BLOCK_SIZE,
                    label,
                )
                .await;
                let disk = match disk {
                    Ok(disk) => disk,
                    Err(err) => {
                        return Err(alloc::format!("create/format failed: {:?}", err));
                    }
                };

                match crate::r::fs::trueosfs::mount_root_async(disk).await {
                    Ok(Some(_)) | Ok(None) => {}
                    Err(err) => {
                        return Err(alloc::format!("mount failed: {:?}", err));
                    }
                }

                Ok(disk)
            });

            match out {
                Ok(disk) => {
                    let info = disk.info();
                    let ready =
                        crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED);
                    print_shell_line(
                        io,
                        alloc::format!(
                            "file ramdisc: ready id={} ({}) size={} bytes trueosfs=1 root_mounted={}",
                            info.id.raw(),
                            info.id,
                            size_bytes,
                            ready as u8
                        )
                        .as_str(),
                    );
                }
                Err(msg) => {
                    print_shell_line(io, alloc::format!("file ramdisc: {}", msg).as_str());
                }
            }

            ParseOutcome::Handled
        }
        Some("format") => {
            let Some(arg) = args.next() else {
                super::format::print_format_disk_table(io);
                print_shell_line(
                    io,
                    "file format: choose a disk id and run `file format <disk-id>`",
                );
                return ParseOutcome::Handled;
            };
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }

            let Some(raw_id) = super::tlb_helper::parse_disc_id_raw(arg) else {
                print_shell_line(io, "file format: invalid disk id");
                super::format::print_format_disk_table(io);
                return ParseOutcome::Handled;
            };
            let Some(disk) = super::tlb_helper::select_top_level_disk(raw_id) else {
                print_shell_line(io, "file format: no such top-level disk");
                super::format::print_format_disk_table(io);
                return ParseOutcome::Handled;
            };

            super::format::start_format_session_for_disk(io, disk, "file format")
        }
        Some(_) => {
            print_usage(io);
            ParseOutcome::Handled
        }
        None => {
            let roots = crate::r::fs::trueosfs::list_roots();
            if roots.is_empty() {
                print_shell_line(io, "file: no TRUEOSFS roots mounted");
                return ParseOutcome::Handled;
            }

            let mut shown = 0usize;
            let mut skipped = 0usize;

            crate::log!("file: command begin roots={}\n", roots.len());

            for root in roots.into_iter() {
                if shown > 0 {
                    print_shell_line(io, "");
                }
                print_shell_line(io, format_root_header(root).as_str());

                if !root.index_ready {
                    crate::r::fs::trueosfs::request_warm_index(root.disk_id);
                    let note = if root.index_building {
                        "  (index build already in progress; browse skipped for now)"
                    } else {
                        "  (index cold; warming in background, browse skipped for now)"
                    };
                    print_shell_line(io, note);
                    crate::log!(
                        "file: command root={} seq={} skipped reason=index-cold building={}\n",
                        root.disk_id,
                        root.seq,
                        root.index_building as u8,
                    );
                    shown = shown.saturating_add(1);
                    continue;
                }

                print_shell_line(
                    io,
                    alloc::format!(
                        "  (scanning tree depth<={} children<={} lines<={})",
                        MAX_DEPTH,
                        MAX_CHILDREN_PER_DIR,
                        MAX_LINES_PER_ROOT
                    )
                    .as_str(),
                );

                let render = RootRender {
                    root,
                    lines: build_root_tree_lines_from_index(root),
                };

                let Ok(lines) = render.lines else {
                    print_shell_line(io, "  [browse failed]");
                    crate::log!(
                        "file: command root={} seq={} result=error\n",
                        render.root.disk_id,
                        render.root.seq,
                    );
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                for line in lines.iter() {
                    print_shell_line(io, line.as_str());
                }
                crate::log!(
                    "file: command root={} seq={} result=ok lines={}\n",
                    render.root.disk_id,
                    render.root.seq,
                    lines.len(),
                );
                shown = shown.saturating_add(1);
            }

            crate::log!("file: command done shown={} skipped={}\n", shown, skipped);

            if shown == 0 {
                print_shell_line(io, "file: no browsable TRUEOSFS roots");
            } else if skipped > 0 {
                print_shell_line(
                    io,
                    alloc::format!("file: skipped {} unavailable root(s)", skipped).as_str(),
                );
            }

            ParseOutcome::Handled
        }
    }
}
