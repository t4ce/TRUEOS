use core::str::SplitWhitespace;

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const MAX_DEPTH: usize = 3;
const MAX_CHILDREN_PER_DIR: usize = 24;
const MAX_LINES_PER_ROOT: usize = 160;
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

fn tree_child_names(
    disk_id: crate::disc::block::DiscId,
    path: &str,
) -> Result<Vec<String>, &'static str> {
    let Some(disk) = crate::disc::block::device_handle(disk_id) else {
        return Err("root handle missing");
    };
    let path_owned = String::from(path);

    let listing = crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::list_dir_async(disk, path_owned.as_str()).await
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
        crate::r::fs::trueosfs::file_info_async(disk, path_owned.as_str()).await
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

fn format_root_header(root: crate::r::fs::trueosfs::RootInfo) -> String {
    let Some(handle) = crate::disc::block::device_handle(root.disk_id) else {
        return alloc::format!("root {} seq={} (device missing)", root.disk_id, root.seq);
    };

    let info = handle.info();
    let label = info.label.as_deref().unwrap_or("-");
    let mode = if info.writable { "rw" } else { "ro" };
    alloc::format!(
        "root {} seq={} kind={:?} label={} {}",
        root.disk_id,
        root.seq,
        info.kind,
        label,
        mode
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

fn print_root_tree(io: &'static dyn ShellBackend2, root: crate::r::fs::trueosfs::RootInfo) {
    print_shell_line(io, format_root_header(root).as_str());

    let mut lines = Vec::new();
    push_tree_lines(&mut lines, root.disk_id, "", 0);
    for line in lines {
        print_shell_line(io, line.as_str());
    }
}

fn root_is_browsable(disk_id: crate::disc::block::DiscId) -> bool {
    matches!(tree_child_names(disk_id, ""), Ok(_))
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

            for root in roots.into_iter() {
                if !root_is_browsable(root.disk_id) {
                    skipped = skipped.saturating_add(1);
                    continue;
                }

                if shown > 0 {
                    print_shell_line(io, "");
                }
                print_root_tree(io, root);
                shown = shown.saturating_add(1);
            }

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
