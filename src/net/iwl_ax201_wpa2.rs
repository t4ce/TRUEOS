//! WPA2-PSK/CCMP station key exchange and software CCMP boundary.
//!
//! Supports RSN descriptor version 2 (HMAC-SHA1-128/AES key wrap), PSK AKM,
//! CCMP-128, and ordinary unfragmented Data/QoS frames. WPA/TKIP, SAE, PMF,
//! pairwise rekey require a fresh, explicit station lifecycle. RSN group-key
//! updates retain the installed pairwise key and never reinstall a known GTK.
//! Cryptographic building blocks are RustCrypto HMAC/SHA1/AES; wire contracts
//! follow IEEE 802.11i and hostap wpa_supplicant/wpa.c. AES wrap is RFC 3394,
//! CCM formatting RFC 3610. Authentication/replay failures never release data.

use aes::Aes128;
use alloc::{vec, vec::Vec};
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use core::sync::atomic::{Ordering, compiler_fence};
use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;
const HEADER: usize = 99;
const MIC: core::ops::Range<usize> = 81..97;
const PAIRWISE: u16 = 1 << 3;
const INSTALL: u16 = 1 << 6;
const ACK: u16 = 1 << 7;
const HAS_MIC: u16 = 1 << 8;
const SECURE: u16 = 1 << 9;
const ENCRYPTED: u16 = 1 << 12;
const PN_MAX: u64 = (1u64 << 48) - 1;

fn erase(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}
fn equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut mac = <HmacSha1 as Mac>::new_from_slice(key).unwrap();
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}
fn valid_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}

/// WPA's PBKDF2-HMAC-SHA1, 4096 iterations; also admits a 64-hex raw PSK.
pub(crate) fn derive_pmk(ssid: &[u8], password: &[u8]) -> Result<[u8; 32], &'static str> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("WPA2 invalid SSID length");
    }
    if password.len() == 64 {
        let mut key = [0; 32];
        for (at, pair) in password.chunks_exact(2).enumerate() {
            let digit = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            key[at] = (digit(pair[0]).ok_or("WPA2 raw PSK must be hexadecimal")? << 4)
                | digit(pair[1]).ok_or("WPA2 raw PSK must be hexadecimal")?;
        }
        return Ok(key);
    }
    if !(8..=63).contains(&password.len()) {
        return Err("WPA2 passphrase must be 8..63 bytes");
    }
    let mut output = [0; 32];
    for block in 1u32..=2 {
        let mut u = hmac(password, &[ssid, &block.to_be_bytes()]);
        let mut combined = u;
        for _ in 1..4096 {
            u = hmac(password, &[&u]);
            for (dst, src) in combined.iter_mut().zip(u) {
                *dst ^= src;
            }
        }
        let start = (block as usize - 1) * 20;
        let count = (32 - start).min(20);
        output[start..start + count].copy_from_slice(&combined[..count]);
        erase(&mut u);
        erase(&mut combined);
    }
    Ok(output)
}
fn derive_ptk(
    pmk: &[u8; 32],
    station: [u8; 6],
    bssid: [u8; 6],
    snonce: &[u8; 32],
    anonce: &[u8; 32],
) -> [u8; 48] {
    let (first_mac, last_mac) = if station < bssid {
        (station, bssid)
    } else {
        (bssid, station)
    };
    let (first_nonce, last_nonce) = if snonce < anonce {
        (snonce, anonce)
    } else {
        (anonce, snonce)
    };
    let mut out = [0; 48];
    for index in 0..3u8 {
        let mut digest = hmac(
            pmk,
            &[
                b"Pairwise key expansion",
                &[0],
                &first_mac,
                &last_mac,
                first_nonce,
                last_nonce,
                &[index],
            ],
        );
        let at = usize::from(index) * 20;
        let count = (48 - at).min(20);
        out[at..at + count].copy_from_slice(&digest[..count]);
        erase(&mut digest);
    }
    out
}

pub(crate) struct GroupKey {
    pub id: u8,
    pub temporal: [u8; 16],
    pub receive_pn: u64,
}
impl Drop for GroupKey {
    fn drop(&mut self) {
        erase(&mut self.temporal);
    }
}
pub(crate) struct Keys {
    pub temporal: [u8; 16],
    pub gtk: GroupKey,
}
impl Drop for Keys {
    fn drop(&mut self) {
        erase(&mut self.temporal);
    }
}
pub(crate) struct HandshakeAction {
    /// EAPOL packet only; caller supplies Ethernet/LLC framing and sends it.
    pub reply: Vec<u8>,
    /// Returned exactly once. Retransmitted M3 must not reinstall these keys.
    pub keys: Option<Keys>,
    /// A new GTK only. An authenticated retry must preserve every installed PN.
    pub group_key: Option<GroupKey>,
}

struct KeyPacket<'a> {
    bytes: &'a [u8],
    info: u16,
    replay: u64,
    nonce: &'a [u8],
    data: &'a [u8],
}
impl<'a> KeyPacket<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() < HEADER
            || bytes.len() > 4096
            || !matches!(bytes[0], 1 | 2 | 3)
            || bytes[1] != 3
            || bytes[4] != 2
        {
            return Err("WPA2 unsupported EAPOL packet");
        }
        let body = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        let data = usize::from(u16::from_be_bytes([bytes[97], bytes[98]]));
        if body + 4 != bytes.len() || HEADER + data != bytes.len() {
            return Err("WPA2 truncated EAPOL key data");
        }
        let info = u16::from_be_bytes([bytes[5], bytes[6]]);
        if info & 7 != 2
            || info & ((3 << 4) | (1 << 10) | (1 << 11) | (1 << 13) | (3 << 14)) != 0
        {
            return Err("WPA2 unsupported key descriptor flags");
        }
        // RSN group-key messages use zero here; the authenticated GTK KDE
        // supplies the CCMP key length. Pairwise messages retain their contract.
        let key_length = u16::from_be_bytes([bytes[7], bytes[8]]);
        if key_length != if info & PAIRWISE != 0 { 16 } else { 0 } {
            return Err("WPA2 requires CCMP-128");
        }
        Ok(Self {
            bytes,
            info,
            replay: u64::from_be_bytes(bytes[9..17].try_into().unwrap()),
            nonce: &bytes[17..49],
            data: &bytes[HEADER..],
        })
    }
}

