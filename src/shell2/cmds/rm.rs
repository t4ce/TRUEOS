use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use regex_automata::meta::Regex;
use spin::Mutex;

use super::super::{MatrixTarget, ShellBackend2, print_matrix_target_line, print_shell_line};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

static NEXT_REMOVE_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static PENDING_REMOVES: Mutex<Vec<PendingRemove>> = Mutex::new(Vec::new());

#[derive(Clone)]
struct PendingRemove {
    id: u64,
    label: String,
    files: Vec<String>,
    folder_count: usize,
    confirm_total: usize,
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

fn normalize_path(path: &str, allow_empty: bool) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, allow_empty)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad path")
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        String::from(child)
    } else {
        alloc::format!("{parent}/{child}")
    }
}

fn root_disk() -> Result<DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("no TRUEOSFS root")
}

fn file_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    let path = String::from(path);
    crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::file_info_async(disk, path.as_str())
            .await
            .map(|info| info.is_some())
    })
}

fn dir_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    let path = String::from(path);
    crate::wait::spawn_and_wait_local(async move {
        if path.is_empty() {
            return Ok(true);
        }
        let marker = alloc::format!("{path}/.keep");
        if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await? {
            return Ok(true);
        }
        crate::r::fs::trueosfs::dir_has_children_async(disk, path.as_str()).await
    })
}

fn list_dir(disk: DeviceHandle, path: &str) -> Result<Vec<String>, block::Error> {
    let path = String::from(path);
    crate::wait::spawn_and_wait_local(async move {
        let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, path.as_str()).await?
        else {
            return Ok(Vec::new());
        };
        Ok(listing
            .lines()
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect())
    })
}

fn delete_file(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    let path = String::from(path);
    crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await
    })
}

fn collect_dir(
    disk: DeviceHandle,
    path: &str,
    files: &mut Vec<String>,
    folders: &mut usize,
) -> Result<(), block::Error> {
    *folders = folders.saturating_add(1);
    for child in list_dir(disk, path)? {
        let child_path = join_path(path, child.as_str());
        if file_exists(disk, child_path.as_str())? {
            files.push(child_path);
        } else if dir_exists(disk, child_path.as_str())? {
            collect_dir(disk, child_path.as_str(), files, folders)?;
        }
    }
    Ok(())
}

fn collect_one(disk: DeviceHandle, path: &str) -> Result<Option<PendingRemove>, block::Error> {
    if file_exists(disk, path)? {
        return Ok(Some(PendingRemove {
            id: 0,
            label: String::from(path),
            files: alloc::vec![String::from(path)],
            folder_count: 0,
            confirm_total: 0,
        }));
    }

    if !dir_exists(disk, path)? {
        return Ok(None);
    }

    let mut files = Vec::new();
    let mut folders = 0;
    collect_dir(disk, path, &mut files, &mut folders)?;
    let confirm_total = folders.saturating_add(files.len());
    Ok(Some(PendingRemove {
        id: 0,
        label: if path.is_empty() {
            String::from("/")
        } else {
            String::from(path)
        },
        files,
        folder_count: folders,
        confirm_total,
    }))
}

fn collect_regex(
    disk: DeviceHandle,
    base: &str,
    pattern: &str,
) -> Result<Option<PendingRemove>, &'static str> {
    let regex = Regex::new(pattern).map_err(|_| "bad regex")?;
    if !dir_exists(disk, base).map_err(|_| "filesystem error")? {
        return Ok(None);
    }

    let mut files = Vec::new();
    let mut folders = 0usize;
    let mut selected = 0usize;
    for child in list_dir(disk, base).map_err(|_| "filesystem error")? {
        let child_path = join_path(base, child.as_str());
        if !regex.is_match(child.as_str()) && !regex.is_match(child_path.as_str()) {
            continue;
        }
        selected = selected.saturating_add(1);
        if file_exists(disk, child_path.as_str()).map_err(|_| "filesystem error")? {
            files.push(child_path);
        } else if dir_exists(disk, child_path.as_str()).map_err(|_| "filesystem error")? {
            collect_dir(disk, child_path.as_str(), &mut files, &mut folders)
                .map_err(|_| "filesystem error")?;
        }
    }

    if selected == 0 {
        return Ok(None);
    }
    let confirm_total = folders.saturating_add(files.len());
    Ok(Some(PendingRemove {
        id: 0,
        label: alloc::format!("{} -regx {pattern}", if base.is_empty() { "." } else { base }),
        files,
        folder_count: folders,
        confirm_total,
    }))
}

