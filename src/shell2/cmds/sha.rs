use alloc::string::String;
use core::fmt::Write;

use sha2::{Digest, Sha256};

use super::super::{ShellBackend2, print_shell_line};
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

struct ShaTiming {
    digest: [u8; 32],
    checksum_ms: u64,
}

fn run_sha(path: &str) -> crate::io::kfs::Result<ShaTiming> {
    let bytes = crate::io::kfs::read_file(path)?;
    let checksum_start_tick = embassy_time_driver::now();
    let digest = Sha256::digest(bytes.as_slice());
    let checksum_ms = elapsed_ms_since(checksum_start_tick);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(ShaTiming {
        digest: out,
        checksum_ms,
    })
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn elapsed_ms_since(start_tick: u64) -> u64 {
    let ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ.max(1);
    ticks.saturating_mul(1000) / hz
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let path = match parse_one_path(rest) {
        Ok(Some(path)) => path,
        Ok(None) => {
            print_shell_line(io, "sha: usage `sha <file>`");
            return ParseOutcome::Handled;
        }
        Err(err) => {
            print_shell_line(io, alloc::format!("sha: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    let start_tick = embassy_time_driver::now();
    match run_sha(path.as_str()) {
        Ok(timing) => print_shell_line(
            io,
            alloc::format!(
                "{}  {}  {}ms  sha={}ms",
                digest_hex(&timing.digest),
                path,
                elapsed_ms_since(start_tick),
                timing.checksum_ms
            )
            .as_str(),
        ),
        Err(crate::io::kfs::FsError::NotFound) => {
            print_shell_line(io, alloc::format!("sha: {path}: not found").as_str())
        }
        Err(err) => print_shell_line(io, alloc::format!("sha: {path}: {:?}", err).as_str()),
    }

    ParseOutcome::Handled
}
