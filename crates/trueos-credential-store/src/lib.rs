#![no_std]
#![forbid(unsafe_code)]

//! Versioned, username-bound storage for the software machine credential.
//!
//! The envelope deliberately does not decide where its recovery key lives.
//! Callers must keep that key outside the filesystem containing the envelope.

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::str;

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, Tag, UnboundKey};
use zeroize::Zeroizing;

pub const USERNAME_MIN_BYTES: usize = 3;
pub const USERNAME_MAX_BYTES: usize = 32;
pub const RECOVERY_KEY_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 12;
pub const SIGNING_SEED_BYTES: usize = 32;
pub const MACHINE_ID_BYTES: usize = 32;
pub const TOTP_SECRET_BYTES: usize = 20;
pub const PUBLIC_KEY_BYTES: usize = 32;
pub const PROVIDER_ID_BYTES: usize = 16;
pub const KEY_HANDLE_BYTES: usize = 32;
pub const FINGERPRINT_BYTES: usize = 16;

const MAGIC: &[u8; 4] = b"TCRY";
const FORMAT_VERSION: u8 = 1;
const ALGORITHM_AES_256_GCM: u8 = 1;
const HEADER_BYTES: usize = 144;
const PLAINTEXT_BYTES: usize = 128;
const TAG_BYTES: usize = 16;
pub const ENVELOPE_BYTES: usize = HEADER_BYTES + PLAINTEXT_BYTES + TAG_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsernameError {
    InvalidLength,
    NonAscii,
    InvalidStart,
    InvalidCharacter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreError {
    InvalidUsername(UsernameError),
    InvalidGeneration,
    InvalidRecoveryKey,
    InvalidCredential,
    InvalidEnvelope,
    UnsupportedVersion,
    UnsupportedAlgorithm,
    AuthenticationFailed,
    CryptoUnavailable,
}

impl From<UsernameError> for StoreError {
    fn from(error: UsernameError) -> Self {
        Self::InvalidUsername(error)
    }
}

/// Canonicalize a path-safe account name.
///
/// Names are ASCII-only, lowercase on disk, 3..=32 bytes, start with an
/// alphanumeric byte, and otherwise contain only `[a-z0-9._-]`.
pub fn normalize_username(input: &str) -> Result<String, UsernameError> {
    if !(USERNAME_MIN_BYTES..=USERNAME_MAX_BYTES).contains(&input.len()) {
        return Err(UsernameError::InvalidLength);
    }
    if !input.is_ascii() {
        return Err(UsernameError::NonAscii);
    }

    let mut normalized = String::with_capacity(input.len());
    for (index, byte) in input.bytes().enumerate() {
        let byte = byte.to_ascii_lowercase();
        if index == 0 && !byte.is_ascii_alphanumeric() {
            return Err(UsernameError::InvalidStart);
        }
        if !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')) {
            return Err(UsernameError::InvalidCharacter);
        }
        normalized.push(char::from(byte));
    }
    Ok(normalized)
}

fn validate_canonical_username(username: &str) -> Result<(), UsernameError> {
    let normalized = normalize_username(username)?;
    if normalized == username {
        Ok(())
    } else {
        Err(UsernameError::InvalidCharacter)
    }
}

pub struct CredentialData {
    pub account_id: u64,
    pub role: u8,
    pub provider_id: [u8; PROVIDER_ID_BYTES],
    pub key_handle: [u8; KEY_HANDLE_BYTES],
    pub fingerprint: [u8; FINGERPRINT_BYTES],
    pub machine_id: [u8; MACHINE_ID_BYTES],
    pub public_key: [u8; PUBLIC_KEY_BYTES],
    pub signing_seed: Zeroizing<[u8; SIGNING_SEED_BYTES]>,
    pub totp_secret: Zeroizing<[u8; TOTP_SECRET_BYTES]>,
    pub totp_active: bool,
    pub last_accepted_step: Option<u64>,
}

