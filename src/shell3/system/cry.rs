//! Account and key command, backed by the existing crypt services.
use crate::crypt::{self, CryError};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Write;
use qrcodegen::{QrCode, QrCodeEcc, Version};
use spin::Mutex;
use zeroize::Zeroizing;
const QR_QUIET_ZONE: i32 = 4;
const QR_MAX_VERSION: Version = Version::new(10);
const QR_BUFFER_BYTES: usize = QR_MAX_VERSION.buffer_len();
pub(super) fn start(frontend: super::tui::Frontend) -> Result<(), String> {
    super::admin::start("cry", frontend)
}
pub(crate) async fn write_persistence(plan: &crypt::CryPersistencePlan) -> Result<(), String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    crate::machine_key::mark_account_history(disk).await?;
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
    crate::machine_key::seal(disk, &plan.username, plan.recovery_key_bytes()).await?;
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

pub(crate) fn parse_recovery_key(input: &str) -> Option<Zeroizing<[u8; 32]>> {
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

pub(crate) fn error_text(error: CryError) -> String {
    match error {
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
            return alloc::format!("Too many attempts; retry in {retry_after_seconds}s");
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
        CryError::InvalidSshKey => "invalid OpenSSH Ed25519 public key",
        CryError::SshKeyAlreadyEnrolled => "SSH key is already enrolled",
        CryError::SshKeyNotFound => "SSH key is not enrolled",
        CryError::SshKeyLimit => "SSH authorized-key limit reached",
    }
    .into()
}

pub(crate) fn full_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(crate) struct AccountEnrollment {
    disk: crate::disc::block::DeviceHandle,
    draft: crypt::CryAccountDraft,
    pub(crate) username: String,
    payload: Zeroizing<String>,
    plan: Mutex<Option<crypt::CryPersistencePlan>>,
    writing: core::sync::atomic::AtomicBool,
}

impl AccountEnrollment {
    pub(crate) fn qr_lines(&self) -> Result<Zeroizing<Vec<String>>, String> {
        enrollment_qr_lines(&self.payload)
    }
    pub(crate) fn code_verified(&self) -> bool {
        self.plan.lock().is_some()
    }
    pub(crate) async fn save(&self, code: &str) -> Result<Zeroizing<String>, String> {
        let pending = self.plan.lock().take();
        let plan = if let Some(plan) = pending {
            plan
        } else {
            let proof = self.draft.prepare_login(code).map_err(error_text)?;
            match self.draft.prepare_persistence(proof.challenge_sequence) {
                Ok(plan) => plan,
                Err(error) => {
                    self.draft.abort_pending(proof.challenge_sequence);
                    return Err(error_text(error));
                }
            }
        };
        let recovery = Zeroizing::new(full_hex(plan.recovery_key_bytes()));
        let result = async {
            let disk = self.disk;
            if crate::r::fs::trueosfs::primary_root_handle() != Some(disk) {
                return Err(String::from("The TRUEOSFS root changed. Restore it before retrying."));
            }
            if !self.writing.load(core::sync::atomic::Ordering::Acquire) {
                ensure_account_name_available(&self.username).await?;
                self.writing
                    .store(true, core::sync::atomic::Ordering::Release);
            }
            // Keep the temporary plaintext provider separate from the automatic
            // boot blob, and save the recovery boundary before the envelope.
            crate::machine_key::seal_saved_account(disk, &plan.username, plan.recovery_key_bytes())
                .await?;
            let secrets = alloc::format!("{}/secrets", plan.account_dir);
            if !crate::r::fs::trueosfs::dir_create_all_async(disk, &secrets)
                .await
                .map_err(|error| alloc::format!("Directory error: {error:?}"))?
            {
                return Err(String::from("Directory allocation failed."));
            }
            write_and_verify(disk, &plan.secret_path, &plan.envelope).await?;
            write_and_verify(disk, &plan.profile_path, &plan.profile).await?;
            Ok::<_, String>(())
        }
        .await;
        if let Err(error) = result {
            *self.plan.lock() = Some(plan);
            return Err(error);
        }
        self.draft.complete_persistence(plan).map_err(error_text)?;
        Ok(recovery)
    }
}

pub(crate) async fn ensure_account_name_available(username: &str) -> Result<(), String> {
    let username = crypt::canonical_username(username).map_err(error_text)?;
    if crypt::status().username.as_deref() == Some(username.as_str()) {
        return Err(alloc::format!("Account {username} already exists. Choose Sign in."));
    }
    let disk =
        crate::r::fs::trueosfs::primary_root_handle().ok_or("No TRUEOSFS root is mounted.")?;
    for path in [
        alloc::format!("users/{username}/account.v1"),
        alloc::format!("users/{username}/secrets/cry.v1.aes256gcm"),
        alloc::format!("users/{username}/secrets/uncrypted.blob"),
    ] {
        if crate::r::fs::trueosfs::file_out_async(disk, &path)
            .await
            .map_err(|error| alloc::format!("Account lookup failed: {error:?}"))?
            .is_some()
        {
            return Err(alloc::format!(
                "Account {username} is already saved. Choose another name."
            ));
        }
    }
    Ok(())
}

pub(crate) async fn create_account_enrollment(
    username: &str,
) -> Result<Arc<AccountEnrollment>, String> {
    ensure_account_name_available(username).await?;
    let disk =
        crate::r::fs::trueosfs::primary_root_handle().ok_or("No TRUEOSFS root is mounted.")?;
    let draft = crypt::CryAccountDraft::new(username).map_err(error_text)?;
    let enrollment = draft.begin_enrollment().map_err(error_text)?;
    Ok(Arc::new(AccountEnrollment {
        disk,
        draft,
        username: enrollment.username,
        payload: enrollment.qr_payload,
        plan: Mutex::new(None),
        writing: core::sync::atomic::AtomicBool::new(false),
    }))
}

struct PendingProof(u64);

impl Drop for PendingProof {
    fn drop(&mut self) {
        crypt::abort_pending_login(self.0);
    }
}

pub(crate) async fn login_for_scope(scope: u8, code: &str) -> Result<String, String> {
    let proof = crypt::prepare_login(code, scope).map_err(error_text)?;
    let _pending = PendingProof(proof.challenge_sequence);
    let plan = crypt::prepare_persistence(proof.challenge_sequence).map_err(error_text)?;
    write_persistence(&plan).await?;
    let report = crypt::complete_persisted_login(plan).map_err(error_text)?;
    Ok(if report.enrollment_activated {
        "2FA confirmed; account saved and signed in."
    } else {
        "Signed in; account saved."
    }
    .into())
}

pub(crate) async fn change_ssh_key(
    code: &str,
    key: [u8; 32],
    remove: bool,
) -> Result<String, String> {
    let proof = crypt::prepare_ssh_key_change(code, key, remove).map_err(error_text)?;
    let _pending = PendingProof(proof.challenge_sequence);
    let plan = crypt::prepare_persistence(proof.challenge_sequence).map_err(error_text)?;
    write_persistence(&plan).await?;
    crypt::complete_persisted_remote_login(plan).map_err(error_text)?;
    Ok(alloc::format!("SSH key {}.", if remove { "removed" } else { "added" }))
}

pub(crate) async fn unlock_account(username: &str, encoded: &str) -> Result<String, String> {
    let username = crypt::canonical_username(username).map_err(error_text)?;
    let key = parse_recovery_key(encoded).ok_or("Recovery key must be 64 hexadecimal digits.")?;
    let disk =
        crate::r::fs::trueosfs::primary_root_handle().ok_or("No TRUEOSFS root is mounted.")?;
    let path = alloc::format!("users/{username}/secrets/cry.v1.aes256gcm");
    let envelope = crate::r::fs::trueosfs::file_out_async(disk, &path)
        .await
        .map_err(|_| String::from("Credential read failed."))?
        .ok_or("Credential not found.")?;
    crypt::unlock_persisted(&username, &key, &envelope).map_err(error_text)?;
    if crate::machine_key::seal(disk, &username, &key)
        .await
        .is_err()
    {
        return Ok("Unlocked; boot-key save failed. Enter an authenticator code.".into());
    }
    Ok("Account unlocked. Enter an authenticator code to sign in.".into())
}

pub(crate) fn enrollment_qr_lines(payload: &str) -> Result<Zeroizing<Vec<String>>, String> {
    let mut temp = Zeroizing::new([0u8; QR_BUFFER_BYTES]);
    let mut output = Zeroizing::new([0u8; QR_BUFFER_BYTES]);
    let qr = QrCode::encode_text(
        payload,
        &mut *temp,
        &mut *output,
        QrCodeEcc::Medium,
        Version::MIN,
        QR_MAX_VERSION,
        None,
        true,
    )
    .map_err(|_| String::from("QR payload did not fit."))?;
    let first = -QR_QUIET_ZONE;
    let last = qr.size() + QR_QUIET_ZONE;
    let mut lines = Zeroizing::new(Vec::new());
    let mut top = first;
    while top < last {
        let mut line = String::new();
        for x in first..last {
            line.push(match (qr.get_module(x, top), qr.get_module(x, top + 1)) {
                (false, false) => '█',
                (true, false) => '▄',
                (false, true) => '▀',
                (true, true) => ' ',
            });
        }
        lines.push(line);
        top += 2;
    }
    Ok(lines)
}