pub(crate) struct Session {
    pmk: [u8; 32],
    ptk: [u8; 48],
    snonce: [u8; 32],
    anonce: [u8; 32],
    station: [u8; 6],
    bssid: [u8; 6],
    station_rsn: Vec<u8>,
    ap_rsn: Vec<u8>,
    m1_replay: Option<u64>,
    last_m3: Option<u64>,
    installed: bool,
    installed_gtk: Option<GroupKey>,
    group_keys: [Option<GroupKey>; 4],
    last_authenticated_replay: Option<u64>,
    last_group_mic: Option<[u8; 16]>,
}
impl Session {
    pub fn new(
        ssid: &[u8],
        password: &[u8],
        station: [u8; 6],
        bssid: [u8; 6],
        snonce: [u8; 32],
        rsn_ie: &[u8],
    ) -> Result<Self, &'static str> {
        Self::with_ies(ssid, password, station, bssid, snonce, rsn_ie, rsn_ie)
    }
    pub fn with_ies(
        ssid: &[u8],
        password: &[u8],
        station: [u8; 6],
        bssid: [u8; 6],
        snonce: [u8; 32],
        station_rsn: &[u8],
        ap_rsn: &[u8],
    ) -> Result<Self, &'static str> {
        if !valid_mac(station) || !valid_mac(bssid) || station == bssid || snonce == [0; 32] {
            return Err("WPA2 invalid station identity or nonce");
        }
        validate_station_rsn(station_rsn)?;
        if ap_rsn.len() < 4
            || ap_rsn.len() > 257
            || ap_rsn[0] != 48
            || usize::from(ap_rsn[1]) + 2 != ap_rsn.len()
        {
            return Err("WPA2 invalid AP RSN IE");
        }
        Ok(Self {
            pmk: derive_pmk(ssid, password)?,
            ptk: [0; 48],
            snonce,
            anonce: [0; 32],
            station,
            bssid,
            station_rsn: station_rsn.to_vec(),
            ap_rsn: ap_rsn.to_vec(),
            m1_replay: None,
            last_m3: None,
            installed: false,
            installed_gtk: None,
            group_keys: core::array::from_fn(|_| None),
            last_authenticated_replay: None,
            last_group_mic: None,
        })
    }
    pub fn established(&self) -> bool {
        self.installed
    }
    pub fn handle_eapol(&mut self, bytes: &[u8]) -> Result<HandshakeAction, &'static str> {
        let packet = KeyPacket::parse(bytes)?;
        if packet.info & PAIRWISE == 0 {
            return self.handle_group_key(packet);
        }
        if packet.info & ACK == 0 {
            return Err("WPA2 expected authenticator ACK");
        }
        if packet.info & HAS_MIC == 0 {
            if packet.info & (INSTALL | SECURE | ENCRYPTED) != 0 || packet.nonce == [0; 32] {
                return Err("WPA2 invalid message 1");
            }
            if self.installed {
                return Err("WPA2 pairwise rekey requires reassociation");
            }
            if let Some(previous) = self.m1_replay {
                if packet.replay < previous
                    || packet.replay == previous && !equal(packet.nonce, &self.anonce)
                {
                    return Err("WPA2 replayed message 1");
                }
            }
            self.anonce.copy_from_slice(packet.nonce);
            self.m1_replay = Some(packet.replay);
            self.ptk = derive_ptk(
                &self.pmk,
                self.station,
                self.bssid,
                &self.snonce,
                &self.anonce,
            );
            let reply = self.reply(
                packet.bytes[0],
                2 | PAIRWISE | HAS_MIC,
                packet.replay,
                &self.snonce,
                &self.station_rsn,
            );
            return Ok(HandshakeAction { reply, keys: None, group_key: None });
        }
        let m1 = self.m1_replay.ok_or("WPA2 message 3 before message 1")?;
        if packet.info & (INSTALL | SECURE | ENCRYPTED) != (INSTALL | SECURE | ENCRYPTED)
            || !equal(packet.nonce, &self.anonce)
            || packet.replay <= m1
        {
            return Err("WPA2 invalid message 3");
        }
        if self
            .last_m3
            .is_some_and(|previous| packet.replay < previous)
            || self.last_authenticated_replay.is_some_and(|previous| {
                packet.replay < previous
                    || packet.replay == previous && self.last_group_mic.is_some()
            })
        {
            return Err("WPA2 replayed message 3");
        }
        self.verify_mic(&packet)?;
        let mut plain = unwrap_key(self.ptk[16..32].try_into().unwrap(), packet.data)?;
        let parsed = parse_key_data(&plain, &self.ap_rsn, true);
        erase(&mut plain);
        let mut gtk = parsed?;
        let rsc = u64::from_le_bytes(packet.bytes[65..73].try_into().unwrap());
        if rsc > PN_MAX {
            return Err("WPA2 invalid group receive sequence counter");
        }
        gtk.receive_pn = rsc;
        let keys = if self.installed {
            let previous = self
                .installed_gtk
                .as_ref()
                .ok_or("WPA2 missing installed GTK")?;
            if previous.id != gtk.id || !equal(&previous.temporal, &gtk.temporal) {
                return Err("WPA2 key replacement requires reassociation");
            }
            None
        } else {
            let temporal = self.ptk[32..48].try_into().unwrap();
            self.installed_gtk = Some(GroupKey {
                id: gtk.id,
                temporal: gtk.temporal,
                receive_pn: gtk.receive_pn,
            });
            self.group_keys[usize::from(gtk.id)] = Some(GroupKey {
                id: gtk.id,
                temporal: gtk.temporal,
                receive_pn: gtk.receive_pn,
            });
            self.installed = true;
            Some(Keys { temporal, gtk })
        };
        self.last_m3 = Some(packet.replay);
        self.last_authenticated_replay = Some(packet.replay);
        self.last_group_mic = None;
        let reply = self.reply(
            packet.bytes[0],
            2 | PAIRWISE | HAS_MIC | SECURE,
            packet.replay,
            &[0; 32],
            &[],
        );
        Ok(HandshakeAction { reply, keys, group_key: None })
    }
    fn verify_mic(&self, packet: &KeyPacket<'_>) -> Result<(), &'static str> {
        let mut authenticated = packet.bytes.to_vec();
        authenticated[MIC].fill(0);
        let mut tag = hmac(&self.ptk[..16], &[&authenticated]);
        let valid = equal(&tag[..16], &packet.bytes[MIC]);
        erase(&mut tag);
        if valid { Ok(()) } else { Err("WPA2 EAPOL MIC mismatch") }
    }
    fn handle_group_key(&mut self, packet: KeyPacket<'_>) -> Result<HandshakeAction, &'static str> {
        // RSN/CCMP group M1: descriptor v2, !Pairwise, ACK, MIC, Secure,
        // encrypted GTK KDE. Pairwise INSTALL/nonce rules do not apply here.
        if packet.info != (2 | ACK | HAS_MIC | SECURE | ENCRYPTED) {
            return Err("WPA2 invalid group message 1 flags");
        }
        if !self.installed {
            return Err("WPA2 group message 1 before pairwise keys");
        }
        let previous = self.last_authenticated_replay.ok_or("WPA2 missing replay counter")?;
        if packet.replay < previous {
            return Err("WPA2 replayed group message 1");
        }
        self.verify_mic(&packet)?;
        if packet.replay == previous
            && !self.last_group_mic.as_ref().is_some_and(|mic| equal(mic, &packet.bytes[MIC]))
        {
            return Err("WPA2 changed group replay counter");
        }
        let mut plain = unwrap_key(self.ptk[16..32].try_into().unwrap(), packet.data)?;
        let parsed = parse_key_data(&plain, &self.ap_rsn, false);
        erase(&mut plain);
        let mut gtk = parsed?;
        let rsc = u64::from_le_bytes(packet.bytes[65..73].try_into().unwrap());
        if rsc > PN_MAX {
            return Err("WPA2 invalid group receive sequence counter");
        }
        gtk.receive_pn = rsc;
        let slot = usize::from(gtk.id);
        // hostap can increment the replay counter when retrying the same GTK.
        // Compare installed material in every key slot, not just the last G1:
        // ACK retries without rebuilding CcmpKey or lowering its receive PN.
        let known = self.group_keys[slot].as_ref()
            .is_some_and(|key| equal(&key.temporal, &gtk.temporal));
        if self.group_keys.iter().flatten()
            .any(|key| key.id != gtk.id && equal(&key.temporal, &gtk.temporal))
        {
            return Err("WPA2 GTK material reused at another key ID");
        }
        let group_key = if known {
            None
        } else {
            self.group_keys[slot] = Some(GroupKey {
                id: gtk.id, temporal: gtk.temporal, receive_pn: gtk.receive_pn,
            });
            Some(gtk)
        };
        self.last_authenticated_replay = Some(packet.replay);
        self.last_group_mic = Some(packet.bytes[MIC].try_into().unwrap());
        let reply = self.reply(packet.bytes[0], 2 | HAS_MIC | SECURE,
            packet.replay, &[0; 32], &[]);
        Ok(HandshakeAction { reply, keys: None, group_key })
    }
    fn reply(&self, version: u8, info: u16, replay: u64, nonce: &[u8; 32], data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; HEADER + data.len()];
        bytes[0] = version;
        bytes[1] = 3;
        bytes[4] = 2;
        let body_length = (bytes.len() - 4) as u16;
        bytes[2..4].copy_from_slice(&body_length.to_be_bytes());
        bytes[5..7].copy_from_slice(&info.to_be_bytes());
        bytes[9..17].copy_from_slice(&replay.to_be_bytes());
        bytes[17..49].copy_from_slice(nonce);
        bytes[97..99].copy_from_slice(&(data.len() as u16).to_be_bytes());
        bytes[HEADER..].copy_from_slice(data);
        let mut tag = hmac(&self.ptk[..16], &[&bytes]);
        bytes[MIC].copy_from_slice(&tag[..16]);
        erase(&mut tag);
        bytes
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        erase(&mut self.pmk);
        erase(&mut self.ptk);
        erase(&mut self.snonce);
        erase(&mut self.anonce);
    }
}

