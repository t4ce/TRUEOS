use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;
use spin::Mutex;

use crate::net::adapter::{
    net_debug_counters, register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue,
    SocketKind,
};

// Reuse the QEMU slirp defaults (DNS at 10.0.2.3).
const SLIRP_DNS_IP: [u8; 4] = [10, 0, 2, 3];

const DEFAULT_HTTPS_PORT: u16 = 443;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMode {
    None,
    Tofu,
    Pin([u8; 32]),
}

#[derive(Clone, Debug)]
struct ParsedUrl {
    host: HString<96>,
    port: u16,
    path: HString<160>,
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn parse_https_url(url: &str) -> Result<ParsedUrl, &'static str> {
    // Accept:
    // - https://host[:port][/path]
    // - host[:port][/path]
    // (no IPv6 bracket support here yet)

    let mut u = url.trim();
    if u.is_empty() {
        return Err("empty url");
    }

    if let Some(rest) = u.strip_prefix("https://") {
        u = rest;
    } else if let Some(_) = u.strip_prefix("http://") {
        return Err("use https:// (not http://)");
    }

    let (hostport, path) = match u.split_once('/') {
        Some((a, b)) => (a, b),
        None => (u, ""),
    };

    let (host_str, port) = match hostport.split_once(':') {
        Some((h, p)) => {
            let p = p.trim();
            if p.is_empty() {
                return Err("empty port");
            }
            let port = p.parse::<u16>().map_err(|_| "bad port")?;
            (h, port)
        }
        None => (hostport, DEFAULT_HTTPS_PORT),
    };

    let host_str = host_str.trim();
    if host_str.is_empty() {
        return Err("empty host");
    }

    let mut host: HString<96> = HString::new();
    for ch in host_str.chars() {
        if host.push(ch).is_err() {
            break;
        }
    }

    let mut out_path: HString<160> = HString::new();
    if path.is_empty() {
        let _ = out_path.push('/');
    } else {
        let _ = out_path.push('/');
        for ch in path.chars() {
            if out_path.push(ch).is_err() {
                break;
            }
        }
    }

    Ok(ParsedUrl {
        host,
        port,
        path: out_path,
    })
}

fn dns_query(id: u16, host: &str, qtype: u16) -> Vec<u8> {
    let mut q = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&0x0100u16.to_be_bytes()); // RD
    q.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    q.extend_from_slice(&0u16.to_be_bytes());
    q.extend_from_slice(&0u16.to_be_bytes());
    q.extend_from_slice(&0u16.to_be_bytes());

    for label in host.split('.') {
        let bytes = label.as_bytes();
        let len = bytes.len().min(63);
        q.push(len as u8);
        q.extend_from_slice(&bytes[..len]);
    }
    q.push(0);

    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q
}

fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
    if *idx >= pkt.len() {
        return false;
    }

    let mut steps: u8 = 0;
    loop {
        if *idx >= pkt.len() {
            return false;
        }
        let b = pkt[*idx];
        if b == 0 {
            *idx += 1;
            return true;
        }
        if (b & 0xC0) == 0xC0 {
            if *idx + 1 >= pkt.len() {
                return false;
            }
            *idx += 2;
            return true;
        }

        let len = b as usize;
        *idx += 1;
        if *idx + len > pkt.len() {
            return false;
        }
        *idx += len;

        steps = steps.wrapping_add(1);
        if steps > 64 {
            return false;
        }
    }
}

