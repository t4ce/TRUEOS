use core::str::SplitWhitespace;

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const MAX_DEPTH: usize = 3;
const MAX_CHILDREN_PER_DIR: usize = 24;
const MAX_LINES_PER_ROOT: usize = 160;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "file: usage `file`");
}

fn tree_child_names(
    disk_id: crate::disc::block::DiscId,
    path: &str,
) -> Result<Vec<String>, &'static str> {
    let Some(disk) = crate::disc::block::device_handle(disk_id) else {
        return Err("root handle missing");
    };
    let path_owned = String::from(path);

    let listing = crate::wait::spawn_and_wait_local(async move {
        crate::v::fs::trueosfs::list_dir_async(disk, path_owned.as_str()).await
    });

    match listing {
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

fn file_size_bytes(disk_id: crate::disc::block::DiscId, path: &str) -> Option<u64> {
    let disk = crate::disc::block::device_handle(disk_id)?;
    let path_owned = String::from(path);
    match crate::wait::spawn_and_wait_local(async move {
        crate::v::fs::trueosfs::file_info_async(disk, path_owned.as_str()).await
    }) {
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

fn format_root_header(root: crate::v::fs::trueosfs::RootInfo) -> String {
    let Some(handle) = crate::disc::block::device_handle(root.disk_id) else {
        return alloc::format!("root {} seq={} (device missing)", root.disk_id, root.seq);
    };

    let info = handle.info();
    let label = info.label.as_deref().unwrap_or("-");
    let mode = if info.writable { "rw" } else { "ro" };
    alloc::format!(
        "root {} seq={} kind={:?} label={} {}",
        root.disk_id, root.seq, info.kind, label, mode
    )
}

fn push_tree_lines(
    out: &mut Vec<String>,
    disk_id: crate::disc::block::DiscId,
    path: &str,
    depth: usize,
) {
    if depth >= MAX_DEPTH || out.len() >= MAX_LINES_PER_ROOT {
        return;
    }

    let children = match tree_child_names(disk_id, path) {
        Ok(children) => children,
        Err(err) => {
            let indent = "  ".repeat(depth + 1);
            out.push(alloc::format!("{}[{}]", indent, err));
            return;
        }
    };

    if children.is_empty() {
        if depth == 0 {
            out.push(String::from("  (empty)"));
        }
        return;
    }

    for (index, name) in children.iter().take(MAX_CHILDREN_PER_DIR).enumerate() {
        if out.len() >= MAX_LINES_PER_ROOT {
            break;
        }

        let full_path = child_path(path, name);
        let nested = tree_child_names(disk_id, full_path.as_str()).ok();
        let is_dir = nested.as_ref().is_some_and(|entries| !entries.is_empty());
        let indent = "  ".repeat(depth + 1);
        let branch = if is_dir { "+ " } else { "- " };

        if is_dir {
            out.push(alloc::format!("{}{}{}/", indent, branch, name));
            push_tree_lines(out, disk_id, full_path.as_str(), depth + 1);
        } else if let Some(size) = file_size_bytes(disk_id, full_path.as_str()) {
            out.push(alloc::format!("{}{}{} ({} bytes)", indent, branch, name, size));
        } else {
            out.push(alloc::format!("{}{}{}", indent, branch, name));
        }

        if index + 1 == MAX_CHILDREN_PER_DIR && children.len() > MAX_CHILDREN_PER_DIR {
            out.push(alloc::format!(
                "{}  ... {} more entries",
                indent,
                children.len() - MAX_CHILDREN_PER_DIR
            ));
        }
    }
}

fn print_root_tree(io: &'static dyn ShellBackend2, root: crate::v::fs::trueosfs::RootInfo) {
    print_shell_line(io, format_root_header(root).as_str());

    let mut lines = Vec::new();
    push_tree_lines(&mut lines, root.disk_id, "", 0);
    for line in lines {
        print_shell_line(io, line.as_str());
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let roots = crate::v::fs::trueosfs::list_roots();
    if roots.is_empty() {
        print_shell_line(io, "file: no TRUEOSFS roots mounted");
        return ParseOutcome::Handled;
    }

    for (index, root) in roots.into_iter().enumerate() {
        if index > 0 {
            print_shell_line(io, "");
        }
        print_root_tree(io, root);
    }

    ParseOutcome::Handled
}
