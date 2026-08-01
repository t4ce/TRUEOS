use alloc::string::String;
use core::fmt::Write;

use qrcodegen::{QrCode, QrCodeEcc, Version};

use super::super::{
    ShellBackend2, TRANSPORT_CONTAINER_SCOPE, TRANSPORT_NET_TCP_SCOPE,
    claim_matrix_target_for_app_slot_selected, konsole_viewport_size_for_target,
    matrix_target_for_backend, output_target_for_backend, print_native_line, print_shell_line,
    transport_scope_for_backend,
};
use crate::crypt::{self, CryError, CryTwoFactorState};
use crate::shell2::shell2_cmd::ParseOutcome;

const QR_QUIET_ZONE: i32 = 4;
const QR_MAX_VERSION: Version = Version::new(10);
const QR_BUFFER_BYTES: usize = QR_MAX_VERSION.buffer_len();
const CRY_SLOT: &str = "cry";

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "cry status");
    print_shell_line(io, "cry key setup");
    print_shell_line(io, "cry 2fa setup");
    print_shell_line(io, "cry login <6-digit authenticator code>");
    print_shell_line(io, "cry logout");
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let active_target = matrix_target_for_backend(io);
    let _ = claim_matrix_target_for_app_slot_selected(&active_target, CRY_SLOT, CRY_SLOT);
    let mut args = rest.split_whitespace();
    match (args.next(), args.next(), args.next(), args.next()) {
        (None, None, None, None) => {
            print_status(io);
            usage(io);
        }
        (Some(command), None, None, None) if command.eq_ignore_ascii_case("status") => {
            print_status(io)
        }
        (Some(command), Some(action), None, None)
            if command.eq_ignore_ascii_case("key") && action.eq_ignore_ascii_case("setup") =>
        {
            setup_key(io)
        }
        (Some(command), Some(action), None, None)
            if command.eq_ignore_ascii_case("2fa") && action.eq_ignore_ascii_case("setup") =>
        {
            present_totp_enrollment(io)
        }
        (Some(command), Some(code), None, None) if command.eq_ignore_ascii_case("login") => {
            login_root(io, code)
        }
        (Some(command), Some(account), Some(code), None)
            if command.eq_ignore_ascii_case("login") && account.eq_ignore_ascii_case("root") =>
        {
            login_root(io, code)
        }
        (Some(command), None, None, None) if command.eq_ignore_ascii_case("login") => {
            print_shell_line(io, "cry login: enter `cry login <6-digit authenticator code>`");
        }
        (Some(command), None, None, None) if command.eq_ignore_ascii_case("logout") => {
            let ended = crypt::logout(transport_scope_for_backend(io));
            print_shell_line(
                io,
                if ended {
                    "cry logout: session=ended input-recording=off"
                } else {
                    "cry logout: matching-session=none input-recording=off"
                },
            );
        }
        (Some(command), None, None, None) if command.eq_ignore_ascii_case("help") => usage(io),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

pub(crate) fn try_parse_slot_input(
    io: &'static dyn ShellBackend2,
    submitted: &str,
) -> Option<ParseOutcome> {
    let submitted = submitted.trim();
    if submitted.len() != 6 || !submitted.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let output_mask = output_target_for_backend(io);
    if crate::shell2::matrix::active_slot_app_label(output_mask).as_deref() != Some(CRY_SLOT) {
        return None;
    }

    login_root(io, submitted);
    Some(ParseOutcome::Handled)
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
                "cry key setup: input-recording=off-until-2fa-login reboot-persistence=off",
            );
            present_totp_enrollment(io);
        }
        Err(CryError::AlreadyConfigured) => {
            print_shell_line(io, "cry key setup: key already configured; continuing 2fa setup");
            present_totp_enrollment(io);
        }
        Err(error) => print_error(io, "key setup", error),
    }
}

fn present_totp_enrollment(io: &'static dyn ShellBackend2) {
    match crypt::begin_totp_enrollment() {
        Ok(enrollment) => {
            match render_qr(io, enrollment.qr_payload.as_str(), &enrollment.account_tag) {
                Ok(()) => {}
                Err(QrPresentationError::PayloadTooLong) => {
                    print_shell_line(io, "cry 2fa setup: QR payload did not fit")
                }
                Err(QrPresentationError::ViewportTooSmall {
                    required_cols,
                    required_rows,
                    available_cols,
                    available_rows,
                }) => print_shell_line(
                    io,
                    alloc::format!(
                        "cry 2fa setup: viewport too small; need {}x{}, have {}x{}",
                        required_cols,
                        required_rows,
                        available_cols,
                        available_rows,
                    )
                    .as_str(),
                ),
            }
        }
        Err(error) => print_error(io, "2fa setup", error),
    }
}

