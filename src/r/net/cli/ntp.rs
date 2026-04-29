#![allow(dead_code)]

use alloc::{format, string::String};
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use v::vnet;

use crate::t::net::dns::{self, DnsConfig};
use crate::r::net::{NetProfile, VNet};

const NTP_SERVER_HOSTS: [&str; 4] = [
    "time.google.com",
    "time.cloudflare.com",
    "time.nist.gov",
    "pool.ntp.org",
];
const NTP_PORT: u16 = 123;
const NTP_TIMEOUT_MS: u64 = 4000;
const NTP_REFRESH_SECS: u64 = 60;
const SECONDS_PER_DAY: u64 = 86_400;
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

// NTP packet payload is 48 bytes = 12 32-bit words.
static NTP_FRAME_WORDS: Mutex<[u32; 12]> = Mutex::new([0u32; 12]);
static NTP_LAST_SYNC_UNIX_SECS: AtomicU64 = AtomicU64::new(0);
static NTP_LAST_SYNC_TICKS: AtomicU64 = AtomicU64::new(0);
static NTP_LOCAL_PORT_SEQ: AtomicU16 = AtomicU16::new(0);
static NTP_HOST_SEQ: AtomicU16 = AtomicU16::new(0);
const NTP_UNIX_EPOCH_OFFSET: u64 = 2_208_988_800;

#[inline]
fn alloc_local_port() -> u16 {
    // Ephemeral range fragment to reduce collisions with other UDP users.
    let seq = NTP_LOCAL_PORT_SEQ.fetch_add(1, Ordering::Relaxed);
    49152u16.wrapping_add(seq % 8192)
}

#[inline]
fn parse_ntp_words(packet: &[u8]) -> Option<[u32; 12]> {
    if packet.len() < 48 {
        return None;
    }

    let mut out = [0u32; 12];
    let mut i = 0usize;
    while i < 12 {
        let off = i * 4;
        out[i] = u32::from_be_bytes([
            packet[off],
            packet[off + 1],
            packet[off + 2],
            packet[off + 3],
        ]);
        i += 1;
    }
    Some(out)
}

fn build_ntp_request() -> [u8; 48] {
    let mut req = [0u8; 48];
    // LI=0, VN=4, Mode=3 (client)
    req[0] = 0x23;
    req[2] = 6;
    req[3] = 0xEC;

    if let Some(unix) = crate::time::unix_time_seconds() {
        let ntp_secs = unix.saturating_add(NTP_UNIX_EPOCH_OFFSET);
        req[40..44].copy_from_slice(&(ntp_secs as u32).to_be_bytes());
    }

    req
}

#[inline]
fn unix_from_ntp_frame(words: &[u32; 12]) -> Option<u64> {
    // Transmit timestamp seconds is word 10 in the 12-word NTP frame.
    let ntp_secs = words[10] as u64;
    if ntp_secs < NTP_UNIX_EPOCH_OFFSET {
        return None;
    }
    Some(ntp_secs - NTP_UNIX_EPOCH_OFFSET)
}

async fn open_udp(net: &VNet, local_port: u16) -> Option<vnet::NetHandle> {
    let _ = net.submit(vnet::Command::OpenUdp { port: local_port });

    let deadline = Instant::now() + EmbassyDuration::from_millis(NTP_TIMEOUT_MS);
    loop {
        for _ in 0..64 {
            let Some(ev) = net.pop_event() else {
                break;
            };
            if let vnet::Event::Opened { handle, kind } = ev
                && kind == vnet::SocketKind::Udp
            {
                return Some(handle);
            }
        }

        if Instant::now() >= deadline {
            return None;
        }

        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}

#[inline]
fn next_ntp_host() -> &'static str {
    let idx = (NTP_HOST_SEQ.fetch_add(1, Ordering::Relaxed) as usize) % NTP_SERVER_HOSTS.len();
    NTP_SERVER_HOSTS[idx]
}

async fn query_ntp_words_for_device(dev_idx: usize, host: &str) -> Option<[u32; 12]> {
    let dns_cfg = DnsConfig::for_device(dev_idx);
    let remote_ip = dns::resolve_ipv4_for_device(dev_idx, host, dns_cfg)
        .await
        .ok()?;

    let net = VNet::open(dev_idx)?;
    let udp = open_udp(&net, alloc_local_port()).await?;

    let req = build_ntp_request();
    let _ = net.submit(vnet::Command::SendUdp {
        handle: udp,
        remote: vnet::EndpointV4 {
            addr: remote_ip,
            port: NTP_PORT,
        },
        data: vnet::ByteBuf::from_slice_trunc(&req),
    });

    let deadline = Instant::now() + EmbassyDuration::from_millis(NTP_TIMEOUT_MS);
    loop {
        for _ in 0..64 {
            let Some(ev) = net.pop_event() else {
                break;
            };
            if let vnet::Event::UdpPacket { handle, from, data } = ev {
                if handle != udp || from.port != NTP_PORT || from.addr != remote_ip {
                    continue;
                }

                let _ = net.submit(vnet::Command::Close { handle: udp });
                return parse_ntp_words(data.as_slice());
            }
        }

        if Instant::now() >= deadline {
            let _ = net.submit(vnet::Command::Close { handle: udp });
            return None;
        }

        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}

#[inline]
pub fn ntp_frame_snapshot() -> [u32; 12] {
    *NTP_FRAME_WORDS.lock()
}

#[inline]
pub fn ntp_last_sync_unix_seconds() -> Option<u64> {
    let v = NTP_LAST_SYNC_UNIX_SECS.load(Ordering::Acquire);
    if v == 0 { None } else { Some(v) }
}

#[inline]
pub fn current_unix_seconds() -> Option<u64> {
    let base = NTP_LAST_SYNC_UNIX_SECS.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }

    let synced_at_ticks = NTP_LAST_SYNC_TICKS.load(Ordering::Acquire);
    let now_ticks = embassy_time_driver::now();
    let elapsed_ticks = now_ticks.saturating_sub(synced_at_ticks);
    let hz = embassy_time_driver::TICK_HZ;
    let elapsed_secs = if hz == 0 { 0 } else { elapsed_ticks / hz };
    Some(base.saturating_add(elapsed_secs))
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = (yoe as i32) + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

pub fn kernel_date_day_month_year() -> String {
    let unix_secs = current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or(0);
    let days = (unix_secs / SECONDS_PER_DAY) as i64;
    let (year, month, day) = civil_from_days(days);
    let month_name = MONTH_NAMES[(month.saturating_sub(1) as usize).min(MONTH_NAMES.len() - 1)];
    format!("{} {} {}", day, month_name, year)
}

#[embassy_executor::task]
pub async fn ntp_sync_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let profile = NetProfile::default();
        let Some(dev_idx) = profile.resolve_device_index() else {
            Timer::after(EmbassyDuration::from_secs(NTP_REFRESH_SECS)).await;
            continue;
        };
        let host = next_ntp_host();

        if let Some(words) = query_ntp_words_for_device(dev_idx, host).await {
            *NTP_FRAME_WORDS.lock() = words;
            if let Some(unix) = unix_from_ntp_frame(&words) {
                NTP_LAST_SYNC_UNIX_SECS.store(unix, Ordering::Release);
                NTP_LAST_SYNC_TICKS.store(embassy_time_driver::now(), Ordering::Release);
            }
        }

        Timer::after(EmbassyDuration::from_secs(NTP_REFRESH_SECS)).await;
    }
}