fn push_pending(mut pending: PendingRemove) -> u64 {
    let id = NEXT_REMOVE_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    pending.id = id;
    PENDING_REMOVES.lock().push(pending);
    id
}

fn take_pending(id: u64) -> Option<PendingRemove> {
    let mut pending = PENDING_REMOVES.lock();
    let idx = pending.iter().position(|item| item.id == id)?;
    Some(pending.remove(idx))
}

fn print_usage(io: &'static dyn ShellBackend2, name: &str) {
    print_shell_line(
        io,
        alloc::format!("{name}: usage `{name} <file-or-dir>` | `{name} -regx <pattern> [dir]`")
            .as_str(),
    );
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, name: &str, rest: &str) -> ParseOutcome {
    let args = match parse_args(rest) {
        Ok(args) => args,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    if args.is_empty()
        || args
            .iter()
            .any(|arg| arg == "-h" || arg == "--help" || arg == "help")
    {
        print_usage(io, name);
        return ParseOutcome::Handled;
    }

    let disk = match root_disk() {
        Ok(disk) => disk,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    let pending = if args.first().map(|arg| arg.as_str()) == Some("-regx") {
        if args.len() < 2 || args.len() > 3 {
            print_usage(io, name);
            return ParseOutcome::Handled;
        }
        let base = match normalize_path(args.get(2).map(|arg| arg.as_str()).unwrap_or("."), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };
        match collect_regex(disk, base.as_str(), args[1].as_str()) {
            Ok(Some(pending)) => pending,
            Ok(None) => {
                print_shell_line(io, alloc::format!("{name}: no regex matches").as_str());
                return ParseOutcome::Handled;
            }
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        }
    } else {
        if args.len() != 1 {
            print_usage(io, name);
            return ParseOutcome::Handled;
        }
        let path = match normalize_path(args[0].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };
        if path.is_empty() {
            print_shell_line(
                io,
                alloc::format!("{name}: refusing to remove filesystem root").as_str(),
            );
            return ParseOutcome::Handled;
        }
        match collect_one(disk, path.as_str()) {
            Ok(Some(pending)) => pending,
            Ok(None) => {
                print_shell_line(io, alloc::format!("{name}: {}: not found", args[0]).as_str());
                return ParseOutcome::Handled;
            }
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {:?}", err).as_str());
                return ParseOutcome::Handled;
            }
        }
    };

    let id = push_pending(pending.clone());
    if pending.folder_count == 0 && pending.files.len() == 1 {
        print_shell_line(io, alloc::format!("{name}: remove {}?", pending.label).as_str());
        print_shell_line(io, alloc::format!("{name}: type `sure`").as_str());
    } else {
        print_shell_line(
            io,
            alloc::format!(
                "{name}: {} contains {} folders + {} files = {} entries",
                pending.label,
                pending.folder_count,
                pending.files.len(),
                pending.confirm_total
            )
            .as_str(),
        );
        print_shell_line(
            io,
            alloc::format!("{name}: type `sure {}`", pending.confirm_total).as_str(),
        );
    }
    ParseOutcome::StartSession(CommandSessionKind::RemoveSure(id))
}

pub(crate) fn handle_session_input(
    target: &MatrixTarget,
    submitted: &str,
    session_id: u64,
) -> CommandSessionInputResult {
    let Some(pending) = take_pending(session_id) else {
        print_matrix_target_line(target, "rm: session expired");
        return CommandSessionInputResult::CompleteIdle;
    };

    let expected = if pending.folder_count == 0 && pending.files.len() == 1 {
        String::from("sure")
    } else {
        alloc::format!("sure {}", pending.confirm_total)
    };

    if !submitted.trim().eq_ignore_ascii_case(expected.as_str()) {
        print_matrix_target_line(target, "rm: cancelled");
        return CommandSessionInputResult::CompleteIdle;
    }

    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_matrix_target_line(target, "rm: no TRUEOSFS root");
        return CommandSessionInputResult::CompleteIdle;
    };

    let mut removed = 0usize;
    let mut missed = 0usize;
    for path in pending.files.iter().rev() {
        match delete_file(disk, path.as_str()) {
            Ok(true) => removed = removed.saturating_add(1),
            Ok(false) => missed = missed.saturating_add(1),
            Err(err) => {
                print_matrix_target_line(target, alloc::format!("rm: {path}: {:?}", err).as_str());
                missed = missed.saturating_add(1);
            }
        }
    }

    print_matrix_target_line(
        target,
        alloc::format!("rm: removed {removed} files, {missed} missed").as_str(),
    );
    CommandSessionInputResult::CompleteIdle
}
