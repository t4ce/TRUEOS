extern crate alloc;

use alloc::{string::String, vec::Vec};

use aes::cipher::{
    BlockDecrypt, BlockSizeUser, KeyInit, KeyIvInit, StreamCipher, generic_array::GenericArray,
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha1::{Digest, Sha1};

use crate::r::spotify_discovery::DEVICE_ID;

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;
type HmacSha1 = Hmac<Sha1>;

const DH_BYTES: usize = 96;
const DH_LIMBS: usize = DH_BYTES / 4;
const PRIVATE_BYTES: usize = 95;

const DH_PRIME_BE: [u8; DH_BYTES] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc9, 0x0f, 0xda, 0xa2, 0x21, 0x68, 0xc2, 0x34,
    0xc4, 0xc6, 0x62, 0x8b, 0x80, 0xdc, 0x1c, 0xd1, 0x29, 0x02, 0x4e, 0x08, 0x8a, 0x67, 0xcc, 0x74,
    0x02, 0x0b, 0xbe, 0xa6, 0x3b, 0x13, 0x9b, 0x22, 0x51, 0x4a, 0x08, 0x79, 0x8e, 0x34, 0x04, 0xdd,
    0xef, 0x95, 0x19, 0xb3, 0xcd, 0x3a, 0x43, 0x1b, 0x30, 0x2b, 0x0a, 0x6d, 0xf2, 0x5f, 0x14, 0x37,
    0x4f, 0xe1, 0x35, 0x6d, 0x6d, 0x51, 0xc2, 0x45, 0xe4, 0x85, 0xb5, 0x76, 0x62, 0x5e, 0x7e, 0xc6,
    0xf4, 0x4c, 0x42, 0xe9, 0xa6, 0x3a, 0x36, 0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

#[derive(Clone, Copy)]
struct DhInt {
    limbs: [u32; DH_LIMBS],
}

impl DhInt {
    const fn zero() -> Self {
        Self {
            limbs: [0; DH_LIMBS],
        }
    }

    const fn one() -> Self {
        let mut limbs = [0; DH_LIMBS];
        limbs[0] = 1;
        Self { limbs }
    }

    const fn two() -> Self {
        let mut limbs = [0; DH_LIMBS];
        limbs[0] = 2;
        Self { limbs }
    }

    fn prime() -> Self {
        Self::from_be(&DH_PRIME_BE)
    }

    fn from_be(bytes: &[u8]) -> Self {
        let mut out = [0u32; DH_LIMBS];
        let mut limb_idx = 0usize;
        let mut i = bytes.len();
        while i > 0 && limb_idx < DH_LIMBS {
            let start = i.saturating_sub(4);
            let mut limb = 0u32;
            for &byte in &bytes[start..i] {
                limb = (limb << 8) | byte as u32;
            }
            out[limb_idx] = limb;
            limb_idx += 1;
            i = start;
        }
        Self { limbs: out }
    }

    fn to_be_vec(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DH_BYTES);
        let mut started = false;
        for limb in self.limbs.iter().rev() {
            let bytes = limb.to_be_bytes();
            for byte in bytes {
                if byte != 0 || started {
                    started = true;
                    out.push(byte);
                }
            }
        }
        if out.is_empty() {
            out.push(0);
        }
        out
    }

    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        for idx in (0..DH_LIMBS).rev() {
            if self.limbs[idx] < other.limbs[idx] {
                return core::cmp::Ordering::Less;
            }
            if self.limbs[idx] > other.limbs[idx] {
                return core::cmp::Ordering::Greater;
            }
        }
        core::cmp::Ordering::Equal
    }

    fn sub_assign_wrapping(&mut self, other: &Self) {
        let mut borrow = 0u64;
        for idx in 0..DH_LIMBS {
            let lhs = self.limbs[idx] as u64;
            let rhs = other.limbs[idx] as u64 + borrow;
            self.limbs[idx] = lhs.wrapping_sub(rhs) as u32;
            borrow = (lhs < rhs) as u64;
        }
    }

    fn add_mod(&self, other: &Self, modulus: &Self) -> Self {
        let mut out = Self::zero();
        let mut carry = 0u64;
        for idx in 0..DH_LIMBS {
            let sum = self.limbs[idx] as u64 + other.limbs[idx] as u64 + carry;
            out.limbs[idx] = sum as u32;
            carry = sum >> 32;
        }

        if carry != 0 || out.cmp(modulus) != core::cmp::Ordering::Less {
            out.sub_assign_wrapping(modulus);
        }
        out
    }

    fn double_mod(&self, modulus: &Self) -> Self {
        self.add_mod(self, modulus)
    }

    fn bit(&self, bit: usize) -> bool {
        let limb = bit / 32;
        if limb >= DH_LIMBS {
            return false;
        }
        ((self.limbs[limb] >> (bit % 32)) & 1) != 0
    }

    fn mul_mod(&self, other: &Self, modulus: &Self) -> Self {
        let mut result = Self::zero();
        let mut base = *self;
        for bit in 0..(DH_BYTES * 8) {
            if other.bit(bit) {
                result = result.add_mod(&base, modulus);
            }
            base = base.double_mod(modulus);
        }
        result
    }

    fn pow_mod_le(base: &Self, exponent_le: &[u8], modulus: &Self) -> Self {
        let mut result = Self::one();
        let mut power = *base;
        for byte in exponent_le {
            for bit in 0..8 {
                if ((byte >> bit) & 1) != 0 {
                    result = result.mul_mod(&power, modulus);
                }
                power = power.mul_mod(&power, modulus);
            }
        }
        result
    }
}

