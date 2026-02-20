use alloc::{format, string::String as AString};

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
        while !crate::v::readiness::is_set(crate::v::readiness::NET_GATEWAY_REACHABLE)
            && Instant::now() < deadline
        {
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        crate::log!(
            "boot-netbench: net_ready={} waited_ms={}\n",
            crate::v::readiness::is_set(crate::v::readiness::NET_GATEWAY_REACHABLE) as u8,
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

        let ip4 = crate::v::net::dns::resolve_ipv4_for_device(
            nic_index,
            host.as_str(),
            crate::v::net::dns::DnsConfig::default(),
        )
        .await;

        let ip6 = if ip4.is_err() {
            crate::v::net::dns::resolve_ipv6_for_device(
                nic_index,
                host.as_str(),
                crate::v::net::dns::DnsConfig::default(),
            )
            .await
            .ok()
        } else {
            None
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
            path.as_str(),
            host.as_str()
        );

        let submitted = if let Ok(ip) = ip4 {
            crate::net::adapter::internal_netbench_submit(nic_index, ip, port, request.as_bytes())
        } else if let Some(ip) = ip6 {
            crate::net::adapter::internal_netbench_submit_v6(
                nic_index,
                ip,
                port,
                request.as_bytes(),
            )
        } else {
            crate::log!("boot-netbench: dns failed host={} (no A/AAAA)\n", host);
            return;
        };

        if !submitted {
            crate::log!("boot-netbench: internal submit rejected (already pending)\n");
            return;
        }

        crate::log!("boot-netbench: internal submit ok\n");
    }
    .await;
}
