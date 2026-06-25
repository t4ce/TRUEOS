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

fn path_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    Ok(file_exists(disk, path)? || dir_exists(disk, path)?)
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

fn rename_file(disk: DeviceHandle, src: &str, dst: &str) -> Result<bool, block::Error> {
    let src = String::from(src);
    let dst = String::from(dst);
    crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::file_rename_async(disk, src.as_str(), dst.as_str()).await
    })
}

fn rename_dir(disk: DeviceHandle, src: &str, dst: &str) -> Result<bool, block::Error> {
    let src = String::from(src);
    let dst = String::from(dst);
    crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::dir_rename_async(disk, src.as_str(), dst.as_str()).await
    })
}

fn collect_dir_files(
    disk: DeviceHandle,
    path: &str,
    files: &mut Vec<String>,
) -> Result<(), block::Error> {
    for child in list_dir(disk, path)? {
        let child_path = join_path(path, child.as_str());
        if file_exists(disk, child_path.as_str())? {
            files.push(child_path);
        } else if dir_exists(disk, child_path.as_str())? {
            collect_dir_files(disk, child_path.as_str(), files)?;
        }
    }
    Ok(())
}

fn move_path(disk: DeviceHandle, src: &str, dst: &str) -> Result<(usize, usize), block::Error> {
    if src == dst || dst.starts_with(alloc::format!("{src}/").as_str()) {
        return Ok((0, 1));
    }

    if path_exists(disk, dst)? {
        return Ok((0, 1));
    }

    if file_exists(disk, src)? {
        return if rename_file(disk, src, dst)? {
            Ok((1, 0))
        } else {
            Ok((0, 1))
        };
    }

    if !dir_exists(disk, src)? {
        return Ok((0, 1));
    }

    let mut files = Vec::new();
    collect_dir_files(disk, src, &mut files)?;
    let count = files.len();
    if count == 0 {
        return Ok((0, 1));
    }
    if rename_dir(disk, src, dst)? {
        Ok((count, 0))
    } else {
        Ok((0, count.max(1)))
    }
}

fn move_children(
    disk: DeviceHandle,
    src_dir: &str,
    dst_dir: &str,
) -> Result<(usize, usize), block::Error> {
    if src_dir == dst_dir
        || dst_dir.starts_with(alloc::format!("{src_dir}/").as_str())
        || !dir_exists(disk, src_dir)?
        || !dir_exists(disk, dst_dir)?
    {
        return Ok((0, 1));
    }

    let mut files = Vec::new();
    collect_dir_files(disk, src_dir, &mut files)?;
    let count = files.len();
    if count == 0 {
        return Ok((0, 1));
    }

    if rename_dir(disk, src_dir, dst_dir)? {
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

    if args.first().map(|arg| arg.as_str()) == Some("-regx") {
        if args.len() != 4 {
            print_usage(io, name);
            return ParseOutcome::Handled;
        }
        let regex = match Regex::new(args[1].as_str()) {
            Ok(regex) => regex,
            Err(_) => {
                print_shell_line(io, alloc::format!("{name}: bad regex").as_str());
                return ParseOutcome::Handled;
            }
        };
        let src_dir = match normalize_path(args[2].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };
        let dst_dir = match normalize_path(args[3].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };

        let mut moved = 0usize;
        let mut missed = 0usize;
        match list_dir(disk, src_dir.as_str()) {
            Ok(children) => {
                for child in children {
                    let src = join_path(src_dir.as_str(), child.as_str());
                    if !regex.is_match(child.as_str()) && !regex.is_match(src.as_str()) {
                        continue;
                    }
                    let dst = join_path(dst_dir.as_str(), child.as_str());
                    match move_path(disk, src.as_str(), dst.as_str()) {
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
                return ParseOutcome::Handled;
            }
        }
        print_shell_line(
            io,
            alloc::format!("{name}: moved {moved} files, {missed} missed").as_str(),
        );
        return ParseOutcome::Handled;
    }

    if args.len() != 2 {
        print_usage(io, name);
        return ParseOutcome::Handled;
    }
    let src = match normalize_path(args[0].as_str(), false) {
        Ok(path) => path,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };
    let mut dst = match normalize_path(args[1].as_str(), true) {
        Ok(path) => path,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };
    if let Some(src_dir) = src.strip_suffix("/*") {
        match move_children(disk, src_dir, dst.as_str()) {
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
        return ParseOutcome::Handled;
    }

    if dir_exists(disk, dst.as_str()).unwrap_or(false) {
        dst = join_path(dst.as_str(), basename(src.as_str()));
    }

    match move_path(disk, src.as_str(), dst.as_str()) {
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
    ParseOutcome::Handled
}
