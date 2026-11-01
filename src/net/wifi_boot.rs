//! Optional private boot credentials supplied by the local build.
//!
//! The payload is TW01, one-byte SSID/password lengths, then UTF-8 bytes.
//! An empty payload disables boot connection. Never log or format its contents.

#[cfg(not(test))]
const DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wifi-boot.bin"));
#[cfg(test)]
const DATA: &[u8] = &[];

pub(super) fn credentials() -> Option<(&'static str, &'static str)> {
    parse(DATA)
}

fn parse(data: &[u8]) -> Option<(&str, &str)> {
    if data.len() < 6 || &data[..4] != b"TW01" {
        return None;
    }
    let ssid_len = usize::from(data[4]);
    let password_len = usize::from(data[5]);
    if !(1..=32).contains(&ssid_len)
        || !(8..=64).contains(&password_len)
        || data.len() != 6 + ssid_len + password_len
    {
        return None;
    }
    let ssid = core::str::from_utf8(&data[6..6 + ssid_len]).ok()?;
    let password = core::str::from_utf8(&data[6 + ssid_len..]).ok()?;
    if ssid
        .bytes()
        .chain(password.bytes())
        .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
        || password_len == 64 && !password.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some((ssid, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(ssid: &[u8], password: &[u8]) -> std::vec::Vec<u8> {
        [
            b"TW01".as_slice(),
            &[ssid.len() as u8, password.len() as u8],
            ssid,
            password,
        ]
        .concat()
    }

    #[test]
    fn empty_build_input_disables_boot_credentials() {
        assert!(credentials().is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn private_format_preserves_spaces_and_utf8_bytes() {
        let bytes = payload(" café ".as_bytes(), " pässword ".as_bytes());
        let (ssid, password) = parse(&bytes).expect("valid private format");
        assert!(ssid == " café ");
        assert!(password == " pässword ");
    }

    #[test]
    fn lengths_are_bounded_by_bytes() {
        for ssid_len in [0, 33] {
            assert!(parse(&payload(&std::vec![b'x'; ssid_len], b"password")).is_none());
        }
        for password_len in [0, 7, 65] {
            assert!(parse(&payload(b"test", &std::vec![b'x'; password_len])).is_none());
        }
        assert!(parse(&payload("é".repeat(16).as_bytes(), b"password")).is_some());
        assert!(parse(&payload("é".repeat(17).as_bytes(), b"password")).is_none());
        assert!(parse(&payload(b"test", "é".repeat(32).as_bytes())).is_none());
        assert!(parse(&payload(&[b'x'; 32], &[b'x'; 63])).is_some());
    }

    #[test]
    fn raw_psk_requires_exactly_64_hexadecimal_bytes() {
        assert!(
            parse(&payload(
                b"test",
                b"0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789ABCDEF"
            ))
            .is_some()
        );
        assert!(parse(&payload(b"test", &[b'g'; 64])).is_none());
        assert!(parse(&payload(b"test", &[b'x'; 63])).is_some());
    }

    #[test]
    fn magic_truncation_and_trailing_data_are_rejected() {
        let bytes = payload(b"test", b"password");
        for len in 0..bytes.len() {
            assert!(parse(&bytes[..len]).is_none());
        }
        let mut bad_magic = bytes.clone();
        bad_magic[3] = b'2';
        assert!(parse(&bad_magic).is_none());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(parse(&trailing).is_none());
        for at in [4, 5] {
            let mut bad_length = bytes.clone();
            bad_length[at] += 1;
            assert!(parse(&bad_length).is_none());
        }
    }

    #[test]
    fn invalid_utf8_or_line_separators_are_rejected() {
        for (ssid, password) in [
            (&b"te\xfft"[..], &b"password"[..]),
            (&b"test"[..], &b"pass\xfford"[..]),
            (&b"te\0t"[..], &b"password"[..]),
            (&b"te\nt"[..], &b"password"[..]),
            (&b"te\rt"[..], &b"password"[..]),
            (&b"test"[..], &b"pass\0ord"[..]),
            (&b"test"[..], &b"pass\nord"[..]),
            (&b"test"[..], &b"pass\rord"[..]),
        ] {
            assert!(parse(&payload(ssid, password)).is_none());
        }
    }
}
