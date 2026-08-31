//! Machine-login ceremony used by the Shell2 `cry` command.
//!
//! A verified login is durably committed before it opens the encrypted-input
//! recording gate. Software credentials can survive reboot in a username-bound
//! AES-256-GCM envelope whose recovery key is kept outside TRUEOSFS; a future
//! hardware provider can replace that recovery boundary without changing the
//! machine-login proof.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_credential_store::{
    CredentialData as StoredCredentialData, StoreError, UsernameError, normalize_username,
    open as open_credential, seal as seal_credential,
};
use trueos_crypto::{
    AccountId, AlgorithmId, BootId, IsolationClass, KeyDescriptor, KeyHandle, KeyProfile,
    KeyPurpose, KeyPurposeSet, KeyRef, KeySpec, MachineId, MachineLoginChallenge,
    MachineLoginChallengeError, MachineRole, PersistenceClass, ProviderId, SignIntent,
    TOTP_BASE32_BYTES, TOTP_PERIOD_SECONDS, TOTP_SECRET_BYTES, TotpError,
    encode_totp_secret_base32, verify_totp_sha1,
};
use zeroize::{Zeroize, Zeroizing};

const LOGIN_CHALLENGE_TTL_SECONDS: u64 = 30;
const MIN_TOTP_UNIX_SECONDS: u64 = 1_577_836_800; // 2020-01-01T00:00:00Z
const TOTP_SKEW_STEPS: u8 = 1;
const TOTP_MAX_FAILURES_PER_STEP: u8 = 5;
const TOTP_ISSUER: &str = "TRUEOS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CryError {
    AlreadyConfigured,
    NotConfigured,
    TwoFactorAlreadyActive,
    TwoFactorNotConfigured,
    WallClockUnavailable,
    InvalidTotpCode,
    TotpReplay,
    TotpRateLimited { retry_after_seconds: u64 },
    Totp(TotpError),
    EntropyUnavailable,
    InvalidGeneratedIdentity,
    InvalidKeySpec,
    PurposeDenied,
    Challenge(MachineLoginChallengeError),
    SignatureRejected,
    InvalidUsername(UsernameError),
    LoginPending,
    NotAuthenticated,
    Persistence(StoreError),
    PersistenceStateChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CrySetupReport {
    pub username: String,
    pub key: KeyRef,
    pub fingerprint: [u8; 16],
    pub machine: MachineId,
    pub account: AccountId,
    pub role: MachineRole,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CryLoginReport {
    pub username: String,
    pub key: KeyRef,
    pub fingerprint: [u8; 16],
    pub account: AccountId,
    pub role: MachineRole,
    pub challenge_sequence: u64,
    pub totp_step: u64,
    pub enrollment_activated: bool,
    pub issued_at_ticks: u64,
    pub expires_at_ticks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CrySessionSnapshot {
    pub account: AccountId,
    pub role: MachineRole,
    pub challenge_sequence: u64,
    pub totp_step: u64,
    pub authenticated_at_ticks: u64,
    pub scope_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CryTwoFactorState {
    NotConfigured,
    Pending,
    Active,
}

pub(crate) struct CryTotpEnrollment {
    pub qr_payload: Zeroizing<String>,
    pub account_tag: [u8; 4],
    pub username: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CryTotpClock {
    pub unix_seconds: u64,
    pub step: u64,
    pub seconds_remaining: u64,
    pub ntp_minus_boot_seconds: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CryStatus {
    pub configured: bool,
    pub username: Option<String>,
    pub key: Option<KeyRef>,
    pub fingerprint: Option<[u8; 16]>,
    pub account: Option<AccountId>,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
    pub two_factor: CryTwoFactorState,
    pub totp_clock: Option<CryTotpClock>,
    pub session: Option<CrySessionSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CryUnlockReport {
    pub username: String,
    pub key: KeyRef,
    pub fingerprint: [u8; 16],
    pub account: AccountId,
    pub role: MachineRole,
    pub generation: u64,
}

pub(crate) struct CryPersistencePlan {
    pub username: String,
    pub account_dir: String,
    pub profile_path: String,
    pub secret_path: String,
    pub profile: Vec<u8>,
    pub envelope: Zeroizing<Vec<u8>>,
    pub generation: u64,
    pub challenge_sequence: u64,
    pub initial: bool,
    recovery_key: Zeroizing<[u8; 32]>,
}

impl CryPersistencePlan {
    pub(crate) fn recovery_key_bytes(&self) -> &[u8; 32] {
        &self.recovery_key
    }
}

#[derive(Clone, Copy)]
struct AccountCredential {
    descriptor: KeyDescriptor,
    public_key: [u8; 32],
    fingerprint: [u8; 16],
    account: AccountId,
    maximum_role: MachineRole,
}

#[derive(Clone, Copy)]
struct AuthenticatedSession {
    account: AccountId,
    role: MachineRole,
    challenge_sequence: u64,
    totp_step: u64,
    authenticated_at_ticks: u64,
    scope_id: u8,
}

struct PendingLogin {
    session: AuthenticatedSession,
    report: CryLoginReport,
}

struct DurableCredential {
    recovery_key: Zeroizing<[u8; 32]>,
    generation: u64,
}

pub(crate) struct CryUserInputRecordKey {
    pub account: AccountId,
    pub challenge_sequence: u64,
    pub authenticated_at_ticks: u64,
    pub scope_id: u8,
    key: Zeroizing<[u8; 32]>,
}

impl CryUserInputRecordKey {
    pub(crate) fn key_bytes(&self) -> &[u8; 32] {
        &self.key
    }
}

struct TotpFactor {
    secret: [u8; TOTP_SECRET_BYTES],
    active: bool,
    last_accepted_step: Option<u64>,
    failure_window_step: u64,
    failed_attempts: u8,
    blocked_until_ticks: u64,
}

impl TotpFactor {
    fn pending(secret: [u8; TOTP_SECRET_BYTES]) -> Self {
        Self {
            secret,
            active: false,
            last_accepted_step: None,
            failure_window_step: 0,
            failed_attempts: 0,
            blocked_until_ticks: 0,
        }
    }
}

impl Drop for TotpFactor {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

struct CryState {
    username: Option<String>,
    signing_key: Option<SigningKey>,
    credential: Option<AccountCredential>,
    machine: Option<MachineId>,
    boot: Option<BootId>,
    totp: Option<TotpFactor>,
    challenge_sequence: u64,
    session: Option<AuthenticatedSession>,
    pending_login: Option<PendingLogin>,
    durable: Option<DurableCredential>,
}

impl CryState {
    const fn new() -> Self {
        Self {
            username: None,
            signing_key: None,
            credential: None,
            machine: None,
            boot: None,
            totp: None,
            challenge_sequence: 0,
            session: None,
            pending_login: None,
            durable: None,
        }
    }
}

static CRY_STATE: Mutex<CryState> = Mutex::new(CryState::new());

pub(crate) fn canonical_username(input: &str) -> Result<String, CryError> {
    normalize_username(input).map_err(CryError::InvalidUsername)
}

pub(crate) fn setup_root_key(username: &str) -> Result<CrySetupReport, CryError> {
    let username = canonical_username(username)?;
    let mut state = CRY_STATE.lock();
    if state.credential.is_some() {
        return Err(CryError::AlreadyConfigured);
    }

    let mut secret_seed = [0u8; 32];
    let result = setup_root_key_inner(&mut state, username, &mut secret_seed);
    secret_seed.zeroize();
    result
}

fn setup_root_key_inner(
    state: &mut CryState,
    username: String,
    secret_seed: &mut [u8; 32],
) -> Result<CrySetupReport, CryError> {
    if !crate::tyche::fill_bytes(secret_seed) {
        return Err(CryError::EntropyUnavailable);
    }

    let mut provider_bytes = [0u8; 16];
    let mut handle_bytes = [0u8; 32];
    let mut machine_bytes = [0u8; 32];
    let mut boot_bytes = [0u8; 16];
    if !crate::tyche::fill_bytes(&mut provider_bytes)
        || !crate::tyche::fill_bytes(&mut handle_bytes)
        || !crate::tyche::fill_bytes(&mut machine_bytes)
        || !crate::tyche::fill_bytes(&mut boot_bytes)
    {
        return Err(CryError::EntropyUnavailable);
    }

    let provider = ProviderId::new(provider_bytes).ok_or(CryError::InvalidGeneratedIdentity)?;
    let handle = KeyHandle::new(handle_bytes).ok_or(CryError::InvalidGeneratedIdentity)?;
    let machine = MachineId::new(machine_bytes).ok_or(CryError::InvalidGeneratedIdentity)?;
    let boot = BootId::new(boot_bytes).ok_or(CryError::InvalidGeneratedIdentity)?;
    let key = KeyRef::new(provider, handle);
    let spec = KeySpec::new(KeyProfile::single(AlgorithmId::ED25519), KeyPurposeSet::MACHINE_LOGIN);
    spec.validate().map_err(|_| CryError::InvalidKeySpec)?;

    let signing_key = SigningKey::from_bytes(secret_seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let fingerprint = public_key_fingerprint(&public_key);
    let credential = AccountCredential {
        descriptor: KeyDescriptor {
            key,
            profile: spec.profile,
            purposes: spec.purposes,
        },
        public_key,
        fingerprint,
        account: AccountId::ROOT,
        maximum_role: MachineRole::Root,
    };

    state.username = Some(username.clone());
    state.signing_key = Some(signing_key);
    state.credential = Some(credential);
    state.machine = Some(machine);
    state.boot = Some(boot);
    state.totp = None;
    state.challenge_sequence = 0;
    state.session = None;
    state.pending_login = None;
    state.durable = None;

    Ok(CrySetupReport {
        username,
        key,
        fingerprint,
        machine,
        account: credential.account,
        role: credential.maximum_role,
        isolation: IsolationClass::Software,
        persistence: PersistenceClass::Volatile,
    })
}

pub(crate) fn begin_totp_enrollment() -> Result<CryTotpEnrollment, CryError> {
    let _ = totp_unix_seconds()?;
    let mut state = CRY_STATE.lock();
    if state.credential.is_none() {
        return Err(CryError::NotConfigured);
    }
    if state.totp.as_ref().is_some_and(|factor| factor.active) {
        return Err(CryError::TwoFactorAlreadyActive);
    }

    if state.totp.is_none() {
        let mut secret = [0u8; TOTP_SECRET_BYTES];
        if !crate::tyche::fill_bytes(&mut secret) {
            secret.zeroize();
            return Err(CryError::EntropyUnavailable);
        }
        state.totp = Some(TotpFactor::pending(secret));
        secret.zeroize();
    }

    let factor = state
        .totp
        .as_ref()
        .ok_or(CryError::TwoFactorNotConfigured)?;
    let fingerprint = state.credential.ok_or(CryError::NotConfigured)?.fingerprint;
    let username = state.username.clone().ok_or(CryError::NotConfigured)?;
    let account_tag: [u8; 4] = fingerprint[..4]
        .try_into()
        .map_err(|_| CryError::InvalidGeneratedIdentity)?;
    let mut base32_bytes = encode_totp_secret_base32(&factor.secret);
    let mut base32 = String::with_capacity(TOTP_BASE32_BYTES);
    for byte in base32_bytes {
        base32.push(byte as char);
    }
    let payload = Zeroizing::new(alloc::format!(
        "otpauth://totp/{TOTP_ISSUER}:{username}-{:02x}{:02x}{:02x}{:02x}?secret={base32}&issuer={TOTP_ISSUER}",
        account_tag[0],
        account_tag[1],
        account_tag[2],
        account_tag[3],
    ));
    base32_bytes.zeroize();
    base32.zeroize();
    Ok(CryTotpEnrollment {
        qr_payload: payload,
        account_tag,
        username,
    })
}

pub(crate) fn prepare_login(code: &str, scope_id: u8) -> Result<CryLoginReport, CryError> {
    let code = parse_totp_code(code).ok_or(CryError::InvalidTotpCode)?;
    let unix_seconds = totp_unix_seconds()?;
    let mut nonce = [0u8; 32];
    if !crate::tyche::fill_bytes(&mut nonce) {
        return Err(CryError::EntropyUnavailable);
    }

    let now = embassy_time_driver::now();
    let ttl = embassy_time_driver::TICK_HZ.saturating_mul(LOGIN_CHALLENGE_TTL_SECONDS);
    let expires = now.saturating_add(ttl.max(1));
    let mut state = CRY_STATE.lock();
    if state.pending_login.is_some() {
        return Err(CryError::LoginPending);
    }
    let credential = state.credential.ok_or(CryError::NotConfigured)?;
    let username = state.username.clone().ok_or(CryError::NotConfigured)?;
    let machine = state.machine.ok_or(CryError::NotConfigured)?;
    let boot = state.boot.ok_or(CryError::NotConfigured)?;
    if state.signing_key.is_none() {
        return Err(CryError::NotConfigured);
    }

    let current_step = unix_seconds / TOTP_PERIOD_SECONDS;
    let matched_step = {
        let factor = state
            .totp
            .as_ref()
            .ok_or(CryError::TwoFactorNotConfigured)?;
        if now < factor.blocked_until_ticks {
            return Err(CryError::TotpRateLimited {
                retry_after_seconds: ticks_to_seconds_ceil(
                    factor.blocked_until_ticks.saturating_sub(now),
                ),
            });
        }
        verify_totp_sha1(&factor.secret, unix_seconds, code, TOTP_SKEW_STEPS)
            .map_err(CryError::Totp)?
    };

    let Some(totp_step) = matched_step else {
        let factor = state
            .totp
            .as_mut()
            .ok_or(CryError::TwoFactorNotConfigured)?;
        if factor.failure_window_step != current_step {
            factor.failure_window_step = current_step;
            factor.failed_attempts = 0;
        }
        factor.failed_attempts = factor.failed_attempts.saturating_add(1);
        if factor.failed_attempts >= TOTP_MAX_FAILURES_PER_STEP {
            factor.failed_attempts = 0;
            let retry_after_seconds =
                (TOTP_PERIOD_SECONDS - (unix_seconds % TOTP_PERIOD_SECONDS)).max(1);
            factor.blocked_until_ticks = now
                .saturating_add(embassy_time_driver::TICK_HZ.saturating_mul(retry_after_seconds));
            return Err(CryError::TotpRateLimited {
                retry_after_seconds,
            });
        }
        return Err(CryError::InvalidTotpCode);
    };

    let enrollment_activated = {
        let factor = state
            .totp
            .as_mut()
            .ok_or(CryError::TwoFactorNotConfigured)?;
        if factor
            .last_accepted_step
            .is_some_and(|accepted| totp_step <= accepted)
        {
            return Err(CryError::TotpReplay);
        }
        let activated = !factor.active;
        factor.active = true;
        factor.last_accepted_step = Some(totp_step);
        factor.failure_window_step = current_step;
        factor.failed_attempts = 0;
        factor.blocked_until_ticks = 0;
        activated
    };

    state.challenge_sequence = state.challenge_sequence.saturating_add(1).max(1);
    let challenge_sequence = state.challenge_sequence;
    let challenge = MachineLoginChallenge::new(
        machine,
        boot,
        credential.account,
        credential.maximum_role,
        now,
        expires,
        challenge_sequence,
        nonce,
    )
    .map_err(CryError::Challenge)?;
    let intent = SignIntent::MachineLogin {
        challenge: &challenge,
    };
    if !credential.descriptor.permits(intent.required_purpose())
        || intent.required_purpose() != KeyPurpose::MachineLogin
    {
        return Err(CryError::PurposeDenied);
    }

    let signed = challenge.encode();
    let signature = state
        .signing_key
        .as_ref()
        .ok_or(CryError::NotConfigured)?
        .sign(&signed);
    let verifier = VerifyingKey::from_bytes(&credential.public_key)
        .map_err(|_| CryError::SignatureRejected)?;
    verifier
        .verify_strict(&signed, &signature)
        .map_err(|_| CryError::SignatureRejected)?;
    if !challenge.is_valid_at(embassy_time_driver::now())
        || challenge.machine() != machine
        || challenge.boot() != boot
        || challenge.account() != credential.account
        || challenge.requested_role() != credential.maximum_role
    {
        return Err(CryError::SignatureRejected);
    }

    let session = AuthenticatedSession {
        account: credential.account,
        role: credential.maximum_role,
        challenge_sequence: challenge.sequence(),
        totp_step,
        authenticated_at_ticks: now,
        scope_id,
    };
    let report = CryLoginReport {
        username,
        key: credential.descriptor.key,
        fingerprint: credential.fingerprint,
        account: credential.account,
        role: credential.maximum_role,
        challenge_sequence: challenge.sequence(),
        totp_step,
        enrollment_activated,
        issued_at_ticks: challenge.issued_at_ticks(),
        expires_at_ticks: challenge.expires_at_ticks(),
    };
    // A verified proof is not yet a session. The accepted TOTP step must be
    // durably sealed before command recording or other session gates open.
    state.session = None;
    state.pending_login = Some(PendingLogin {
        session,
        report: report.clone(),
    });

    Ok(report)
}

pub(crate) fn prepare_persistence(challenge_sequence: u64) -> Result<CryPersistencePlan, CryError> {
    let state = CRY_STATE.lock();
    let pending = state
        .pending_login
        .as_ref()
        .ok_or(CryError::PersistenceStateChanged)?;
    if pending.report.challenge_sequence != challenge_sequence {
        return Err(CryError::PersistenceStateChanged);
    }

    let username = state.username.clone().ok_or(CryError::NotConfigured)?;
    let signing_key = state.signing_key.as_ref().ok_or(CryError::NotConfigured)?;
    let credential = state.credential.ok_or(CryError::NotConfigured)?;
    let machine = state.machine.ok_or(CryError::NotConfigured)?;
    let factor = state
        .totp
        .as_ref()
        .filter(|factor| factor.active)
        .ok_or(CryError::TwoFactorNotConfigured)?;

    let initial = state.durable.is_none();
    let generation = match state.durable.as_ref() {
        Some(durable) => durable
            .generation
            .checked_add(1)
            .ok_or(CryError::PersistenceStateChanged)?,
        None => 1,
    };
    let mut recovery_key = Zeroizing::new([0u8; 32]);
    if let Some(durable) = state.durable.as_ref() {
        recovery_key.copy_from_slice(durable.recovery_key.as_slice());
    } else if !crate::tyche::fill_bytes(recovery_key.as_mut()) {
        return Err(CryError::EntropyUnavailable);
    }

    let mut nonce = [0u8; 12];
    if !crate::tyche::fill_bytes(&mut nonce) {
        return Err(CryError::EntropyUnavailable);
    }

    let stored = StoredCredentialData {
        account_id: credential.account.raw(),
        role: role_raw(credential.maximum_role),
        provider_id: *credential.descriptor.key.provider.as_bytes(),
        key_handle: *credential.descriptor.key.handle.as_bytes(),
        fingerprint: credential.fingerprint,
        machine_id: *machine.as_bytes(),
        public_key: credential.public_key,
        signing_seed: Zeroizing::new(signing_key.to_bytes()),
        totp_secret: Zeroizing::new(factor.secret),
        totp_active: factor.active,
        last_accepted_step: factor.last_accepted_step,
    };
    let envelope = seal_credential(username.as_str(), &recovery_key, generation, nonce, &stored)
        .map_err(CryError::Persistence)?;

    let account_dir = alloc::format!("users/{username}");
    let profile_path = alloc::format!("{account_dir}/account.v1");
    let secret_path = alloc::format!("{account_dir}/secrets/cry.v1.aes256gcm");
    let profile = encode_account_profile(username.as_str(), &credential, secret_path.as_str());

    Ok(CryPersistencePlan {
        username,
        account_dir,
        profile_path,
        secret_path,
        profile,
        envelope,
        generation,
        challenge_sequence,
        initial,
        recovery_key,
    })
}

pub(crate) fn complete_persisted_login(
    plan: CryPersistencePlan,
) -> Result<CryLoginReport, CryError> {
    let mut state = CRY_STATE.lock();
    let pending = state
        .pending_login
        .as_ref()
        .ok_or(CryError::PersistenceStateChanged)?;
    if pending.report.challenge_sequence != plan.challenge_sequence
        || pending.report.username != plan.username
    {
        return Err(CryError::PersistenceStateChanged);
    }

    match state.durable.as_ref() {
        None if plan.initial && plan.generation == 1 => {}
        Some(durable)
            if !plan.initial
                && durable.generation.checked_add(1) == Some(plan.generation)
                && durable.recovery_key.as_slice() == plan.recovery_key.as_slice() => {}
        _ => return Err(CryError::PersistenceStateChanged),
    }

    let mut recovery_key = Zeroizing::new([0u8; 32]);
    recovery_key.copy_from_slice(plan.recovery_key.as_slice());
    state.durable = Some(DurableCredential {
        recovery_key,
        generation: plan.generation,
    });
    let pending = state
        .pending_login
        .take()
        .ok_or(CryError::PersistenceStateChanged)?;
    state.session = Some(pending.session);
    Ok(pending.report)
}

pub(crate) fn abort_pending_login(challenge_sequence: u64) {
    let mut state = CRY_STATE.lock();
    if state
        .pending_login
        .as_ref()
        .is_some_and(|pending| pending.report.challenge_sequence == challenge_sequence)
    {
        state.pending_login = None;
    }
}

pub(crate) fn unlock_persisted(
    username: &str,
    recovery_key: &[u8; 32],
    envelope: &[u8],
) -> Result<CryUnlockReport, CryError> {
    let username = canonical_username(username)?;
    if CRY_STATE.lock().credential.is_some() {
        return Err(CryError::AlreadyConfigured);
    }

    let opened = open_credential(username.as_str(), recovery_key, envelope)
        .map_err(CryError::Persistence)?;
    if opened.credential.account_id != AccountId::ROOT.raw()
        || opened.credential.role != role_raw(MachineRole::Root)
        || !opened.credential.totp_active
        || opened.credential.last_accepted_step.is_none()
    {
        return Err(CryError::Persistence(StoreError::InvalidCredential));
    }

    let provider =
        ProviderId::new(opened.credential.provider_id).ok_or(CryError::InvalidGeneratedIdentity)?;
    let handle =
        KeyHandle::new(opened.credential.key_handle).ok_or(CryError::InvalidGeneratedIdentity)?;
    let machine =
        MachineId::new(opened.credential.machine_id).ok_or(CryError::InvalidGeneratedIdentity)?;
    let mut boot_bytes = [0u8; 16];
    if !crate::tyche::fill_bytes(&mut boot_bytes) {
        return Err(CryError::EntropyUnavailable);
    }
    let boot = BootId::new(boot_bytes).ok_or(CryError::InvalidGeneratedIdentity)?;

    let mut signing_seed = Zeroizing::new([0u8; 32]);
    signing_seed.copy_from_slice(opened.credential.signing_seed.as_slice());
    let signing_key = SigningKey::from_bytes(&signing_seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let fingerprint = public_key_fingerprint(&public_key);
    if public_key != opened.credential.public_key || fingerprint != opened.credential.fingerprint {
        return Err(CryError::Persistence(StoreError::InvalidCredential));
    }

    let spec = KeySpec::new(KeyProfile::single(AlgorithmId::ED25519), KeyPurposeSet::MACHINE_LOGIN);
    spec.validate().map_err(|_| CryError::InvalidKeySpec)?;
    let key = KeyRef::new(provider, handle);
    let credential = AccountCredential {
        descriptor: KeyDescriptor {
            key,
            profile: spec.profile,
            purposes: spec.purposes,
        },
        public_key,
        fingerprint,
        account: AccountId::ROOT,
        maximum_role: MachineRole::Root,
    };
    let mut totp_secret = [0u8; TOTP_SECRET_BYTES];
    totp_secret.copy_from_slice(opened.credential.totp_secret.as_slice());
    let mut durable_key = Zeroizing::new([0u8; 32]);
    durable_key.copy_from_slice(recovery_key);

    let mut state = CRY_STATE.lock();
    if state.credential.is_some() {
        totp_secret.zeroize();
        return Err(CryError::AlreadyConfigured);
    }
    state.username = Some(username.clone());
    state.signing_key = Some(signing_key);
    state.credential = Some(credential);
    state.machine = Some(machine);
    state.boot = Some(boot);
    state.totp = Some(TotpFactor {
        secret: totp_secret,
        active: true,
        last_accepted_step: opened.credential.last_accepted_step,
        failure_window_step: 0,
        failed_attempts: 0,
        blocked_until_ticks: 0,
    });
    state.challenge_sequence = 0;
    state.session = None;
    state.pending_login = None;
    state.durable = Some(DurableCredential {
        recovery_key: durable_key,
        generation: opened.generation,
    });

    Ok(CryUnlockReport {
        username,
        key,
        fingerprint,
        account: AccountId::ROOT,
        role: MachineRole::Root,
        generation: opened.generation,
    })
}

pub(crate) fn authenticated_recovery_key(scope_id: u8) -> Option<Zeroizing<[u8; 32]>> {
    let state = CRY_STATE.lock();
    if !state
        .session
        .is_some_and(|session| session.scope_id == scope_id)
        || !state.totp.as_ref().is_some_and(|factor| factor.active)
    {
        return None;
    }
    let durable = state.durable.as_ref()?;
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(durable.recovery_key.as_slice());
    Some(key)
}

pub(crate) fn logout(scope_id: u8) -> bool {
    let mut state = CRY_STATE.lock();
    let session_ended = state
        .session
        .is_some_and(|session| session.scope_id == scope_id);
    if session_ended {
        state.session.take();
    }
    let pending_ended = state
        .pending_login
        .as_ref()
        .is_some_and(|pending| pending.session.scope_id == scope_id);
    if pending_ended {
        state.pending_login.take();
    }
    session_ended || pending_ended
}

/// Point-in-time authorization decision for kernel services which require an
/// authenticated two-factor session. The session remains owned by `crypt`;
/// callers receive neither a cloned credential nor reusable auth material.
pub(crate) fn has_authenticated_two_factor_session(scope_id: u8) -> bool {
    let state = CRY_STATE.lock();
    state.totp.as_ref().is_some_and(|factor| factor.active)
        && state
            .session
            .is_some_and(|session| session.scope_id == scope_id)
}

pub(crate) fn authenticated_user_input_record_key(scope_id: u8) -> Option<CryUserInputRecordKey> {
    const DOMAIN: &[u8] = b"TRUEOS/user-input-record/chacha20-poly1305/v1";

    let state = CRY_STATE.lock();
    let session = state.session?;
    if session.scope_id != scope_id {
        return None;
    }
    let signing_key = state.signing_key.as_ref()?;
    let credential = state.credential?;

    let signing_seed = Zeroizing::new(signing_key.to_bytes());
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(signing_seed.as_slice());
    hasher.update(credential.account.raw().to_le_bytes());
    let mut digest = hasher.finalize();
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(digest.as_slice());
    digest.as_mut_slice().zeroize();

    Some(CryUserInputRecordKey {
        account: session.account,
        challenge_sequence: session.challenge_sequence,
        authenticated_at_ticks: session.authenticated_at_ticks,
        scope_id: session.scope_id,
        key,
    })
}

pub(crate) fn status() -> CryStatus {
    let state = CRY_STATE.lock();
    let session = state.session.map(|session| CrySessionSnapshot {
        account: session.account,
        role: session.role,
        challenge_sequence: session.challenge_sequence,
        totp_step: session.totp_step,
        authenticated_at_ticks: session.authenticated_at_ticks,
        scope_id: session.scope_id,
    });
    let totp_clock = totp_clock_status();
    CryStatus {
        configured: state.credential.is_some(),
        username: state.username.clone(),
        key: state.credential.map(|credential| credential.descriptor.key),
        fingerprint: state.credential.map(|credential| credential.fingerprint),
        account: state.credential.map(|credential| credential.account),
        isolation: IsolationClass::Software,
        persistence: if state.durable.is_some() {
            PersistenceClass::Sealed
        } else {
            PersistenceClass::Volatile
        },
        two_factor: match state.totp.as_ref() {
            Some(factor) if factor.active => CryTwoFactorState::Active,
            Some(_) => CryTwoFactorState::Pending,
            None => CryTwoFactorState::NotConfigured,
        },
        totp_clock,
        session,
    }
}

pub(crate) fn totp_clock_status() -> Option<CryTotpClock> {
    let unix_seconds = crate::r::net::ntp::current_unix_seconds()
        .filter(|seconds| *seconds >= MIN_TOTP_UNIX_SECONDS)?;
    let elapsed_in_step = unix_seconds % TOTP_PERIOD_SECONDS;
    let ntp_minus_boot_seconds = crate::time::unix_time_seconds().map(|boot_seconds| {
        let difference = i128::from(unix_seconds) - i128::from(boot_seconds);
        difference.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    });
    Some(CryTotpClock {
        unix_seconds,
        step: unix_seconds / TOTP_PERIOD_SECONDS,
        seconds_remaining: TOTP_PERIOD_SECONDS.saturating_sub(elapsed_in_step).max(1),
        ntp_minus_boot_seconds,
    })
}

fn parse_totp_code(code: &str) -> Option<u32> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    code.bytes()
        .try_fold(0u32, |value, byte| value.checked_mul(10)?.checked_add(u32::from(byte - b'0')))
}

fn totp_unix_seconds() -> Result<u64, CryError> {
    totp_clock_status()
        .map(|clock| clock.unix_seconds)
        .ok_or(CryError::WallClockUnavailable)
}

fn ticks_to_seconds_ceil(ticks: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    ticks.saturating_add(hz - 1) / hz
}

fn public_key_fingerprint(public_key: &[u8; 32]) -> [u8; 16] {
    let digest = Sha256::digest(public_key);
    let mut fingerprint = [0u8; 16];
    fingerprint.copy_from_slice(&digest[..16]);
    fingerprint
}

fn role_raw(role: MachineRole) -> u8 {
    match role {
        MachineRole::User => 0,
        MachineRole::Administrator => 1,
        MachineRole::Root => 2,
    }
}

fn role_name(role: MachineRole) -> &'static str {
    match role {
        MachineRole::User => "user",
        MachineRole::Administrator => "administrator",
        MachineRole::Root => "root",
    }
}

fn encode_account_profile(
    username: &str,
    credential: &AccountCredential,
    secret_path: &str,
) -> Vec<u8> {
    let mut profile = String::new();
    let _ = writeln!(profile, "format=TRUEOS-cry-account-v1");
    let _ = writeln!(profile, "username={username}");
    let _ = writeln!(profile, "account_id={}", credential.account.raw());
    let _ = writeln!(profile, "role={}", role_name(credential.maximum_role));
    let _ = writeln!(profile, "signing_algorithm=ed25519");
    let _ = writeln!(profile, "credential_cipher=aes-256-gcm");
    let _ = writeln!(profile, "wrapping_key=external-recovery-key");
    let _ = write!(profile, "provider=");
    append_hex(&mut profile, credential.descriptor.key.provider.as_bytes());
    profile.push('\n');
    let _ = write!(profile, "key_handle=");
    append_hex(&mut profile, credential.descriptor.key.handle.as_bytes());
    profile.push('\n');
    let _ = write!(profile, "fingerprint=");
    append_hex(&mut profile, &credential.fingerprint);
    profile.push('\n');
    let _ = writeln!(profile, "sealed_credential={secret_path}");
    profile.into_bytes()
}

fn append_hex(output: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
}