fn validate_station_rsn(ie: &[u8]) -> Result<(), &'static str> {
    // Exactly one pairwise cipher and one AKM, selected CCMP/PSK, no PMF.
    if ie.len() < 20
        || ie.len() > 22
        || ie[0] != 48
        || usize::from(ie[1]) + 2 != ie.len()
        || ie[2..4] != [1, 0]
        || ie[4..8] != [0, 0x0f, 0xac, 4]
        || ie[8..10] != [1, 0]
        || ie[10..14] != [0, 0x0f, 0xac, 4]
        || ie[14..16] != [1, 0]
        || ie[16..20] != [0, 0x0f, 0xac, 2]
    {
        return Err("WPA2 requires selected CCMP/PSK RSN IE");
    }
    if ie.len() == 22 && u16::from_le_bytes([ie[20], ie[21]]) & 0x00c0 != 0 {
        return Err("WPA2 PMF not supported");
    }
    Ok(())
}
fn parse_key_data(bytes: &[u8], expected_rsn: &[u8], require_rsn: bool) -> Result<GroupKey, &'static str> {
    let mut at = 0;
    let mut rsn = false;
    let mut gtk = None;
    while at < bytes.len() {
        if bytes[at..].iter().all(|&b| b == 0) || bytes[at] == 0xdd && bytes.get(at + 1) == Some(&0)
        {
            break;
        }
        let length = usize::from(*bytes.get(at + 1).ok_or("WPA2 truncated key IE")?);
        let end = at.checked_add(2 + length).ok_or("WPA2 key IE overflow")?;
        let ie = bytes.get(at..end).ok_or("WPA2 truncated key IE")?;
        if ie[0] == 48 {
            if rsn || !equal(ie, expected_rsn) {
                return Err("WPA2 AP RSN changed during handshake");
            }
            rsn = true;
        } else if ie[0] == 0xdd && ie.get(2..6) == Some(&[0, 0x0f, 0xac, 1][..]) {
            if gtk.is_some() || ie.len() != 24 || ie[6] & !7 != 0 || ie[7] != 0 {
                return Err("WPA2 malformed GTK KDE");
            }
            gtk = Some(GroupKey {
                id: ie[6] & 3,
                temporal: ie[8..24].try_into().unwrap(),
                receive_pn: 0,
            });
        }
        at = end;
    }
    if require_rsn && !rsn {
        return Err("WPA2 message 3 has no matching RSN IE");
    }
    gtk.ok_or("WPA2 key data has no GTK")
}

