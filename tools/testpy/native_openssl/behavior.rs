use openssl::{
    asn1::Asn1Time,
    bn::{BigNum, BigNumContext},
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    rsa::Rsa,
    sign::Signer,
    x509::{X509, X509NameBuilder, extension::{BasicConstraints, KeyUsage}},
};
use webauthn_rs_core::{crypto, proto::*};
use webauthn_attestation_ca::AttestationCaList;
use std::ffi::{c_char,c_int,c_void};

fn ec_roundtrip() -> PKey<Private> {
    let group=EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let ec=EcKey::generate(&group).unwrap();
    let mut x=BigNum::new().unwrap();let mut y=BigNum::new().unwrap();
    ec.public_key().affine_coordinates_gfp(&group,&mut x,&mut y,&mut BigNumContext::new().unwrap()).unwrap();
    let cose=COSEKey {type_:COSEAlgorithm::ES256,key:COSEKeyType::EC_EC2(COSEEC2Key {
        curve:ECDSACurve::SECP256R1,x:x.to_vec_padded(32).unwrap().into(),y:y.to_vec_padded(32).unwrap().into()})};
    let key=PKey::from_ec_key(ec).unwrap();
    let signature=Signer::new(MessageDigest::sha256(),&key).unwrap().sign_oneshot_to_vec(b"native WebAuthn assertion").unwrap();
    assert!(cose.verify_signature(&signature,b"native WebAuthn assertion").unwrap());
    assert!(!cose.verify_signature(&signature,b"altered WebAuthn assertion").unwrap());
    key
}

fn certificate(key:&PKey<Private>,issuer:Option<(&X509,&PKey<Private>)>,ca:bool,expired:bool)->X509 {
    let mut name=X509NameBuilder::new().unwrap();name.append_entry_by_text("CN",if ca{"native fixture CA"}else{"native fixture device"}).unwrap();let name=name.build();
    let mut cert=X509::builder().unwrap();cert.set_version(2).unwrap();
    let serial=BigNum::from_u32(if ca{1}else{2}).unwrap().to_asn1_integer().unwrap();cert.set_serial_number(&serial).unwrap();
    cert.set_subject_name(&name).unwrap();cert.set_issuer_name(issuer.map_or(&*name,|(cert,_)|cert.subject_name())).unwrap();cert.set_pubkey(key).unwrap();
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()as i64;
    cert.set_not_before(&Asn1Time::from_unix(now-86400).unwrap()).unwrap();
    cert.set_not_after(&Asn1Time::from_unix(if expired{now-3600}else{now+86400}).unwrap()).unwrap();
    let mut constraints=BasicConstraints::new();constraints.critical();if ca{constraints.ca();}cert.append_extension(constraints.build().unwrap()).unwrap();
    let mut usage=KeyUsage::new();usage.digital_signature();if ca{usage.key_cert_sign();}cert.append_extension(usage.build().unwrap()).unwrap();
    cert.sign(issuer.map_or(key,|(_,key)|key),MessageDigest::sha256()).unwrap();cert.build()
}

