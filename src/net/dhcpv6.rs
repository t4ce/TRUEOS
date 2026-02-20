//! Minimal DHCPv6 (RFC 8415) encode/decode helpers.
//
// This is intentionally tiny: enough for Solicit/Request (IA_NA) and
// Information-Request (DNS servers via option 23).

extern crate alloc;

use alloc::vec::Vec;

pub const CLIENT_PORT: u16 = 546;
pub const SERVER_PORT: u16 = 547;

// ff02::1:2 (All_DHCP_Relay_Agents_and_Servers)
pub const ALL_SERVERS_MCAST: [u8; 16] = [
    0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0x02,
];

// Message types (subset).
const MSG_SOLICIT: u8 = 1;
const MSG_ADVERTISE: u8 = 2;
const MSG_REQUEST: u8 = 3;
const MSG_REPLY: u8 = 7;
const MSG_INFO_REQUEST: u8 = 11;

// Options (subset).
const OPT_CLIENTID: u16 = 1;
const OPT_SERVERID: u16 = 2;
const OPT_IA_NA: u16 = 3;
const OPT_IAADDR: u16 = 5;
const OPT_ORO: u16 = 6;
const OPT_ELAPSED_TIME: u16 = 8;
const OPT_DNS_SERVERS: u16 = 23;

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_opt(out: &mut Vec<u8>, code: u16, data: &[u8]) {
    push_u16(out, code);
    push_u16(out, data.len() as u16);
    out.extend_from_slice(data);
}

pub fn duid_ll_from_mac(mac: [u8; 6]) -> [u8; 10] {
    // DUID-LL (RFC 8415): type=3, hwtype=1 (Ethernet), lladdr(6)
    let mut out = [0u8; 10];
    out[0..2].copy_from_slice(&3u16.to_be_bytes());
    out[2..4].copy_from_slice(&1u16.to_be_bytes());
    out[4..10].copy_from_slice(&mac);
    out
}

pub fn build_solicit(xid: [u8; 3], duid: &[u8], iaid: u32, request_dns: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.push(MSG_SOLICIT);
    out.extend_from_slice(&xid);

    push_opt(&mut out, OPT_CLIENTID, duid);

    // Elapsed Time (0 for initial)
    push_opt(&mut out, OPT_ELAPSED_TIME, &0u16.to_be_bytes());

    if request_dns {
        // Option Request Option: ask for DNS Recursive Name Server option.
        let mut oro = Vec::with_capacity(2);
        push_u16(&mut oro, OPT_DNS_SERVERS);
        push_opt(&mut out, OPT_ORO, &oro);
    }

    // IA_NA: IAID + T1 + T2 + (no suboptions in Solicit)
    let mut ia = Vec::with_capacity(12);
    push_u32(&mut ia, iaid);
    push_u32(&mut ia, 0);
    push_u32(&mut ia, 0);
    push_opt(&mut out, OPT_IA_NA, &ia);

    out
}

pub fn build_request(
    xid: [u8; 3],
    duid: &[u8],
    server_id: &[u8],
    iaid: u32,
    requested_addr: Option<[u8; 16]>,
    request_dns: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.push(MSG_REQUEST);
    out.extend_from_slice(&xid);

    push_opt(&mut out, OPT_CLIENTID, duid);
    push_opt(&mut out, OPT_SERVERID, server_id);
    push_opt(&mut out, OPT_ELAPSED_TIME, &0u16.to_be_bytes());

    if request_dns {
        let mut oro = Vec::with_capacity(2);
        push_u16(&mut oro, OPT_DNS_SERVERS);
        push_opt(&mut out, OPT_ORO, &oro);
    }

    let mut ia = Vec::with_capacity(64);
    push_u32(&mut ia, iaid);
    push_u32(&mut ia, 0);
    push_u32(&mut ia, 0);

    if let Some(addr) = requested_addr {
        // IAADDR: addr(16) + preferred_lifetime(4) + valid_lifetime(4)
        // Lifetimes set to 0 here; server fills real values in Reply.
        let mut iaaddr = Vec::with_capacity(24);
        iaaddr.extend_from_slice(&addr);
        push_u32(&mut iaaddr, 0);
        push_u32(&mut iaaddr, 0);
        push_opt(&mut ia, OPT_IAADDR, &iaaddr);
    }

    push_opt(&mut out, OPT_IA_NA, &ia);
    out
}

pub fn build_info_request(xid: [u8; 3], duid: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.push(MSG_INFO_REQUEST);
    out.extend_from_slice(&xid);

    push_opt(&mut out, OPT_CLIENTID, duid);
    push_opt(&mut out, OPT_ELAPSED_TIME, &0u16.to_be_bytes());

    let mut oro = Vec::with_capacity(2);
    push_u16(&mut oro, OPT_DNS_SERVERS);
    push_opt(&mut out, OPT_ORO, &oro);

    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedKind {
    Advertise,
    Reply,
    Other(u8),
}

pub struct Parsed<'a> {
    pub kind: ParsedKind,
    pub xid: [u8; 3],
    pub server_id: Option<&'a [u8]>,
    pub client_id: Option<&'a [u8]>,
    pub ia_addr: Option<[u8; 16]>,
    pub dns_count: u8,
}

pub fn parse<'a>(buf: &'a [u8], dns_out: &mut [[u8; 16]]) -> Option<Parsed<'a>> {
    if buf.len() < 4 {
        return None;
    }
    let msg_type = buf[0];
    let xid = [buf[1], buf[2], buf[3]];

    let kind = match msg_type {
        MSG_ADVERTISE => ParsedKind::Advertise,
        MSG_REPLY => ParsedKind::Reply,
        other => ParsedKind::Other(other),
    };

    let mut server_id: Option<&[u8]> = None;
    let mut client_id: Option<&[u8]> = None;
    let mut ia_addr: Option<[u8; 16]> = None;
    let mut dns_count: u8 = 0;

    let mut idx = 4usize;
    while idx + 4 <= buf.len() {
        let code = u16::from_be_bytes([buf[idx], buf[idx + 1]]);
        let len = u16::from_be_bytes([buf[idx + 2], buf[idx + 3]]) as usize;
        idx += 4;
        if idx + len > buf.len() {
            break;
        }
        let data = &buf[idx..idx + len];

        match code {
            OPT_SERVERID => server_id = Some(data),
            OPT_CLIENTID => client_id = Some(data),
            OPT_DNS_SERVERS => {
                let mut off = 0usize;
                while off + 16 <= data.len() && (dns_count as usize) < dns_out.len() {
                    dns_out[dns_count as usize].copy_from_slice(&data[off..off + 16]);
                    dns_count = dns_count.saturating_add(1);
                    off += 16;
                }
            }
            OPT_IA_NA => {
                // IA_NA: 12 bytes header then suboptions.
                if data.len() >= 12 {
                    let mut sub = 12usize;
                    while sub + 4 <= data.len() {
                        let scode = u16::from_be_bytes([data[sub], data[sub + 1]]);
                        let slen = u16::from_be_bytes([data[sub + 2], data[sub + 3]]) as usize;
                        sub += 4;
                        if sub + slen > data.len() {
                            break;
                        }
                        let sdata = &data[sub..sub + slen];
                        if scode == OPT_IAADDR && sdata.len() >= 16 {
                            let mut a = [0u8; 16];
                            a.copy_from_slice(&sdata[..16]);
                            ia_addr = Some(a);
                        }
                        sub += slen;
                    }
                }
            }
            _ => {}
        }

        idx += len;
    }

    Some(Parsed {
        kind,
        xid,
        server_id,
        client_id,
        ia_addr,
        dns_count,
    })
}