pub struct AddUserResult {
    pub username_len: usize,
    pub encrypted_len: usize,
    pub decrypted_len: usize,
    pub credential: SpotifyCredential,
}

#[derive(Clone)]
pub struct SpotifyCredential {
    pub username: String,
    pub auth_type: u32,
    pub auth_data: Vec<u8>,
}

pub struct SpotifyZeroconf {
    private_key_le: [u8; PRIVATE_BYTES],
    public_key_b64: String,
    active_user: String,
    credential_blob: Vec<u8>,
}

impl SpotifyZeroconf {
    pub fn new() -> Self {
        let mut private_key_le = [0u8; PRIVATE_BYTES];
        if !crate::tyche::fill_bytes(&mut private_key_le) {
            let mut seed = crate::chronos::monotonic_nanos().to_le_bytes();
            for (idx, byte) in private_key_le.iter_mut().enumerate() {
                *byte = seed[idx % seed.len()].wrapping_add((idx as u8).wrapping_mul(37));
                if idx % seed.len() == seed.len() - 1 {
                    let digest = Sha1::digest(seed);
                    seed.copy_from_slice(&digest[..8]);
                }
            }
        }

        let modulus = DhInt::prime();
        let public_key = DhInt::pow_mod_le(&DhInt::two(), &private_key_le, &modulus).to_be_vec();
        let public_key_b64 = base64::engine::general_purpose::STANDARD.encode(public_key);

        Self {
            private_key_le,
            public_key_b64,
            active_user: String::new(),
            credential_blob: Vec::new(),
        }
    }

    pub fn public_key_b64(&self) -> &str {
        self.public_key_b64.as_str()
    }

    pub fn active_user(&self) -> &str {
        self.active_user.as_str()
    }

    pub fn add_user(&mut self, form: &str) -> Result<AddUserResult, &'static str> {
        let username = form_value(form, "userName").ok_or("missing-userName")?;
        let blob = form_value(form, "blob").ok_or("missing-blob")?;
        let client_key = form_value(form, "clientKey").ok_or("missing-clientKey")?;

        let username = decode_form_value(username)?;
        let blob = decode_form_base64_value(blob)?;
        let client_key = decode_form_base64_value(client_key)?;

        let encrypted_blob = base64::engine::general_purpose::STANDARD
            .decode(blob.as_slice())
            .map_err(|_| "blob-base64")?;
        if encrypted_blob.len() < 36 {
            return Err("blob-too-small");
        }

        let client_key = base64::engine::general_purpose::STANDARD
            .decode(client_key.as_slice())
            .map_err(|_| "client-key-base64")?;
        crate::log!(
            "spotify-zeroconf: addUser decoded encrypted_blob_len={} client_key_len={}\n",
            encrypted_blob.len(),
            client_key.len()
        );
        let shared_key = self.shared_secret(client_key.as_slice());
        let base_key = Sha1::digest(shared_key);
        let base_key = &base_key[..16];

        let checksum_key = hmac_sha1(base_key, b"checksum")?;
        let encryption_key = hmac_sha1(base_key, b"encryption")?;

        let encrypted_len = encrypted_blob.len();
        let iv = &encrypted_blob[0..16];
        let encrypted = &encrypted_blob[16..encrypted_len - 20];
        let checksum = &encrypted_blob[encrypted_len - 20..encrypted_len];

        let mut mac =
            <HmacSha1 as Mac>::new_from_slice(checksum_key.as_slice()).map_err(|_| "hmac-key")?;
        mac.update(encrypted);
        mac.verify_slice(checksum).map_err(|_| "blob-mac")?;

        let mut decrypted = encrypted.to_vec();
        let mut cipher =
            Aes128Ctr::new_from_slices(&encryption_key[0..16], iv).map_err(|_| "aes")?;
        cipher.apply_keystream(decrypted.as_mut_slice());

        let username = String::from_utf8(username).map_err(|_| "username-utf8")?;
        let credential = parse_spotify_credential(username.as_str(), decrypted.as_slice())?;

        self.active_user = username;
        self.credential_blob.clear();
        self.credential_blob.extend_from_slice(decrypted.as_slice());

        crate::log!(
            "spotify-zeroconf: credentials decrypted user_len={} blob_len={} auth_type={} auth_data_len={} device_id={}\n",
            self.active_user.len(),
            self.credential_blob.len(),
            credential.auth_type,
            credential.auth_data.len(),
            DEVICE_ID
        );

