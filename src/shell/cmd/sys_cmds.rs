use crate::shell::table::{Table, TableColumn};
use crate::shell::{CommandAction, ShellBackend, ShellIo};

// use embassy_executor::task;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use crate::v::net::VNet;
use trueos_v::vnet as api;

enum AcpiAction {
    Reset,
    State(u8),
}

fn parse_acpi_state(raw: &str) -> Option<AcpiAction> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("reboot") {
        return Some(AcpiAction::Reset);
    }
    if let Some(rest) = s.strip_prefix('s').or_else(|| s.strip_prefix('S')) {
        return match rest {
            "0" => Some(AcpiAction::State(0)),
            "1" => Some(AcpiAction::State(1)),
            "2" => Some(AcpiAction::State(2)),
            "3" => Some(AcpiAction::State(3)),
            "4" => Some(AcpiAction::State(4)),
            "5" => Some(AcpiAction::State(5)),
            _ => None,
        };
    }
    None
}

pub(crate) fn cmd_acpi(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let print_usage = |io: &dyn ShellIo| {
        let cols = [
            TableColumn {
                header: "State",
                width: 8,
            },
            TableColumn {
                header: "Description",
                width: 32,
            },
        ];
        let t = Table::new(&cols);
        t.print_header(io);
        t.print_row(io, ["reboot", "ACPI reset"]);
        t.print_row(io, ["S0", "Running"]);
        t.print_row(io, ["S1", "Light sleep"]);
        t.print_row(io, ["S2", "Deeper sleep (rare)"]);
        t.print_row(io, ["S3", "Suspend to RAM"]);
        t.print_row(io, ["S4", "Hibernate (suspend to disk)"]);
        t.print_row(io, ["S5", "Soft off (shutdown)"]);
    };

    let Some(state) = args.and_then(|a| a.get_str(0)) else {
        print_usage(ctx.io);
        return CommandAction::None;
    };

    let Some(action) = parse_acpi_state(state) else {
        print_usage(ctx.io);
        return CommandAction::None;
    };

    match action {
        AcpiAction::Reset => CommandAction::Pending(crate::shell::PendingAction::AcpiReset),
        AcpiAction::State(level) => {
            if level == 0 {
                ctx.io.write_str("acpi: already in S0 (running)\r\n");
                return CommandAction::None;
            }
            CommandAction::Pending(crate::shell::PendingAction::AcpiState(level))
        }
    }
}