fn login_root(io: &'static dyn ShellBackend2, code: &str) {
    let scope_id = transport_scope_for_backend(io);
    match crypt::login_root(code, scope_id) {
        Ok(report) => {
            crate::shell2::matrix::clear_active_lines(output_target_for_backend(io));
            print_network_trust_warning(io);
            if report.enrollment_activated {
                print_shell_line(io, "cry 2fa: enrollment=confirmed profile=totp-sha1-6digit-30s");
            }
            print_shell_line(
                io,
                alloc::format!(
                    "cry login: proof=verified account={} role={:?} factors=machine-key+totp challenge={} totp-step={} fingerprint={}",
                    account_name(report.account),
                    report.role,
                    report.challenge_sequence,
                    report.totp_step,
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
                alloc::format!(
                    "cry login: input-recording=on scope={} storage=chacha20-poly1305",
                    scope_name(scope_id),
                )
                .as_str(),
            );
        }
        Err(error) => {
            print_network_trust_warning(io);
            print_error(io, "login", error);
        }
    }
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crypt::status();
    let scope_id = transport_scope_for_backend(io);
    let recording_on = status
        .session
        .is_some_and(|session| session.scope_id == scope_id);
    print_shell_line(
        io,
        alloc::format!(
            "cry: configured={} 2fa={:?} totp-clock={} isolation={:?} persistence={:?} input-recording={}",
            status.configured as u8,
            status.two_factor,
            if status.totp_clock.is_some() { "ntp" } else { "waiting-for-ntp" },
            status.isolation,
            status.persistence,
            if recording_on { "on" } else { "off" },
        )
        .as_str(),
    );
    print_totp_clock(io, status.totp_clock);
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
                "cry: session=verified account={} role={:?} factors=machine-key+totp challenge={} totp-step={} authenticated_tick={} scope={} encrypted-input-recording={}",
                account_name(session.account),
                session.role,
                session.challenge_sequence,
                session.totp_step,
                session.authenticated_at_ticks,
                scope_name(session.scope_id),
                if recording_on { "on" } else { "off-for-this-shell" },
            )
            .as_str(),
        ),
        None => print_shell_line(io, "cry: session=anonymous"),
    }

    if status.two_factor == CryTwoFactorState::Pending {
        print_shell_line(
            io,
            "cry: 2fa enrollment pending; scan `cry 2fa setup`, then run `cry login <code>`",
        );
    }
}

fn scope_name(scope_id: u8) -> &'static str {
    if (scope_id & TRANSPORT_NET_TCP_SCOPE) != 0 {
        "net-tcp"
    } else if (scope_id & TRANSPORT_CONTAINER_SCOPE) != 0 {
        "container"
    } else {
        "local"
    }
}

