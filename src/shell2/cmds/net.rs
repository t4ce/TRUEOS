use core::str::SplitWhitespace;

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet as api;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::{TlbTable, print_table};
use crate::r::net::VNet;
use crate::shell2::shell2_cmd::ParseOutcome;

const NET_MENU_HEADERS: [&str; 2] = ["Subcommand", "Arguments"];
const NET_MENU_ROWS: [[&str; 2]; 4] = [
    ["icmp", "<target> [index|vid:pid|bb:dd.f]"],
    ["irc", "<host> [#channel]"],
    ["nic", "[index|vid:pid|bb:dd.f]"],
    ["hostname", "[name]"],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &NET_MENU_HEADERS, &NET_MENU_ROWS);
}

fn print_menu(io: &'static dyn ShellBackend2) {
    print_table(io, &NET_MENU_HEADERS, &NET_MENU_ROWS);
}

fn parse_device_selector(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.as_bytes().iter().all(|b| b.is_ascii_digit()) {
        return trimmed.parse::<usize>().ok();
    }
    if trimmed.contains('.') && trimmed.contains(':') {
        let (bus_s, rest) = trimmed.split_once(':')?;
        let (slot_s, func_s) = rest.split_once('.')?;
        let bus = u8::from_str_radix(bus_s.trim(), 16).ok()?;
        let slot = u8::from_str_radix(slot_s.trim(), 16).ok()?;
        let func = func_s
            .trim()
            .parse::<u8>()
            .ok()
            .or_else(|| u8::from_str_radix(func_s.trim(), 16).ok())?;
        return crate::net::find_device_by_bdf(bus, slot, func);
    }
    if let Some((vid_s, pid_s)) = trimmed.split_once(':') {
        let vid = u16::from_str_radix(vid_s.trim(), 16).ok()?;
        let pid = u16::from_str_radix(pid_s.trim(), 16).ok()?;
        return crate::net::find_device_by_vidpid(vid, pid);
    }
    None
}

fn ipv6_text(index: usize) -> alloc::string::String {
    let ip = crate::net::adapter::ipv6_global_at(index)
        .or_else(|| crate::net::adapter::ipv6_link_local_at(index));
    if let Some(ip) = ip {
        return alloc::format!(
            "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            ip[4],
            ip[5],
            ip[6],
            ip[7],
            ip[8],
            ip[9],
            ip[10],
            ip[11],
            ip[12],
            ip[13],
            ip[14],
            ip[15]
        );
    }
    alloc::string::String::from("::")
}

