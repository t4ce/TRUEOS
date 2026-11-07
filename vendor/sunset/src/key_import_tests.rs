//! Test identities are fixed public fixtures, never application credentials.
use super::*;
use alloc::{format, vec, vec::Vec};
use der::Encode;

fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut result = vec![tag];
    if value.len() < 128 { result.push(value.len() as u8); }
    else if value.len() < 256 { result.extend_from_slice(&[0x81, value.len() as u8]); }
    else { result.extend_from_slice(&[0x82, (value.len() >> 8) as u8, value.len() as u8]); }
    result.extend_from_slice(value);
    result
}
fn seq(parts: &[&[u8]]) -> Vec<u8> { tlv(0x30, &parts.concat()) }
fn oid(curve: Curve) -> Vec<u8> { curve.oid().to_der().unwrap() }
fn sec1(scalar: &[u8], params: Option<&[u8]>, public: Option<&[u8]>) -> Vec<u8> {
    let mut fields = vec![2, 1, 1];
    fields.extend(tlv(4, scalar));
    if let Some(parameters) = params { fields.extend(tlv(0xa0, parameters)); }
    if let Some(public) = public {
        let mut bits = vec![0]; bits.extend_from_slice(public);
        fields.extend(tlv(0xa1, &tlv(3, &bits)));
    }
    tlv(0x30, &fields)
}
fn pem(label: &'static str, bytes: &[u8]) -> Zeroizing<String> {
    SecretDocument::try_from(bytes).unwrap().to_pem(label, pkcs8::LineEnding::LF).unwrap()
}
fn pkcs8(inner: &[u8], curve: Curve) -> Zeroizing<String> {
    let algorithm = seq(&[&EC.to_der().unwrap(), &oid(curve)]);
    pem("PRIVATE KEY", &seq(&[&[2, 1, 0], &algorithm, &tlv(4, inner)]))
}
fn enabled_keys() -> Vec<(Curve, SignKey, Vec<u8>, &'static [u8])> {
    let mut keys = Vec::new();
    #[cfg(feature = "ecdsa256")]
    keys.push((Curve::P256, SignKey::ECDSA256(ecdsa::SigningKey::from_slice(&[37; 32]).unwrap()), vec![37; 32], domains::NISTP256));
    #[cfg(feature = "ecdsa384")]
    keys.push((Curve::P384, SignKey::ECDSA384(ecdsa::SigningKey::from_slice(&[37; 48]).unwrap()), vec![37; 48], domains::NISTP384));
    #[cfg(feature = "ecdsa521")]
    { let mut scalar = vec![37; 66]; scalar[0] = 1;
      keys.push((Curve::P521, SignKey::ECDSA521(ecdsa::SigningKey::from_slice(&scalar).unwrap()), scalar, domains::NISTP521)); }
    keys
}
fn point(key: &SignKey) -> Vec<u8> {
    match key {
        #[cfg(feature = "ecdsa256")]
        SignKey::ECDSA256(key) => key.verifying_key().to_encoded_point(false).as_bytes().to_vec(),
        #[cfg(feature = "ecdsa384")]
        SignKey::ECDSA384(key) => key.verifying_key().to_encoded_point(false).as_bytes().to_vec(),
        #[cfg(feature = "ecdsa521")]
        SignKey::ECDSA521(key) => key.verifying_key().to_encoded_point(false).as_bytes().to_vec(),
        _ => panic!("not an EC test identity"),
    }
}

#[test]
fn openssh_roundtrip_fingerprints_and_comment_independent_identity() {
    let mut keys = vec![SignKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[19; 32]))];
    keys.extend(enabled_keys().into_iter().map(|(_, key, _, _)| key));
    for key in keys {
        let private = key.to_openssh_private().unwrap();
        assert_eq!(SignKey::import_private(private.as_bytes()).unwrap(), key);
        let public = key.pubkey().to_openssh().unwrap();
        assert!(key.pubkey().matches_openssh(&format!("{public} imported@test")).unwrap());
        assert!(key.pubkey().fingerprint_sha256().unwrap().starts_with("SHA256:"));
        let other = SignKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[20; 32]));
        assert!(!key.pubkey().matches_openssh(&other.pubkey().to_openssh().unwrap()).unwrap());
        assert!(!format!("{key:?}").contains(&private[private.len()/2..private.len()/2+12]));
    }
}

