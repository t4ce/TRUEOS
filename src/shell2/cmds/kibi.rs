use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use trueos_io::Read;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_shell_line,
    switch_matrix_target_slot,
};

struct MatrixInput {
    target: MatrixTarget,
}

impl Read for MatrixInput {
    fn read(&mut self, buf: &mut [u8]) -> trueos_io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            if let Some(byte) = crate::shell2::read_matrix_target_byte(&self.target) {
                buf[0] = byte;
                return Ok(1);
            }
            crate::wait::park_step();
        }
    }
}

fn is_slot_arg(arg: &str) -> bool {
    arg.starts_with('§') && arg.len() > '§'.len_utf8()
}

fn run_kibi(
    target: MatrixTarget,
    args: Vec<String>,
    file_name: Option<&str>,
) -> Result<(), trueos_kibi::Error> {
    let mut input = MatrixInput {
        target: target.clone(),
    };
    let result = crate::r::io::env::with_launch_context_console_and_fs_root(
        args,
        BTreeMap::new(),
        Some(target),
        None,
        || trueos_kibi::run(file_name, &mut input),
    );
    result
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let raw_args: Vec<&str> = rest.split_whitespace().collect();
    let first = raw_args.first().copied();
    let second = raw_args.get(1).copied();
    let remaining = raw_args.len().saturating_sub(2);

    if matches!(first, Some("--version")) && second.is_none() {
        print_shell_line(io, concat!("kibi ", env!("CARGO_PKG_VERSION")));
        return ParseOutcome::Handled;
    }

    let base_target = matrix_target_for_backend(io);
    let (target, file_name) = match (first, second, remaining) {
        (Some(slot), file, 0) if is_slot_arg(slot) => {
            (switch_matrix_target_slot(&base_target, slot), file)
        }
        (file, None, 0) => (base_target, file),
        _ => {
            print_shell_line(io, "kibi: usage `kibi` | `kibi <file>` | `kibi §slot [file]`");
            return ParseOutcome::Handled;
        }
    };

    let mut args = vec![String::from("kibi")];
    if let Some(slot) = first.filter(|arg| is_slot_arg(arg)) {
        args.push(String::from(slot));
    }
    if let Some(file) = file_name {
        args.push(String::from(file));
    }

    if let Err(err) = run_kibi(target, args, file_name) {
        print_shell_line(io, alloc::format!("kibi: {:?}", err).as_str());
    }

    ParseOutcome::Handled
}
