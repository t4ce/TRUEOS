pub(crate) fn cmd_net(
    ctx: &mut ShellCommandCtx<'_>,
    _args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let cols = [
        TableColumn {
            header: "Subcommand",
            width: 22,
        },
        TableColumn {
            header: "Arguments",
            width: 28,
        },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    t.print_row(ctx.io, ["net.icmp", "<target> [index|vid:pid|bb:dd.f]"]);
    t.print_row(ctx.io, ["net.nic", "[index|vid:pid|bb:dd.f]"]);
    t.print_row(ctx.io, ["net.hostname", "[name]"]);

    CommandAction::None
}

pub(crate) fn cmd_net_icmp(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let target_str = args.and_then(|a| a.get_str(0)).unwrap_or("");
    if target_str.is_empty() {
        ctx.io
            .write_str("net.icmp: usage net.icmp <host> [index|vid:pid|bb:dd.f]\r\n");
        return CommandAction::None;
    }

    let device_index = args.and_then(|a| a.get_str(1)).and_then(|sel| {
        let t = sel.trim();
        if t.is_empty() {
            return None;
        }
        if t.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            return t.parse::<usize>().ok();
        }
        if t.contains('.') && t.contains(':') {
            let (bus_s, rest) = t.split_once(':')?;
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
        if let Some((vid_s, pid_s)) = t.split_once(':') {
            let vid = u16::from_str_radix(vid_s.trim(), 16).ok()?;
            let pid = u16::from_str_radix(pid_s.trim(), 16).ok()?;
            return crate::net::find_device_by_vidpid(vid, pid);
        }
        None
    });

    let mut target: heapless::String<128> = heapless::String::new();
    if target.push_str(target_str).is_err() {
        ctx.io.write_str("net.icmp: target too long\r\n");
        return CommandAction::None;
    }

    // SAFETY: `spawn_and_wait_local()` blocks until the future completes, so `ctx.io`
    // (which is backed by the active shell backend, typically `ReverseOutput`) stays alive.
    // We need a `'static` IO to satisfy the executor API.
    let io_static: &'static dyn ShellBackend = unsafe { core::mem::transmute(ctx.io) };

    crate::wait::spawn_and_wait_local(async move {
        // DNS
        let ip_res = match device_index {
            Some(dev_idx) => {
                crate::v::net::dns::resolve_ipv4_for_device(
                    dev_idx,
                    target.as_str(),
                    crate::v::net::dns::DnsConfig::for_device(dev_idx),
                )
                .await
            }
            None => {
                crate::v::net::dns::resolve_ipv4_primary(
                    target.as_str(),
                    crate::v::net::dns::DnsConfig::default(),
                )
                .await
            }
        };

        let ip = match ip_res {
            Ok(addr) => addr,
            Err(e) => {
                io_static.write_fmt(format_args!("net.icmp: resolve failed {:?}\r\n", e));
                return;
            }
        };

        io_static.write_fmt(format_args!(
            "PING {} ({}.{}.{}.{}): 56 data bytes\r\n",
            target.as_str(),
            ip[0],
            ip[1],
            ip[2],
            ip[3]
        ));

        let vnet = match device_index {
            Some(i) => VNet::open(i),
            None => VNet::open_primary(),
        };
        let Some(vnet) = vnet else {
            io_static.write_str("net.icmp: no network device\r\n");
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
                io_static.write_str("net.icmp: send failed\r\n");
            }

            // Wait for reply
            let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_secs(2);
            let mut got = false;
            while embassy_time::Instant::now() < deadline {
                if let Some(ev) = vnet.pop_event()
                    && let api::Event::IcmpReply {
                        from,
                        seq: rseq,
                        rtt_ms,
                        ..
                    } = ev
                    && from == ip
                    && rseq == seq
                {
                    io_static.write_fmt(format_args!(
                        "64 bytes from {}.{}.{}.{}: icmp_seq={} time={}ms\r\n",
                        from[0], from[1], from[2], from[3], seq, rtt_ms
                    ));
                    got = true;
                    break;
                }
                embassy_time::Timer::after(embassy_time::Duration::from_millis(10)).await;
            }
            if !got {
                io_static.write_fmt(format_args!("net.icmp: request seq={} timeout\r\n", seq));
            }

            seq += 1;
            embassy_time::Timer::after(embassy_time::Duration::from_millis(1000)).await;
        }
    });

    CommandAction::None
}

