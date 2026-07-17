//! Inert machine-login ceremony used by the Shell2 `cry` command.
//!
//! This is deliberately not an access-control hook. It exercises enrollment,
//! challenge construction, signing, and verification while the future account
//! gate and persistent/hardware providers remain separate work.

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_crypto::{
    AccountId, AlgorithmId, BootId, IsolationClass, KeyDescriptor, KeyHandle, KeyProfile,
    KeyPurpose, KeyPurposeSet, KeyRef, KeySpec, MachineId, MachineLoginChallenge,
    MachineLoginChallengeError, MachineRole, PersistenceClass, ProviderId, SignIntent,
};
use zeroize::Zeroize;

const LOGIN_CHALLENGE_TTL_SECONDS: u64 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CryError {
    AlreadyConfigured,
    NotConfigured,
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
    pub issued_at_ticks: u64,
    pub expires_at_ticks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CrySessionSnapshot {
    pub account: AccountId,
    pub role: MachineRole,
    pub challenge_sequence: u64,
    pub authenticated_at_ticks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CryStatus {
    pub configured: bool,
    pub key: Option<KeyRef>,
    pub fingerprint: Option<[u8; 16]>,
    pub account: Option<AccountId>,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
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
    authenticated_at_ticks: u64,
}

struct CryState {
    signing_key: Option<SigningKey>,
    credential: Option<AccountCredential>,
    machine: Option<MachineId>,
    boot: Option<BootId>,
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

pub(crate) fn login_root() -> Result<CryLoginReport, CryError> {
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
        authenticated_at_ticks: now,
    });

    Ok(CryLoginReport {
        key: credential.descriptor.key,
        fingerprint: credential.fingerprint,
        account: credential.account,
        role: credential.maximum_role,
        challenge_sequence: challenge.sequence(),
        issued_at_ticks: challenge.issued_at_ticks(),
        expires_at_ticks: challenge.expires_at_ticks(),
    })
}

pub(crate) fn logout() -> bool {
    CRY_STATE.lock().session.take().is_some()
}

pub(crate) fn status() -> CryStatus {
    let state = CRY_STATE.lock();
    let session = state.session.map(|session| CrySessionSnapshot {
        account: session.account,
        role: session.role,
        challenge_sequence: session.challenge_sequence,
        authenticated_at_ticks: session.authenticated_at_ticks,
    });
    CryStatus {
        configured: state.credential.is_some(),
        key: state.credential.map(|credential| credential.descriptor.key),
        fingerprint: state.credential.map(|credential| credential.fingerprint),
        account: state.credential.map(|credential| credential.account),
        isolation: IsolationClass::Software,
        persistence: PersistenceClass::Volatile,
        session,
    }
}

fn public_key_fingerprint(public_key: &[u8; 32]) -> [u8; 16] {
    let digest = Sha256::digest(public_key);
    let mut fingerprint = [0u8; 16];
    fingerprint.copy_from_slice(&digest[..16]);
    fingerprint
}