fn main(){
    // Race the first use and repeat thread exit: initialization, RCU state and
    // OpenSSL error/TLS cleanup all run through the private native boundary.
    let workers:Vec<_>=(0..4).map(|_|std::thread::spawn(||{
        for _ in 0..3 {ec_roundtrip();let mut bytes=[0u8;32];openssl::rand::rand_bytes(&mut bytes).unwrap();assert!(bytes.iter().any(|&b|b!=0));}
    })).collect();
    for worker in workers{worker.join().unwrap();}
    println!("ok concurrent native initialization, RCU, entropy and thread cleanup");
    let key=ec_roundtrip();println!("ok actual WebAuthn ES256 COSE verification and tamper rejection");
    let rsa=Rsa::generate(2048).unwrap();let cose=COSEKey{type_:COSEAlgorithm::RS256,key:COSEKeyType::RSA(COSERSAKey{n:rsa.n().to_vec().into(),e:[1,0,1]})};let rsa=PKey::from_rsa(rsa).unwrap();
    let signature=Signer::new(MessageDigest::sha256(),&rsa).unwrap().sign_oneshot_to_vec(b"native RSA").unwrap();
    assert!(cose.verify_signature(&signature,b"native RSA").unwrap());assert!(!cose.verify_signature(&signature,b"changed RSA").unwrap());
    println!("ok actual WebAuthn RSA COSE verification and tamper rejection");
    let ed=PKey::generate_ed25519().unwrap();let cose=COSEKey{type_:COSEAlgorithm::EDDSA,key:COSEKeyType::EC_OKP(COSEOKPKey{curve:EDDSACurve::ED25519,x:ed.raw_public_key().unwrap().into()})};
    let signature=Signer::new_without_digest(&ed).unwrap().sign_oneshot_to_vec(b"native Ed25519").unwrap();assert!(cose.verify_signature(&signature,b"native Ed25519").unwrap());assert!(!cose.verify_signature(&signature,b"changed Ed25519").unwrap());
    println!("ok actual WebAuthn Ed25519 COSE verification and tamper rejection");
    let ca_key=ec_roundtrip();let root=certificate(&ca_key,None,true,false);
    let leaf=certificate(&key,Some((&root,&ca_key)),false,false);
    let leaf=X509::from_der(&leaf.to_der().unwrap()).unwrap();let root=X509::from_pem(&root.to_pem().unwrap()).unwrap();
    let cas=AttestationCaList::try_from(root.to_pem().unwrap().as_slice()).unwrap();
    let chain=ParsedAttestationData::Basic(vec![leaf.clone(),root.clone()]);
    assert!(webauthn_rs_core::attestation::verify_attestation_ca_chain(&chain,&cas,false).unwrap().is_some());
    assert!(crypto::verify_signature(COSEAlgorithm::ES256,&leaf,&Signer::new(MessageDigest::sha256(),&key).unwrap().sign_oneshot_to_vec(b"certificate assertion").unwrap(),b"certificate assertion").unwrap());
    let expired=certificate(&key,Some((&root,&ca_key)),false,true);let chain=ParsedAttestationData::Basic(vec![expired,root]);
    assert!(webauthn_rs_core::attestation::verify_attestation_ca_chain(&chain,&cas,false).is_err());
    assert!(X509::from_der(b"invalid DER fixture").is_err());
    println!("ok actual WebAuthn memory attestation chain, native UTC expiry and error stack");
    assert_eq!(crypto::compute_sha256(b"abc"),[0xba,0x78,0x16,0xbf,0x8f,0x01,0xcf,0xea,0x41,0x41,0x40,0xde,0x5d,0xae,0x22,0x23,0xb0,0x03,0x61,0xa3,0x96,0x17,0x7a,0x9c,0xb4,0x10,0xff,0x61,0xf2,0x00,0x15,0xad]);
    assert_eq!(Asn1Time::from_unix(1_234_567_890).unwrap().to_string(),"Feb 13 23:31:30 2009 GMT");
    unsafe extern "C" {
        fn CRYPTO_aligned_alloc(size:usize,alignment:usize,free:*mut *mut c_void,file:*const c_char,line:c_int)->*mut c_void;
        fn OSSL_STORE_open(uri:*const c_char,ui:*mut c_void,ui_data:*mut c_void,post:*mut c_void,post_data:*mut c_void)->*mut c_void;
    }
    unsafe {
        let mut original=std::ptr::null_mut();let aligned=CRYPTO_aligned_alloc(733,4096,&mut original,c"fixture".as_ptr(),1);
        assert!(!aligned.is_null());assert_eq!(aligned as usize%4096,0);assert!(!original.is_null());
        openssl_sys::CRYPTO_free(original,c"fixture".as_ptr(),1);
        assert!(OSSL_STORE_open(c"file:/native-provider-fixture.pem".as_ptr(),std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut()).is_null());
        assert_ne!(openssl_sys::ERR_get_error(),0);
    }
    println!("ok native SHA/UTC, generic aligned allocation and explicit unavailable file URI");
}
