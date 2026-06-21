use alloc::string::String;

use super::super::{ShellBackend2, output_target_for_backend, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn parse_one_path(rest: &str) -> Result<Option<String>, &'static str> {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut saw = false;
    let mut finished = false;

    for ch in rest.trim().chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            saw = true;
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
                out.push(ch);
                saw = true;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            saw = true;
            continue;
        }
        if ch.is_whitespace() {
            if saw {
                finished = true;
            }
            continue;
        }
        if finished {
            return Err("too many arguments");
        }
        out.push(ch);
        saw = true;
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if escaped {
        out.push('\\');
    }
    if !saw || out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let path = match parse_one_path(rest) {
        Ok(Some(path)) => path,
        Ok(None) => {
            print_shell_line(io, "7z: usage `7z <file|archive.7z>`");
            return ParseOutcome::Handled;
        }
        Err(err) => {
            print_shell_line(io, alloc::format!("7z: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    let result = if path.ends_with(".7z") {
        crate::r::codec::enqueue_7z_extract_file(path.as_str(), output_target_for_backend(io))
    } else {
        crate::r::codec::enqueue_7z_compress_file(path.as_str(), output_target_for_backend(io))
    };

    match result {
        Ok(job) => {
            if let Some(slot) = job.slot {
                print_shell_line(
                    io,
                    alloc::format!("7z: queued job={} slot=§{}", job.id, slot).as_str(),
                );
            } else {
                print_shell_line(io, alloc::format!("7z: queued job={}", job.id).as_str());
            }
        }
        Err(err) => print_shell_line(io, alloc::format!("7z: {}", err).as_str()),
    }

    ParseOutcome::Handled
}