fn cmd_net_icmp(
    io: &'static dyn ShellBackend2,
    target_str: &str,
    selector: Option<&str>,
    extra: Option<&str>,
) {
    if target_str.is_empty() || extra.is_some() {
        line(io, "net: usage `net icmp <host> [index|vid:pid|bb:dd.f]`");
        return;
    }

    let device_index = match selector {
        Some(sel) => match parse_device_selector(sel) {
            Some(index) => Some(index),
            None => {
                line(io, "net: usage `net icmp <host> [index|vid:pid|bb:dd.f]`");
                return;
            }
        },
        None => None,
    };

    let mut target: heapless::String<128> = heapless::String::new();
    if target.push_str(target_str).is_err() {
        line(io, "net icmp: target too long");
        return;
    }

    crate::wait::spawn_and_wait_local(async move {
        let ip_res = match device_index {
            Some(dev_idx) => {
                crate::r::net::dns::resolve_ipv4_for_device(
                    dev_idx,
                    target.as_str(),
                    crate::r::net::dns::DnsConfig::for_device(dev_idx),
                )
                .await
            }
            None => {
                crate::r::net::dns::resolve_ipv4_primary(
                    target.as_str(),
                    crate::r::net::dns::DnsConfig::default(),
                )
                .await
            }
        };

        let ip = match ip_res {
            Ok(addr) => addr,
            Err(err) => {
                let msg = alloc::format!("net icmp: resolve failed {:?}", err);
                line(io, msg.as_str());
                return;
            }
        };

        let header = alloc::format!(
            "PING {} ({}.{}.{}.{}): 56 data bytes",
            target.as_str(),
            ip[0],
            ip[1],
            ip[2],
            ip[3]
        );
        line(io, header.as_str());

        let vnet = match device_index {
            Some(index) => VNet::open(index),
            None => VNet::open_primary(),
        };
        let Some(vnet) = vnet else {
            line(io, "net icmp: no network device");
            return;
        };

        let mut seq = 1u16;
        for _ in 0..4 {
            let payload = [0u8; 56];
            if vnet
                .submit(api::Command::IcmpEcho {
                    target: ip,
                    seq,
                    data: api::ByteBuf::from_slice_trunc(&payload),
                })
                .is_err()
            {
                line(io, "net icmp: send failed");
            }

            let deadline = Instant::now() + EmbassyDuration::from_secs(2);
            let mut got = false;
            while Instant::now() < deadline {
                if let Some(ev) = vnet.pop_event()
                    && let api::Event::IcmpReply {
                        from,
                        seq: reply_seq,
                        rtt_ms,
                        ..
                    } = ev
                    && from == ip
                    && reply_seq == seq
                {
                    let msg = alloc::format!(
                        "64 bytes from {}.{}.{}.{}: icmp_seq={} time={}ms",
                        from[0],
                        from[1],
                        from[2],
                        from[3],
                        seq,
                        rtt_ms
                    );
                    line(io, msg.as_str());
                    got = true;
                    break;
                }
                Timer::after(EmbassyDuration::from_millis(10)).await;
            }

            if !got {
                let msg = alloc::format!("net icmp: request seq={} timeout", seq);
                line(io, msg.as_str());
            }

            seq = seq.wrapping_add(1);
            Timer::after(EmbassyDuration::from_millis(1000)).await;
        }
    });
}

fn cmd_net_irc(
    io: &'static dyn ShellBackend2,
    host: &str,
    channel: Option<&str>,
    extra: Option<&str>,
) {
    if host.is_empty() || extra.is_some() {
        line(io, "net: usage `net irc <host> [#channel]`");
        return;
    }

    let mut server: heapless::String<128> = heapless::String::new();
    if server.push_str(host).is_err() {
        line(io, "net irc: host too long");
        return;
    }

    let mut chan: Option<heapless::String<64>> = None;
    if let Some(ch) = channel {
        let mut s: heapless::String<64> = heapless::String::new();
        if s.push_str(ch).is_err() {
            line(io, "net irc: channel too long");
            return;
        }
        chan = Some(s);
    }

    crate::wait::spawn_and_wait_local(async move {
        use crate::r::net::cli::irc::{IRC_DEFAULT_PORT, IrcError, IrcSession};

        let msg = alloc::format!("irc: connecting {}:{}", server.as_str(), IRC_DEFAULT_PORT);
        line(io, msg.as_str());

        let mut session = match IrcSession::connect(
            server.as_str(),
            IRC_DEFAULT_PORT,
            crate::r::net::NetProfile::default(),
            10_000,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                let msg = alloc::format!("irc: connect failed {:?}", e);
                line(io, msg.as_str());
                return;
            }
        };

        if let Err(e) = session
            .register("trueos", "trueos", "TrueOS kernel", 15_000)
            .await
        {
            let msg = alloc::format!("irc: register failed {:?}", e);
            line(io, msg.as_str());
            let _ = session.quit("bye", 1_000).await;
            return;
        }
        line(io, "irc: registered");

        if let Some(ch) = chan.as_ref() {
            let msg = alloc::format!("irc: joining {}", ch.as_str());
            line(io, msg.as_str());
            if let Err(e) = session.join(ch.as_str()) {
                let msg = alloc::format!("irc: join failed {:?}", e);
                line(io, msg.as_str());
            }
        }

        let deadline = Instant::now() + EmbassyDuration::from_secs(10);
        while Instant::now() < deadline {
            match session.recv_line(500).await {
                Ok(Some(l)) => line(io, l.as_str()),
                Ok(None) => {}
                Err(_) => break,
            }
        }

        let _ = session.quit("bye", 2_000).await;
        line(io, "irc: done");
    });
}

