use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const DEMO_PATH: &str = "shader/demo.c4";
const DEMO_SOURCE: &str = "{ int out; out = 1234 + 6; }\n";

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match args.next() {
        Some("list") => list(io),
        Some("compile") => compile(io, args.next()),
        Some("demo") => demo(io),
        Some("now") => compile_now(io, args.next()),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "shader: usage `shader list` | `shader compile [shader/file.c4]` | `shader demo` | `shader now <shader/file.c4>`",
    );
}

fn list(io: &'static dyn ShellBackend2) {
    match crate::r::shader::list_compile_candidates_sync() {
        Ok(candidates) if candidates.is_empty() => {
            print_shell_line(io, "shader: no compile candidates in /shader");
        }
        Ok(candidates) => {
            print_shell_line(
                io,
                format!("shader: compile candidates={}", candidates.len()).as_str(),
            );
            for path in candidates.iter() {
                print_shell_line(io, format!("shader: candidate {}", path).as_str());
            }
        }
        Err(err) => {
            print_shell_line(io, format!("shader: list failed err={:?}", err).as_str());
        }
    }
}

fn compile(io: &'static dyn ShellBackend2, path: Option<&str>) {
    match path {
        Some(path) => match crate::r::shader::enqueue_compile_path(path) {
            Ok(job) => print_shell_line(
                io,
                format!(
                    "shader: queued job={} path={} pending={}",
                    job.id,
                    path,
                    crate::r::shader::pending_jobs()
                )
                .as_str(),
            ),
            Err(err) => {
                print_shell_line(io, format!("shader: queue failed err={:?}", err).as_str())
            }
        },
        None => {
            let job = crate::r::shader::enqueue_scan_dir();
            print_shell_line(
                io,
                format!(
                    "shader: queued scan job={} dir=/shader pending={}",
                    job.id,
                    crate::r::shader::pending_jobs()
                )
                .as_str(),
            );
        }
    }
}

fn demo(io: &'static dyn ShellBackend2) {
    if let Err(err) = crate::r::io::kfs::create_dir_all("shader") {
        print_shell_line(io, format!("shader: demo mkdir failed err={:?}", err).as_str());
        return;
    }
    if !write_file(DEMO_PATH, DEMO_SOURCE.as_bytes()) {
        print_shell_line(io, "shader: demo write failed");
        return;
    }
    match crate::r::shader::enqueue_compile_path(DEMO_PATH) {
        Ok(job) => print_shell_line(
            io,
            format!(
                "shader: demo wrote {} bytes={} queued job={} artifact=shader/demo.eu32",
                DEMO_PATH,
                DEMO_SOURCE.len(),
                job.id
            )
            .as_str(),
        ),
        Err(err) => {
            print_shell_line(io, format!("shader: demo queue failed err={:?}", err).as_str())
        }
    }
}

fn compile_now(io: &'static dyn ShellBackend2, path: Option<&str>) {
    let Some(path) = path else {
        usage(io);
        return;
    };
    match crate::r::shader::compile_path_sync(path) {
        Ok(report) => print_shell_line(
            io,
            format!(
                "shader: compiled source={} artifact={} words={} bytes={} expected=0x{:08X}",
                report.source_path,
                report.artifact_path,
                report.words,
                report.artifact_bytes,
                report.expected_store_value
            )
            .as_str(),
        ),
        Err(err) => print_shell_line(io, format!("shader: compile failed err={:?}", err).as_str()),
    }
}

fn write_file(path: &str, bytes: &[u8]) -> bool {
    let Ok(handle) = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64) else {
        return false;
    };
    if crate::r::io::kfs::write_file_chunk(handle, bytes).is_err() {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return false;
    }
    crate::r::io::kfs::write_file_finish(handle).is_ok()
}
