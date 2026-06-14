use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "disc: usage `disc` | `disc format <disc-id>` | `disc log [disc-id] [--max N]`",
    );
}

fn print_disc_table(io: &'static dyn ShellBackend2) {
    let choices = super::tlb_helper::collect_top_level_disk_choices();
    super::tlb_helper::print_disk_choice_table(io, "disc", "disk devices", choices.as_slice());
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
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
