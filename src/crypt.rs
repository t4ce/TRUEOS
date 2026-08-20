//! Machine-login ceremony used by the Shell2 `cry` command.
//!
//! The verified session gates encrypted user-input persistence. Broader shell
//! authority and persistent/hardware key providers remain separate work.

use alloc::string::String;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use spin::Mutex;
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CrySetupReport {
    pub key: KeyRef,
    pub fingerprint: [u8; 16],
    pub machine: MachineId,
    pub account: AccountId,
    pub role: MachineRole,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CryLoginReport {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CryTotpClock {
    pub unix_seconds: u64,
    pub step: u64,
    pub seconds_remaining: u64,
    pub ntp_minus_boot_seconds: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CryStatus {
    pub configured: bool,
    pub key: Option<KeyRef>,
    pub fingerprint: Option<[u8; 16]>,
    pub account: Option<AccountId>,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
    pub two_factor: CryTwoFactorState,
    pub totp_clock: Option<CryTotpClock>,
    pub session: Option<CrySessionSnapshot>,
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
    signing_key: Option<SigningKey>,
    credential: Option<AccountCredential>,
    machine: Option<MachineId>,
    boot: Option<BootId>,
    totp: Option<TotpFactor>,
    challenge_sequence: u64,
    session: Option<AuthenticatedSession>,
}

impl CryState {
    const fn new() -> Self {
        Self {
            signing_key: None,
            credential: None,
            machine: None,
            boot: None,
            totp: None,
            challenge_sequence: 0,
            session: None,
        }
    }
}

static CRY_STATE: Mutex<CryState> = Mutex::new(CryState::new());

pub(crate) fn setup_root_key() -> Result<CrySetupReport, CryError> {
    let mut state = CRY_STATE.lock();
    if state.credential.is_some() {
        return Err(CryError::AlreadyConfigured);
    }

    let mut secret_seed = [0u8; 32];
    let result = setup_root_key_inner(&mut state, &mut secret_seed);
    secret_seed.zeroize();
    result
}

fn setup_root_key_inner(
    state: &mut CryState,
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

    state.signing_key = Some(signing_key);
    state.credential = Some(credential);
    state.machine = Some(machine);
    state.boot = Some(boot);
    state.totp = None;
    state.challenge_sequence = 0;
    state.session = None;

    Ok(CrySetupReport {
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
    let account_tag: [u8; 4] = fingerprint[..4]
        .try_into()
        .map_err(|_| CryError::InvalidGeneratedIdentity)?;
    let mut base32_bytes = encode_totp_secret_base32(&factor.secret);
    let mut base32 = String::with_capacity(TOTP_BASE32_BYTES);
    for byte in base32_bytes {
        base32.push(byte as char);
    }
    let payload = Zeroizing::new(alloc::format!(
        "otpauth://totp/{TOTP_ISSUER}:root-{:02x}{:02x}{:02x}{:02x}?secret={base32}&issuer={TOTP_ISSUER}",
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
    })
}

pub(crate) fn login_root(code: &str, scope_id: u8) -> Result<CryLoginReport, CryError> {
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
    let credential = state.credential.ok_or(CryError::NotConfigured)?;
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

    state.session = Some(AuthenticatedSession {
        account: credential.account,
        role: credential.maximum_role,
        challenge_sequence: challenge.sequence(),
        totp_step,
        authenticated_at_ticks: now,
        scope_id,
    });

    Ok(CryLoginReport {
        key: credential.descriptor.key,
        fingerprint: credential.fingerprint,
        account: credential.account,
        role: credential.maximum_role,
        challenge_sequence: challenge.sequence(),
        totp_step,
        enrollment_activated,
        issued_at_ticks: challenge.issued_at_ticks(),
        expires_at_ticks: challenge.expires_at_ticks(),
    })
}

pub(crate) fn logout(scope_id: u8) -> bool {
    let mut state = CRY_STATE.lock();
    if state
        .session
        .is_some_and(|session| session.scope_id == scope_id)
    {
        state.session.take();
        true
    } else {
        false
    }
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
        key: state.credential.map(|credential| credential.descriptor.key),
        fingerprint: state.credential.map(|credential| credential.fingerprint),
        account: state.credential.map(|credential| credential.account),
        isolation: IsolationClass::Software,
        persistence: PersistenceClass::Volatile,
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
