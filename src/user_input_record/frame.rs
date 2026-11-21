//! Version 1 of the encrypted Shell3 history frame.
use alloc::string::String;
use chacha20poly1305::{ChaCha20Poly1305, Nonce, Tag, aead::AeadInPlace};
use zeroize::Zeroizing;

pub(super) const COMMAND_BYTES: usize = 192;
pub(super) const HEADER_BYTES: usize = 32;
pub(super) const PLAINTEXT_BYTES: usize = 40 + COMMAND_BYTES;
pub(super) const TAG_BYTES: usize = 16;
pub(super) const RECORD_BYTES: usize = HEADER_BYTES + PLAINTEXT_BYTES + TAG_BYTES;
pub(super) const FORMAT_VERSION: u8 = 1;
pub(super) const ALGORITHM_CHACHA20_POLY1305: u8 = 1;

pub(super) fn recallable(text: &str) -> bool {
    // A full v1 text field is ambiguous because older writers truncated long
    // commands to this size. Never recall a possibly incomplete command.
    text.len() < COMMAND_BYTES
        && !text.trim().is_empty()
        && !text.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
}

pub(super) fn decode(
    cipher: &ChaCha20Poly1305,
    record: &[u8],
    account: u64,
) -> Option<Zeroizing<String>> {
    if record.len() != RECORD_BYTES {
        return None;
    }
    let (header, payload) = record.split_at(HEADER_BYTES);
    if &header[..4] != b"TUIR"
        || header[4] != FORMAT_VERSION
        || header[5] != ALGORITHM_CHACHA20_POLY1305
        || header[6..8] != (HEADER_BYTES as u16).to_le_bytes()
        || header[8..12] != (RECORD_BYTES as u32).to_le_bytes()
    {
        return None;
    }

    let (encrypted, tag) = payload.split_at(PLAINTEXT_BYTES);
    let mut plaintext: Zeroizing<[u8; PLAINTEXT_BYTES]> =
        Zeroizing::new(encrypted.try_into().ok()?);
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&header[20..32]),
            header,
            &mut *plaintext,
            Tag::from_slice(tag),
        )
        .ok()?;
    if plaintext[8..16] != account.to_le_bytes() {
        return None;
    }
    let length = usize::from(u16::from_le_bytes(plaintext[32..34].try_into().ok()?));
    let text = core::str::from_utf8(plaintext.get(40..40 + length)?).ok()?;
    recallable(text).then(|| Zeroizing::new(String::from(text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chacha20poly1305::aead::KeyInit;

    fn record(text: &[u8], account: u64) -> [u8; RECORD_BYTES] {
        let cipher = ChaCha20Poly1305::new_from_slice(&[13; 32]).unwrap();
        let mut record = [0; RECORD_BYTES];
        let (header, payload) = record.split_at_mut(HEADER_BYTES);
        let (body, tag) = payload.split_at_mut(PLAINTEXT_BYTES);
        header[..4].copy_from_slice(b"TUIR");
        header[4] = FORMAT_VERSION;
        header[5] = ALGORITHM_CHACHA20_POLY1305;
        header[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        header[8..12].copy_from_slice(&(RECORD_BYTES as u32).to_le_bytes());
        body[8..16].copy_from_slice(&account.to_le_bytes());
        body[32..34].copy_from_slice(&(text.len() as u16).to_le_bytes());
        body[40..40 + text.len()].copy_from_slice(text);
        tag.copy_from_slice(
            &cipher
                .encrypt_in_place_detached(Nonce::from_slice(&header[20..]), header, body)
                .unwrap(),
        );
        record
    }

    #[test]
    fn accepts_only_untampered_frames_for_the_account() {
        let cipher = ChaCha20Poly1305::new_from_slice(&[13; 32]).unwrap();
        let wrong_key = ChaCha20Poly1305::new_from_slice(&[14; 32]).unwrap();
        let valid = record(b"gpu", 42);
        assert_eq!(decode(&cipher, &valid, 42).unwrap().as_str(), "gpu");
        assert!(decode(&cipher, &valid, 43).is_none());
        assert!(decode(&wrong_key, &valid, 42).is_none());
        assert!(decode(&cipher, &valid[..RECORD_BYTES - 1], 42).is_none());
        for offset in [0, 12, 20, HEADER_BYTES, RECORD_BYTES - 1] {
            let mut changed = valid;
            changed[offset] ^= 1;
            assert!(decode(&cipher, &changed, 42).is_none());
        }
    }

    #[test]
    fn skips_authentic_but_unsafe_recall_text() {
        let cipher = ChaCha20Poly1305::new_from_slice(&[13; 32]).unwrap();
        for text in [
            b"gpu\naction".as_slice(),
            b"\x1b[2J",
            b"\xff",
            "gpu\u{202e}".as_bytes(),
            &[b'a'; COMMAND_BYTES],
        ] {
            assert!(decode(&cipher, &record(text, 42), 42).is_none());
        }
    }
}