fn cmd_net_nic(io: &'static dyn ShellBackend2, selector: Option<&str>, extra: Option<&str>) {
    if extra.is_some() {
        line(io, "net: usage `net nic [index|vid:pid|bb:dd.f]`");
        return;
    }

    let specific_index = match selector {
        Some(sel) => match parse_device_selector(sel) {
            Some(index) => Some(index),
            None => {
                line(io, "net: usage `net nic [index|vid:pid|bb:dd.f]`");
                return;
            }
        },
        None => None,
    };

    let count = crate::net::device_count();
    if count == 0 {
        line(io, "net nic: no nics");
        return;
    }

    let headers = [
        "Idx",
        "BDF",
        "VID:PID",
        "Interface",
        "MAC",
        "IPv4",
        "Mode",
        "IPv6",
    ];
    let table = TlbTable::with_width(&headers, line_width_for_backend(io).saturating_sub(2));
    table.emit_header(|text| line(io, text));

    for index in 0..count {
        if let Some(target_idx) = specific_index
            && index != target_idx
        {
            continue;
        }

        let idx = alloc::format!("{}", index);
        let bdf = if let Some((bus, slot, func)) = crate::net::bdf_at(index) {
            alloc::format!("{:02x}:{:02x}.{}", bus, slot, func)
        } else {
            alloc::string::String::from("-")
        };
        let vidpid = if let Some((vid, pid)) = crate::net::pci_id_at(index) {
            alloc::format!("{:04x}:{:04x}", vid, pid)
        } else {
            alloc::string::String::from("-")
        };
        let iface = crate::net::device_name_at(index).unwrap_or("Unknown");
        let mac_raw = crate::net::mac_address_at(index).unwrap_or([0; 6]);
        let mac = alloc::format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac_raw[0],
            mac_raw[1],
            mac_raw[2],
            mac_raw[3],
            mac_raw[4],
            mac_raw[5]
        );
        let ipv4 = if let Some(ip) = crate::net::adapter::ipv4_at(index) {
            alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        } else {
            alloc::string::String::from("-")
        };
        let mode = if let Some(has_lease) = crate::net::adapter::dhcp_has_lease_at(index) {
            if has_lease { "dhcp" } else { "fallback" }
        } else {
            "-"
        };
        let ipv6 = ipv6_text(index);

        table.emit_row(
            &[&idx, &bdf, &vidpid, iface, &mac, &ipv4, mode, &ipv6],
            |text| line(io, text),
        );
    }

    table.emit_footer(|text| line(io, text));
}

fn cmd_net_hostname(io: &'static dyn ShellBackend2, name: Option<&str>, extra: Option<&str>) {
    if extra.is_some() {
        line(io, "net: usage `net hostname [name]`");
        return;
    }

    match name {
        Some("") => line(io, "net hostname: name cannot be empty"),
        Some(hostname) => {
            crate::net::adapter::set_hostname(hostname);
            let msg = alloc::format!("net hostname: set to '{}'", hostname);
            line(io, msg.as_str());
        }
        None => {
            let current = crate::net::adapter::get_hostname();
            let msg = alloc::format!("net hostname: {}", current);
            line(io, msg.as_str());
        }
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None => print_menu(io),
        Some("icmp") => {
            let target = args.next().unwrap_or("");
            let selector = args.next();
            let extra = args.next();
            cmd_net_icmp(io, target, selector, extra);
        }
        Some("irc") => {
            let host = args.next().unwrap_or("");
            let channel = args.next();
            let extra = args.next();
            cmd_net_irc(io, host, channel, extra);
        }
        Some("nic") => {
            let selector = args.next();
            let extra = args.next();
            cmd_net_nic(io, selector, extra);
        }
        Some("hostname") => {
            let name = args.next();
            let extra = args.next();
            cmd_net_hostname(io, name, extra);
        }
        Some(_) => print_usage(io),
    }

    ParseOutcome::Handled
}
