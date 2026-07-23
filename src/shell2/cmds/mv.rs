use alloc::string::String;
use alloc::vec::Vec;

use regex_automata::meta::Regex;

use super::super::{ShellBackend2, print_shell_line};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::shell2_cmd::ParseOutcome;

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

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn root_disk() -> Result<DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("no TRUEOSFS root")
}

async fn file_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map(|info| info.is_some())
}

async fn dir_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    if path.is_empty() {
        return Ok(true);
    }
    let marker = alloc::format!("{path}/.keep");
    if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await? {
        return Ok(true);
    }
    crate::r::fs::trueosfs::dir_has_children_async(disk, path).await
}

async fn path_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    if file_exists(disk, path).await? {
        return Ok(true);
    }
    dir_exists(disk, path).await
}

async fn list_dir(disk: DeviceHandle, path: &str) -> Result<Vec<String>, block::Error> {
    let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, path).await? else {
        return Ok(Vec::new());
    };
    Ok(listing
        .lines()
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect())
}

async fn rename_file(
    disk: DeviceHandle,
    src: &str,
    dst: &str,
) -> Result<bool, block::Error> {
    crate::r::fs::trueosfs::file_rename_async(disk, src, dst).await
}

async fn rename_dir(
    disk: DeviceHandle,
    src: &str,
    dst: &str,
) -> Result<bool, block::Error> {
    crate::r::fs::trueosfs::dir_rename_async(disk, src, dst).await
}

async fn collect_dir_files(
    disk: DeviceHandle,
    path: &str,
    files: &mut Vec<String>,
) -> Result<(), block::Error> {
    let mut pending_dirs = alloc::vec![String::from(path)];
    while let Some(dir) = pending_dirs.pop() {
        for child in list_dir(disk, dir.as_str()).await? {
            let child_path = join_path(dir.as_str(), child.as_str());
            if file_exists(disk, child_path.as_str()).await? {
                files.push(child_path);
            } else if dir_exists(disk, child_path.as_str()).await? {
                pending_dirs.push(child_path);
            }
        }
    }
    Ok(())
}

async fn move_path(
    disk: DeviceHandle,
    src: &str,
    dst: &str,
) -> Result<(usize, usize), block::Error> {
    if src == dst || dst.starts_with(alloc::format!("{src}/").as_str()) {
        return Ok((0, 1));
    }

    if path_exists(disk, dst).await? {
        return Ok((0, 1));
    }

    if file_exists(disk, src).await? {
        return if rename_file(disk, src, dst).await? {
            Ok((1, 0))
        } else {
            Ok((0, 1))
        };
    }

    if !dir_exists(disk, src).await? {
        return Ok((0, 1));
    }

    let mut files = Vec::new();
    collect_dir_files(disk, src, &mut files).await?;
    let count = files.len();
    if count == 0 {
        return Ok((0, 1));
    }
    if rename_dir(disk, src, dst).await? {
        Ok((count, 0))
    } else {
        Ok((0, count.max(1)))
    }
}

async fn move_children(
    disk: DeviceHandle,
    src_dir: &str,
    dst_dir: &str,
) -> Result<(usize, usize), block::Error> {
    if src_dir == dst_dir
        || dst_dir.starts_with(alloc::format!("{src_dir}/").as_str())
        || !dir_exists(disk, src_dir).await?
        || !dir_exists(disk, dst_dir).await?
    {
        return Ok((0, 1));
    }

    let mut files = Vec::new();
    collect_dir_files(disk, src_dir, &mut files).await?;
    let count = files.len();
    if count == 0 {
        return Ok((0, 1));
    }

    if rename_dir(disk, src_dir, dst_dir).await? {
        Ok((count, 0))
    } else {
        Ok((0, count.max(1)))
    }
}

fn print_usage(io: &'static dyn ShellBackend2, name: &str) {
    print_shell_line(
        io,
        alloc::format!(
            "{name}: usage `{name} <src> <dst>` | `{name} <src-dir>/* <dst-dir>` | `{name} -regx <pattern> <src-dir> <dst-dir>`"
        )
        .as_str(),
    );
}

async fn run_move(
    io: &'static dyn ShellBackend2,
    name: String,
    args: Vec<String>,
    disk: DeviceHandle,
) {
    if args.first().map(|arg| arg.as_str()) == Some("-regx") {
        if args.len() != 4 {
            print_usage(io, name.as_str());
            return;
        }
        let regex = match Regex::new(args[1].as_str()) {
            Ok(regex) => regex,
            Err(_) => {
                print_shell_line(io, alloc::format!("{name}: bad regex").as_str());
                return;
            }
        };
        let src_dir = match normalize_path(args[2].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return;
            }
        };
        let dst_dir = match normalize_path(args[3].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return;
            }
        };

        let mut moved = 0usize;
        let mut missed = 0usize;
        match list_dir(disk, src_dir.as_str()).await {
            Ok(children) => {
                for child in children {
                    let src = join_path(src_dir.as_str(), child.as_str());
                    if !regex.is_match(child.as_str()) && !regex.is_match(src.as_str()) {
                        continue;
                    }
                    let dst = join_path(dst_dir.as_str(), child.as_str());
                    match move_path(disk, src.as_str(), dst.as_str()).await {
                        Ok((ok, fail)) => {
                            moved = moved.saturating_add(ok);
                            missed = missed.saturating_add(fail);
                        }
                        Err(_) => missed = missed.saturating_add(1),
                    }
                }
            }
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {:?}", err).as_str());
                return;
            }
        }
        print_shell_line(
            io,
            alloc::format!("{name}: moved {moved} files, {missed} missed").as_str(),
        );
        return;
    }

    if args.len() != 2 {
        print_usage(io, name.as_str());
        return;
    }
    let src = match normalize_path(args[0].as_str(), false) {
        Ok(path) => path,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return;
        }
    };
    let mut dst = match normalize_path(args[1].as_str(), true) {
        Ok(path) => path,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return;
        }
    };
    if let Some(src_dir) = src.strip_suffix("/*") {
        match move_children(disk, src_dir, dst.as_str()).await {
            Ok((moved, 0)) if moved > 0 => {
                print_shell_line(io, alloc::format!("{name}: moved {moved} files").as_str());
            }
            Ok((moved, missed)) => {
                print_shell_line(
                    io,
                    alloc::format!("{name}: moved {moved} files, {missed} missed").as_str(),
                );
            }
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {:?}", err).as_str());
            }
        }
        return;
    }

    if dir_exists(disk, dst.as_str()).await.unwrap_or(false) {
        dst = join_path(dst.as_str(), basename(src.as_str()));
    }

    match move_path(disk, src.as_str(), dst.as_str()).await {
        Ok((moved, 0)) if moved > 0 => {
            print_shell_line(io, alloc::format!("{name}: moved {moved} files").as_str());
        }
        Ok((moved, missed)) => {
            print_shell_line(
                io,
                alloc::format!("{name}: moved {moved} files, {missed} missed").as_str(),
            );
        }
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {:?}", err).as_str());
        }
    }
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

    // Moving can perform several directory and rename operations. Keep the
    // whole sequence in one native future so no intermediate probe can block
    // the executor responsible for completing it.
    crate::wait::spawn_local_detached(run_move(io, String::from(name), args, disk));
    ParseOutcome::Handled
}
