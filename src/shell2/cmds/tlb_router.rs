use core::str::SplitWhitespace;

use trueos_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn multiline(io: &'static dyn ShellBackend2, text: &str) {
    for text_line in text.lines() {
        line(io, text_line.trim_end_matches('\r'));
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.clone().next() {
        Some("nct") => {
            let _ = args.next();
            match (args.next(), args.next()) {
                (Some("probe"), None) => {
                    multiline(io, &super::tlb_nct_probe::build_probe_text());
                }
                _ => line(io, "tlb: usage `tlb nct probe`"),
            }
            ParseOutcome::Handled
        }
        Some("mei") => {
            let _ = args.next();
            match (args.next(), args.next()) {
                (Some("probe"), None) => {
                    multiline(io, &super::tlb_mei_probe::build_probe_text());
                }
                _ => line(io, "tlb: usage `tlb mei probe`"),
            }
            ParseOutcome::Handled
        }
        None => {
            let outcome = super::tlb_core::try_parse(spawner, io, args);
            line(io, "nct       Verify NCT5585D Super-I/O identity (`tlb nct probe`)");
            line(io, "mei       Verify reversible MEI status-window access (`tlb mei probe`)");
            outcome
        }
        _ => super::tlb_core::try_parse(spawner, io, args),
    }
}