        Ok(AddUserResult {
            username_len: self.active_user.len(),
            encrypted_len,
            decrypted_len: self.credential_blob.len(),
            credential,
        })
    }

    fn shared_secret(&self, remote_key_be: &[u8]) -> Vec<u8> {
        let modulus = DhInt::prime();
        let remote = DhInt::from_be(remote_key_be);
        DhInt::pow_mod_le(&remote, &self.private_key_le, &modulus).to_be_vec()
    }
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut mac = <HmacSha1 as Mac>::new_from_slice(key).map_err(|_| "hmac-key")?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn parse_spotify_credential(
    username: &str,
    encrypted_blob: &[u8],
) -> Result<SpotifyCredential, &'static str> {
    let mut blob = base64::engine::general_purpose::STANDARD
        .decode(encrypted_blob)
        .map_err(|_| "credential-blob-base64")?;
    if blob.len() < 16 || blob.len() % 16 != 0 {
        return Err("credential-blob-size");
    }

    let mut key = [0u8; 24];
    let secret = Sha1::digest(DEVICE_ID.as_bytes());
    pbkdf2_hmac_sha1(secret.as_slice(), username.as_bytes(), 0x100, &mut key[0..20])?;
    let hash = Sha1::digest(&key[..20]);
    key[..20].copy_from_slice(hash.as_slice());
    key[20..].copy_from_slice(&20u32.to_be_bytes());

    let cipher = aes::Aes192::new(GenericArray::from_slice(&key));
    let block_size = aes::Aes192::block_size();
    for chunk in blob.chunks_exact_mut(block_size) {
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
    }

    let len = blob.len();
    for idx in 0..len - 0x10 {
        blob[len - idx - 1] ^= blob[len - idx - 0x11];
    }

    let mut cursor = BlobCursor::new(blob.as_slice());
    cursor.read_u8()?;
    cursor.read_bytes()?;
    cursor.read_u8()?;
    let auth_type = cursor.read_int()?;
    cursor.read_u8()?;
    let auth_data = cursor.read_bytes()?;

    Ok(SpotifyCredential {
        username: String::from(username),
        auth_type,
        auth_data,
    })
}

fn pbkdf2_hmac_sha1(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    out: &mut [u8],
) -> Result<(), &'static str> {
    if iterations == 0 {
        return Err("pbkdf2-iterations");
    }

    let mut block_index = 1u32;
    let mut offset = 0usize;
    while offset < out.len() {
        let mut mac = <HmacSha1 as Mac>::new_from_slice(password).map_err(|_| "pbkdf2-key")?;
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let mut u = mac.finalize().into_bytes();
        let mut t = u;

        for _ in 1..iterations {
            let mut mac = <HmacSha1 as Mac>::new_from_slice(password).map_err(|_| "pbkdf2-key")?;
            mac.update(u.as_slice());
            u = mac.finalize().into_bytes();
            for (dst, src) in t.iter_mut().zip(u.iter()) {
                *dst ^= *src;
            }
        }

        let take = (out.len() - offset).min(t.len());
        out[offset..offset + take].copy_from_slice(&t[..take]);
        offset += take;
        block_index = block_index.checked_add(1).ok_or("pbkdf2-block")?;
    }

    Ok(())
}

struct BlobCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BlobCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, &'static str> {
        let byte = *self.bytes.get(self.pos).ok_or("blob-eof")?;
        self.pos += 1;
        Ok(byte)
    }

    fn read_int(&mut self) -> Result<u32, &'static str> {
        let lo = self.read_u8()? as u32;
        if lo & 0x80 == 0 {
            return Ok(lo);
        }
        let hi = self.read_u8()? as u32;
        Ok((lo & 0x7f) | (hi << 7))
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, &'static str> {
        let len = self.read_int()? as usize;
        let end = self.pos.checked_add(len).ok_or("blob-len")?;
        let slice = self.bytes.get(self.pos..end).ok_or("blob-eof")?;
        self.pos = end;
        Ok(slice.to_vec())
    }
}

fn form_value<'a>(form: &'a str, key: &str) -> Option<&'a str> {
    for part in form.split('&') {
        let Some((part_key, value)) = part.split_once('=') else {
            continue;
        };
        if part_key == key {
            return Some(value);
        }
    }
    None
}

fn decode_form_base64_value(value: &str) -> Result<Vec<u8>, &'static str> {
    decode_form_value_with_plus(value, true)
}

fn decode_form_value(value: &str) -> Result<Vec<u8>, &'static str> {
    decode_form_value_with_plus(value, false)
}

fn decode_form_value_with_plus(value: &str, preserve_plus: bool) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(if preserve_plus { b'+' } else { b' ' });
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_value(bytes[i + 1]).ok_or("bad-percent")?;
                let lo = hex_value(bytes[i + 2]).ok_or("bad-percent")?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'%' => return Err("bad-percent"),
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
