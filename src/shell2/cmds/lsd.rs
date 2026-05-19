use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::super::{
    ShellBackend2, line_width_for_backend, matrix_target_for_backend, print_native_line,
    print_shell_line,
};
use crate::shell2::shell2_cmd::ParseOutcome;

fn run_lsd(io: &'static dyn ShellBackend2, args: Vec<String>) -> trueos_io::Result<()> {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io).saturating_sub(2);
    crate::r::io::env::with_launch_context_console_and_fs_root(
        args.clone(),
        BTreeMap::new(),
        Some(target),
        None,
        || {
            trueos_lsd::run_with_writer_and_width(args.as_slice(), width, |line| {
                print_native_line(io, line)
            })
        },
    )
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = vec![String::from("lsd")];
    args.extend(rest.split_whitespace().map(String::from));
    let display_path = args.get(1).cloned();

    if let Err(err) = run_lsd(io, args) {
        if err.kind() == trueos_io::ErrorKind::NotFound {
            let path = display_path.as_deref().unwrap_or(".");
            print_shell_line(io, alloc::format!("lsd: {path}: not found").as_str());
        } else {
            print_shell_line(io, alloc::format!("lsd: {}", err).as_str());
        }
    }

    ParseOutcome::Handled
}
