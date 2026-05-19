use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::super::{matrix_target_for_backend, print_shell_line, ShellBackend2};
use crate::shell2::shell2_cmd::ParseOutcome;

fn run_lsd(io: &'static dyn ShellBackend2, args: Vec<String>) -> trueos_io::Result<()> {
    let target = matrix_target_for_backend(io);
    crate::r::io::env::with_launch_context_console_and_fs_root(
        args.clone(),
        BTreeMap::new(),
        Some(target),
        None,
        || trueos_lsd::run(args.as_slice()),
    )
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = vec![String::from("lsd")];
    args.extend(rest.split_whitespace().map(String::from));

    if let Err(err) = run_lsd(io, args) {
        print_shell_line(io, alloc::format!("lsd: {}", err).as_str());
    }

    ParseOutcome::Handled
}