/// RFC 3394 section 2.2.2; unwrap integrity is verified before returning keys.
fn unwrap_key(kek: &[u8; 16], wrapped: &[u8]) -> Result<Vec<u8>, &'static str> {
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 || wrapped.len() > 2048 {
        return Err("WPA2 invalid wrapped key length");
    }
    let cipher = Aes128::new(GenericArray::from_slice(kek));
    let n = wrapped.len() / 8 - 1;
    let mut a: [u8; 8] = wrapped[..8].try_into().unwrap();
    let mut plain = wrapped[8..].to_vec();
    for j in (0..6).rev() {
        for i in (1..=n).rev() {
            let t = (n * j + i) as u64;
            let mut block = [0; 16];
            block[..8].copy_from_slice(&(u64::from_be_bytes(a) ^ t).to_be_bytes());
            block[8..].copy_from_slice(&plain[(i - 1) * 8..i * 8]);
            cipher.decrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            plain[(i - 1) * 8..i * 8].copy_from_slice(&block[8..]);
            erase(&mut block);
        }
    }
    if !equal(&a, &[0xa6; 8]) {
        erase(&mut plain);
        return Err("WPA2 key wrap integrity failure");
    }
    Ok(plain)
}

pub(crate) struct CcmpKey {
    key: [u8; 16],
    id: u8,
    peer: [u8; 6],
    tx_pn: u64,
    rx_pn: [u64; 17],
}
impl Drop for CcmpKey {
    fn drop(&mut self) {
        erase(&mut self.key);
    }
}
struct MacHeader {
    length: usize,
    tid: usize,
    aad: Vec<u8>,
    nonce: [u8; 13],
}
fn mac_header(frame: &[u8], pn: u64) -> Result<MacHeader, &'static str> {
    if frame.len() < 24
        || frame.len() > 4112
        || frame[0] & 0x0f != 8
        || frame[0] & 0x40 != 0
        || frame[1] & 3 == 3
        || frame[1] & 0x84 != 0
        || frame[22] & 0x0f != 0
    {
        return Err("CCMP unsupported fragmented/WDS/HT data frame");
    }
    let qos = frame[0] & 0x80 != 0;
    let length = if qos { 26 } else { 24 };
    if frame.len() < length || qos && frame[24] & 0x80 != 0 {
        return Err("CCMP unsupported A-MSDU");
    }
    let priority = if qos { frame[24] & 15 } else { 0 };
    let tid = if qos { usize::from(priority) } else { 16 };
    let mut aad = Vec::with_capacity(24);
    aad.extend_from_slice(&[frame[0] & 0x8f, frame[1] & 0xc7]);
    aad.extend_from_slice(&frame[4..22]);
    aad.extend_from_slice(&[frame[22] & 15, 0]);
    if qos {
        aad.extend_from_slice(&[priority, 0]);
    }
    let mut nonce = [0; 13];
    nonce[0] = priority;
    nonce[1..7].copy_from_slice(&frame[10..16]);
    let pnbytes = pn.to_be_bytes();
    nonce[7..].copy_from_slice(&pnbytes[2..]);
    Ok(MacHeader {
        length,
        tid,
        aad,
        nonce,
    })
}
impl CcmpKey {
    pub fn new(
        key: [u8; 16],
        id: u8,
        expected_rx_transmitter: [u8; 6],
    ) -> Result<Self, &'static str> {
        if id > 3 || !valid_mac(expected_rx_transmitter) {
            return Err("CCMP invalid key ID or peer");
        }
        Ok(Self {
            key,
            id,
            peer: expected_rx_transmitter,
            tx_pn: 0,
            rx_pn: [0; 17],
        })
    }
    /// Initial GTK receive floor from authenticated EAPOL-Key RSC. Never call
    /// this on an installed key in response to a retransmitted message 3.
    pub fn with_receive_pn(
        key: [u8; 16],
        id: u8,
        expected_rx_transmitter: [u8; 6],
        pn: u64,
    ) -> Result<Self, &'static str> {
        if pn > PN_MAX {
            return Err("CCMP invalid receive sequence counter");
        }
        let mut key = Self::new(key, id, expected_rx_transmitter)?;
        key.rx_pn.fill(pn);
        Ok(key)
    }
    pub fn encrypt(&mut self, frame: &[u8]) -> Result<Vec<u8>, &'static str> {
        if frame.len() > 4096 {
            return Err("CCMP excessive frame length");
        }
        if frame.get(1).is_some_and(|byte| byte & 0x40 != 0) {
            return Err("CCMP frame already protected");
        }
        if self.tx_pn == PN_MAX {
            return Err("CCMP packet number exhausted");
        }
        let pn = self.tx_pn + 1;
        let mut out = frame.to_vec();
        if out.len() < 24 {
            return Err("CCMP short frame");
        }
        out[1] |= 0x40;
        let header = mac_header(&out, pn)?;
        let plaintext = &out[header.length..];
        let (encrypted, mic) = ccm_encrypt(&self.key, &header.nonce, &header.aad, plaintext)?;
        out.truncate(header.length);
        let b = pn.to_le_bytes();
        out.extend_from_slice(&[b[0], b[1], 0, 0x20 | (self.id << 6), b[2], b[3], b[4], b[5]]);
        out.extend_from_slice(&encrypted);
        out.extend_from_slice(&mic);
        self.tx_pn = pn;
        Ok(out)
    }
    pub fn decrypt(&mut self, frame: &[u8]) -> Result<Vec<u8>, &'static str> {
        if frame.len() < 40 || frame[1] & 0x40 == 0 || frame[10..16] != self.peer {
            return Err("CCMP unexpected unprotected frame or transmitter");
        }
        let preliminary = mac_header(frame, 0)?;
        let at = preliminary.length;
        let iv = frame.get(at..at + 8).ok_or("CCMP missing header")?;
        if iv[2] != 0 || iv[3] != (0x20 | (self.id << 6)) {
            return Err("CCMP invalid IV or key ID");
        }
        let pn = u64::from_le_bytes([iv[0], iv[1], iv[4], iv[5], iv[6], iv[7], 0, 0]);
        let header = mac_header(frame, pn)?;
        if pn == 0 || pn <= self.rx_pn[header.tid] {
            return Err("CCMP replayed packet");
        }
        let ciphertext = frame
            .get(at + 8..frame.len() - 8)
            .ok_or("CCMP truncated encrypted frame")?;
        let mut plaintext = ccm_decrypt(
            &self.key,
            &header.nonce,
            &header.aad,
            ciphertext,
            frame[frame.len() - 8..].try_into().unwrap(),
        )?;
        let mut out = frame[..at].to_vec();
        out[1] &= !0x40;
        out.extend_from_slice(&plaintext);
        erase(&mut plaintext);
        self.rx_pn[header.tid] = pn;
        Ok(out)
    }
}
fn block_encrypt(cipher: &Aes128, block: &mut [u8; 16]) {
    cipher.encrypt_block(GenericArray::from_mut_slice(block));
}
fn ccm_mac(
    cipher: &Aes128,
    nonce: &[u8; 13],
    aad: &[u8],
    message: &[u8],
) -> Result<[u8; 8], &'static str> {
    if message.len() > u16::MAX as usize || aad.len() > 0xfeff {
        return Err("CCMP excessive packet length");
    }
    let mut state = [0u8; 16];
    state[0] = 0x19 | if aad.is_empty() { 0 } else { 0x40 };
    state[1..14].copy_from_slice(nonce);
    state[14..].copy_from_slice(&(message.len() as u16).to_be_bytes());
    block_encrypt(cipher, &mut state);
    let mut absorb = |bytes: &[u8]| {
        for chunk in bytes.chunks(16) {
            for (left, right) in state.iter_mut().zip(chunk) {
                *left ^= *right;
            }
            block_encrypt(cipher, &mut state);
        }
    };
    if !aad.is_empty() {
        let mut encoded = Vec::with_capacity(aad.len() + 18);
        encoded.extend_from_slice(&(aad.len() as u16).to_be_bytes());
        encoded.extend_from_slice(aad);
        encoded.resize(encoded.len().div_ceil(16) * 16, 0);
        absorb(&encoded);
    }
    absorb(message);
    Ok(state[..8].try_into().unwrap())
}
fn ccm_crypt(cipher: &Aes128, nonce: &[u8; 13], input: &[u8]) -> Vec<u8> {
    let mut output = input.to_vec();
    for (index, chunk) in output.chunks_mut(16).enumerate() {
        let mut counter = [0; 16];
        counter[0] = 1;
        counter[1..14].copy_from_slice(nonce);
        counter[14..].copy_from_slice(&((index + 1) as u16).to_be_bytes());
        block_encrypt(cipher, &mut counter);
        for (left, right) in chunk.iter_mut().zip(counter) {
            *left ^= right;
        }
    }
    output
}
fn ccm_s0(cipher: &Aes128, nonce: &[u8; 13]) -> [u8; 16] {
    let mut counter = [0; 16];
    counter[0] = 1;
    counter[1..14].copy_from_slice(nonce);
    block_encrypt(cipher, &mut counter);
    counter
}
fn ccm_encrypt(
    key: &[u8; 16],
    nonce: &[u8; 13],
    aad: &[u8],
    message: &[u8],
) -> Result<(Vec<u8>, [u8; 8]), &'static str> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut tag = ccm_mac(&cipher, nonce, aad, message)?;
    let s0 = ccm_s0(&cipher, nonce);
    for (a, b) in tag.iter_mut().zip(s0) {
        *a ^= b;
    }
    Ok((ccm_crypt(&cipher, nonce, message), tag))
}
fn ccm_decrypt(
    key: &[u8; 16],
    nonce: &[u8; 13],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 8],
) -> Result<Vec<u8>, &'static str> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut plaintext = ccm_crypt(&cipher, nonce, ciphertext);
    let mut expected = ccm_mac(&cipher, nonce, aad, &plaintext)?;
    let s0 = ccm_s0(&cipher, nonce);
    for (a, b) in expected.iter_mut().zip(s0) {
        *a ^= b;
    }
    if !equal(&expected, tag) {
        erase(&mut plaintext);
        return Err("CCMP authentication failure");
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    const RSN: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    #[test]
    fn ieee_psk_pmk_vector_and_raw_hex_psk() {
        assert_eq!(
            derive_pmk(b"IEEE", b"password").unwrap().as_slice(),
            hex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );
        assert_eq!(
            derive_pmk(b"ThisIsASSID", b"ThisIsAPassword")
                .unwrap()
                .as_slice(),
            hex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af")
        );
        assert_eq!(
            derive_pmk(
                b"test",
                b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
            )
            .unwrap()
            .as_slice(),
            &(0..32u8).collect::<Vec<_>>()
        );
        assert!(derive_pmk(b"", b"password").is_err());
        assert!(derive_pmk(b"test", b"short").is_err());
    }
    #[test]
    fn rfc3394_unwrap_integrity_and_known_vector() {
        let key = hex("000102030405060708090a0b0c0d0e0f");
        let mut wrapped = hex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5");
        assert_eq!(
            unwrap_key(key.as_slice().try_into().unwrap(), &wrapped).unwrap(),
            hex("00112233445566778899aabbccddeeff")
        );
        wrapped[5] ^= 1;
        assert!(unwrap_key(key.as_slice().try_into().unwrap(), &wrapped).is_err());
        assert!(unwrap_key(key.as_slice().try_into().unwrap(), &wrapped[..16]).is_err());
    }
    #[test]
    fn rfc3610_ccm_packet_vector_one() {
        let key: [u8; 16] = hex("c0c1c2c3c4c5c6c7c8c9cacbcccdcecf").try_into().unwrap();
        let nonce: [u8; 13] = hex("00000003020100a0a1a2a3a4a5").try_into().unwrap();
        let aad = hex("0001020304050607");
        let message = hex("08090a0b0c0d0e0f101112131415161718191a1b1c1d1e");
        let (ciphertext, tag) = ccm_encrypt(&key, &nonce, &aad, &message).unwrap();
        assert_eq!(
            ciphertext,
            hex("588c979a61c663d2f066d0c2c0f989806d5f6b61dac384")
        );
        assert_eq!(tag.as_slice(), hex("17e8d12cfdf926e0"));
        assert_eq!(
            ccm_decrypt(&key, &nonce, &aad, &ciphertext, &tag).unwrap(),
            message
        );
        let mut tampered = tag;
        tampered[1] ^= 1;
        assert!(ccm_decrypt(&key, &nonce, &aad, &ciphertext, &tampered).is_err());
    }
    fn fixture() -> Session {
        Session::new(
            b"test",
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            [2, 0, 0, 0, 0, 1],
            [2, 0, 0, 0, 0, 2],
            [0x22; 32],
            &RSN,
        )
        .unwrap()
    }
    fn m1(counter: u64) -> Vec<u8> {
        let mut bytes = vec![0; HEADER];
        bytes[0] = 2;
        bytes[1] = 3;
        bytes[2..4].copy_from_slice(&95u16.to_be_bytes());
        bytes[4] = 2;
        bytes[5..7].copy_from_slice(&(2 | PAIRWISE | ACK).to_be_bytes());
        bytes[7..9].copy_from_slice(&16u16.to_be_bytes());
        bytes[9..17].copy_from_slice(&counter.to_be_bytes());
        bytes[17..49].fill(0x11);
        bytes
    }
    fn wrap(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        let cipher = Aes128::new(GenericArray::from_slice(key));
        let n = plain.len() / 8;
        let mut out = vec![0; plain.len() + 8];
        out[..8].fill(0xa6);
        out[8..].copy_from_slice(plain);
        for j in 0..6 {
            for i in 1..=n {
                let mut block = [0; 16];
                block[..8].copy_from_slice(&out[..8]);
                block[8..].copy_from_slice(&out[i * 8..i * 8 + 8]);
                block_encrypt(&cipher, &mut block);
                let a = u64::from_be_bytes(block[..8].try_into().unwrap()) ^ ((n * j + i) as u64);
                out[..8].copy_from_slice(&a.to_be_bytes());
                out[i * 8..i * 8 + 8].copy_from_slice(&block[8..]);
            }
        }
        out
    }
    fn m3(session: &Session, counter: u64) -> Vec<u8> {
        let mut plain = RSN.to_vec();
        plain.extend_from_slice(&[0xdd, 22, 0, 15, 172, 1, 1, 0]);
        plain.extend_from_slice(&[0x77; 16]);
        plain.extend_from_slice(&[0xdd, 0]);
        let data = wrap(session.ptk[16..32].try_into().unwrap(), &plain);
        let mut bytes = m1(counter);
        bytes[5..7].copy_from_slice(
            &(2 | PAIRWISE | ACK | HAS_MIC | SECURE | INSTALL | ENCRYPTED).to_be_bytes(),
        );
        bytes[97..99].copy_from_slice(&(data.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&data);
        let length = (bytes.len() - 4) as u16;
        bytes[2..4].copy_from_slice(&length.to_be_bytes());
        let tag = hmac(&session.ptk[..16], &[&bytes]);
        bytes[MIC].copy_from_slice(&tag[..16]);
        bytes
    }
    #[test]
    fn handshake_mic_replay_and_no_key_reinstallation() {
        let mut session = fixture();
        let first = session.handle_eapol(&m1(1)).unwrap();
        assert!(first.keys.is_none());
        assert_eq!(&first.reply[17..49], &[0x22; 32]);
        assert_eq!(&first.reply[HEADER..], &RSN);
        let message = m3(&session, 2);
        let mut corrupt = message.clone();
        corrupt[82] ^= 1;
        assert_eq!(
            session.handle_eapol(&corrupt).err(),
            Some("WPA2 EAPOL MIC mismatch")
        );
        assert!(!session.established());
        let third = session.handle_eapol(&message).unwrap();
        assert!(third.keys.is_some());
        assert!(session.established());
        let keys = third.keys.unwrap();
        assert_eq!(keys.gtk.temporal, [0x77; 16]);
        assert_eq!(keys.gtk.id, 1);
        let retransmit = session.handle_eapol(&message).unwrap();
        assert!(retransmit.keys.is_none());
        assert_eq!(third.reply, retransmit.reply);
        let newer = m3(&session, 3);
        assert!(session.handle_eapol(&newer).unwrap().keys.is_none());
        assert!(session.handle_eapol(&message).is_err());
        assert!(session.handle_eapol(&m1(4)).is_err());
    }
    #[test]
    fn handshake_rejects_unexpected_order_truncation_and_nonce_replay() {
        let mut session = fixture();
        let mut first = m1(7);
        first[17] = 0;
        assert!(session.handle_eapol(&first[..98]).is_err());
        session.handle_eapol(&m1(7)).unwrap();
        assert!(session.handle_eapol(&m1(6)).is_err());
        assert!(session.handle_eapol(&first).is_err());
        let mut message = m3(&session, 8);
        message[17] ^= 1;
        assert!(session.handle_eapol(&message).is_err());
        let mut unsolicited = fixture();
        assert!(unsolicited.handle_eapol(&m3(&session, 8)).is_err());
    }
    fn sign_eapol(session: &Session, bytes: &mut [u8]) {
        bytes[MIC].fill(0);
        let tag = hmac(&session.ptk[..16], &[bytes]);
        bytes[MIC].copy_from_slice(&tag[..16]);
    }
    fn group_message_data(session: &Session, counter: u64, plain: &[u8], rsc: u64) -> Vec<u8> {
        let wrapped = wrap(session.ptk[16..32].try_into().unwrap(), plain);
        let mut bytes = vec![0; HEADER];
        bytes[0] = 2;
        bytes[1] = 3;
        bytes[4] = 2;
        bytes[5..7].copy_from_slice(&(2 | ACK | HAS_MIC | SECURE | ENCRYPTED).to_be_bytes());
        bytes[9..17].copy_from_slice(&counter.to_be_bytes());
        bytes[65..73].copy_from_slice(&rsc.to_le_bytes());
        bytes[97..99].copy_from_slice(&(wrapped.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&wrapped);
        let length = (bytes.len() - 4) as u16;
        bytes[2..4].copy_from_slice(&length.to_be_bytes());
        sign_eapol(session, &mut bytes);
        bytes
    }
    fn group_message(session: &Session, counter: u64, id: u8, material: u8, rsc: u64) -> Vec<u8> {
        // Group rekey carries a GTK KDE alone, without the initial M3 RSN IE.
        let mut plain = vec![0xdd, 22, 0, 15, 172, 1, id, 0];
        plain.extend_from_slice(&[material; 16]);
        group_message_data(session, counter, &plain, rsc)
    }
    fn established_session() -> Session {
        let mut session = fixture();
        session.handle_eapol(&m1(1)).unwrap();
        session.handle_eapol(&m3(&session, 2)).unwrap();
        session
    }
    #[test]
    fn group_rekey_authenticates_gtk_only_data_and_never_reinstalls_known_material() {
        let mut session = established_session();
        let ptk = session.ptk;
        let message = group_message(&session, 3, 2, 0x88, 17);
        let action = session.handle_eapol(&message).unwrap();
        assert!(action.keys.is_none());
        let gtk = action.group_key.unwrap();
        assert_eq!((gtk.id, gtk.temporal, gtk.receive_pn), (2, [0x88; 16], 17));
        assert_eq!(session.ptk, ptk);
        assert!(session.established());
        assert_eq!(action.reply.len(), HEADER);
        assert_eq!(u16::from_be_bytes(action.reply[5..7].try_into().unwrap()), 0x0302);
        assert_eq!(&action.reply[7..9], &[0; 2]);
        assert_eq!(&action.reply[9..17], &3u64.to_be_bytes());
        assert_eq!(&action.reply[17..81], &[0; 64]);
        assert_eq!(&action.reply[97..], &[0; 2]);
        session.verify_mic(&KeyPacket::parse(&action.reply).unwrap()).unwrap();
        let repeated = session.handle_eapol(&message).unwrap();
        assert!(repeated.keys.is_none() && repeated.group_key.is_none());
        assert_eq!(action.reply, repeated.reply);
        // Authenticators may use a fresh replay counter and a lower RSC for a
        // retry; that must not rebuild either GTK slot or the pairwise key.
        let retried = group_message(&session, 4, 2, 0x88, 0);
        assert!(session.handle_eapol(&retried).unwrap().group_key.is_none());
        assert_eq!(session.group_keys[2].as_ref().unwrap().receive_pn, 17);
        assert!(session.handle_eapol(&message).is_err());
        assert!(session.handle_eapol(&m3(&session, 2)).is_err());
        // The initial key remains known even while a second key is active.
        let old_slot = group_message(&session, 5, 1, 0x77, 0);
        assert!(session.handle_eapol(&old_slot).unwrap().group_key.is_none());
        let moved_material = group_message(&session, 6, 3, 0x88, 0);
        assert_eq!(session.handle_eapol(&moved_material).err(),
            Some("WPA2 GTK material reused at another key ID"));
        let replacement = group_message(&session, 6, 2, 0x99, 23);
        assert_eq!(session.handle_eapol(&replacement).unwrap().group_key.unwrap().temporal, [0x99; 16]);
        assert_eq!(session.ptk, ptk);
    }
    #[test]
    fn group_rekey_rejects_mic_wrap_flags_kde_and_replay_failures_without_advancing_state() {
        let mut session = established_session();
        let valid = group_message(&session, 3, 2, 0x88, 17);
        let mut bad = valid.clone();
        bad[82] ^= 1;
        assert_eq!(session.handle_eapol(&bad).err(), Some("WPA2 EAPOL MIC mismatch"));
        let mut bad = valid.clone();
        bad[HEADER] ^= 1;
        sign_eapol(&session, &mut bad);
        assert_eq!(session.handle_eapol(&bad).err(), Some("WPA2 key wrap integrity failure"));
        for flag in [ACK, HAS_MIC, SECURE, ENCRYPTED] {
            let mut bad = valid.clone();
            let info = u16::from_be_bytes(bad[5..7].try_into().unwrap()) & !flag;
            bad[5..7].copy_from_slice(&info.to_be_bytes());
            sign_eapol(&session, &mut bad);
            assert!(session.handle_eapol(&bad).is_err());
        }
        for info in [0x1381u16, 0x13c2, 0x1782, 0x1b82, 0x3382] {
            let mut bad = valid.clone();
            bad[5..7].copy_from_slice(&info.to_be_bytes());
            sign_eapol(&session, &mut bad);
            assert!(session.handle_eapol(&bad).is_err());
        }
        let mut bad = valid.clone();
        bad[7..9].copy_from_slice(&16u16.to_be_bytes());
        sign_eapol(&session, &mut bad);
        assert!(session.handle_eapol(&bad).is_err());
        let mut malformed = vec![0xdd, 22, 0, 15, 172, 1, 2, 1];
        malformed.extend_from_slice(&[0x88; 16]);
        let malformed = group_message_data(&session, 3, &malformed, 0);
        assert_eq!(session.handle_eapol(&malformed).err(), Some("WPA2 malformed GTK KDE"));
        let bad_rsc = group_message(&session, 3, 2, 0x88, 1 << 48);
        assert!(session.handle_eapol(&bad_rsc).is_err());
        let old = group_message(&session, 1, 2, 0x88, 0);
        assert!(session.handle_eapol(&old).is_err());
        assert_eq!(session.last_authenticated_replay, Some(2));
        assert!(session.group_keys[2].is_none());
        session.handle_eapol(&valid).unwrap();
        let changed = group_message(&session, 3, 2, 0x99, 17);
        assert_eq!(session.handle_eapol(&changed).err(), Some("WPA2 changed group replay counter"));
        assert_eq!(session.group_keys[2].as_ref().unwrap().temporal, [0x88; 16]);
        let mut early = fixture();
        assert_eq!(early.handle_eapol(&valid).err(), Some("WPA2 group message 1 before pairwise keys"));
    }
    fn frame(qos: bool) -> Vec<u8> {
        let mut f = vec![0; if qos { 26 } else { 24 }];
        f[0] = if qos { 0x88 } else { 0x08 };
        f[1] = 1;
        f[4..10].copy_from_slice(&[2, 0, 0, 0, 0, 2]);
        f[10..16].copy_from_slice(&[2, 0, 0, 0, 0, 1]);
        f[16..22].copy_from_slice(&[2, 0, 0, 0, 0, 3]);
        if qos {
            f[24] = 5;
        }
        f.extend_from_slice(b"\xaa\xaa\x03\x00\x00\x00\x08\x00hello-ip");
        f
    }
    #[test]
    fn ccmp_roundtrip_authentication_and_per_tid_replay() {
        let peer = [2, 0, 0, 0, 0, 1];
        let mut tx = CcmpKey::new([0x42; 16], 0, [2, 0, 0, 0, 0, 2]).unwrap();
        let mut rx = CcmpKey::new([0x42; 16], 0, peer).unwrap();
        for qos in [false, true] {
            let plain = frame(qos);
            let encrypted = tx.encrypt(&plain).unwrap();
            assert_eq!(rx.decrypt(&encrypted).unwrap(), plain);
            assert!(rx.decrypt(&encrypted).is_err());
        }
        let encrypted = tx.encrypt(&frame(false)).unwrap();
        let mut corrupt = encrypted.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(rx.decrypt(&corrupt).is_err());
        assert!(rx.decrypt(&encrypted).is_ok());
        let mut wrongpeer = CcmpKey::new([0x42; 16], 0, [2, 0, 0, 0, 0, 9]).unwrap();
        assert!(wrongpeer.decrypt(&encrypted).is_err());
        assert!(tx.encrypt(&encrypted).is_err());
        tx.tx_pn = PN_MAX;
        assert!(tx.encrypt(&frame(false)).is_err());
    }
    #[test]
    fn ccmp_header_fields_are_authenticated_but_retry_sequence_are_masked() {
        let mut tx = CcmpKey::new([0x42; 16], 0, [2, 0, 0, 0, 0, 2]).unwrap();
        let mut rx = CcmpKey::new([0x42; 16], 0, [2, 0, 0, 0, 0, 1]).unwrap();
        let encrypted = tx.encrypt(&frame(true)).unwrap();
        let mut retry = encrypted.clone();
        retry[1] |= 8;
        retry[22] |= 0x30;
        retry[23] = 0x55;
        assert!(rx.decrypt(&retry).is_ok());
        let encrypted = tx.encrypt(&frame(true)).unwrap();
        let mut corrupt = encrypted.clone();
        corrupt[16] ^= 2;
        assert!(rx.decrypt(&corrupt).is_err());
        let mut plain = frame(true);
        plain[24] |= 0x80;
        assert!(tx.encrypt(&plain).is_err());
        let mut plain = frame(false);
        plain[1] |= 4;
        assert!(tx.encrypt(&plain).is_err());
    }
    #[test]
    fn gtk_receive_floor_is_preserved_and_authenticated() {
        let mut session = fixture();
        session.handle_eapol(&m1(1)).unwrap();
        let mut message = m3(&session, 2);
        message[65..73].copy_from_slice(&17u64.to_le_bytes());
        message[MIC].fill(0);
        let tag = hmac(&session.ptk[..16], &[&message]);
        message[MIC].copy_from_slice(&tag[..16]);
        let keys = session.handle_eapol(&message).unwrap().keys.unwrap();
        assert_eq!(keys.gtk.receive_pn, 17);
        let peer = [2, 0, 0, 0, 0, 1];
        let mut tx = CcmpKey::new([0x42; 16], 1, [2, 0, 0, 0, 0, 2]).unwrap();
        let mut rx = CcmpKey::with_receive_pn([0x42; 16], 1, peer, 1).unwrap();
        let first = tx.encrypt(&frame(false)).unwrap();
        assert!(rx.decrypt(&first).is_err());
        let second = tx.encrypt(&frame(false)).unwrap();
        assert!(rx.decrypt(&second).is_ok());
        assert!(CcmpKey::with_receive_pn([0x42; 16], 1, peer, PN_MAX + 1).is_err());
    }
}
