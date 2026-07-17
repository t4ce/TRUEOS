//! RFC 6238 TOTP compatibility for common authenticator applications.

use hmac::{Hmac, Mac};
use sha1::Sha1;

pub const TOTP_SECRET_BYTES: usize = 20;
pub const TOTP_BASE32_BYTES: usize = 32;
pub const TOTP_PERIOD_SECONDS: u64 = 30;
pub const TOTP_DIGITS: u32 = 6;

const TOTP_MODULUS: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TotpError {
    SecretTooShort,
    InvalidKey,
}

/// Encode one 160-bit TOTP secret as unpadded RFC 4648 Base32.
pub fn encode_totp_secret_base32(secret: &[u8; TOTP_SECRET_BYTES]) -> [u8; TOTP_BASE32_BYTES] {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    let mut output = [0u8; TOTP_BASE32_BYTES];
    let mut output_index = 0usize;
    let mut accumulator = 0u32;
    let mut bits = 0u8;
    for &byte in secret {
        accumulator = (accumulator << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let alphabet_index = ((accumulator >> bits) & 0x1f) as usize;
            output[output_index] = ALPHABET[alphabet_index];
            output_index += 1;
        }
    }
    debug_assert_eq!(bits, 0);
    debug_assert_eq!(output_index, TOTP_BASE32_BYTES);
    output
}

/// Generate the six-digit SHA-1 TOTP value for one counter step.
///
/// SHA-1 is used here solely because it is the interoperable RFC 6238 profile
/// implemented by Google Authenticator and similar applications. The HMAC
/// construction, not bare SHA-1, is the authentication primitive.
pub fn generate_totp_sha1(secret: &[u8], counter: u64) -> Result<u32, TotpError> {
    if secret.len() < TOTP_SECRET_BYTES {
        return Err(TotpError::SecretTooShort);
    }

    let mut mac = Hmac::<Sha1>::new_from_slice(secret).map_err(|_| TotpError::InvalidKey)?;
    mac.update(&counter.to_be_bytes());
    let tag = mac.finalize().into_bytes();
    let offset = usize::from(tag[tag.len() - 1] & 0x0f);
    let binary = (u32::from(tag[offset] & 0x7f) << 24)
        | (u32::from(tag[offset + 1]) << 16)
        | (u32::from(tag[offset + 2]) << 8)
        | u32::from(tag[offset + 3]);
    Ok(binary % TOTP_MODULUS)
}

/// Verify a code against the current time step and a symmetric skew window.
///
/// Returns the exact accepted counter so the caller can reject replay. All
/// candidate steps are evaluated before returning.
pub fn verify_totp_sha1(
    secret: &[u8],
    unix_seconds: u64,
    code: u32,
    skew_steps: u8,
) -> Result<Option<u64>, TotpError> {
    let current = unix_seconds / TOTP_PERIOD_SECONDS;
    let mut matched = None;
    let skew = u64::from(skew_steps);
    let first = current.saturating_sub(skew);
    let last = current.saturating_add(skew);

    for counter in first..=last {
        let candidate = generate_totp_sha1(secret, counter)?;
        if constant_time_u32_eq(candidate, code) && matched.is_none() {
            matched = Some(counter);
        }
    }
    Ok(matched)
}

#[inline]
fn constant_time_u32_eq(left: u32, right: u32) -> bool {
    let diff = left ^ right;
    ((diff | diff.wrapping_neg()) >> 31) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn rfc_6238_sha1_vectors_reduce_to_six_digits() {
        let vectors = [
            (59, 287_082),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
            (20_000_000_000, 353_130),
        ];

        for (unix_seconds, expected) in vectors {
            assert_eq!(
                generate_totp_sha1(RFC_SECRET, unix_seconds / TOTP_PERIOD_SECONDS),
                Ok(expected),
            );
        }
    }

    #[test]
    fn provisioning_secret_uses_unpadded_rfc_4648_base32() {
        assert_eq!(
            encode_totp_secret_base32(RFC_SECRET.try_into().unwrap()),
            *b"GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
        );
    }

    #[test]
    fn verifier_returns_the_step_for_replay_tracking() {
        let unix_seconds = 1_234_567_890;
        let current = unix_seconds / TOTP_PERIOD_SECONDS;
        let code = generate_totp_sha1(RFC_SECRET, current - 1).unwrap();
        assert_eq!(verify_totp_sha1(RFC_SECRET, unix_seconds, code, 1), Ok(Some(current - 1)),);
        assert_eq!(verify_totp_sha1(RFC_SECRET, unix_seconds, code, 0), Ok(None),);
    }

    #[test]
    fn short_secrets_are_rejected() {
        assert_eq!(
            generate_totp_sha1(&[7; TOTP_SECRET_BYTES - 1], 0),
            Err(TotpError::SecretTooShort),
        );
    }
}