impl CredentialData {
    fn validate(&self) -> Result<(), StoreError> {
        if self.role > 2
            || self.provider_id.iter().all(|byte| *byte == 0)
            || self.key_handle.iter().all(|byte| *byte == 0)
            || self.fingerprint.iter().all(|byte| *byte == 0)
            || self.machine_id.iter().all(|byte| *byte == 0)
            || self.public_key.iter().all(|byte| *byte == 0)
            || self.signing_seed.iter().all(|byte| *byte == 0)
            || self.totp_secret.iter().all(|byte| *byte == 0)
            || (self.totp_active && self.last_accepted_step.is_none())
        {
            return Err(StoreError::InvalidCredential);
        }
        Ok(())
    }
}

pub struct OpenedCredential {
    pub generation: u64,
    pub credential: CredentialData,
}

/// Seal one complete credential snapshot with AES-256-GCM.
///
/// `nonce` must be freshly generated for every generation under a recovery
/// key. The fixed header is authenticated as AAD and binds the username,
/// account metadata, key reference, fingerprint, generation, and algorithm.
pub fn seal(
    username: &str,
    recovery_key: &[u8; RECOVERY_KEY_BYTES],
    generation: u64,
    nonce: [u8; NONCE_BYTES],
    credential: &CredentialData,
) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    validate_canonical_username(username)?;
    if generation == 0 {
        return Err(StoreError::InvalidGeneration);
    }
    if recovery_key.iter().all(|byte| *byte == 0) {
        return Err(StoreError::InvalidRecoveryKey);
    }
    credential.validate()?;

    let mut header = [0u8; HEADER_BYTES];
    header[0..4].copy_from_slice(MAGIC);
    header[4] = FORMAT_VERSION;
    header[5] = ALGORITHM_AES_256_GCM;
    header[6] = username.len() as u8;
    header[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    header[10..12].copy_from_slice(&(PLAINTEXT_BYTES as u16).to_le_bytes());
    header[12..16].copy_from_slice(&(ENVELOPE_BYTES as u32).to_le_bytes());
    header[16..24].copy_from_slice(&generation.to_le_bytes());
    header[24..32].copy_from_slice(&credential.account_id.to_le_bytes());
    header[32] = credential.role;
    header[36..48].copy_from_slice(&nonce);
    header[48..64].copy_from_slice(&credential.provider_id);
    header[64..96].copy_from_slice(&credential.key_handle);
    header[96..112].copy_from_slice(&credential.fingerprint);
    header[112..112 + username.len()].copy_from_slice(username.as_bytes());

    let mut plaintext = Zeroizing::new([0u8; PLAINTEXT_BYTES]);
    plaintext[0..32].copy_from_slice(credential.signing_seed.as_slice());
    plaintext[32..64].copy_from_slice(&credential.machine_id);
    plaintext[64..84].copy_from_slice(credential.totp_secret.as_slice());
    plaintext[84] = u8::from(credential.totp_active);
    plaintext[85] = u8::from(credential.last_accepted_step.is_some());
    if let Some(step) = credential.last_accepted_step {
        plaintext[88..96].copy_from_slice(&step.to_le_bytes());
    }
    plaintext[96..128].copy_from_slice(&credential.public_key);

    let unbound =
        UnboundKey::new(&AES_256_GCM, recovery_key).map_err(|_| StoreError::CryptoUnavailable)?;
    let key = LessSafeKey::new(unbound);
    let tag = key
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(header.as_slice()),
            plaintext.as_mut_slice(),
        )
        .map_err(|_| StoreError::CryptoUnavailable)?;

    let mut envelope = Zeroizing::new(Vec::with_capacity(ENVELOPE_BYTES));
    envelope.extend_from_slice(&header);
    envelope.extend_from_slice(plaintext.as_slice());
    envelope.extend_from_slice(tag.as_ref());
    Ok(envelope)
}