fn dns_parse_first_a(pkt: &[u8], want_id: u16) -> Result<Option<[u8; 4]>, &'static str> {
    if pkt.len() < 12 {
        return Err("dns too short");
    }

    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    if id != want_id {
        return Ok(None);
    }

    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    let rcode = (flags & 0x000F) as u8;
    if rcode != 0 {
        return Err("dns rcode != 0");
    }

    let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;

    let mut idx: usize = 12;
    for _ in 0..qd {
        if !dns_skip_name(pkt, &mut idx) {
            return Err("dns bad qname");
        }
        if idx + 4 > pkt.len() {
            return Err("dns qtype/qclass truncated");
        }
        idx += 4;
    }

    for _ in 0..an {
        if !dns_skip_name(pkt, &mut idx) {
            return Err("dns bad aname");
        }
        if idx + 10 > pkt.len() {
            return Err("dns answer hdr truncated");
        }
        let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
        let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
        let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
        idx += 10;
        if idx + rdlen > pkt.len() {
            return Err("dns rdata truncated");
        }

        if typ == 1 && class == 1 && rdlen == 4 {
            return Ok(Some([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]));
        }
        idx += rdlen;
    }

    Ok(None)
}

fn push_u24_be(out: &mut Vec<u8>, value: usize) {
    out.push(((value >> 16) & 0xFF) as u8);
    out.push(((value >> 8) & 0xFF) as u8);
    out.push((value & 0xFF) as u8);
}

fn build_tls12_client_hello(host: &str) -> Vec<u8> {
    // TLS 1.2 ClientHello with SNI.
    // NOTE: This only negotiates and parses the beginning of the handshake;
    // it does not complete key exchange or send encrypted HTTP yet.

    let mut body = Vec::new();

    // client_version (TLS 1.2)
    body.extend_from_slice(&0x0303u16.to_be_bytes());

    // random (32 bytes)
    let mut random = [0u8; 32];
    for (i, b) in random.iter_mut().enumerate() {
        *b = (0xA5u8).wrapping_add(i as u8);
    }
    body.extend_from_slice(&random);

    // session id
    body.push(0);

    // cipher suites (small set of common TLS 1.2 ECDHE/RSA suites)
    let ciphers: [u16; 6] = [
        0xC02F, // TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
        0xC030, // TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
        0xC02B, // TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        0xC02C, // TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
        0x009C, // TLS_RSA_WITH_AES_128_GCM_SHA256
        0x009D, // TLS_RSA_WITH_AES_256_GCM_SHA384
    ];
    body.extend_from_slice(&((ciphers.len() * 2) as u16).to_be_bytes());
    for c in ciphers {
        body.extend_from_slice(&c.to_be_bytes());
    }

    // compression methods
    body.push(1);
    body.push(0);

    // extensions
    let mut exts = Vec::new();

    // SNI extension (0x0000)
    {
        let host_bytes = host.as_bytes();
        let mut sni = Vec::new();
        let server_name_len = 1 + 2 + host_bytes.len();
        sni.extend_from_slice(&(server_name_len as u16).to_be_bytes());
        sni.push(0);
        sni.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
        sni.extend_from_slice(host_bytes);

        exts.extend_from_slice(&0x0000u16.to_be_bytes());
        exts.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        exts.extend_from_slice(&sni);
    }

    // supported_groups (0x000a)
    {
        let groups: [u16; 2] = [0x001D, 0x0017]; // x25519, secp256r1
        let mut v = Vec::new();
        v.extend_from_slice(&((groups.len() * 2) as u16).to_be_bytes());
        for g in groups {
            v.extend_from_slice(&g.to_be_bytes());
        }

        exts.extend_from_slice(&0x000Au16.to_be_bytes());
        exts.extend_from_slice(&(v.len() as u16).to_be_bytes());
        exts.extend_from_slice(&v);
    }

    // ec_point_formats (0x000b)
    {
        let v: [u8; 2] = [1, 0];
        exts.extend_from_slice(&0x000Bu16.to_be_bytes());
        exts.extend_from_slice(&(v.len() as u16).to_be_bytes());
        exts.extend_from_slice(&v);
    }

    // signature_algorithms (0x000d)
    {
        let sigs: [u16; 6] = [
            0x0401, // rsa_pkcs1_sha256
            0x0501, // rsa_pkcs1_sha384
            0x0601, // rsa_pkcs1_sha512
            0x0403, // ecdsa_secp256r1_sha256
            0x0503, // ecdsa_secp384r1_sha384
            0x0804, // rsa_pss_rsae_sha256
        ];
        let mut v = Vec::new();
        v.extend_from_slice(&((sigs.len() * 2) as u16).to_be_bytes());
        for s in sigs {
            v.extend_from_slice(&s.to_be_bytes());
        }

        exts.extend_from_slice(&0x000Du16.to_be_bytes());
        exts.extend_from_slice(&(v.len() as u16).to_be_bytes());
        exts.extend_from_slice(&v);
    }

    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);

    // Handshake wrapper: ClientHello
    let mut hs = Vec::new();
    hs.push(1); // client_hello
    push_u24_be(&mut hs, body.len());
    hs.extend_from_slice(&body);

    // TLS record wrapper
    let mut rec = Vec::new();
    rec.push(0x16); // handshake
    rec.extend_from_slice(&0x0303u16.to_be_bytes()); // record version
    rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    rec.extend_from_slice(&hs);

    rec
}

