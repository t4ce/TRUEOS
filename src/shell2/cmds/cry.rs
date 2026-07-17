use alloc::string::String;
use core::fmt::Write;

use super::super::{ShellBackend2, print_shell_line};
use crate::crypt::{self, CryError};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "cry status");
    print_shell_line(io, "cry key setup");
    print_shell_line(io, "cry login [root]");
    print_shell_line(io, "cry logout");
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next(), args.next()) {
        (None, None, None) => {
            print_status(io);
            usage(io);
        }
        (Some(command), None, None) if command.eq_ignore_ascii_case("status") => print_status(io),
        (Some(command), Some(action), None)
            if command.eq_ignore_ascii_case("key") && action.eq_ignore_ascii_case("setup") =>
        {
            setup_key(io)
        }
        (Some(command), None, None) if command.eq_ignore_ascii_case("login") => login_root(io),
        (Some(command), Some(account), None)
            if command.eq_ignore_ascii_case("login") && account.eq_ignore_ascii_case("root") =>
        {
            login_root(io)
        }
        (Some(command), None, None) if command.eq_ignore_ascii_case("logout") => {
            let ended = crypt::logout();
            print_shell_line(
                io,
                if ended {
                    "cry logout: session=ended scope=cry-only"
                } else {
                    "cry logout: session=none"
                },
            );
        }
        (Some(command), None, None) if command.eq_ignore_ascii_case("help") => usage(io),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn setup_key(io: &'static dyn ShellBackend2) {
    match crypt::setup_root_key() {
        Ok(report) => {
            print_shell_line(
                io,
                alloc::format!(
                    "cry key setup: enrolled account={} role={:?} algorithm=ed25519 isolation={:?} persistence={:?}",
                    account_name(report.account),
                    report.role,
                    report.isolation,
                    report.persistence,
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "cry key setup: provider={} key={} fingerprint={} machine={}",
                    short_hex(report.key.provider.as_bytes()),
                    short_hex(report.key.handle.as_bytes()),
                    full_hex(&report.fingerprint),
                    short_hex(report.machine.as_bytes()),
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "cry key setup: assurance=ceremony-only shell-gate=off reboot-persistence=off",
            );
        }
        Err(error) => print_error(io, "key setup", error),
    }
}

fn login_root(io: &'static dyn ShellBackend2) {
    match crypt::login_root() {
        Ok(report) => {
            print_shell_line(
                io,
                alloc::format!(
                    "cry login: proof=verified account={} role={:?} challenge={} fingerprint={}",
                    account_name(report.account),
                    report.role,
                    report.challenge_sequence,
                    full_hex(&report.fingerprint),
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "cry login: issued_tick={} expires_tick={} provider={} key={}",
                    report.issued_at_ticks,
                    report.expires_at_ticks,
                    short_hex(report.key.provider.as_bytes()),
                    short_hex(report.key.handle.as_bytes()),
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "cry login: scope=cry-session shell-authority=unchanged pre-mode=not-wired",
            );
        }
        Err(error) => print_error(io, "login", error),
    }
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crypt::status();
    print_shell_line(
        io,
        alloc::format!(
            "cry: configured={} isolation={:?} persistence={:?} shell-gate=off",
            status.configured as u8,
            status.isolation,
            status.persistence,
        )
        .as_str(),
    );
    if let (Some(key), Some(fingerprint), Some(account)) =
        (status.key, status.fingerprint, status.account)
    {
        print_shell_line(
            io,
            alloc::format!(
                "cry: account={} provider={} key={} fingerprint={}",
                account_name(account),
                short_hex(key.provider.as_bytes()),
                short_hex(key.handle.as_bytes()),
                full_hex(&fingerprint),
            )
            .as_str(),
        );
    }
    match status.session {
        Some(session) => print_shell_line(
            io,
            alloc::format!(
                "cry: session=verified account={} role={:?} challenge={} authenticated_tick={} scope=cry-only",
                account_name(session.account),
                session.role,
                session.challenge_sequence,
                session.authenticated_at_ticks,
            )
            .as_str(),
        ),
        None => print_shell_line(io, "cry: session=anonymous"),
    }
}

fn print_error(io: &'static dyn ShellBackend2, operation: &str, error: CryError) {
    let detail = match error {
        CryError::AlreadyConfigured => "key already configured for this boot",
        CryError::NotConfigured => "run `cry key setup` first",
        CryError::EntropyUnavailable => "strong entropy unavailable",
        CryError::InvalidGeneratedIdentity => "generated identity was invalid",
        CryError::InvalidKeySpec => "machine-login key profile was rejected",
        CryError::PurposeDenied => "credential does not permit machine login",
        CryError::Challenge(_) => "machine-login challenge was invalid",
        CryError::SignatureRejected => "machine-login proof was rejected",
    };
    print_shell_line(io, alloc::format!("cry {operation}: {detail}").as_str());
}

fn account_name(account: trueos_crypto::AccountId) -> &'static str {
    if account == trueos_crypto::AccountId::ROOT {
        "root"
    } else {
        "user"
    }
}

fn short_hex(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut out = String::with_capacity(take * 2);
    for byte in &bytes[..take] {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn full_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
