use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const RAMDISK_BLOCK_SIZE: u32 = 512;
const DEFAULT_RAMDISC_BYTES: u64 = 128 * 1024 * 1024;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "disc: usage `disc` | `disc format <disc-id>` | `disc ramdisc [size]` | `disc log [disc-id] [--max N]`",
    );
}

fn print_disc_table(io: &'static dyn ShellBackend2) {
    let choices = super::tlb_helper::collect_top_level_disk_choices();
    super::tlb_helper::print_disk_choice_table(io, "disc", "disk devices", choices.as_slice());
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

fn create_ramdisc(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> ParseOutcome {
    let size_arg = args.next();
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let size_bytes = match size_arg {
        Some(raw) => match parse_size_bytes(raw) {
            Some(v) => v,
            None => {
                print_shell_line(io, "disc ramdisc: invalid size (examples: 512MB, 1GB, 1024MiB)");
                return ParseOutcome::Handled;
            }
        },
        None => DEFAULT_RAMDISC_BYTES,
    };

    if size_arg.is_none() {
        print_shell_line(io, "disc ramdisc: using default size 128MiB");
    }

    let label = alloc::format!("ramdisc-{}mb", size_bytes / (1024 * 1024));
    let out: Result<_, alloc::string::String> = crate::wait::spawn_and_wait_local(async move {
        let disk =
            crate::r::disc::ramdisk::create_trueos_public(size_bytes, RAMDISK_BLOCK_SIZE, label)
                .await
                .map_err(|err| alloc::format!("create/format failed: {:?}", err))?;

        crate::r::fs::trueosfs::mount_root_async(disk)
            .await
            .map_err(|err| alloc::format!("mount failed: {:?}", err))?;

        Ok(disk)
    });

    match out {
        Ok(disk) => {
            let info = disk.info();
            let ready = crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED);
            print_shell_line(
                io,
                alloc::format!(
                    "disc ramdisc: ready id={} ({}) size={} bytes trueosfs=1 root_mounted={}",
                    info.id.raw(),
                    info.id,
                    size_bytes,
                    ready as u8
                )
                .as_str(),
            );
        }
        Err(msg) => {
            print_shell_line(io, alloc::format!("disc ramdisc: {}", msg).as_str());
        }
    }

    ParseOutcome::Handled
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        Some("help") | Some("-h") | Some("--help") => {
            print_usage(io);
            ParseOutcome::Handled
        }
        Some("format") => {
            let Some(arg) = args.next() else {
                super::format::print_format_disk_table(io);
                print_shell_line(
                    io,
                    "disc format: choose a disk id and run `disc format <disc-id>`",
                );
                return ParseOutcome::Handled;
            };
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }

            let Some(raw_id) = super::tlb_helper::parse_disc_id_raw(arg) else {
                print_shell_line(io, "disc format: invalid disk id");
                super::format::print_format_disk_table(io);
                return ParseOutcome::Handled;
            };
            let Some(disk) = super::tlb_helper::select_top_level_disk(raw_id) else {
                print_shell_line(io, "disc format: no such top-level disk");
                super::format::print_format_disk_table(io);
                return ParseOutcome::Handled;
            };

            super::format::start_format_session_for_disk(io, disk, "disc format")
        }
        Some("ramdisc") | Some("ramdisk") => create_ramdisc(io, args),
        Some("log") | Some("fslog") => super::fslog::try_parse_as(io, "disc log", args),
        Some(_) => {
            print_usage(io);
            ParseOutcome::Handled
        }
        None => {
            print_disc_table(io);
            ParseOutcome::Handled
        }
    }
}
