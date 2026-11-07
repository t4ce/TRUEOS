//! Byte-only identity-file import; filesystem and trust policy stay in the app.
use alloc::string::{String, ToString};
use der::{Decode, Reader, SliceReader, Tag, TagNumber, Tagged};
use der::asn1::{AnyRef, BitStringRef, ContextSpecific, ObjectIdentifier, OctetStringRef, UintRef};
use pkcs8::SecretDocument;
use zeroize::Zeroizing;
use crate::{Error, PubKey, Result, SignKey};
#[path = "key_import_domains.rs"]
mod domains;
#[path = "key_import_ppk.rs"]
mod ppk;
const MAX_PRIVATE_FILE: usize = 1024 * 1024;
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const PRIME_FIELD: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.1.1");
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Curve { P256, P384, P521 }
impl Curve {
    fn oid(self) -> ObjectIdentifier {
        ObjectIdentifier::new_unwrap(match self {
            Self::P256 => "1.2.840.10045.3.1.7", Self::P384 => "1.3.132.0.34", Self::P521 => "1.3.132.0.35",
        })
    }
    fn named(oid: ObjectIdentifier) -> Result<Self> {
        [Self::P256,Self::P384,Self::P521].into_iter().find(|c|c.oid()==oid).ok_or(Error::BadKey)
    }
}
fn bad<T>(_: T) -> Error { Error::BadKey }
fn encrypted() -> Error { Error::NotAvailable { what: "encrypted key without passphrase" } }

impl SignKey {
    /// Import the same plaintext identity formats used by the resident client.
    /// Encrypted keys require a passphrase, which that client does not request.
    pub fn import_private(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_PRIVATE_FILE { return Err(Error::msg("Private key file is too large")); }
        let text = core::str::from_utf8(input).map_err(bad)?.trim();
        if text.starts_with("PuTTY-User-Key-File-") {
            if text.lines().any(|line| line.strip_prefix("Encryption: ").is_some_and(|value| value != "none")) {
                return Err(encrypted());
            }
            return ppk::import(text);
        }
        if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
            let key=ssh_key::PrivateKey::from_openssh(text).map_err(bad)?;
            if key.is_encrypted() { return Err(encrypted()); }
            return key.try_into();
        }
        if text.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----")
            || text.lines().any(|line|line.starts_with("DEK-Info:") || line.starts_with("Proc-Type: 4,ENCRYPTED")) {
            return Err(encrypted());
        }
        let (label,document)=SecretDocument::from_pem(text).map_err(bad)?;
        match label {
            "EC PRIVATE KEY" => import_ec(document.as_bytes(),None),
            "PRIVATE KEY" => {
                let info=pkcs8::PrivateKeyInfo::from_der(document.as_bytes()).map_err(bad)?;
                if info.algorithm.oid==ED25519 {
                    use pkcs8::DecodePrivateKey;
                    return ed25519_dalek::SigningKey::from_pkcs8_der(document.as_bytes()).map(Self::Ed25519).map_err(bad);
                }
                if info.algorithm.oid==EC {
                    let curve=curve_parameters(info.algorithm.parameters.ok_or(Error::BadKey)?)?;
                    return import_ec(info.private_key,Some(curve));
                }
                Err(Error::NotAvailable {what:"private key algorithm"})
            }
            _ => Err(Error::NotAvailable {what:"private key format"}),
        }
    }

    /// Persist in the existing OpenSSH format; returned secret text zeroizes.
    pub fn to_openssh_private(&self) -> Result<Zeroizing<String>> {
        use ssh_key::private::{Ed25519Keypair,Ed25519PrivateKey,KeypairData};
        let data=match self {
            Self::Ed25519(key)=>KeypairData::Ed25519(Ed25519Keypair {
                public:ssh_key::public::Ed25519PublicKey(key.verifying_key().to_bytes()),
                private:Ed25519PrivateKey::from_bytes(&key.to_bytes()),
            }),
            #[cfg(feature="ecdsa256")]
            Self::ECDSA256(key)=>{
                let mut scalar=Zeroizing::new([0;32]);scalar.copy_from_slice(&key.to_bytes());
                KeypairData::Ecdsa(ssh_key::private::EcdsaKeypair::NistP256 {
                    public:key.verifying_key().to_encoded_point(false).as_bytes().try_into().map_err(bad)?,private:(*scalar).into(),
                })
            },
            #[cfg(feature="ecdsa384")]
            Self::ECDSA384(key)=>{
                let mut scalar=Zeroizing::new([0;48]);scalar.copy_from_slice(&key.to_bytes());
                KeypairData::Ecdsa(ssh_key::private::EcdsaKeypair::NistP384 {
                    public:key.verifying_key().to_encoded_point(false).as_bytes().try_into().map_err(bad)?,private:(*scalar).into(),
                })
            },
            #[cfg(feature="ecdsa521")]
            Self::ECDSA521(key)=>{
                let mut scalar=Zeroizing::new([0;66]);scalar.copy_from_slice(&key.to_bytes());
                KeypairData::Ecdsa(ssh_key::private::EcdsaKeypair::NistP521 {
                    public:key.verifying_key().to_encoded_point(false).as_bytes().try_into().map_err(bad)?,private:(*scalar).into(),
                })
            },
            _=>return Err(Error::NotAvailable{what:"exporting this private key type"}),
        };
        ssh_key::PrivateKey::new(data,"").map_err(bad)?.to_openssh(ssh_key::LineEnding::LF).map_err(bad)
    }
}
impl PubKey<'_> {
    pub fn to_openssh(&self)->Result<String> {
        let key:ssh_key::PublicKey=self.try_into()?;
        key.to_openssh().map_err(bad)
    }
    pub fn fingerprint_sha256(&self)->Result<String> {
        self.fingerprint(ssh_key::HashAlg::Sha256).map(|v|v.to_string())
    }
}