pub(crate) fn cmd_net_nic(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let target = args.and_then(|a| a.get_str(0)).unwrap_or("");
    let specific_index = if target.is_empty() {
        None
    } else {
        let t = target.trim();
        if t.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            match t.parse::<usize>() {
                Ok(i) => Some(i),
                Err(_) => {
                    ctx.io
                        .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
                    return CommandAction::None;
                }
            }
        } else if t.contains('.') && t.contains(':') {
            // BDF selector: bb:dd.f (hex bus/slot; func dec/hex)
            let Some((bus_s, rest)) = t.split_once(':') else {
                ctx.io
                    .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
                return CommandAction::None;
            };
            let Some((slot_s, func_s)) = rest.split_once('.') else {
                ctx.io
                    .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
                return CommandAction::None;
            };
            let bus = u8::from_str_radix(bus_s.trim(), 16).ok();
            let slot = u8::from_str_radix(slot_s.trim(), 16).ok();
            let func = func_s
                .trim()
                .parse::<u8>()
                .ok()
                .or_else(|| u8::from_str_radix(func_s.trim(), 16).ok());
            let Some((bus, slot, func)) = bus.zip(slot).zip(func).map(|((b, s), f)| (b, s, f))
            else {
                ctx.io
                    .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
                return CommandAction::None;
            };
            crate::net::find_device_by_bdf(bus, slot, func)
        } else if let Some((vid_s, pid_s)) = t.split_once(':') {
            let vid = u16::from_str_radix(vid_s.trim(), 16).ok();
            let pid = u16::from_str_radix(pid_s.trim(), 16).ok();
            let Some((vid, pid)) = vid.zip(pid) else {
                ctx.io
                    .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
                return CommandAction::None;
            };
            crate::net::find_device_by_vidpid(vid, pid)
        } else {
            ctx.io
                .write_str("net.nic: usage net.nic [index|vid:pid|bb:dd.f]\r\n");
            return CommandAction::None;
        }
    };

    let count = crate::net::device_count();
    if count == 0 {
        ctx.io.write_str("net.nic: no nics\r\n");
        return CommandAction::None;
    }

    let cols = [
        TableColumn {
            header: "Idx",
            width: 4,
        },
        TableColumn {
            header: "BDF",
            width: 8,
        },
        TableColumn {
            header: "VID:PID",
            width: 9,
        },
        TableColumn {
            header: "Interface",
            width: 20,
        },
        TableColumn {
            header: "MAC Address",
            width: 17,
        },
        TableColumn {
            header: "IPv4",
            width: 15,
        },
        TableColumn {
            header: "Mode",
            width: 8,
        },
        TableColumn {
            header: "IPv6",
            width: 39,
        },
    ];
    let t = Table::new(&cols);
    t.print_header(ctx.io);

    for index in 0..count {
        if let Some(target_idx) = specific_index
            && index != target_idx
        {
            continue;
        }

        let name = crate::net::device_name_at(index).unwrap_or("Unknown");

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

        let ip_raw = crate::net::adapter::ipv4_at(index);
        let ip = if let Some(ip) = ip_raw {
            alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        } else {
            alloc::string::String::from(" - ")
        };
        let mode = if let Some(has_lease) = crate::net::adapter::dhcp_has_lease_at(index) {
            if has_lease { "dhcp" } else { "fallback" }
        } else {
            "-"
        };

        let ipv6 = if let Some(ip) = crate::net::adapter::ipv6_global_at(index) {
            alloc::format!(
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
            )
        } else if let Some(ip) = crate::net::adapter::ipv6_link_local_at(index) {
            alloc::format!(
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
            )
        } else {
            alloc::string::String::from("::")
        };

        let idx_s = alloc::format!("{}", index);
        t.print_row(
            ctx.io,
            &[idx_s, bdf, vidpid, name.into(), mac, ip, mode.into(), ipv6],
        );
    }

    CommandAction::None
}

pub(crate) fn cmd_net_hostname(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let name = args.and_then(|a| a.get_str(0));
    match name {
        Some(n) => {
            if n.is_empty() {
                ctx.io.write_str("net.hostname: name cannot be empty\r\n");
            } else {
                crate::net::adapter::set_hostname(n);
                ctx.io
                    .write_fmt(format_args!("net.hostname: set to '{}'\r\n", n));
            }
        }
        None => {
            let current = crate::net::adapter::get_hostname();
            ctx.io
                .write_fmt(format_args!("net.hostname: {}\r\n", current));
        }
    }
    CommandAction::None
}