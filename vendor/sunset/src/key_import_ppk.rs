//! Plaintext PuTTY v2/v3 container decoding. The client has no passphrase UI;
//! encrypted containers are refused, not decrypted with an empty password.
use super::*;
use alloc::vec::Vec;
use base64ct::{Base64, Encoding};
use hmac::Mac;

fn blob(
    lines: &mut core::str::Lines<'_>,
    count: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    let count: usize = count.parse().map_err(bad)?;
    if count == 0 || count > MAX_PRIVATE_FILE {
        return Err(Error::BadKey);
    }
    let mut encoded = Zeroizing::new(Vec::new());
    for _ in 0..count {
        encoded.extend_from_slice(lines.next().ok_or(Error::BadKey)?.as_bytes());
    }
    let length = Base64::decode_in_place(&mut encoded).map_err(bad)?.len();
    encoded.truncate(length);
    Ok(encoded)
}
fn hex_mac(text: &str, length: usize) -> Result<Vec<u8>> {
    if text.len() != length * 2 {
        return Err(Error::BadKey);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                _ => Err(Error::BadKey),
            };
            Ok(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
fn field<M: Mac>(mac: &mut M, bytes: &[u8]) {
    // Every input is a slice of a <=1MiB file, so this cast cannot truncate.
    mac.update(&(bytes.len() as u32).to_be_bytes());
    mac.update(bytes);
}

pub(super) fn import(text: &str) -> Result<SignKey> {
    let mut lines = text.lines();
    let (header, algorithm) =
        lines.next().and_then(|line| line.split_once(": ")).ok_or(Error::BadKey)?;
    let version = match header {
        "PuTTY-User-Key-File-2" => 2,
        "PuTTY-User-Key-File-3" => 3,
        _ => return Err(Error::BadKey),
    };
    let (mut encryption, mut comment, mut public, mut private, mut mac) =
        (None, None, None, None, None);
    while let Some(line) = lines.next() {
        let (name, value) = line.split_once(": ").ok_or(Error::BadKey)?;
        match name {
            "Encryption" if encryption.is_none() => encryption = Some(value),
            "Comment" if comment.is_none() => comment = Some(value),
            "Public-Lines" if public.is_none() => {
                public = Some(blob(&mut lines, value)?)
            }
            "Private-Lines" if private.is_none() => {
                private = Some(blob(&mut lines, value)?)
            }
            "Private-MAC" if mac.is_none() => mac = Some(value),
            _ => return Err(Error::BadKey),
        }
    }
    let encryption = encryption.ok_or(Error::BadKey)?;
    if encryption != "none" {
        return Err(encrypted());
    }
    let comment = comment.ok_or(Error::BadKey)?;
    let public = public.ok_or(Error::BadKey)?;
    let private = private.ok_or(Error::BadKey)?;
    let mac =
        hex_mac(mac.ok_or(Error::BadKey)?, if version == 2 { 20 } else { 32 })?;
    macro_rules! verify {
        ($hash:ty, $key:expr) => {{
            let mut hmac =
                <hmac::Hmac<$hash> as Mac>::new_from_slice($key).map_err(bad)?;
            for value in [
                algorithm.as_bytes(),
                encryption.as_bytes(),
                comment.as_bytes(),
                &public,
                &private,
            ] {
                field(&mut hmac, value);
            }
            hmac.verify_slice(&mac).map_err(bad)?;
        }};
    }
    if version == 2 {
        use digest::Digest;
        // SHA1 is required only for the existing PPKv2 integrity format.
        let key = sha1::Sha1::digest(b"putty-private-key-file-mac-key");
        verify!(sha1::Sha1, &key);
    } else {
        verify!(sha2::Sha256, b"");
    }
    let public = ssh_key::PublicKey::from_bytes(&public).map_err(bad)?;
    if public.algorithm().as_str() != algorithm {
        return Err(Error::BadKey);
    }
    // All currently supported PPK identities encode one private SSH string.
    // Ed25519 stores seed bytes here (the sign bit is not numeric); ECDSA
    // stores a positive mpint. Borrow the zeroizing buffer instead of copying
    // secret bytes into the codec's non-zeroizing public Mpint type.
    let length = u32::from_be_bytes(
        private.get(..4).ok_or(Error::BadKey)?.try_into().map_err(bad)?,
    ) as usize;
    let scalar = private.get(4..).ok_or(Error::BadKey)?;
    if scalar.len() != length {
        return Err(Error::BadKey);
    }
    macro_rules! ec {
        ($curve:ty, $variant:ident, $length:expr) => {{
            let scalar = match scalar {
                [0, rest @ ..] if rest.first().is_some_and(|byte| *byte >= 128) => {
                    rest
                }
                [first, ..] if *first < 128 => scalar,
                _ => return Err(Error::BadKey),
            };
            if scalar.len() > $length {
                return Err(Error::BadKey);
            }
            let mut padded = Zeroizing::new([0; $length]);
            padded[$length - scalar.len()..].copy_from_slice(scalar);
            SignKey::$variant(
                ecdsa::SigningKey::<$curve>::from_slice(padded.as_slice())
                    .map_err(bad)?,
            )
        }};
    }
    let key = match public.key_data() {
        ssh_key::public::KeyData::Ed25519(_) => {
            if scalar.len() > 32 {
                return Err(Error::BadKey);
            }
            let mut padded = Zeroizing::new([0; 32]);
            padded[32 - scalar.len()..].copy_from_slice(scalar);
            SignKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&padded))
        }
        #[cfg(feature = "ecdsa256")]
        ssh_key::public::KeyData::Ecdsa(
            ssh_key::public::EcdsaPublicKey::NistP256(_),
        ) => ec!(p256::NistP256, ECDSA256, 32),
        #[cfg(feature = "ecdsa384")]
        ssh_key::public::KeyData::Ecdsa(
            ssh_key::public::EcdsaPublicKey::NistP384(_),
        ) => ec!(p384::NistP384, ECDSA384, 48),
        #[cfg(feature = "ecdsa521")]
        ssh_key::public::KeyData::Ecdsa(
            ssh_key::public::EcdsaPublicKey::NistP521(_),
        ) => ec!(p521::NistP521, ECDSA521, 66),
        _ => return Err(Error::NotAvailable { what: "PPK private key algorithm" }),
    };
    let derived: ssh_key::PublicKey = (&key.pubkey()).try_into()?;
    if derived.key_data() != public.key_data() {
        return Err(Error::BadKey);
    }
    Ok(key)
}
