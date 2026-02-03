extern crate alloc;

use core::sync::atomic::{AtomicU16, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet as vnet;

use super::dns::{self, DnsConfig, DnsError};
use super::VNet;

#[derive(Clone, Copy, Debug)]
pub enum PingError {
    NoNic,
    BadHost,
    DnsFailed,
    Timeout,
    SendFailed,
}

#[derive(Clone, Copy, Debug)]
pub struct PingResult {
    pub ip: [u8; 4],
    pub rtt_ms: u32,
}

static PING_SEQ: AtomicU16 = AtomicU16::new(1);

fn alloc_seq() -> u16 {
    let s = PING_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut seq = (s & 0x7FFF) | 0x8000;
    if seq == 0x8000 {
        seq = 0x8001;
    }
    seq
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut idx = 0usize;
    for part in host.split('.') {
        if idx >= 4 {
            return None;
        }
        if part.is_empty() {
            return None;
        }
        if !part.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let v = part.parse::<u8>().ok()?;
        out[idx] = v;
        idx += 1;
    }
    if idx != 4 {
        return None;
    }
    Some(out)
}

pub async fn ping_once(host: &str) -> Result<PingResult, PingError> {
    let host = host.trim();
    if host.is_empty() {
        return Err(PingError::BadHost);
    }

    let ip = if let Some(ip) = parse_ipv4(host) {
        ip
    } else {
        crate::log!("net: ping resolve host={}\n", host);
        match dns::resolve_ipv4_primary(host, DnsConfig::default()).await {
            Ok(ip) => ip,
            Err(err) => {
                crate::log!("net: ping resolve failed host={} err={:?}\n", host, err);
                return Err(match err {
                    DnsError::NoNic | DnsError::Timeout | DnsError::NoAnswer => PingError::DnsFailed,
                    DnsError::BadName => PingError::BadHost,
                });
            }
        }
    };

    crate::log!(
        "net: ping target {}.{}.{}.{}\n",
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );

    let net = VNet::open_primary().ok_or(PingError::NoNic)?;
    let seq = alloc_seq();
    let payload: &[u8] = b"TRUEOS-ping";
    let cmd = vnet::Command::IcmpEcho {
        target: ip,
        seq,
        data: vnet::ByteBuf::from_slice_trunc(payload),
    };
    if net.submit(cmd).is_err() {
        return Err(PingError::SendFailed);
    }

    let deadline = Instant::now() + EmbassyDuration::from_millis(2000);
    loop {
        for _ in 0..64 {
            let Some(ev) = net.pop_event() else {
                break;
            };
            match ev {
                vnet::Event::IcmpReply { from, seq: got, rtt_ms, .. } => {
                    if got == seq && from == ip {
                        crate::log!(
                            "net: ping reply {}.{}.{}.{} seq={} rtt={}ms\n",
                            from[0],
                            from[1],
                            from[2],
                            from[3],
                            got,
                            rtt_ms
                        );
                        return Ok(PingResult { ip: from, rtt_ms });
                    }
                }
                vnet::Event::Error { msg } => {
                    crate::log!("net: ping error seq={} msg={}\n", seq, msg);
                }
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            crate::log!("net: ping timeout seq={}\n", seq);
            return Err(PingError::Timeout);
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}