#[derive(Clone, Copy, Debug)]
struct ServerHelloInfo {
    server_version: u16,
    selected_cipher: u16,
    supported_versions: Option<u16>,
}

fn cipher_name(cipher: u16) -> &'static str {
    match cipher {
        0xC02F => "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
        0xC030 => "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
        0xC02B => "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
        0xC02C => "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
        0x009C => "TLS_RSA_WITH_AES_128_GCM_SHA256",
        0x009D => "TLS_RSA_WITH_AES_256_GCM_SHA384",
        _ => "(unknown)",
    }
}

fn version_name(v: u16) -> &'static str {
    match v {
        0x0301 => "TLS1.0",
        0x0302 => "TLS1.1",
        0x0303 => "TLS1.2",
        0x0304 => "TLS1.3",
        _ => "(unknown)",
    }
}

fn parse_server_hello(msg: &[u8]) -> Result<ServerHelloInfo, &'static str> {
    // TLS ServerHello (RFC 5246):
    // server_version(2) random(32) session_id_len(1) session_id
    // cipher_suite(2) compression(1) extensions_len(2) extensions
    if msg.len() < 2 + 32 + 1 + 2 + 1 {
        return Err("serverhello too short");
    }

    let mut idx = 0usize;
    let server_version = u16::from_be_bytes([msg[idx], msg[idx + 1]]);
    idx += 2;

    idx += 32; // random

    let sid_len = msg[idx] as usize;
    idx += 1;
    if idx + sid_len > msg.len() {
        return Err("serverhello sid truncated");
    }
    idx += sid_len;

    if idx + 2 + 1 > msg.len() {
        return Err("serverhello cipher/compression truncated");
    }
    let selected_cipher = u16::from_be_bytes([msg[idx], msg[idx + 1]]);
    idx += 2;
    let _compression = msg[idx];
    idx += 1;

    let mut supported_versions: Option<u16> = None;

    if idx + 2 <= msg.len() {
        let ext_len = u16::from_be_bytes([msg[idx], msg[idx + 1]]) as usize;
        idx += 2;
        if idx + ext_len > msg.len() {
            return Err("serverhello extensions truncated");
        }
        let exts = &msg[idx..idx + ext_len];

        let mut eidx = 0usize;
        while eidx + 4 <= exts.len() {
            let etyp = u16::from_be_bytes([exts[eidx], exts[eidx + 1]]);
            let elen = u16::from_be_bytes([exts[eidx + 2], exts[eidx + 3]]) as usize;
            eidx += 4;
            if eidx + elen > exts.len() {
                break;
            }
            let data = &exts[eidx..eidx + elen];
            eidx += elen;

            // supported_versions extension (0x002b) in TLS1.3 ServerHello contains selected version.
            if etyp == 0x002B && data.len() == 2 {
                supported_versions = Some(u16::from_be_bytes([data[0], data[1]]));
            }
        }
    }

    Ok(ServerHelloInfo {
        server_version,
        selected_cipher,
        supported_versions,
    })
}