pub(crate) fn cmd_hv(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    #[inline]
    fn print_usage(io: &dyn ShellIo) {
        io.write_str("hv: usage hv [status|start|stop|log]\r\n");
        io.write_str("hv: single-VM milestone target is vm1\r\n");
    }

    let op = args.and_then(|a| a.get_str(0)).unwrap_or("status").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let s = crate::hv::status();
        ctx.io.write_fmt(format_args!(
            "hv: vmx intel={} msr={} vmx={} fc_lock={} fc_vmx_outside_smx={}\r\n",
            s.vendor_intel as u8,
            s.has_msr as u8,
            s.has_vmx as u8,
            s.feature_control_locked as u8,
            s.feature_control_vmx_outside_smx as u8
        ));
        ctx.io.write_fmt(format_args!(
            "hv: vm1 running={} starting={} marker_seen={} guest_module={}\r\n",
            s.vm1_running as u8,
            s.vm1_starting as u8,
            s.vm1_marker_seen as u8,
            s.guest_module_present as u8
        ));
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("start") {
        // SAFETY: Upgrade IO lifetime for background task. Backend is static (Serial/VGA).
        let io_static: &'static dyn ShellBackend = unsafe { core::mem::transmute(ctx.io) };
        match crate::hv::start(ctx.spawner, io_static) {
            Ok(()) => ctx.io.write_str("hv: vm1 started\r\n"),
            Err(e) => ctx
                .io
                .write_fmt(format_args!("hv: start failed: {:?}\r\n", e)),
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("stop") {
        if crate::hv::stop() {
            ctx.io.write_str("hv: vm1 stop requested\r\n");
        } else {
            ctx.io.write_str("hv: vm1 not running\r\n");
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("log") {
        crate::hv::write_logs(ctx.io);
        return CommandAction::None;
    }

    print_usage(ctx.io);
    CommandAction::None
}

pub(crate) fn cmd_gfx(
    ctx: &mut ShellCommandCtx<'_>,
    _args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    crate::gfx::init(crate::limine::framebuffer_response());
    // match ctx
    //     .spawner
    //     .spawn(trueos_qjs::stream_gfx_smoke::boot_stream_gfx_smoke_task())
    // {
    //     Ok(()) => ctx.io.write_str("gfx: started stream_gfx_smoke task (20Hz)\r\n"),
    //     Err(e) => ctx.io.write_fmt(format_args!(
    //         "gfx: stream_gfx_smoke task spawn failed: {:?}\r\n",
    //         e
    //     )),
    // }
    match ctx.spawner.spawn(trueos_qjs::pixi_ui::boot_pixi_ui_task()) {
        Ok(()) => ctx.io.write_str("gfx: started pixi_ui task (20Hz)\r\n"),
        Err(e) => ctx
            .io
            .write_fmt(format_args!("gfx: pixi_ui task spawn failed: {:?}\r\n", e)),
    }

    // match ctx
    //     .spawner
    //     .spawn(trueos_qjs::webgl_smoke::boot_webgl_smoke_task())
    // {
    //     Ok(()) => ctx.io.write_str("gfx: started webgl_smoke task (20Hz, no-qjs)\r\n"),
    //     Err(e) => ctx.io.write_fmt(format_args!(
    //         "gfx: webgl_smoke task spawn failed: {:?}\r\n",
    //         e
    //     )),
    // }
    CommandAction::None
}

fn smp_state_name(st: u8) -> &'static str {
    match st {
        crate::smp::STATE_IDLE => "idle",
        crate::smp::STATE_PENDING => "pending",
        crate::smp::STATE_RUNNING => "running",
        crate::smp::STATE_DONE => "done",
        _ => "unknown",
    }
}

pub(crate) fn cmd_smp(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    if !crate::smp::is_init() {
        ctx.io.write_str("smp: not initialized\r\n");
        return CommandAction::None;
    }

    let total = crate::smp::cpu_count();
    ctx.io
        .write_fmt(format_args!("smp: cpu_count={}\r\n", total));

    let slot_opt = args.and_then(|a| a.get_usize(0));

    let dump_slot = |slot: usize| {
        let Some(r) = crate::smp::read(slot) else {
            ctx.io
                .write_fmt(format_args!("smp: slot={} <unavailable>\r\n", slot));
            return;
        };
        ctx.io.write_fmt(format_args!(
            "smp: slot={} online={} state={} seq={} ret=0x{:016X}\r\n",
            slot,
            if r.online { 1 } else { 0 },
            smp_state_name(r.state),
            r.seq,
            r.ret
        ));
    };

    if let Some(slot) = slot_opt {
        if slot >= total {
            ctx.io.write_str("smp: usage smp [slot]\r\n");
            return CommandAction::None;
        }
        dump_slot(slot);
        return CommandAction::None;
    }

    for slot in 0..total {
        dump_slot(slot);
    }

    CommandAction::None
}

pub(crate) fn cmd_turbo(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let op = args.and_then(|a| a.get_str(0)).unwrap_or("").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let armed = crate::turbo::armed();
        match crate::turbo::local_state() {
            Ok(st) => {
                ctx.io
                    .write_fmt(format_args!("turbo: armed={} state={:?}\r\n", armed, st));
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io
                    .write_fmt(format_args!("turbo: unsupported (intel-only)\r\n"));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // Reads should never require arming; keep for forward-compat.
                ctx.io.write_fmt(format_args!("turbo: disarmed\r\n"));
            }
        }
        if !armed {
            ctx.io
                .write_str("turbo: writes are disarmed (run 'turbo arm')\r\n");
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("arm") {
        crate::turbo::set_armed(true);
        ctx.io.write_str("turbo: armed\r\n");
        return CommandAction::None;
    }
    if op.eq_ignore_ascii_case("disarm") {
        crate::turbo::set_armed(false);
        ctx.io.write_str("turbo: disarmed\r\n");
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("verify") {
        let spins = args.and_then(|a| a.get_usize(1)).unwrap_or(200_000);

        match crate::turbo::verify_all(spins) {
            Ok(r) => {
                ctx.io.write_fmt(format_args!(
                    "turbo: verify spins={} turbo={} noturbo={} unknown={} completed_aps={}/{} online_aps={} busy={} total_cpus={} seq={}{}\r\n",
                    spins,
                    r.turbo_cpus,
                    r.noturbo_cpus,
                    r.unknown_cpus,
                    r.completed_aps,
                    r.submitted_aps,
                    r.online_aps,
                    r.busy_aps,
                    r.total_cpus,
                    r.seq,
                    if r.timed_out { " TIMEOUT" } else { "" }
                ));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // verify is read-only; keep for forward-compat and clarity.
                ctx.io
                    .write_str("turbo: msr disarmed (verify should not require arm)\r\n");
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
            }
        }

        return CommandAction::None;
    }

    let enable = if op.eq_ignore_ascii_case("on") {
        Some(true)
    } else if op.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    };

    let Some(enable) = enable else {
        ctx.io
            .write_str("turbo: usage turbo [status|arm|disarm|on|off|verify [spins]]\r\n");
        return CommandAction::None;
    };

    match crate::turbo::set_enabled_all(enable) {
        Ok(r) => {
            ctx.io.write_fmt(format_args!(
                "turbo: requested={} ap_submitted={}/{} busy={} total_cpus={} seq={}\r\n",
                if r.requested_enable { "on" } else { "off" },
                r.submitted_aps,
                r.targeted_aps,
                r.busy_aps,
                r.total_cpus,
                r.seq
            ));
        }
        Err(crate::turbo::TurboSetError::Disarmed) => {
            ctx.io
                .write_str("turbo: msr disarmed (run 'turbo arm')\r\n");
        }
        Err(crate::turbo::TurboSetError::Unsupported) => {
            ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
        }
    }

    CommandAction::None
}

pub(crate) fn cmd_pci_usb(
    ctx: &mut ShellCommandCtx<'_>,
    _args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let sub = _args.and_then(|a| a.get_str(0)).unwrap_or("").trim();

    if sub == "dump" {
        ctx.io.write_str(
            "pci.usb: targeted descriptor dump is printed automatically when an unclaimed device matches known LED IDs (0416:A125 or 1462:7E03).\r\n",
        );
        ctx.io
            .write_str("pci.usb: replug the device (or reboot) to re-trigger enumeration.\r\n");
        return CommandAction::None;
    }

    let ctrls = crate::usb::xhci::xhc_list();
    if ctrls.is_empty() {
        ctx.io.write_str("pci.usb: no xhci controllers\r\n");
        return CommandAction::None;
    }

    for info in ctrls.iter() {
        ctx.io.write_fmt(format_args!(
            "pci.usb: xHCI {} {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X} ac64={}\r\n",
            info.controller_id,
            info.bus,
            info.slot,
            info.function,
            info.bar_phys,
            info.bar_size,
            info.supports_64bit
        ));

        let devs = crate::usb::list_device_summaries(info.controller_id);
        if devs.is_empty() {
            ctx.io.write_str("  (no devices)\r\n");
            continue;
        }

        for d in devs.iter() {
            ctx.io.write_fmt(format_args!(
                "  port={} slot={} kind={} vid=0x{:04X} pid=0x{:04X} cls={:02X}/{:02X}/{:02X}\r\n",
                d.port,
                d.slot_id,
                d.kind,
                d.vid.unwrap_or(0),
                d.pid.unwrap_or(0),
                d.class.unwrap_or(0),
                d.subclass.unwrap_or(0),
                d.protocol.unwrap_or(0)
            ));
        }
    }

    CommandAction::None
}

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
    t.print_row(ctx.io, ["net.http", "<url>"]);
    t.print_row(ctx.io, ["net.https", "<host>"]);

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
                        && from == ip && rseq == seq {
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
            && index != target_idx {
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

pub(crate) fn cmd_net_http(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let url = args.and_then(|a| a.get_str(0)).unwrap_or("");
    if url.is_empty() {
        ctx.io
            .write_str("net.http: usage net.http <host|http://url>\r\n");
        ctx.io
            .write_str("net.http: example net.http http://example.com/\r\n");
        ctx.io
            .write_str("net.http: note: plaintext HTTP only (no TLS)\r\n");
        return CommandAction::None;
    }

    let mut title: heapless::String<{ crate::matrix::TITLE_LEN }> = heapless::String::new();
    let _ = title.push_str("get ");
    for ch in url.chars() {
        if title.push(ch).is_err() {
            break;
        }
    }

    match crate::matrix::alloc_slot(title.as_str()) {
        Some(slot) => {
            let mut u: heapless::String<256> = heapless::String::new();
            for ch in url.chars() {
                if u.push(ch).is_err() {
                    break;
                }
            }
            let _ = ctx
                .spawner
                .spawn(crate::tst::html::http_get_matrix_job(slot, u));
            ctx.io
                .write_fmt(format_args!("net.http: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("net.http: matrix full\r\n"),
    }

    CommandAction::None
}

pub(crate) fn cmd_net_https(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let host = args.and_then(|a| a.get_str(0)).unwrap_or("");

    let mut title: heapless::String<{ crate::matrix::TITLE_LEN }> = heapless::String::new();
    let _ = title.push_str("https ");
    let show_host = if host.is_empty() { "example.com" } else { host };
    for ch in show_host.chars() {
        if title.push(ch).is_err() {
            break;
        }
    }

    match crate::matrix::alloc_slot(title.as_str()) {
        Some(slot) => {
            let mut h: heapless::String<96> = heapless::String::new();
            for ch in host.chars() {
                if h.push(ch).is_err() {
                    break;
                }
            }
            let _ = ctx
                .spawner
                .spawn(crate::tst::tls_demo::tls_demo_matrix_job(slot, h));
            ctx.io
                .write_fmt(format_args!("net.https: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("net.https: matrix full\r\n"),
    }

    CommandAction::None
}

#[cfg(feature = "dma_nic_fpga")]
pub(crate) fn cmd_dmafpga(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let Some(arg) = args.and_then(|a| a.get_str(0)) else {
        ctx.io
            .write_str("dmafpga: usage dmafpga <https://url>|status|off\r\n");
        return CommandAction::None;
    };

    if arg.eq_ignore_ascii_case("status") {
        let s = crate::net::dma_fpga_stream_status();
        ctx.io.write_fmt(format_args!(
            "dmafpga: active={} filter={} rx_seen={} matched={} queued={} queue_fail={}\r\n",
            s.active as u8,
            s.filter_enabled as u8,
            s.rx_packets_seen,
            s.rx_packets_matched,
            s.queued_packets,
            s.queue_failures
        ));
        return CommandAction::None;
    }

    if arg.eq_ignore_ascii_case("off") {
        crate::net::dma_fpga_stream_end();
        let s = crate::net::dma_fpga_stream_status();
        ctx.io.write_fmt(format_args!(
            "dmafpga: stopped rx_seen={} matched={} queued={} queue_fail={}\r\n",
            s.rx_packets_seen, s.rx_packets_matched, s.queued_packets, s.queue_failures
        ));
        return CommandAction::None;
    }

    if !arg.starts_with("https://") {
        ctx.io
            .write_str("dmafpga: only https:// URLs are supported\r\n");
        return CommandAction::None;
    }

    let mut url: heapless::String<256> = heapless::String::new();
    for ch in arg.chars() {
        if url.push(ch).is_err() {
            break;
        }
    }
    if url.is_empty() {
        ctx.io.write_str("dmafpga: URL too long/invalid\r\n");
        return CommandAction::None;
    }

    // if ctx.spawner.spawn(net_dmafpga_task(ctx.io, url)).is_err() {
    //    ctx.io.write_str("dmafpga: spawn failed\r\n");
    //    return CommandAction::None;
    // }
    // ctx.io.write_str("dmafpga: started\r\n");
    ctx.io
        .write_str("dmafpga: background task disabled in prepend mode\r\n");
    CommandAction::None
}

#[cfg(feature = "dma_nic_fpga")]
#[task]
async fn net_dmafpga_task(io: &'static dyn ShellBackend, url: heapless::String<256>) {
    if let Err(e) = crate::net::dma_fpga_stream_begin() {
        io.write_fmt(format_args!("dmafpga: begin failed ({})\r\n", e));
        return;
    }

    let Some((host, remote_port)) = parse_https_host_port(url.as_str()) else {
        io.write_str("dmafpga: bad https URL\r\n");
        crate::net::dma_fpga_stream_end();
        return;
    };

    match crate::v::net::dns::resolve_ipv4_primary(
        host.as_str(),
        crate::v::net::dns::DnsConfig::default(),
    )
    .await
    {
        Ok(remote_ip) => {
            let filter = crate::net::DmaFpgaFlowFilter {
                proto: crate::net::DmaFpgaIpProto::Tcp,
                src_ip: Some(remote_ip),
                dst_ip: None,
                src_port: Some(remote_port),
                dst_port: None,
            };
            if let Err(e) = crate::net::dma_fpga_stream_set_filter(filter) {
                io.write_fmt(format_args!("dmafpga: filter set failed ({})\r\n", e));
                crate::net::dma_fpga_stream_end();
                return;
            }
            io.write_fmt(format_args!(
                "dmafpga: filter tcp src={}.{}.{}.{}:{}\r\n",
                remote_ip[0], remote_ip[1], remote_ip[2], remote_ip[3], remote_port
            ));
        }
        Err(err) => {
            io.write_fmt(format_args!("dmafpga: dns failed {:?}\r\n", err));
            crate::net::dma_fpga_stream_end();
            return;
        }
    }

    io.write_fmt(format_args!("dmafpga: fetching {}\r\n", url.as_str()));

    let fetch =
        crate::v::net::https::fetch_https_body_async(url.as_str(), 30_000, 16 * 1024 * 1024).await;

    match fetch {
        Ok(body) => {
            io.write_fmt(format_args!("dmafpga: fetch ok bytes={}\r\n", body.len()));
        }
        Err(e) => {
            io.write_fmt(format_args!("dmafpga: fetch failed {:?}\r\n", e));
        }
    }

    crate::net::dma_fpga_stream_end();
    let s = crate::net::dma_fpga_stream_status();
    io.write_fmt(format_args!(
        "dmafpga: done rx_seen={} matched={} queued={} queue_fail={}\r\n",
        s.rx_packets_seen, s.rx_packets_matched, s.queued_packets, s.queue_failures
    ));
}

#[cfg(feature = "dma_nic_fpga")]
fn parse_https_host_port(url: &str) -> Option<(heapless::String<96>, u16)> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }

    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            let parsed = p.parse::<u16>().ok()?;
            (h, parsed)
        } else {
            (authority, 443u16)
        }
    } else {
        (authority, 443u16)
    };

    if host.is_empty() {
        return None;
    }

    let mut h: heapless::String<96> = heapless::String::new();
    for ch in host.chars() {
        if h.push(ch).is_err() {
            return None;
        }
    }
    Some((h, port))
}
