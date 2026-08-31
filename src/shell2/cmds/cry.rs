use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use qrcodegen::{QrCode, QrCodeEcc, Version};
use trueos_executor::Spawner;
use zeroize::Zeroizing;

use super::super::{
    MatrixTarget, ShellBackend2, TRANSPORT_CONTAINER_SCOPE, TRANSPORT_NET_TCP_SCOPE,
    claim_matrix_target_for_app_slot_selected, konsole_viewport_size_for_target,
    matrix_target_for_backend, output_target_for_backend, print_matrix_target_line,
    print_native_line, print_ordered_block, print_shell_line, set_matrix_target_active,
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
    print_shell_line(io, "cry key setup <username>");
    print_shell_line(io, "cry unlock <username> <64-hex recovery key>");
    print_shell_line(io, "cry 2fa setup");
    print_shell_line(io, "cry login [username] <6-digit authenticator code>");
    print_shell_line(io, "cry recovery show");
    print_shell_line(io, "cry logout");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let active_target = matrix_target_for_backend(io);
    let _ = claim_matrix_target_for_app_slot_selected(&active_target, CRY_SLOT, CRY_SLOT);
    let mut args = rest.split_whitespace();
    match (args.next(), args.next(), args.next(), args.next(), args.next()) {
        (None, None, None, None, None) => {
            print_status(io);
            usage(io);
        }
        (Some(command), None, None, None, None) if command.eq_ignore_ascii_case("status") => {
            print_status(io)
        }
        (Some(command), Some(action), Some(username), None, None)
            if command.eq_ignore_ascii_case("key") && action.eq_ignore_ascii_case("setup") =>
        {
            setup_key(io, username)
        }
        (Some(command), Some(action), None, None, None)
            if command.eq_ignore_ascii_case("key") && action.eq_ignore_ascii_case("setup") =>
        {
            print_shell_line(io, "cry key setup: username required (3-32 ASCII characters)")
        }
        (Some(command), Some(username), Some(recovery_key), None, None)
            if command.eq_ignore_ascii_case("unlock") =>
        {
            submit_unlock(spawner, io, username, recovery_key)
        }
        (Some(command), Some(action), None, None, None)
            if command.eq_ignore_ascii_case("2fa") && action.eq_ignore_ascii_case("setup") =>
        {
            present_totp_enrollment(io)
        }
        (Some(command), Some(code), None, None, None) if command.eq_ignore_ascii_case("login") => {
            submit_login(spawner, io, None, code)
        }
        (Some(command), Some(username), Some(code), None, None)
            if command.eq_ignore_ascii_case("login") =>
        {
            submit_login(spawner, io, Some(username), code)
        }
        (Some(command), None, None, None, None) if command.eq_ignore_ascii_case("login") => {
            print_shell_line(io, "cry login: enter `cry login <6-digit authenticator code>`");
        }
        (Some(command), Some(action), None, None, None)
            if command.eq_ignore_ascii_case("recovery") && action.eq_ignore_ascii_case("show") =>
        {
            show_recovery_key(io)
        }
        (Some(command), None, None, None, None) if command.eq_ignore_ascii_case("logout") => {
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
        (Some(command), None, None, None, None) if command.eq_ignore_ascii_case("help") => {
            usage(io)
        }
        _ => usage(io),
    }
    ParseOutcome::Handled
}

pub(crate) fn try_parse_slot_input(
    spawner: &Spawner,
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

    submit_login(spawner, io, None, submitted);
    Some(ParseOutcome::Handled)
}

fn setup_key(io: &'static dyn ShellBackend2, username: &str) {
    match crypt::setup_root_key(username) {
        Ok(report) => {
            print_shell_line(
                io,
                alloc::format!(
                    "cry key setup: enrolled username={} account-id={} role={:?} algorithm=ed25519 isolation={:?} persistence={:?}",
                    report.username,
                    report.account.raw(),
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
                "cry key setup: input-recording=off reboot-persistence=pending-first-verified-login",
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
            match render_qr(
                io,
                enrollment.qr_payload.as_str(),
                enrollment.username.as_str(),
                &enrollment.account_tag,
            ) {
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

fn submit_login(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    requested_username: Option<&str>,
    code: &str,
) {
    let scope_id = transport_scope_for_backend(io);
    if let Some(requested) = requested_username {
        let requested = match crypt::canonical_username(requested) {
            Ok(username) => username,
            Err(error) => {
                print_error(io, "login", error);
                return;
            }
        };
        if crypt::status().username.as_deref() != Some(requested.as_str()) {
            print_shell_line(io, "cry login: username does not match the loaded credential");
            return;
        }
    }

    let report = match crypt::prepare_login(code, scope_id) {
        Ok(report) => report,
        Err(error) => {
            print_network_trust_warning(io);
            print_error(io, "login", error);
            return;
        }
    };
    let plan = match crypt::prepare_persistence(report.challenge_sequence) {
        Ok(plan) => plan,
        Err(error) => {
            crypt::abort_pending_login(report.challenge_sequence);
            print_error(io, "login persistence", error);
            return;
        }
    };

    crate::shell2::matrix::clear_active_lines(output_target_for_backend(io));
    print_network_trust_warning(io);
    print_shell_line(io, "cry login: proof=verified persistence=committing input-recording=off");
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match persist_login_task(target.clone(), scope_id, plan) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            crypt::abort_pending_login(report.challenge_sequence);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "cry login: persistence task unavailable; session not issued");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn persist_login_task(target: MatrixTarget, scope_id: u8, plan: crypt::CryPersistencePlan) {
    let challenge_sequence = plan.challenge_sequence;
    let username = plan.username.clone();
    let secret_path = plan.secret_path.clone();
    let generation = plan.generation;
    let initial = plan.initial;
    let recovery_key = initial.then(|| Zeroizing::new(full_hex(plan.recovery_key_bytes())));

    if let Err(error) = write_persistence(&plan).await {
        crypt::abort_pending_login(challenge_sequence);
        crate::log_warn!(target: "storage";
            "cry-persistence: status=failed username={} generation={} reason={}\n",
            username,
            generation,
            error,
        );
        print_matrix_target_line(
            &target,
            alloc::format!(
                "cry login: persistence failed ({error}); session not issued; input-recording=off"
            )
            .as_str(),
        );
        set_matrix_target_active(&target, false);
        return;
    }

    match crypt::complete_persisted_login(plan) {
        Ok(report) => {
            crate::log_important!(target: "storage";
                "cry-persistence: status=sealed username={} path={} algorithm=aes-256-gcm generation={} recovery-key=external\n",
                username,
                secret_path,
                generation,
            );
            if report.enrollment_activated {
                print_matrix_target_line(
                    &target,
                    "cry 2fa: enrollment=confirmed profile=totp-sha1-6digit-30s",
                );
            }
            if let Some(recovery_key) = recovery_key.as_ref() {
                print_matrix_target_line(
                    &target,
                    alloc::format!("cry recovery-key: {}", recovery_key.as_str()).as_str(),
                );
                print_matrix_target_line(
                    &target,
                    "cry recovery-key: save outside TRUEOSFS; it is required by `cry unlock` after reboot",
                );
            }
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "cry persistence: username={} path={} algorithm=aes-256-gcm generation={} key-storage=external",
                    username,
                    secret_path,
                    generation,
                )
                .as_str(),
            );
            print_login_report(&target, &report, scope_id);
        }
        Err(_) => {
            print_matrix_target_line(
                &target,
                "cry login: persisted state changed before commit; session not issued",
            );
        }
    }
    set_matrix_target_active(&target, false);
}

async fn write_persistence(plan: &crypt::CryPersistencePlan) -> Result<(), String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    let secrets_dir = alloc::format!("{}/secrets", plan.account_dir);
    match crate::r::fs::trueosfs::dir_create_all_async(disk, secrets_dir.as_str()).await {
        Ok(true) => {}
        Ok(false) => return Err(String::from("directory allocation failed")),
        Err(error) => return Err(alloc::format!("directory error: {error:?}")),
    }

    write_and_verify(disk, plan.secret_path.as_str(), plan.envelope.as_slice()).await?;
    if plan.initial {
        write_and_verify(disk, plan.profile_path.as_str(), plan.profile.as_slice()).await?;
    }
    Ok(())
}

async fn write_and_verify(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    match crate::r::fs::trueosfs::file_write_all_async(disk, path, bytes).await {
        Ok(true) => {}
        Ok(false) => return Err(alloc::format!("write allocation failed: {path}")),
        Err(error) => return Err(alloc::format!("write error {path}: {error:?}")),
    }
    match crate::r::fs::trueosfs::file_out_async(disk, path).await {
        Ok(Some(readback)) if readback.as_slice() == bytes => Ok(()),
        Ok(Some(_)) => Err(alloc::format!("readback mismatch: {path}")),
        Ok(None) => Err(alloc::format!("readback missing: {path}")),
        Err(error) => Err(alloc::format!("readback error {path}: {error:?}")),
    }
}

fn print_login_report(target: &MatrixTarget, report: &crypt::CryLoginReport, scope_id: u8) {
    print_matrix_target_line(
        target,
        alloc::format!(
            "cry login: proof=verified username={} account-id={} role={:?} factors=machine-key+totp challenge={} totp-step={} fingerprint={}",
            report.username,
            report.account.raw(),
            report.role,
            report.challenge_sequence,
            report.totp_step,
            full_hex(&report.fingerprint),
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "cry login: issued_tick={} expires_tick={} provider={} key={}",
            report.issued_at_ticks,
            report.expires_at_ticks,
            short_hex(report.key.provider.as_bytes()),
            short_hex(report.key.handle.as_bytes()),
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "cry login: input-recording=on scope={} storage=chacha20-poly1305",
            scope_name(scope_id),
        )
        .as_str(),
    );
}

fn submit_unlock(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    username: &str,
    encoded_key: &str,
) {
    let username = match crypt::canonical_username(username) {
        Ok(username) => username,
        Err(error) => {
            print_error(io, "unlock", error);
            return;
        }
    };
    let recovery_key = match parse_recovery_key(encoded_key) {
        Some(key) => key,
        None => {
            print_shell_line(io, "cry unlock: recovery key must be exactly 64 hexadecimal digits");
            return;
        }
    };

    print_network_trust_warning(io);
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match unlock_task(target.clone(), username, recovery_key) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "cry unlock: task unavailable");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn unlock_task(target: MatrixTarget, username: String, recovery_key: Zeroizing<[u8; 32]>) {
    let secret_path = alloc::format!("users/{username}/secrets/cry.v1.aes256gcm");
    let result = async {
        let disk = crate::r::fs::trueosfs::primary_root_handle()
            .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
        let envelope = crate::r::fs::trueosfs::file_out_async(disk, secret_path.as_str())
            .await
            .map_err(|error| alloc::format!("read error: {error:?}"))?
            .ok_or_else(|| String::from("credential not found"))?;
        crypt::unlock_persisted(username.as_str(), &recovery_key, envelope.as_slice())
            .map_err(|_| String::from("credential envelope rejected"))
    }
    .await;

    match result {
        Ok(report) => {
            crate::log_important!(target: "storage";
                "cry-persistence: status=unsealed username={} path={} generation={} session=locked\n",
                report.username,
                secret_path,
                report.generation,
            );
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "cry unlock: credential=opened username={} account-id={} role={:?} provider={} key={} fingerprint={} generation={}",
                    report.username,
                    report.account.raw(),
                    report.role,
                    short_hex(report.key.provider.as_bytes()),
                    short_hex(report.key.handle.as_bytes()),
                    full_hex(&report.fingerprint),
                    report.generation,
                )
                .as_str(),
            );
            print_matrix_target_line(
                &target,
                "cry unlock: session=locked input-recording=off; enter a fresh authenticator code",
            );
        }
        Err(error) => {
            crate::log_warn!(target: "storage";
                "cry-persistence: status=unlock-failed username={} path={} reason={}\n",
                username,
                secret_path,
                error,
            );
            print_matrix_target_line(&target, alloc::format!("cry unlock: {error}").as_str());
        }
    }
    set_matrix_target_active(&target, false);
}

fn show_recovery_key(io: &'static dyn ShellBackend2) {
    let scope_id = transport_scope_for_backend(io);
    let Some(key) = crypt::authenticated_recovery_key(scope_id) else {
        print_error(io, "recovery show", CryError::NotAuthenticated);
        return;
    };
    print_network_trust_warning(io);
    let encoded = Zeroizing::new(full_hex(key.as_slice()));
    print_shell_line(io, alloc::format!("cry recovery-key: {}", encoded.as_str()).as_str());
    print_shell_line(io, "cry recovery-key: keep outside TRUEOSFS");
}

fn parse_recovery_key(input: &str) -> Option<Zeroizing<[u8; 32]>> {
    if input.len() != 64 || !input.is_ascii() {
        return None;
    }
    let mut key = Zeroizing::new([0u8; 32]);
    for (index, pair) in input.as_bytes().chunks_exact(2).enumerate() {
        key[index] = hex_nibble(pair[0])?.checked_mul(16)? | hex_nibble(pair[1])?;
    }
    if key.iter().all(|byte| *byte == 0) {
        None
    } else {
        Some(key)
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crypt::status();
    let username = status.username.as_deref().unwrap_or("-");
    let scope_id = transport_scope_for_backend(io);
    let recording_on = status
        .session
        .is_some_and(|session| session.scope_id == scope_id);
    print_shell_line(
        io,
        alloc::format!(
            "cry: configured={} username={} 2fa={:?} totp-clock={} isolation={:?} persistence={:?} input-recording={}",
            status.configured as u8,
            username,
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
                "cry: username={} account-id={} provider={} key={} fingerprint={}",
                username,
                account.raw(),
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
                "cry: session=verified username={} account-id={} role={:?} factors=machine-key+totp challenge={} totp-step={} authenticated_tick={} scope={} encrypted-input-recording={}",
                username,
                session.account.raw(),
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
    if status.configured
        && status.persistence == trueos_crypto::PersistenceClass::Volatile
        && status.two_factor == CryTwoFactorState::Active
    {
        print_shell_line(
            io,
            "cry: verified login will seal this credential before opening a session",
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
        CryError::InvalidUsername(_) => {
            "username must be 3-32 ASCII characters matching [a-z0-9][a-z0-9._-]*"
        }
        CryError::LoginPending => "another verified login is waiting for durable storage",
        CryError::NotAuthenticated => "a verified 2fa session is required",
        CryError::Persistence(_) => "credential persistence rejected the encrypted state",
        CryError::PersistenceStateChanged => "credential persistence state changed; retry",
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
    username: &str,
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

    let mut enrollment_block = Vec::with_capacity(
        (symbol_with_quiet_zone as usize).div_ceil(2) + 4 + network_warning_rows,
    );
    if transport_scope_for_backend(io) == TRANSPORT_NET_TCP_SCOPE {
        enrollment_block.push(String::from(
            "cry 2fa: warning: this F4 session crosses the network; use only on a trusted link",
        ));
    }
    enrollment_block.push(alloc::format!(
        "cry 2fa: scan with Google Authenticator; account={username}-{}",
        full_hex(account_tag),
    ));
    enrollment_block.push(String::from(
        "cry 2fa: QR contains the shared secret; enter the 6-digit code in this slot",
    ));

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
        enrollment_block.push(line);
        top += 2;
    }
    enrollment_block
        .push(String::from("cry 2fa: waiting for the first authenticator code (digits only)"));

    crate::shell2::matrix::clear_active_lines(output_mask);
    print_ordered_block(io, enrollment_block.as_slice());
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
