//! Bounded native command IO, before Shell3's terminal or UI presentation.
//!
//! The existing Shell3 service worker polls this object. Commands are UTF-8
//! lines, not keyboard events, shell syntax, or an ANSI terminal byte stream.
use alloc::{collections::VecDeque, format, string::String, vec::Vec};

const LINE_LIMIT: usize = 1024;
const INPUT_LIMIT: usize = 64 * 1024;
const COMMAND_LIMIT: usize = 256;
const OUTPUT_LIMIT: usize = 1024 * 1024;
const SNAPSHOT_WIDTH: usize = 80;
const OUTPUT_OVERFLOW: &[u8] = b"error: captured command output exceeded 1 MiB; session stopped.\n";

enum Command {
    Line(String),
    Invalid(&'static str),
}

pub(super) struct Captured {
    vm: u8,
    line: Vec<u8>,
    line_error: Option<&'static str>,
    after_cr: bool,
    pending: VecDeque<Command>,
    pending_bytes: usize,
    output: Vec<u8>,
    stdin_closed: bool,
    result: i32,
    exit: Option<i32>,
}

impl Captured {
    pub(super) fn new(vm: u8) -> Self {
        Self {
            vm,
            line: Vec::new(),
            line_error: None,
            after_cr: false,
            pending: VecDeque::new(),
            pending_bytes: 0,
            output: Vec::new(),
            stdin_closed: false,
            result: 0,
            exit: None,
        }
    }

    /// Input can split a UTF-8 scalar or CRLF between calls. Nothing is echoed.
    pub(super) fn input(&mut self, bytes: &[u8]) {
        if self.stdin_closed || self.exit.is_some() {
            return;
        }
        for &byte in bytes {
            if self.exit.is_some() {
                break;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            if matches!(byte, b'\r' | b'\n') {
                self.finish_line();
            } else if self.line_error.is_none() {
                if byte < 0x20 || byte == 0x7f {
                    self.line.clear();
                    self.line_error = Some(
                        "control input is not a native command; use explicit adm/cmd/hv names instead of terminal keys",
                    );
                } else if self.line.len() >= LINE_LIMIT {
                    self.line.clear();
                    self.line_error = Some("command exceeded 1024 bytes; line discarded");
                } else if self.pending_bytes + self.line.len() >= INPUT_LIMIT {
                    self.stop_input_overflow();
                } else {
                    self.line.push(byte);
                }
            }
        }
    }

    pub(super) fn close_stdin(&mut self) {
        if self.stdin_closed || self.exit.is_some() {
            return;
        }
        if !self.line.is_empty() || self.line_error.is_some() {
            self.finish_line();
        }
        self.stdin_closed = true;
    }

    pub(super) fn interrupt(&mut self) {
        if self.exit.is_some() {
            return;
        }
        self.clear_input();
        self.stdin_closed = true;
        self.write_line("error: captured command session interrupted");
        self.exit = Some(130);
    }

    /// At most one finite command executes per pass of the existing worker.
    pub(super) fn poll(&mut self) {
        if self.exit.is_some() {
            return;
        }
        if let Some(command) = self.pending.pop_front() {
            let code = match command {
                Command::Invalid(error) => {
                    self.write_line(&format!("error: {error}"));
                    2
                }
                Command::Line(line) => {
                    self.pending_bytes = self.pending_bytes.saturating_sub(line.len());
                    self.dispatch(&line)
                }
            };
            if code != 0 {
                self.result = code;
            }
        }
        if self.stdin_closed && self.pending.is_empty() && self.exit.is_none() {
            self.exit = Some(self.result);
        }
    }

    pub(super) fn drain_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.output)
    }

    pub(super) fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    fn clear_input(&mut self) {
        self.line.clear();
        self.line_error = None;
        self.pending.clear();
        self.pending_bytes = 0;
    }