fn parse_first_certificate_der(msg: &[u8]) -> Result<Option<&[u8]>, &'static str> {
    // TLS1.2 Certificate (RFC 5246):
    // certificate_list_length(3) { cert_len(3) cert_bytes }+
    if msg.len() < 3 {
        return Err("certificate too short");
    }

    let total_len = ((msg[0] as usize) << 16) | ((msg[1] as usize) << 8) | (msg[2] as usize);
    let mut idx = 3usize;
    if idx + total_len > msg.len() {
        // Some servers send cert chain split across multiple handshake messages; for now just
        // treat it as incomplete.
        return Ok(None);
    }

    if idx + 3 > msg.len() {
        return Err("certificate list truncated");
    }
    let cert_len = ((msg[idx] as usize) << 16) | ((msg[idx + 1] as usize) << 8) | (msg[idx + 2] as usize);
    idx += 3;
    if idx + cert_len > msg.len() {
        return Err("certificate entry truncated");
    }

    Ok(Some(&msg[idx..idx + cert_len]))
}

// Minimal SHA-256 (enough for TOFU/pin fingerprints).
fn sha256(data: &[u8]) -> [u8; 32] {
    const H0: [u32; 8] = [
        0x6a09e667,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];

    const K: [u32; 64] = [
        0x428a2f98,
        0x71374491,
        0xb5c0fbcf,
        0xe9b5dba5,
        0x3956c25b,
        0x59f111f1,
        0x923f82a4,
        0xab1c5ed5,
        0xd807aa98,
        0x12835b01,
        0x243185be,
        0x550c7dc3,
        0x72be5d74,
        0x80deb1fe,
        0x9bdc06a7,
        0xc19bf174,
        0xe49b69c1,
        0xefbe4786,
        0x0fc19dc6,
        0x240ca1cc,
        0x2de92c6f,
        0x4a7484aa,
        0x5cb0a9dc,
        0x76f988da,
        0x983e5152,
        0xa831c66d,
        0xb00327c8,
        0xbf597fc7,
        0xc6e00bf3,
        0xd5a79147,
        0x06ca6351,
        0x14292967,
        0x27b70a85,
        0x2e1b2138,
        0x4d2c6dfc,
        0x53380d13,
        0x650a7354,
        0x766a0abb,
        0x81c2c92e,
        0x92722c85,
        0xa2bfe8a1,
        0xa81a664b,
        0xc24b8b70,
        0xc76c51a3,
        0xd192e819,
        0xd6990624,
        0xf40e3585,
        0x106aa070,
        0x19a4c116,
        0x1e376c08,
        0x2748774c,
        0x34b0bcb5,
        0x391c0cb3,
        0x4ed8aa4a,
        0x5b9cca4f,
        0x682e6ff3,
        0x748f82ee,
        0x78a5636f,
        0x84c87814,
        0x8cc70208,
        0x90befffa,
        0xa4506ceb,
        0xbef9a3f7,
        0xc67178f2,
    ];

    #[inline]
    fn rotr(x: u32, n: u32) -> u32 {
        (x >> n) | (x << (32 - n))
    }

    #[inline]
    fn ch(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (!x & z)
    }

    #[inline]
    fn maj(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (x & z) ^ (y & z)
    }

    #[inline]
    fn bsig0(x: u32) -> u32 {
        rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22)
    }

    #[inline]
    fn bsig1(x: u32) -> u32 {
        rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25)
    }

    #[inline]
    fn ssig0(x: u32) -> u32 {
        rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3)
    }

    #[inline]
    fn ssig1(x: u32) -> u32 {
        rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10)
    }

    let mut h = H0;

    // Pad to a multiple of 512 bits.
    let bit_len = (data.len() as u64) * 8;
    let mut msg = Vec::with_capacity(((data.len() + 9 + 63) / 64) * 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];

    for chunk in msg.chunks_exact(64) {
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..64 {
            w[i] = ssig1(w[i - 2])
                .wrapping_add(w[i - 7])
                .wrapping_add(ssig0(w[i - 15]))
                .wrapping_add(w[i - 16]);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let t1 = hh
                .wrapping_add(bsig1(e))
                .wrapping_add(ch(e, f, g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = bsig0(a).wrapping_add(maj(a, b, c));

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn parse_hex_32(s: &str) -> Result<[u8; 32], &'static str> {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    let t = s.trim();
    let t = t.strip_prefix("0x").unwrap_or(t);
    let bytes = t.as_bytes();
    if bytes.len() != 64 {
        return Err("pin must be 64 hex chars");
    }

    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(bytes[i * 2]).ok_or("bad hex")?;
        let lo = hex_val(bytes[i * 2 + 1]).ok_or("bad hex")?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn fmt_hex32(out: &mut HString<80>, bytes: &[u8; 32]) {
    out.clear();
    for b in bytes {
        let _ = write!(out, "{:02x}", b);
    }
}

#[derive(Clone)]
struct PinEntry {
    host: HString<96>,
    sha256: [u8; 32],
}

static PINS: Mutex<[Option<PinEntry>; 8]> = Mutex::new([None, None, None, None, None, None, None, None]);

static HTTPS_JOB_SEQ: AtomicU32 = AtomicU32::new(1);

fn tofu_check_or_store(host: &HString<96>, leaf_hash: [u8; 32]) -> Result<(), &'static str> {
    let mut pins = PINS.lock();

    // Check existing
    for slot in pins.iter_mut() {
        if let Some(e) = slot {
            if &e.host == host {
                if e.sha256 == leaf_hash {
                    return Ok(());
                }
                return Err("tofu mismatch");
            }
        }
    }

    // Store new
    for slot in pins.iter_mut() {
        if slot.is_none() {
            *slot = Some(PinEntry {
                host: host.clone(),
                sha256: leaf_hash,
            });
            return Ok(());
        }
    }

    // No space: overwrite slot 0
    pins[0] = Some(PinEntry {
        host: host.clone(),
        sha256: leaf_hash,
    });
    Ok(())
}

#[task]
pub async fn https_matrix_job(slot_id: u8, url: HString<256>, pin_mode: PinMode) {
    crate::matrix::push_line(slot_id, "https: starting");

    if crate::net::mac_address().is_none() {
        crate::matrix::push_line(slot_id, "https: disabled (no NIC)");
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
        return;
    }

    let parsed = match parse_https_url(url.as_str()) {
        Ok(p) => p,
        Err(e) => {
            crate::matrix::push_line(slot_id, "https: bad url");
            crate::matrix::push_line(slot_id, e);
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    let dns_id: u16 = 0xB200u16.wrapping_add(slot_id as u16);

    let seq = HTTPS_JOB_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(format!("net-https-{}-{}", slot_id + 1, seq));
    let cmds_name = leak_str(format!("{}-cmd", owner));
    let evts_name = leak_str(format!("{}-evt", owner));

    let cmds = NetQueue::new_leaked(cmds_name, 96);
    let events = NetQueue::new_leaked(evts_name, 96);
    register_app_queues(owner, cmds, events);

    let udp_port: u16 = 4320u16.wrapping_add(slot_id as u16);

    crate::matrix::push_line(slot_id, "https: opening udp/tcp");

    let _ = cmds.push(NetCommand::OpenUdp { port: udp_port });

    let mut udp_handle: Option<NetHandle> = None;
    let mut tcp_handle: Option<NetHandle> = None;
    let mut resolved: Option<[u8; 4]> = None;

    let mut sent_hello = false;

    let mut rx_raw: Vec<u8> = Vec::new();
    let mut hs_buf: Vec<u8> = Vec::new();

    let mut server_hello: Option<ServerHelloInfo> = None;
    let mut leaf_hash: Option<[u8; 32]> = None;

    let need_cert = match pin_mode {
        PinMode::None => false,
        PinMode::Tofu | PinMode::Pin(_) => true,
    };

    let deadline = Instant::now() + EmbassyDuration::from_secs(8);
    let mut ticks: u32 = 0;

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => match kind {
                    SocketKind::Udp => {
                        udp_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "https: udp opened");

                        // Re-send DNS now that we have a real UDP handle.
                        let dns = dns_query(dns_id, parsed.host.as_str(), 1);
                        let _ = cmds.push(NetCommand::SendUdp {
                            handle,
                            remote: NetEndpoint {
                                addr: SLIRP_DNS_IP,
                                port: 53,
                            },
                            data: dns,
                        });
                    }
                    SocketKind::Tcp => {
                        tcp_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "https: tcp opened");
                    }
                },
                NetEvent::UdpPacket { handle, data, .. } => {
                    if udp_handle != Some(handle) {
                        continue;
                    }
                    match dns_parse_first_a(&data, dns_id) {
                        Ok(Some(ip)) => {
                            resolved = Some(ip);
                            let mut line: HString<96> = HString::new();
                            let _ = write!(
                                line,
                                "https: nslookup {} => {}.{}.{}.{}",
                                parsed.host.as_str(),
                                ip[0],
                                ip[1],
                                ip[2],
                                ip[3]
                            );
                            crate::matrix::push_line(slot_id, line.as_str());

                            let _ = cmds.push(NetCommand::OpenTcpConnect {
                                remote: NetEndpoint {
                                    addr: ip,
                                    port: parsed.port,
                                },
                            });
                        }
                        Ok(None) => {}
                        Err(e) => {
                            crate::matrix::push_line(slot_id, "https: dns parse error");
                            crate::matrix::push_line(slot_id, e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            return;
                        }
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    crate::matrix::push_line(slot_id, "https: tcp established");

                    if !sent_hello {
                        let ch = build_tls12_client_hello(parsed.host.as_str());
                        let _ = cmds.push(NetCommand::SendTcp { handle, data: ch });
                        sent_hello = true;
                        crate::matrix::push_line(slot_id, "https: sent tls clienthello (tls1.2+sni)");
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }

                    rx_raw.extend_from_slice(&data);

                    // Drain full TLS records into handshake buffer.
                    let mut consumed = 0usize;
                    while consumed + 5 <= rx_raw.len() {
                        let typ = rx_raw[consumed];
                        let len = u16::from_be_bytes([rx_raw[consumed + 3], rx_raw[consumed + 4]]) as usize;
                        if consumed + 5 + len > rx_raw.len() {
                            break;
                        }
                        if typ == 0x16 {
                            hs_buf.extend_from_slice(&rx_raw[consumed + 5..consumed + 5 + len]);
                        }
                        consumed += 5 + len;
                    }
                    if consumed > 0 {
                        rx_raw.drain(0..consumed);
                    }

                    // Parse handshake messages.
                    loop {
                        if hs_buf.len() < 4 {
                            break;
                        }
                        let htyp = hs_buf[0];
                        let hlen = ((hs_buf[1] as usize) << 16)
                            | ((hs_buf[2] as usize) << 8)
                            | (hs_buf[3] as usize);
                        if hs_buf.len() < 4 + hlen {
                            break;
                        }
                        let msg = hs_buf[4..4 + hlen].to_vec();
                        hs_buf.drain(0..4 + hlen);

                        if htyp == 2 && server_hello.is_none() {
                            match parse_server_hello(&msg) {
                                Ok(info) => {
                                    server_hello = Some(info);
                                    let negotiated = info.supported_versions.unwrap_or(info.server_version);
                                    let mut line: HString<96> = HString::new();
                                    let _ = write!(
                                        line,
                                        "https: negotiated {} cipher=0x{:04x}",
                                        version_name(negotiated),
                                        info.selected_cipher
                                    );
                                    crate::matrix::push_line(slot_id, line.as_str());

                                    let mut line2: HString<96> = HString::new();
                                    let _ = write!(line2, "https: cipher {}", cipher_name(info.selected_cipher));
                                    crate::matrix::push_line(slot_id, line2.as_str());
                                }
                                Err(e) => {
                                    crate::matrix::push_line(slot_id, "https: bad serverhello");
                                    crate::matrix::push_line(slot_id, e);
                                    crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                                    return;
                                }
                            }
                        }

                        if htyp == 11 && need_cert && leaf_hash.is_none() {
                            match parse_first_certificate_der(&msg) {
                                Ok(Some(der)) => {
                                    let h = sha256(der);
                                    leaf_hash = Some(h);

                                    let mut hex: HString<80> = HString::new();
                                    fmt_hex32(&mut hex, &h);
                                    let mut line: HString<96> = HString::new();
                                    let _ = write!(line, "https: leafcert sha256={}", hex.as_str());
                                    crate::matrix::push_line(slot_id, line.as_str());
                                }
                                Ok(None) => {
                                    // Not enough bytes yet; wait for more.
                                }
                                Err(e) => {
                                    crate::matrix::push_line(slot_id, "https: cert parse error");
                                    crate::matrix::push_line(slot_id, e);
                                    crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                                    return;
                                }
                            }
                        }

                        // Finish conditions
                        if server_hello.is_some() && (!need_cert || leaf_hash.is_some()) {
                            break;
                        }
                    }

                    if server_hello.is_some() && (!need_cert || leaf_hash.is_some()) {
                        // Pin/TOFU checks
                        if let Some(h) = leaf_hash {
                            match pin_mode {
                                PinMode::None => {}
                                PinMode::Tofu => {
                                    if let Err(_) = tofu_check_or_store(&parsed.host, h) {
                                        crate::matrix::push_line(slot_id, "https: tofu mismatch");
                                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                                        return;
                                    }
                                    crate::matrix::push_line(slot_id, "https: tofu ok");
                                }
                                PinMode::Pin(expected) => {
                                    if expected != h {
                                        crate::matrix::push_line(slot_id, "https: pin mismatch");
                                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                                        return;
                                    }
                                    crate::matrix::push_line(slot_id, "https: pin ok");
                                }
                            }
                        }

                        crate::matrix::push_line(slot_id, "https: ok (serverhello parsed)");
                        crate::matrix::push_line(
                            slot_id,
                            "https: note: full TLS handshake + encrypted HTTP not implemented yet",
                        );
                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);

                        if let Some(h) = tcp_handle {
                            let _ = cmds.push(NetCommand::Close { handle: h });
                        }
                        if let Some(h) = udp_handle {
                            let _ = cmds.push(NetCommand::Close { handle: h });
                        }
                        return;
                    }
                }
                NetEvent::Error { msg } => {
                    let _ = msg;
                }
                NetEvent::TcpSent { .. } => {}
                NetEvent::Closed { .. } => {}
            }
        }

        ticks = ticks.wrapping_add(1);
        if (ticks % 20) == 0 {
            let (rx, tx, dropped) = net_debug_counters();
            let _ = (rx, tx, dropped);
        }

        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "https: timed out");
            if resolved.is_none() {
                crate::matrix::push_line(slot_id, "https: dns unresolved");
            }
            if !sent_hello {
                crate::matrix::push_line(slot_id, "https: no tls traffic");
            }
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            if let Some(h) = tcp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            if let Some(h) = udp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

// Re-export helpers for shell parsing.
pub fn parse_pin_hex(s: &str) -> Result<[u8; 32], &'static str> {
    parse_hex_32(s)
}