fn print_error(io: &'static dyn ShellBackend2, operation: &str, error: CryError) {
    let detail = match error {
        CryError::AlreadyConfigured => "key already configured for this boot",
        CryError::NotConfigured => "run `cry key setup` first",
        CryError::TwoFactorAlreadyActive => "2fa is already active",
        CryError::TwoFactorNotConfigured => "run `cry 2fa setup` first",
        CryError::WallClockUnavailable => {
            "TOTP clock is not synchronized; wait for network time and retry"
        }
        CryError::InvalidTotpCode => "invalid 6-digit authenticator code",
        CryError::TotpReplay => "authenticator code already consumed; wait for the next code",
        CryError::TotpRateLimited {
            retry_after_seconds,
        } => {
            print_shell_line(
                io,
                alloc::format!(
                    "cry {operation}: too many attempts; retry in {retry_after_seconds}s"
                )
                .as_str(),
            );
            return;
        }
        CryError::Totp(_) => "TOTP computation failed",
        CryError::EntropyUnavailable => "strong entropy unavailable",
        CryError::InvalidGeneratedIdentity => "generated identity was invalid",
        CryError::InvalidKeySpec => "machine-login key profile was rejected",
        CryError::PurposeDenied => "credential does not permit machine login",
        CryError::Challenge(_) => "machine-login challenge was invalid",
        CryError::SignatureRejected => "machine-login proof was rejected",
    };
    print_shell_line(io, alloc::format!("cry {operation}: {detail}").as_str());
    if matches!(error, CryError::InvalidTotpCode | CryError::WallClockUnavailable) {
        print_totp_clock(io, crypt::totp_clock_status());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QrPresentationError {
    PayloadTooLong,
    ViewportTooSmall {
        required_cols: usize,
        required_rows: usize,
        available_cols: usize,
        available_rows: usize,
    },
}

fn render_qr(
    io: &'static dyn ShellBackend2,
    payload: &str,
    account_tag: &[u8; 4],
) -> Result<(), QrPresentationError> {
    let mut temp = [0u8; QR_BUFFER_BYTES];
    let mut output = [0u8; QR_BUFFER_BYTES];
    let qr = QrCode::encode_text(
        payload,
        &mut temp,
        &mut output,
        QrCodeEcc::Medium,
        Version::MIN,
        QR_MAX_VERSION,
        None,
        true,
    )
    .map_err(|_| QrPresentationError::PayloadTooLong)?;

    let output_mask = output_target_for_backend(io);
    let symbol_with_quiet_zone = qr.size() + QR_QUIET_ZONE * 2;
    let required_cols = symbol_with_quiet_zone as usize;
    let network_warning_rows =
        usize::from(transport_scope_for_backend(io) == TRANSPORT_NET_TCP_SCOPE);
    let required_rows = (symbol_with_quiet_zone as usize).div_ceil(2) + 3 + network_warning_rows;
    let target = matrix_target_for_backend(io);
    let (available_cols, available_rows) = konsole_viewport_size_for_target(&target);
    if required_cols > available_cols || required_rows > available_rows {
        return Err(QrPresentationError::ViewportTooSmall {
            required_cols,
            required_rows,
            available_cols,
            available_rows,
        });
    }

    crate::shell2::matrix::clear_active_lines(output_mask);
    print_network_trust_warning(io);
    print_native_line(
        io,
        alloc::format!(
            "cry 2fa: scan with Google Authenticator; account=root-{}",
            full_hex(account_tag),
        )
        .as_str(),
    );
    print_native_line(
        io,
        "cry 2fa: QR contains the shared secret; enter the 6-digit code in this slot",
    );

    let first = -QR_QUIET_ZONE;
    let last = qr.size() + QR_QUIET_ZONE;
    let mut top = first;
    while top < last {
        let bottom = top + 1;
        let mut line = String::with_capacity(required_cols.saturating_mul(3) + 16);
        line.push_str("\x1b[30;47m");
        for x in first..last {
            let upper_dark = qr.get_module(x, top);
            let lower_dark = qr.get_module(x, bottom);
            line.push(match (upper_dark, lower_dark) {
                (false, false) => ' ',
                (true, false) => '▀',
                (false, true) => '▄',
                (true, true) => '█',
            });
        }
        line.push_str("\x1b[0m");
        print_native_line(io, line.as_str());
        top += 2;
    }
    print_native_line(io, "cry 2fa: waiting for the first authenticator code (digits only)");
    Ok(())
}

fn print_totp_clock(io: &'static dyn ShellBackend2, clock: Option<crypt::CryTotpClock>) {
    match clock {
        Some(clock) => {
            let seconds_in_day = clock.unix_seconds % 86_400;
            let utc_hour = seconds_in_day / 3_600;
            let utc_minute = (seconds_in_day % 3_600) / 60;
            let utc_second = seconds_in_day % 60;
            let boot_delta = clock
                .ntp_minus_boot_seconds
                .map(|delta| alloc::format!(" boot-delta={delta}s"))
                .unwrap_or_default();
            print_shell_line(
                io,
                alloc::format!(
                    "cry: totp-clock=ntp utc={utc_hour:02}:{utc_minute:02}:{utc_second:02} unix={} step={} next-code-in={}s{}",
                    clock.unix_seconds,
                    clock.step,
                    clock.seconds_remaining,
                    boot_delta,
                )
                .as_str(),
            );
        }
        None => print_shell_line(
            io,
            "cry: totp-clock=waiting-for-ntp; keep the network up and retry shortly",
        ),
    }
}

fn print_network_trust_warning(io: &'static dyn ShellBackend2) {
    if transport_scope_for_backend(io) == TRANSPORT_NET_TCP_SCOPE {
        print_native_line(
            io,
            "cry 2fa: warning: this F4 session crosses the network; use only on a trusted link",
        );
    }
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
