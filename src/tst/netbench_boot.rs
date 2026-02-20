use alloc::{format, string::String as AString};
use core::net::{Ipv4Addr, Ipv6Addr};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

// Boot-time netbench runner.
//
// This version does NOT pull TCP payload bytes through vnet events (which copy
// into fixed-size ByteBuf chunks). It only resolves DNS and then submits an
// internal netbench run that counts bytes directly inside `NetService`.

const WAIT_FOR_NET_READY_MS: u64 = 15_000;

fn parse_http_url(url: &str) -> Option<(AString, u16, AString)> {
    let mut u = url.trim();
    if let Some(rest) = u.strip_prefix("http://") {
        u = rest;
    } else {
        return None;
    }

    let (hostport, path) = match u.split_once('/') {
        Some((a, b)) => (a, format!("/{}", b)),
        None => (u, AString::from("/")),
    };
    if hostport.is_empty() {
        return None;
    }

    // IPv6 literals require bracket form in URLs.
    if let Some(rest) = hostport.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        if inside.is_empty() {
            return None;
        }
        let _ip6: Ipv6Addr = inside.parse().ok()?;
        let port = if after.is_empty() {
            80
        } else if let Some(p) = after.strip_prefix(':') {
            if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
                p.parse::<u16>().ok()?
            } else {
                return None;
            }
        } else {
            return None;
        };
        return Some((format!("[{}]", inside), port, path));
    }

    let (host, port) = if let Some((h, p)) = hostport.rsplit_once(':') {
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            (h, p.parse::<u16>().ok()?)
        } else {
            (hostport, 80)
        }
    } else {
        (hostport, 80)
    };

    if host.is_empty() {
        return None;
    }

    // Keep Host header format stable for IPv4 literals.
    if host.parse::<Ipv4Addr>().is_ok() {
        return Some((AString::from(host), port, path));
    }

    Some((AString::from(host), port, path))
}

#[embassy_executor::task]
pub async fn boot_netbench_task() {
    async move {
        if crate::net::device_count() == 0 {
            crate::log!("boot-netbench: skipped (no NIC)\n");
            return;
        }

        // Ensure we start after net bring-up is at least somewhat stable.
        // Bound the wait so we still run even if the ICMP reachability probe
        // never flips the flag (common on networks that drop ping).
        let deadline = Instant::now() + EmbassyDuration::from_millis(WAIT_FOR_NET_READY_MS);
        while !crate::v::readiness::is_set(crate::v::readiness::NET_V6_GATEWAY_REACHABLE)
            && Instant::now() < deadline
        {
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        crate::log!(
            "boot-netbench: net_ready_v6={} waited_ms={}\n",
            crate::v::readiness::is_set(crate::v::readiness::NET_V6_GATEWAY_REACHABLE) as u8,
            WAIT_FOR_NET_READY_MS
        );

        // Primary is currently pinned to dev0; keep it explicit here.
        let nic_index = crate::net::primary_device_index();

        crate::log!(
            "boot-netbench: starting dev={} name={}\n",
            nic_index,
            crate::net::device_name_at(nic_index).unwrap_or("Unknown")
        );

        let url = crate::shell::bench::NETBENCH_URL;
        let Some((host, port, path)) = parse_http_url(url) else {
            crate::log!("boot-netbench: bad url={}\n", url);
            return;
        };

        let mut literal_v6: Option<[u8; 16]> = None;

        if let Some(inner) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
            && let Ok(ip6) = inner.parse::<Ipv6Addr>() {
                literal_v6 = Some(ip6.octets());
            }

        // Boot netbench is intentionally IPv6-only.
        let ip6 = if let Some(ip) = literal_v6 {
            ip
        } else {
            match crate::v::net::dns::resolve_ipv6_for_device(
                nic_index,
                host.as_str(),
                crate::v::net::dns::DnsConfig::for_device(nic_index),
            )
            .await
            {
                Ok(ip) => ip,
                Err(_) => {
                    crate::log!("boot-netbench: dns6 failed host={} (no AAAA)\n", host);
                    return;
                }
            }
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
            path.as_str(),
            host.as_str()
        );

        let submitted = crate::net::adapter::internal_netbench_submit_v6(
            nic_index,
            ip6,
            port,
            request.as_bytes(),
        );

        if !submitted {
            crate::log!("boot-netbench: internal submit rejected (already pending)\n");
            return;
        }

        crate::log!("boot-netbench: internal submit ok\n");
    }
    .await;
}
