use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::shell2_localcoder;

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let mut prompt_parts: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        prompt_parts.push(String::from(arg));
    }

    let prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join(" "))
    };

    if !shell2_localcoder::submit(spawner, io, prompt) {
        print_shell_line(io, "lc: spawn failed");
    }

    ParseOutcome::Handled
}
