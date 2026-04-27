use alloc::format;
use alloc::string::String;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const STEP_LIMIT: usize = 100_000;

enum SourceInput {
    File { path: String },
    Inline { source: String },
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "c4: usage `c4 <file.c4>` | `c4 file <file.c4>` | `c4 inline <source>`");
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let Some(input) = parse_input(rest.trim_start(), io) else {
        return ParseOutcome::Handled;
    };

    let (source_name, source, stem) = match input {
        SourceInput::File { path } => {
            let bytes = match crate::r::io::kfs::read_file(path.as_str()) {
                Ok(bytes) => bytes,
                Err(err) => {
                    print_shell_line(
                        io,
                        format!("c4: read failed path={} err={:?}", path.as_str(), err).as_str(),
                    );
                    return ParseOutcome::Handled;
                }
            };
            let source = String::from_utf8_lossy(bytes.as_slice()).into_owned();
            let stem = output_stem(path.as_str());
            print_shell_line(
                io,
                format!("c4: source path={} bytes={}", path.as_str(), source.len()).as_str(),
            );
            (path, source, stem)
        }
        SourceInput::Inline { source } => {
            let stem = String::from("inline");
            print_shell_line(io, format!("c4: inline bytes={}", source.len()).as_str());
            (String::from("<inline>"), source, stem)
        }
    };

    let program = match trueos_c4::parse_program(source.as_str()) {
        Ok(program) => program,
        Err(err) => {
            print_shell_line(
                io,
                format!("c4: parse failed source={} err={:?}", source_name.as_str(), err).as_str(),
            );
            return ParseOutcome::Handled;
        }
    };

    let rust_source = trueos_c4::emit_rust(&program);
    let vm = match trueos_c4::emit_vm_object(&program) {
        Ok(vm) => vm,
        Err(err) => {
            print_shell_line(io, format!("c4: vm emit failed err={:?}", err).as_str());
            return ParseOutcome::Handled;
        }
    };

    let rust_path = artifact_path(stem.as_str(), ".rs");
    let vm_path = artifact_path(stem.as_str(), ".vm");
    write_artifact(io, rust_path.as_str(), rust_source.as_bytes());
    write_artifact(io, vm_path.as_str(), vm.bytes.as_slice());

    print_shell_line(
        io,
        format!(
            "c4: TC4O emitted path={} bytes={} code={} symbols={} stack={}",
            vm_path.as_str(),
            vm.bytes.len(),
            vm.code_len,
            vm.symbol_count,
            vm.stack_bytes
        )
        .as_str(),
    );

    match trueos_c4::run_vm_object(vm.bytes.as_slice(), STEP_LIMIT) {
        Ok(report) => {
            print_shell_line(
                io,
                format!(
                    "c4: TC4O ok code={} symbols={} stack={} steps={}",
                    report.code_len, report.symbol_count, report.stack_bytes, report.steps
                )
                .as_str(),
            );
            for local in report.locals.iter() {
                print_shell_line(io, format!("c4: local {}", format_local(local)).as_str());
            }
        }
        Err(err) => {
            print_shell_line(io, format!("c4: TC4O failed err={:?}", err).as_str());
        }
    }

    ParseOutcome::Handled
}

fn parse_input(rest: &str, io: &'static dyn ShellBackend2) -> Option<SourceInput> {
    if rest.trim().is_empty() {
        print_usage(io);
        return None;
    }

    let (head, tail) = split_first(rest);
    if head.eq_ignore_ascii_case("inline")
        || head.eq_ignore_ascii_case("source")
        || head.eq_ignore_ascii_case("src")
    {
        let source = tail.trim_start();
        if source.is_empty() {
            print_usage(io);
            return None;
        }
        return Some(SourceInput::Inline {
            source: String::from(source),
        });
    }

    if head.eq_ignore_ascii_case("file") {
        let (path, _) = split_first(tail.trim_start());
        if path.is_empty() {
            print_usage(io);
            return None;
        }
        return Some(SourceInput::File {
            path: String::from(path),
        });
    }

    if rest.trim_start().starts_with('{') {
        return Some(SourceInput::Inline {
            source: String::from(rest.trim_start()),
        });
    }

    Some(SourceInput::File {
        path: String::from(head),
    })
}

fn split_first(text: &str) -> (&str, &str) {
    let trimmed = text.trim_start();
    match trimmed.find(char::is_whitespace) {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => (trimmed, ""),
    }
}

fn output_stem(path: &str) -> String {
    if let Some(stem) = path.strip_suffix(".c4") {
        String::from(stem)
    } else {
        String::from(path)
    }
}

fn artifact_path(stem: &str, suffix: &str) -> String {
    let mut out = String::from(stem);
    out.push_str(suffix);
    out
}

fn write_artifact(io: &'static dyn ShellBackend2, path: &str, bytes: &[u8]) {
    let handle = match crate::r::io::kfs::write_file_begin(path, bytes.len() as u64) {
        Ok(handle) => handle,
        Err(err) => {
            print_shell_line(
                io,
                format!("c4: write begin failed path={} err={:?}", path, err).as_str(),
            );
            return;
        }
    };

    if let Err(err) = crate::r::io::kfs::write_file_chunk(handle, bytes) {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        print_shell_line(
            io,
            format!("c4: write chunk failed path={} err={:?}", path, err).as_str(),
        );
        return;
    }

    if let Err(err) = crate::r::io::kfs::write_file_finish(handle) {
        print_shell_line(
            io,
            format!("c4: write finish failed path={} err={:?}", path, err).as_str(),
        );
        return;
    }

    print_shell_line(io, format!("c4: wrote path={} bytes={}", path, bytes.len()).as_str());
}

fn format_local(local: &trueos_c4::VmLocalReport) -> String {
    match &local.value {
        trueos_c4::VmValue::Int(value) => format!("{}={}", local.name, value),
        trueos_c4::VmValue::Bool(value) => format!("{}={}", local.name, value),
        trueos_c4::VmValue::FloatBits(value) => format!("{}=f64bits:0x{:016x}", local.name, value),
        trueos_c4::VmValue::Bytes(bytes) => {
            let mut out = format!("{}=[", local.name);
            for (idx, chunk) in bytes.chunks(4).enumerate() {
                if idx != 0 {
                    out.push_str(", ");
                }
                if chunk.len() == 4 {
                    let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    out.push_str(format!("{}", value).as_str());
                } else {
                    out.push('?');
                }
            }
            out.push(']');
            out
        }
    }
}
