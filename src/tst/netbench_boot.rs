use alloc::{format, string::String as AString};
use core::net::Ipv4Addr;

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

    // Note: boot-netbench is IPv4-only for consistency and because our test URL
    // uses the IPv4 host.

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

        // Ensure we start after IPv4 is actually usable (DHCP lease and IPv4 address).
        // This keeps behavior consistent with the IPv4-only benchmark URL.
        let nic_index = crate::net::primary_device_index();
        let deadline = Instant::now() + EmbassyDuration::from_millis(WAIT_FOR_NET_READY_MS);
        loop {
            let has_lease = crate::net::adapter::dhcp_has_lease_at(nic_index).unwrap_or(false);
            let ip4 = crate::net::adapter::ipv4_at(nic_index);
            if has_lease && ip4.is_some() {
                let ip = ip4.unwrap();
                crate::log!(
                    "boot-netbench: ipv4_ready=1 dev={} ip={}.{}.{}.{}\n",
                    nic_index,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );
                break;
            }
            if Instant::now() >= deadline {
                crate::log!(
                    "boot-netbench: ipv4_ready=0 (timeout) dev={}\n",
                    nic_index
                );
                break;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

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

        // Boot netbench is intentionally IPv4-only.
        let ip = match crate::v::net::dns::resolve_ipv4_for_device(
            nic_index,
            host.as_str(),
            crate::v::net::dns::DnsConfig::for_device(nic_index),
        )
        .await
        {
            Ok(ip) => ip,
            Err(e) => {
                crate::log!("boot-netbench: dns failed host={} err={:?}\n", host, e);
                return;
            }
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
            path.as_str(),
            host.as_str()
        );

        let submitted =
            crate::net::adapter::internal_netbench_submit(nic_index, ip, port, request.as_bytes());

        if !submitted {
            crate::log!("boot-netbench: internal submit rejected (already pending)\n");
            return;
        }

        crate::log!("boot-netbench: internal submit ok\n");
    }
    .await;
}