    fn stop_input_overflow(&mut self) {
        self.clear_input();
        self.stdin_closed = true;
        self.write_line("error: captured command input queue limit exceeded; session stopped");
        self.exit = Some(2);
    }

    fn finish_line(&mut self) {
        let command = if let Some(error) = self.line_error.take() {
            self.line.clear();
            Some(Command::Invalid(error))
        } else {
            let line = core::mem::take(&mut self.line);
            match String::from_utf8(line) {
                Ok(line) if line.chars().any(char::is_control) => {
                    Some(Command::Invalid("control input is not a native command"))
                }
                Ok(line) if line.trim().is_empty() => None,
                Ok(line) => Some(Command::Line(line)),
                Err(_) => Some(Command::Invalid(
                    "command is not valid UTF-8; line discarded",
                )),
            }
        };
        let Some(command) = command else { return };
        if self.pending.len() >= COMMAND_LIMIT {
            self.stop_input_overflow();
            return;
        }
        if let Command::Line(line) = &command {
            if self.pending_bytes + line.len() > INPUT_LIMIT {
                self.stop_input_overflow();
                return;
            }
            self.pending_bytes += line.len();
        }
        self.pending.push_back(command);
    }

    fn write_line(&mut self, text: &str) {
        if self.exit.is_some() {
            return;
        }
        // Reserve room for the explicit failure, rather than silently clipping.
        let limit = OUTPUT_LIMIT - OUTPUT_OVERFLOW.len();
        if text.len().saturating_add(1) > limit.saturating_sub(self.output.len()) {
            self.output.extend_from_slice(OUTPUT_OVERFLOW);
            self.clear_input();
            self.stdin_closed = true;
            self.exit = Some(1);
            return;
        }
        self.output.extend_from_slice(text.as_bytes());
        self.output.push(b'\n');
    }

    fn write_lines(&mut self, lines: Vec<String>) {
        for line in lines {
            self.write_line(&line);
            if self.exit.is_some() {
                break;
            }
        }
    }

    fn help(&mut self) {
        self.write_line(
            "TRUEOS native captured commands (UTF-8 lines; no terminal or Unix shell syntax)",
        );
        self.write_line("help | env | status | ram | smp | net | gpu | exit");
        self.write_line("Explicit namespaces: adm help/env/ram/smp/net/gpu; hv help/status/peer/probe; cmd help");
        self.write_line(
            "Commands finish after their output is collected. stdin EOF finishes the session.",
        );
        self.write_line("Interactive helpers and cmd <app> require an explicit terminal session.");
        self.write_line("AppDB (installed; discovery only):");
        for name in super::service::read_appdb_names() {
            self.write_line(&format!("  {name}"));
        }
        self.write_line("Aka (configured; discovery only):");
        for name in crate::r::restart::startup_alias_names() {
            self.write_line(&format!("  {name}"));
        }
    }

    fn terminal_required(&mut self, name: &str) -> i32 {
        self.write_line(&format!("error: {name} requires an explicit terminal session; captured IO does not launch or navigate interactive applications"));
        2
    }