/// Authenticate and open a credential envelope for exactly `username`.
pub fn open(
    expected_username: &str,
    recovery_key: &[u8; RECOVERY_KEY_BYTES],
    envelope: &[u8],
) -> Result<OpenedCredential, StoreError> {
    validate_canonical_username(expected_username)?;
    if recovery_key.iter().all(|byte| *byte == 0) {
        return Err(StoreError::InvalidRecoveryKey);
    }
    if envelope.len() != ENVELOPE_BYTES || envelope.get(0..4) != Some(MAGIC.as_slice()) {
        return Err(StoreError::InvalidEnvelope);
    }
    if envelope[4] != FORMAT_VERSION {
        return Err(StoreError::UnsupportedVersion);
    }
    if envelope[5] != ALGORITHM_AES_256_GCM {
        return Err(StoreError::UnsupportedAlgorithm);
    }
    if envelope[7] != 0
        || envelope[8..10] != (HEADER_BYTES as u16).to_le_bytes()
        || envelope[10..12] != (PLAINTEXT_BYTES as u16).to_le_bytes()
        || envelope[12..16] != (ENVELOPE_BYTES as u32).to_le_bytes()
        || envelope[33..36].iter().any(|byte| *byte != 0)
    {
        return Err(StoreError::InvalidEnvelope);
    }

    let username_len = usize::from(envelope[6]);
    if !(USERNAME_MIN_BYTES..=USERNAME_MAX_BYTES).contains(&username_len)
        || envelope[112 + username_len..HEADER_BYTES]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(StoreError::InvalidEnvelope);
    }
    let stored_username = str::from_utf8(&envelope[112..112 + username_len])
        .map_err(|_| StoreError::InvalidEnvelope)?;
    validate_canonical_username(stored_username).map_err(StoreError::InvalidUsername)?;
    if stored_username != expected_username {
        return Err(StoreError::AuthenticationFailed);
    }

    let generation = u64::from_le_bytes(
        envelope[16..24]
            .try_into()
            .map_err(|_| StoreError::InvalidEnvelope)?,
    );
    if generation == 0 {
        return Err(StoreError::InvalidGeneration);
    }
    let nonce: [u8; NONCE_BYTES] = envelope[36..48]
        .try_into()
        .map_err(|_| StoreError::InvalidEnvelope)?;
    let tag = Tag::try_from(&envelope[HEADER_BYTES + PLAINTEXT_BYTES..])
        .map_err(|_| StoreError::InvalidEnvelope)?;
    let mut plaintext =
        Zeroizing::new(envelope[HEADER_BYTES..HEADER_BYTES + PLAINTEXT_BYTES].to_vec());

    let unbound =
        UnboundKey::new(&AES_256_GCM, recovery_key).map_err(|_| StoreError::CryptoUnavailable)?;
    let key = LessSafeKey::new(unbound);
    key.open_in_place_separate_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(&envelope[..HEADER_BYTES]),
        tag,
        plaintext.as_mut_slice(),
        0..,
    )
    .map_err(|_| StoreError::AuthenticationFailed)?;

    if plaintext[86..88].iter().any(|byte| *byte != 0)
        || !matches!(plaintext[84], 0 | 1)
        || !matches!(plaintext[85], 0 | 1)
    {
        return Err(StoreError::InvalidCredential);
    }
    let has_last_step = plaintext[85] == 1;
    let raw_last_step = u64::from_le_bytes(
        plaintext[88..96]
            .try_into()
            .map_err(|_| StoreError::InvalidCredential)?,
    );
    if !has_last_step && raw_last_step != 0 {
        return Err(StoreError::InvalidCredential);
    }

    let credential = CredentialData {
        account_id: u64::from_le_bytes(
            envelope[24..32]
                .try_into()
                .map_err(|_| StoreError::InvalidEnvelope)?,
        ),
        role: envelope[32],
        provider_id: envelope[48..64]
            .try_into()
            .map_err(|_| StoreError::InvalidEnvelope)?,
        key_handle: envelope[64..96]
            .try_into()
            .map_err(|_| StoreError::InvalidEnvelope)?,
        fingerprint: envelope[96..112]
            .try_into()
            .map_err(|_| StoreError::InvalidEnvelope)?,
        signing_seed: Zeroizing::new(
            plaintext[0..32]
                .try_into()
                .map_err(|_| StoreError::InvalidCredential)?,
        ),
        machine_id: plaintext[32..64]
            .try_into()
            .map_err(|_| StoreError::InvalidCredential)?,
        totp_secret: Zeroizing::new(
            plaintext[64..84]
                .try_into()
                .map_err(|_| StoreError::InvalidCredential)?,
        ),
        totp_active: plaintext[84] == 1,
        last_accepted_step: has_last_step.then_some(raw_last_step),
        public_key: plaintext[96..128]
            .try_into()
            .map_err(|_| StoreError::InvalidCredential)?,
    };
    credential.validate()?;

    Ok(OpenedCredential {
        generation,
        credential,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> CredentialData {
        CredentialData {
            account_id: 0,
            role: 2,
            provider_id: [1; PROVIDER_ID_BYTES],
            key_handle: [2; KEY_HANDLE_BYTES],
            fingerprint: [3; FINGERPRINT_BYTES],
            machine_id: [4; MACHINE_ID_BYTES],
            public_key: [5; PUBLIC_KEY_BYTES],
            signing_seed: Zeroizing::new([6; SIGNING_SEED_BYTES]),
            totp_secret: Zeroizing::new([7; TOTP_SECRET_BYTES]),
            totp_active: true,
            last_accepted_step: Some(42),
        }
    }

    #[test]
    fn username_policy_is_path_safe_and_canonical() {
        assert_eq!(normalize_username("Root").unwrap(), "root");
        assert_eq!(normalize_username("a_b-c.d").unwrap(), "a_b-c.d");
        assert_eq!(normalize_username("ab"), Err(UsernameError::InvalidLength));
        assert_eq!(normalize_username(".root"), Err(UsernameError::InvalidStart));
        assert_eq!(normalize_username("../root"), Err(UsernameError::InvalidStart));
        assert_eq!(normalize_username("ro/ot"), Err(UsernameError::InvalidCharacter));
        assert_eq!(normalize_username("r\\oot"), Err(UsernameError::InvalidCharacter));
        assert_eq!(normalize_username("röot"), Err(UsernameError::NonAscii));
    }

    #[test]
    fn aes256_gcm_round_trip_preserves_credential() {
        let key = [0x11; RECOVERY_KEY_BYTES];
        let envelope = seal("root", &key, 7, [0x22; NONCE_BYTES], &credential()).unwrap();
        assert_eq!(envelope.len(), ENVELOPE_BYTES);

        let opened = open("root", &key, envelope.as_slice()).unwrap();
        assert_eq!(opened.generation, 7);
        assert_eq!(opened.credential.account_id, 0);
        assert_eq!(opened.credential.role, 2);
        assert_eq!(opened.credential.provider_id, [1; PROVIDER_ID_BYTES]);
        assert_eq!(opened.credential.key_handle, [2; KEY_HANDLE_BYTES]);
        assert_eq!(opened.credential.fingerprint, [3; FINGERPRINT_BYTES]);
        assert_eq!(opened.credential.machine_id, [4; MACHINE_ID_BYTES]);
        assert_eq!(opened.credential.public_key, [5; PUBLIC_KEY_BYTES]);
        assert_eq!(opened.credential.signing_seed.as_slice(), &[6; SIGNING_SEED_BYTES]);
        assert_eq!(opened.credential.totp_secret.as_slice(), &[7; TOTP_SECRET_BYTES]);
        assert!(opened.credential.totp_active);
        assert_eq!(opened.credential.last_accepted_step, Some(42));
    }

    #[test]
    fn envelope_rejects_wrong_identity_key_and_tampering() {
        let key = [0x11; RECOVERY_KEY_BYTES];
        let envelope = seal("root", &key, 1, [0x22; NONCE_BYTES], &credential()).unwrap();
        assert_eq!(
            open("user", &key, envelope.as_slice()).err(),
            Some(StoreError::AuthenticationFailed)
        );
        assert_eq!(
            open("root", &[0x12; RECOVERY_KEY_BYTES], envelope.as_slice()).err(),
            Some(StoreError::AuthenticationFailed)
        );

        for index in [24usize, HEADER_BYTES, ENVELOPE_BYTES - 1] {
            let mut tampered = envelope.to_vec();
            tampered[index] ^= 1;
            assert!(open("root", &key, tampered.as_slice()).is_err());
        }
    }

    #[test]
    fn envelope_rejects_unknown_version_and_truncation() {
        let key = [0x11; RECOVERY_KEY_BYTES];
        let envelope = seal("root", &key, 1, [0x22; NONCE_BYTES], &credential()).unwrap();
        let mut future = envelope.to_vec();
        future[4] = FORMAT_VERSION + 1;
        assert_eq!(
            open("root", &key, future.as_slice()).err(),
            Some(StoreError::UnsupportedVersion)
        );
        assert_eq!(
            open("root", &key, &envelope[..envelope.len() - 1]).err(),
            Some(StoreError::InvalidEnvelope)
        );
    }
}