/// Decode SEC1 ourselves so the declared curve is checked too. The underlying
/// stable SecretKey decoder deliberately does not validate its params field.
fn import_ec(bytes:&[u8],hint:Option<Curve>)->Result<SignKey> {
    let mut reader=SliceReader::new(bytes).map_err(bad)?;
    let (secret,parameters,public)=reader.sequence(|sequence|{
        if sequence.decode::<u8>()?!=1 {return Err(Tag::Integer.value_error());}
        let secret:OctetStringRef<'_>=sequence.decode()?;
        let parameters=ContextSpecific::<AnyRef<'_>>::decode_explicit(sequence,TagNumber::N0)?.map(|v|v.value);
        let public=ContextSpecific::<BitStringRef<'_>>::decode_explicit(sequence,TagNumber::N1)?
            .map(|v|v.value.as_bytes().ok_or(Tag::BitString.value_error())).transpose()?;
        Ok((secret.as_bytes(),parameters,public))
    }).map_err(bad)?;
    reader.finish(()).map_err(bad)?;
    let curve=match parameters {Some(value)=>curve_parameters(value)?,None=>hint.ok_or(Error::BadKey)?};
    if hint.is_some_and(|hint|hint!=curve) {return Err(Error::BadKey);}
    macro_rules! import {
        ($curve:ty,$variant:ident)=>{{
            let key=ecdsa::SigningKey::<$curve>::from_slice(secret).map_err(bad)?;
            if let Some(public)=public {
                let public=ecdsa::VerifyingKey::<$curve>::from_sec1_bytes(public).map_err(bad)?;
                if &public!=key.verifying_key(){return Err(Error::BadKey);}
            }
            Ok(SignKey::$variant(key))
        }};
    }
    match curve {
        #[cfg(feature="ecdsa256")]
        Curve::P256=>import!(p256::NistP256,ECDSA256),
        #[cfg(feature="ecdsa384")]
        Curve::P384=>import!(p384::NistP384,ECDSA384),
        #[cfg(feature="ecdsa521")]
        Curve::P521=>import!(p521::NistP521,ECDSA521),
        #[allow(unreachable_patterns)]
        _=>Err(Error::NotAvailable{what:"private key curve"}),
    }
}

struct Domain<'a>{prime:&'a[u8],a:&'a[u8],b:&'a[u8],base:&'a[u8],order:&'a[u8],cofactor:u64}
fn domain(bytes:&[u8])->der::Result<Domain<'_>> {
    let mut reader=SliceReader::new(bytes)?;
    let result=reader.sequence(|sequence|{
        if sequence.decode::<u8>()?!=1{return Err(Tag::Integer.value_error());}
        let prime=sequence.sequence(|field|{
            if field.decode::<ObjectIdentifier>()?!=PRIME_FIELD{return Err(Tag::ObjectIdentifier.value_error());}
            Ok(field.decode::<UintRef<'_>>()?.as_bytes())
        })?;
        let(a,b)=sequence.sequence(|curve|{
            let a:OctetStringRef<'_>=curve.decode()?;let b:OctetStringRef<'_>=curve.decode()?;
            if !curve.is_finished(){curve.decode::<BitStringRef<'_>>()?;}
            Ok((a.as_bytes(),b.as_bytes()))
        })?;
        let base:OctetStringRef<'_>=sequence.decode()?;let order:UintRef<'_>=sequence.decode()?;
        let cofactor=if sequence.is_finished(){1}else{sequence.decode::<u64>()?};
        Ok(Domain{prime,a,b,base:base.as_bytes(),order:order.as_bytes(),cofactor})
    })?;
    reader.finish(result)
}
fn unsigned(bytes:&[u8])->&[u8]{let n=bytes.iter().position(|b|*b!=0).unwrap_or(bytes.len());&bytes[n..]}
fn same_domain(a:&Domain<'_>,b:&Domain<'_>)->bool {
    unsigned(a.prime)==unsigned(b.prime) && unsigned(a.a)==unsigned(b.a)
        && unsigned(a.b)==unsigned(b.b) && unsigned(a.order)==unsigned(b.order) && a.cofactor==b.cofactor
}
fn curve_parameters(parameters:AnyRef<'_>)->Result<Curve>{
    if parameters.tag()==Tag::ObjectIdentifier {
        return Curve::named(ObjectIdentifier::from_bytes(parameters.value()).map_err(bad)?);
    }
    if parameters.tag()!=Tag::Sequence{return Err(Error::BadKey);}
    // Re-encode this public value only; secret scalar bytes are never copied here.
    use der::Encode;
    let encoded=parameters.to_der().map_err(bad)?;let supplied=domain(&encoded).map_err(bad)?;
    macro_rules! matches {
        ($constant:ident,$curve:ty,$variant:ident)=>{{
            let expected=domain(domains::$constant).map_err(bad)?;
            if same_domain(&supplied,&expected) {
                let supplied_base=ecdsa::VerifyingKey::<$curve>::from_sec1_bytes(supplied.base).map_err(bad)?;
                let expected_base=ecdsa::VerifyingKey::<$curve>::from_sec1_bytes(expected.base).map_err(bad)?;
                if supplied_base==expected_base {return Ok(Curve::$variant);}
            }
        }};
    }
    #[cfg(feature="ecdsa256")]
    matches!(NISTP256,p256::NistP256,P256);
    #[cfg(feature="ecdsa384")]
    matches!(NISTP384,p384::NistP384,P384);
    #[cfg(feature="ecdsa521")]
    matches!(NISTP521,p521::NistP521,P521);
    Err(Error::BadKey)
}

#[cfg(test)]
#[path = "key_import_tests.rs"]
mod tests;