    fn dispatch(&mut self, line: &str) -> i32 {
        let words: Vec<_> = line.split_whitespace().collect();
        match words.as_slice() {
            ["help"] | ["adm", "help"] | ["hv", "help"] | ["cmd"] | ["cmd", "help"] => self.help(),
            ["env"] | ["adm", "env"] => {
                let Some(text) = crate::hv::blueprint_process_env_text(self.vm) else {
                    self.write_line("error: caller VM environment is unavailable");
                    return 1;
                };
                for line in text.lines() {
                    self.write_line(line);
                }
            }
            ["status"] | ["hv", "status"] => self.write_lines(super::service::hv_status_lines()),
            ["hv", "peer"] => self.write_lines(super::service::peer_lines()),
            ["hv", "probe"] => self.write_lines(super::service::probe_lines()),
            ["ram"] | ["adm", "ram"] => {
                self.write_lines(super::ram::memory_modules_snapshot(SNAPSHOT_WIDTH));
                self.write_lines(super::ram::usage_snapshot(SNAPSHOT_WIDTH));
            }
            ["smp"] | ["adm", "smp"] => self.write_lines(super::smp::snapshot(SNAPSHOT_WIDTH)),
            ["net"] | ["adm", "net"] => {
                self.write_lines(super::system_net::nic_snapshot(SNAPSHOT_WIDTH, None))
            }
            ["gpu"] | ["adm", "gpu"] => self.write_lines(super::gpu::snapshot(SNAPSHOT_WIDTH)),
            ["exit"] => {
                self.clear_input();
                self.stdin_closed = true;
                self.exit = Some(self.result);
            }
            ["cmd", name] => return self.terminal_required(name),
            ["adm", name]
                if matches!(
                    *name,
                    "cry"
                        | "disc"
                        | "tlb"
                        | "vcpy"
                        | "font"
                        | "log"
                        | "sh3"
                        | "pic"
                        | "vid"
                        | "aud"
                        | "vaud"
                        | "cam"
                ) =>
            {
                return self.terminal_required(name);
            }
            _ => {
                self.write_line("error: unknown native command; use help for exact names (no argv, pipes, redirection, or terminal keys)");
                return 2;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_utf8_and_crlf_make_one_command() {
        let mut captured = Captured::new(1);
        captured.input(b"cmd caf\xc3");
        captured.input(b"\xa9\r");
        captured.input(b"\n");
        assert_eq!(captured.pending.len(), 1);
        match captured.pending.front().unwrap() {
            Command::Line(line) => assert_eq!(line, "cmd caf\u{e9}"),
            Command::Invalid(_) => panic!("valid fragmented UTF-8 rejected"),
        }
        assert!(captured.drain_output().is_empty());
    }

    #[test]
    fn eof_submits_partial_line_once() {
        let mut captured = Captured::new(1);
        captured.input(b"help");
        captured.close_stdin();
        captured.close_stdin();
        assert_eq!(captured.pending.len(), 1);
        assert!(captured.stdin_closed);
    }

    #[test]
    fn terminal_keys_reject_entire_line() {
        let mut captured = Captured::new(1);
        captured.input(b"\t\tram\n");
        assert!(matches!(
            captured.pending.front(),
            Some(Command::Invalid(_))
        ));
        assert!(captured.drain_output().is_empty());
    }

    #[test]
    fn overlong_line_does_not_execute_truncated_prefix() {
        let mut captured = Captured::new(1);
        captured.input(&alloc::vec![b'a'; LINE_LIMIT + 1]);
        captured.input(b"\n");
        assert!(matches!(
            captured.pending.front(),
            Some(Command::Invalid(_))
        ));
        assert!(captured.line.is_empty());
    }

    #[test]
    fn invalid_utf8_is_explicit_and_does_not_stop_later_commands() {
        let mut captured = Captured::new(1);
        captured.input(b"\xff\nhelp\n");
        assert_eq!(captured.pending.len(), 2);
        assert!(matches!(
            captured.pending.front(),
            Some(Command::Invalid(_))
        ));
    }

    #[test]
    fn output_limit_retains_explicit_error() {
        let mut captured = Captured::new(1);
        captured.write_line(&"a".repeat(OUTPUT_LIMIT));
        assert_eq!(captured.exit_code(), Some(1));
        let output = captured.drain_output();
        assert!(output.ends_with(OUTPUT_OVERFLOW));
        assert!(output.len() <= OUTPUT_LIMIT);
    }

    #[test]
    fn too_many_commands_stop_with_error_instead_of_dropping_input() {
        let mut captured = Captured::new(1);
        for _ in 0..COMMAND_LIMIT + 1 {
            captured.input(b"help\n");
        }
        assert_eq!(captured.exit_code(), Some(2));
        assert!(captured.pending.is_empty());
        assert!(!captured.drain_output().is_empty());
    }
}