#[test]
fn sec1_named_and_explicit_domains_and_pkcs8_preserve_identity() {
    for (curve, key, scalar, domain) in enabled_keys() {
        let public = point(&key);
        for parameters in [&oid(curve)[..], domain] {
            let inner = sec1(&scalar, Some(parameters), Some(&public));
            let sec1 = pem("EC PRIVATE KEY", &inner);
            assert_eq!(SignKey::import_private(sec1.as_bytes()).unwrap(), key);
            assert_eq!(SignKey::import_private(pkcs8(&inner, curve).as_bytes()).unwrap(), key);
        }
        // PKCS8 may carry the only curve identifier.
        assert_eq!(SignKey::import_private(pkcs8(&sec1(&scalar, None, None), curve).as_bytes()).unwrap(), key);
    }
}

#[test]
fn scalar_public_and_curve_identifiers_are_checked_together() {
    for (curve, key, scalar, _) in enabled_keys() {
        let mut public = point(&key); public[1] ^= 1;
        assert!(SignKey::import_private(pem("EC PRIVATE KEY", &sec1(&scalar, Some(&oid(curve)), Some(&public))).as_bytes()).is_err());
        let mut invalid = scalar.clone(); invalid.fill(0);
        assert!(SignKey::import_private(pem("EC PRIVATE KEY", &sec1(&invalid, Some(&oid(curve)), None)).as_bytes()).is_err());
        invalid.fill(255);
        assert!(SignKey::import_private(pem("EC PRIVATE KEY", &sec1(&invalid, Some(&oid(curve)), None)).as_bytes()).is_err());
        let other = if curve == Curve::P256 { Curve::P384 } else { Curve::P256 };
        assert!(SignKey::import_private(pkcs8(&sec1(&scalar, Some(&oid(curve)), None), other).as_bytes()).is_err());
        // A naked scalar must never be assigned a curve from its byte length.
        assert!(SignKey::import_private(pem("EC PRIVATE KEY", &sec1(&scalar, None, None)).as_bytes()).is_err());
    }
}

#[test]
fn explicit_domain_requires_actual_parameters_not_only_field_length() {
    for (_, _, scalar, domain) in enabled_keys() {
        let mut damaged = domain.to_vec();
        // Alter the prime's least significant byte, retaining exactly its length.
        let supplied = super::domain(domain).unwrap();
        let offset = supplied.prime.as_ptr() as usize - domain.as_ptr() as usize;
        damaged[offset + supplied.prime.len() - 1] ^= 2;
        assert!(SignKey::import_private(pem("EC PRIVATE KEY", &sec1(&scalar, Some(&damaged), None)).as_bytes()).is_err());
    }
}

#[test]
fn openssh_codec_cannot_accept_inconsistent_private_and_public_halves() {
    let ed = ssh_key::private::Ed25519Keypair {
        public: ssh_key::public::Ed25519PublicKey([3; 32]),
        private: ssh_key::private::Ed25519PrivateKey::from_bytes(&[19; 32]),
    };
    let codec = ssh_key::PrivateKey::new(ssh_key::private::KeypairData::Ed25519(ed), "fixture").unwrap();
    // Some codec versions reject during construction/decoding; the backend
    // conversion must independently enforce the same relationship.
    assert!(SignKey::try_from(codec).is_err());
}

#[test]
fn malformed_encrypted_oversized_and_trailing_data_are_refused() {
    for bytes in [b"".as_slice(), b"not a key", b"-----BEGIN PRIVATE KEY-----\nbroken\n-----END PRIVATE KEY-----"] {
        assert!(SignKey::import_private(bytes).is_err());
    }
    assert!(SignKey::import_private(&vec![b'x'; MAX_PRIVATE_FILE + 1]).is_err());
    assert!(matches!(SignKey::import_private(b"-----BEGIN ENCRYPTED PRIVATE KEY-----"), Err(Error::NotAvailable { .. })));
    assert!(matches!(SignKey::import_private(b"PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: aes256-cbc\n"), Err(Error::NotAvailable { .. })));
    for (curve, _, scalar, _) in enabled_keys() {
        let mut bytes = sec1(&scalar, Some(&oid(curve)), None); bytes.push(0);
        assert!(import_ec(&bytes, None).is_err());
        let mut nested = sec1(&scalar, Some(&oid(curve)), None);
        // Extra field inside the sequence must be refused too.
        let length_bytes = if nested[1] < 128 { 2 } else { 2 + usize::from(nested[1] & 127) };
        let mut content = nested.split_off(length_bytes); content.extend_from_slice(&[5,0]);
        assert!(import_ec(&tlv(0x30, &content), None).is_err());
    }
}
