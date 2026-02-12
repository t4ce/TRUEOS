use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt::Write;

use embassy_executor::task;

use crate::shell::{ShellBackend, ShellIo};

use crate::shell::cmdreg::{
    ArgValue, ParsedArgs, ShellCommandCtx, ShellCommand,
};


pub(crate) fn cmd_cmd(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmdreg::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    ctx.io.write_str("\r\n");
    
    let light_green = (100, 255, 100);
    let mut col_count = 0;
    
    for name in cmds {
        // Skip subcommands (containing dot)
        if name.contains('.') {
            continue;
        }

        // [name] + space
        let len = name.len() + 3; 
        if col_count + len > *ctx.term_cols {
            ctx.io.write_str("\r\n");
            col_count = 0;
        }
        
        ctx.io.write_str("[");
        ctx.io.write_fmt(format_args!("{}", crate::ecma48::color(name, light_green)));
        ctx.io.write_str("] ");
        
        col_count += len;
    }
    ctx.io.write_str("\r\n");

    super::CommandAction::None
}

pub(crate) fn cmd_section(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    // No args: list slots.
    let Some(args) = args else {
        let mut buf: heapless::String<512> = heapless::String::new();
        crate::matrix::list_slots(&mut buf);
        ctx.io.write_str(buf.as_str());
        if let Some(active) = crate::shell::statusbar::active_slot() {
            ctx.io.write_fmt(format_args!("status: active §{}\r\n", active + 1));
        } else {
            ctx.io.write_str("status: active (none)\r\n");
        }
        return super::CommandAction::None;
    };

    // With id: set active status slot.
    let id = args.get(0).and_then(|v| v.as_u8()).unwrap_or(0);
    if id == 0 {
        ctx.io.write_str("§: ids are 1..\r\n");
        return super::CommandAction::None;
    }
    let slot_id = id - 1;

    if crate::shell::statusbar::set_active_slot(slot_id) {
        ctx.io.write_fmt(format_args!("status: active §{}\r\n", id));
    } else {
        ctx.io.write_str("§: not found\r\n");
    }

    super::CommandAction::None
}

pub(crate) fn cmd_ecma48(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    // Escaped IO not needed, forward not possible with new lifetimes easily for spawned tasks
    ctx.io.write_str("ecma48: local echo only in prepend mode\r\n");
    let arg = args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    super::ecma48::handle_ecma48(ctx.io, arg);
    super::CommandAction::None
}

pub(crate) fn cmd_net(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    ctx.io.write_str("net: available subcommands\r\n");
    ctx.io.write_str("  net.icmp <target>\r\n");
    ctx.io.write_str("  net.mac [index]\r\n");
    ctx.io.write_str("  net.http <url>\r\n");
    ctx.io.write_str("  net.https <host>\r\n");
    super::CommandAction::None
}

pub(crate) fn cmd_net_icmp(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let target = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    if target.is_empty() {
        ctx.io.write_str("net.icmp: usage net.icmp <host>\r\n");
        return super::CommandAction::None;
    }

    ctx.io.write_fmt(format_args!("net.icmp: ping {}\r\n", target));
    let mut t: heapless::String<64> = heapless::String::new();
    for ch in target.chars() {
        if t.push(ch).is_err() {
            break;
        }
    }
        // Disabled ping spawn as it requires static io lifetime, incompatible with prepend mode
        // if ctx.spawner.spawn(net_ping_task(ctx.io, t)).is_err() {
        //    ctx.io.write_str("net: ping spawn failed\r\n");
        // }
        ctx.io.write_str("net: ping background task disabled in prepend mode\r\n");

    super::CommandAction::None
}

pub(crate) fn cmd_net_mac(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let target = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    
    if target.is_empty() {
        let count = crate::net::device_count();
        if count == 0 {
            ctx.io.write_str("net.mac: no nics\r\n");
            return super::CommandAction::None;
        }
        for index in 0..count {
            let mac = if index == 0 {
                crate::net::mac_address()
            } else {
                crate::net::mac_address_at(index)
            };
            if let Some([a, b, c, d, e, f]) = mac {
                ctx.io.write_fmt(format_args!(
                    "net.mac: mac[{}]={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\r\n",
                    index, a, b, c, d, e, f
                ));
            } else {
                ctx.io.write_fmt(format_args!("net.mac: mac[{}]=unavailable\r\n", index));
            }
        }
    } else if let Ok(index) = target.parse::<usize>() {
        let mac = if index == 0 {
            crate::net::mac_address()
        } else {
            crate::net::mac_address_at(index)
        };

        if let Some([a, b, c, d, e, f]) = mac {
            ctx.io.write_fmt(format_args!(
                "net.mac: mac[{}]={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\r\n",
                index, a, b, c, d, e, f
            ));
        } else {
            ctx.io.write_fmt(format_args!("net.mac: mac[{}]=unavailable\r\n", index));
        }
    } else {
        ctx.io.write_str("net.mac: usage net.mac [index]\r\n");
    }
    super::CommandAction::None
}

pub(crate) fn cmd_net_http(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let url = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        ctx.io.write_str("net.http: usage net.http <host|http://url>\r\n");
        ctx.io.write_str("net.http: example net.http http://example.com/\r\n");
        ctx.io.write_str("net.http: note: plaintext HTTP only (no TLS)\r\n");
        return super::CommandAction::None;
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
                let _ = ctx.spawner.spawn(crate::tst::html::http_get_matrix_job(slot, u),
                );
            ctx.io.write_fmt(format_args!("net.http: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("net.http: matrix full\r\n"),
    }

    super::CommandAction::None
}

pub(crate) fn cmd_net_https(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let host = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");

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
                let _ = ctx.spawner.spawn(crate::tst::tls_demo::tls_demo_matrix_job(slot, h),
                );
            ctx.io.write_fmt(format_args!("net.https: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("net.https: matrix full\r\n"),
    }

    super::CommandAction::None
}

pub(crate) fn cmd_dmafpga(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(arg) = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()) else {
        ctx.io.write_str("dmafpga: usage dmafpga <https://url>|status|off\r\n");
        return super::CommandAction::None;
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
        return super::CommandAction::None;
    }

    if arg.eq_ignore_ascii_case("off") {
        crate::net::dma_fpga_stream_end();
        let s = crate::net::dma_fpga_stream_status();
        ctx.io.write_fmt(format_args!(
            "dmafpga: stopped rx_seen={} matched={} queued={} queue_fail={}\r\n",
            s.rx_packets_seen, s.rx_packets_matched, s.queued_packets, s.queue_failures
        ));
        return super::CommandAction::None;
    }

    if !arg.starts_with("https://") {
        ctx.io.write_str("dmafpga: only https:// URLs are supported\r\n");
        return super::CommandAction::None;
    }

    let mut url: heapless::String<256> = heapless::String::new();
    for ch in arg.chars() {
        if url.push(ch).is_err() {
            break;
        }
    }
    if url.is_empty() {
        ctx.io.write_str("dmafpga: URL too long/invalid\r\n");
        return super::CommandAction::None;
    }

    // if ctx.spawner.spawn(net_dmafpga_task(ctx.io, url)).is_err() {
    //    ctx.io.write_str("dmafpga: spawn failed\r\n");
    //    return super::CommandAction::None;
    // }
    // ctx.io.write_str("dmafpga: started\r\n");
    ctx.io.write_str("dmafpga: background task disabled in prepend mode\r\n");
    super::CommandAction::None
}

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

    match crate::v::net::dns::resolve_ipv4_primary(host.as_str(), crate::v::net::dns::DnsConfig::default()).await {
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

    let fetch = crate::v::net::https::fetch_https_body_async(url.as_str(), 30_000, 16 * 1024 * 1024).await;

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

#[task]
async fn net_ping_task(io: &'static dyn ShellBackend, target: heapless::String<64>) {
    let res = crate::v::net::ping::ping_once(target.as_str()).await;
    match res {
        Ok(result) => {
            let [a, b, c, d] = result.ip;
            io.write_fmt(format_args!(
                "net: reply from {}.{}.{}.{} rtt={}ms\r\n",
                a, b, c, d, result.rtt_ms
            ));
        }
        Err(err) => match err {
            crate::v::net::ping::PingError::NoNic => {
                io.write_str("net: ping failed (no nic)\r\n");
            }
            crate::v::net::ping::PingError::BadHost => {
                io.write_str("net: ping failed (bad host)\r\n");
            }
            crate::v::net::ping::PingError::DnsFailed => {
                io.write_str("net: ping failed (dns)\r\n");
            }
            crate::v::net::ping::PingError::Timeout => {
                io.write_str("net: ping timeout\r\n");
            }
            crate::v::net::ping::PingError::SendFailed => {
                io.write_str("net: ping failed (send)\r\n");
            }
        },
    }
}

pub(crate) fn cmd_update(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::UpdateSelectDisk);
    super::CommandAction::ShowUpdateDiskTable
}

pub(crate) fn cmd_install(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::SelectDisk);
    super::CommandAction::ShowInstallDiskTable
}

pub(crate) fn cmd_format(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::FormatSelectDisk);
    super::CommandAction::ShowFormatDiskTable
}

pub(crate) fn cmd_file(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::FileSelectMount);
    super::CommandAction::ShowFileMountTable
}

pub(crate) fn cmd_bench(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::BenchSelectDisk);
    super::CommandAction::ShowBenchDiskTable
}

pub(crate) fn cmd_netbench(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::NetbenchSelectNic);
    super::CommandAction::ShowNetbenchNicTable
}

pub(crate) fn cmd_qjs(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let src = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    let src = src.trim();
    if src.is_empty() {
        super::shellqjs::help(ctx.io);
        super::CommandAction::None
    } else {
        let mut buf: heapless::String<192> = heapless::String::new();
        for ch in src.chars() {
            if buf.push(ch).is_err() {
                break;
            }
        }
        super::CommandAction::Qjs { src: buf }
    }
}

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

pub(crate) fn cmd_acpi(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    #[inline]
    fn print_acpi_usage(io: &dyn ShellIo) {
        io.write_str("acpi: usage acpi <reboot|s0|s1|s2|s3|s4|s5>\r\n");
        io.write_str("reboot = ACPI reset\r\n");
        io.write_str("S0 = running\r\n");
        io.write_str("S1 = light sleep\r\n");
        io.write_str("S2 = deeper sleep (rare)\r\n");
        io.write_str("S3 = suspend to RAM\r\n");
        io.write_str("S4 = hibernate (suspend to disk)\r\n");
        io.write_str("S5 = soft off (shutdown)\r\n");
    }

    let Some(state) = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()) else {
        print_acpi_usage(ctx.io);
        return super::CommandAction::None;
    };

    let Some(action) = parse_acpi_state(state) else {
        print_acpi_usage(ctx.io);
        return super::CommandAction::None;
    };

    match action {
        AcpiAction::Reset => super::CommandAction::Pending(super::PendingAction::AcpiReset),
        AcpiAction::State(level) => {
            if level == 0 {
                ctx.io.write_str("acpi: already in S0 (running)\r\n");
                return super::CommandAction::None;
            }
            super::CommandAction::Pending(super::PendingAction::AcpiState(level))
        }
    }
}

pub(crate) fn cmd_hv(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    #[inline]
    fn print_usage(io: &dyn ShellIo) {
        io.write_str("hv: usage hv [status|start|stop|log]\r\n");
        io.write_str("hv: single-VM milestone target is vm1\r\n");
    }

    let op = args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("status")
        .trim();

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
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("start") {
        ctx.io.write_str("hv: start disabled in prepend mode (lifetime issues)\r\n");
        /*
        match crate::hv::start(ctx.spawner, ctx.io) {
            Ok(()) => ctx.io.write_str("hv: vm1 start queued\r\n"),
            Err(crate::hv::StartError::AlreadyRunning) => {
                ctx.io.write_str("hv: vm1 already running\r\n")
            }
            Err(crate::hv::StartError::VmxUnsupported) => {
                ctx.io.write_str("hv: vmx preflight failed (run 'hv status')\r\n")
            }
            Err(crate::hv::StartError::MissingGuestModule) => {
                ctx.io.write_str("hv: missing kernel file\r\n")
            }
            Err(crate::hv::StartError::SpawnFailed) => {
                ctx.io.write_str("hv: vm1 spawn failed\r\n")
            }
        }
        */
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("stop") {
        if crate::hv::stop() {
            ctx.io.write_str("hv: vm1 stop requested\r\n");
        } else {
            ctx.io.write_str("hv: vm1 not running\r\n");
        }
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("log") {
        crate::hv::write_logs(ctx.io);
        return super::CommandAction::None;
    }

    print_usage(ctx.io);
    super::CommandAction::None
}

pub(crate) fn cmd_go(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::EnterGo
}

pub(crate) fn cmd_mandel(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    crate::vga::draw_mandelbrot();
    ctx.io.write_str("mandel ok\r\n");
    super::CommandAction::None
}

pub(crate) fn cmd_set(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("set: usage set <cols> <rows>\r\n");
        return super::CommandAction::None;
    };

    let cols = args.get(0).and_then(|v| v.as_usize()).unwrap_or(0);
    let rows = args.get(1).and_then(|v| v.as_usize()).unwrap_or(0);

    if cols == 0 || rows == 0 {
        ctx.io.write_str("set: cols/rows must be >= 1\r\n");
        ctx.io.write_str("usage: set <cols> <rows>\r\n");
        return super::CommandAction::None;
    }

    *ctx.term_cols = cols;
    *ctx.term_rows = rows;

    super::apply_shell_scroll_region(ctx.io, rows);
    // Restore cursor to safe area (Row 3) because DECSTBM resets to (1,1)
    ctx.io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));

    let mut buf: heapless::String<64> = heapless::String::new();
    let _ = write!(&mut buf, "term set: {}x{}\r\n", cols, rows);
    ctx.io.write_str(buf.as_str());
    super::draw_corners(ctx.io, cols, rows);
    super::CommandAction::None
}

pub(crate) fn cmd_idle(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let policy = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("").trim();
    if policy.is_empty() {
        ctx.io
            .write_fmt(format_args!("idle: {}\r\n", crate::power::idle_policy().as_str()));
        return super::CommandAction::None;
    }

    let policy = match policy {
        "spin" => crate::power::IdlePolicy::Spin,
        "hlt" => crate::power::IdlePolicy::Halt,
        _ => {
            ctx.io.write_str("idle: usage idle [spin|hlt]\r\n");
            return super::CommandAction::None;
        }
    };
    let prev = crate::power::set_idle_policy(policy);
    ctx.io.write_fmt(format_args!(
        "idle: {} -> {}\r\n",
        prev.as_str(),
        policy.as_str()
    ));
    super::CommandAction::None
}

pub(crate) fn cmd_pstate(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let ratio = args.and_then(|a| a.get(0)).and_then(|v| v.as_u64());

    if ratio.is_none() {
        let cur = crate::power::current_ratio();
        let armed = crate::power::msr_armed();
        let details = crate::power::msr_details().copied();

        match (cur, armed, details) {
            (Some(cur), true, Some(d)) => ctx.io.write_fmt(format_args!(
                "pstate: current={} min={} max={}\r\n",
                cur,
                d.min_ratio.unwrap_or(0),
                d.max_ratio.unwrap_or(0)
            )),
            (_, false, _) => ctx.io.write_str("pstate: msr disarmed\r\n"),
            (_, true, None) => ctx.io.write_str("pstate: msr details not probed\r\n"),
            _ => ctx.io.write_str("pstate: unsupported\r\n"),
        }
        return super::CommandAction::None;
    }

    let req_u64 = ratio.unwrap();
    let Ok(req) = u8::try_from(req_u64) else {
        ctx.io.write_str("pstate: usage pstate <ratio>\r\n");
        return super::CommandAction::None;
    };

    match crate::power::set_pstate_ratio(req) {
        Ok(applied) => ctx.io.write_fmt(format_args!("pstate: applied {}\r\n", applied)),
        Err(err) => ctx.io.write_fmt(format_args!("pstate: failed: {}\r\n", err)),
    }

    super::CommandAction::None
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

pub(crate) fn cmd_smp(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    if !crate::smp::is_init() {
        ctx.io.write_str("smp: not initialized\r\n");
        return super::CommandAction::None;
    }

    let total = crate::smp::cpu_count();
    ctx.io
        .write_fmt(format_args!("smp: cpu_count={}\r\n", total));

    let slot_opt = args.and_then(|a| a.get(0)).and_then(|v| v.as_usize());

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
            return super::CommandAction::None;
        }
        dump_slot(slot);
        return super::CommandAction::None;
    }

    for slot in 0..total {
        dump_slot(slot);
    }

    super::CommandAction::None
}

pub(crate) fn cmd_turbo(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let op = args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let armed = crate::turbo::armed();
        match crate::turbo::local_state() {
            Ok(st) => {
                ctx.io.write_fmt(format_args!("turbo: armed={} state={:?}\r\n", armed, st));
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_fmt(format_args!("turbo: unsupported (intel-only)\r\n"));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // Reads should never require arming; keep for forward-compat.
                ctx.io.write_fmt(format_args!("turbo: disarmed\r\n"));
            }
        }
        if !armed {
            ctx.io.write_str("turbo: writes are disarmed (run 'turbo arm')\r\n");
        }
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("arm") {
        crate::turbo::set_armed(true);
        ctx.io.write_str("turbo: armed\r\n");
        return super::CommandAction::None;
    }
    if op.eq_ignore_ascii_case("disarm") {
        crate::turbo::set_armed(false);
        ctx.io.write_str("turbo: disarmed\r\n");
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("verify") {
        let spins = args
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_usize())
            .unwrap_or(200_000);

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
                ctx.io.write_str("turbo: msr disarmed (verify should not require arm)\r\n");
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
            }
        }

        return super::CommandAction::None;
    }

    let enable = if op.eq_ignore_ascii_case("on") {
        Some(true)
    } else if op.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    };

    let Some(enable) = enable else {
        ctx.io.write_str("turbo: usage turbo [status|arm|disarm|on|off|verify [spins]]\r\n");
        return super::CommandAction::None;
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
            ctx.io.write_str("turbo: msr disarmed (run 'turbo arm')\r\n");
        }
        Err(crate::turbo::TurboSetError::Unsupported) => {
            ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
        }
    }

    super::CommandAction::None
}

pub(crate) fn cmd_cube(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::EnterCube
}

pub(crate) fn cmd_ico(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::EnterIco
}

pub(crate) fn cmd_txt(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let arg = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("").trim();

    if !arg.is_empty() {
        ctx.io.write_str("txt: argument no longer supported\r\n");
    }

    let Some(slot_id) = crate::matrix::alloc_slot("txt") else {
        ctx.io.write_str("txt: matrix full\r\n");
        return super::CommandAction::None;
    };

    let mut filename: heapless::String<48> = heapless::String::new();
    let _ = write!(filename, "§{}", slot_id + 1);
    super::CommandAction::EnterTxtEdt { filename, slot_id }
}

pub(crate) fn cmd_insane(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let cols = (*ctx.term_cols).max(1);
    ctx.io.write_str("insane: iterating U+0000..=U+10FFFF (Ctrl-C to abort)\r\n");

    let mut col: usize = 0;
    for cp in 0u32..=0x10FFFF {
        if (cp & 0x3FF) == 0 {
            if let Some(b) = ctx.io.read_byte() {
                if b == 0x03 {
                    ctx.io.write_str("\r\ninsane: aborted\r\n");
                    return super::CommandAction::None;
                }
            }
        }

        let ch = match core::char::from_u32(cp) {
            Some(ch) if !ch.is_control() => ch,
            Some(_) => '.',
            None => '\u{FFFD}',
        };

        ctx.io.write_char(ch);

        col += 1;
        if col >= cols {
            ctx.io.write_str("\r\n");
            col = 0;
        }
    }

    if col != 0 {
        ctx.io.write_str("\r\n");
    }
    ctx.io.write_str("insane: done\r\n");
    super::CommandAction::None
}

pub(crate) fn cmd_pci_usb(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let sub = _args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if sub == "dump" {
        ctx.io.write_str(
            "pci.usb: targeted descriptor dump is printed automatically when an unclaimed device matches vid=0x0416 pid=0xA125 (JGINYUE 'LED SheBei').\r\n",
        );
        ctx.io.write_str(
            "pci.usb: replug the device (or reboot) to re-trigger enumeration.\r\n",
        );
        return super::CommandAction::None;
    }

    let ctrls = crate::usb::xhci::xhc_list();
    if ctrls.is_empty() {
        ctx.io.write_str("pci.usb: no xhci controllers\r\n");
        return super::CommandAction::None;
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

    super::CommandAction::None
}

pub(crate) fn cmd_pci(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let mut len: usize = 0;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_silent();
    }

    // Optional enrichment via cached `pci.ids`.
    // On-demand download only when cache is missing; no boot-time background fetch.
    let mut pci_ids_db = crate::pci::pciids::load_sanitized_from_root_blocking().ok().flatten();
    if pci_ids_db.is_none() {
        ctx.io.write_str("pci: pci.ids cache missing; downloading...\r\n");
        let fetched = crate::wait::spawn_and_wait_local(async {
            crate::pci::pciids::ensure_cached_async(false).await
        });
        match fetched {
            Ok(bytes) => {
                if bytes > 0 {
                    ctx.io.write_fmt(format_args!(
                        "pci: pci.ids downloaded bytes={}\r\n",
                        bytes
                    ));
                } else {
                    ctx.io.write_str("pci: pci.ids already cached\r\n");
                }
                pci_ids_db = crate::pci::pciids::load_sanitized_from_root_blocking().ok().flatten();
            }
            Err(_) => {
                ctx.io.write_str("pci: pci.ids download failed (continuing without names)\r\n");
            }
        }
    }

    crate::pci::with_devices(|list| {
        ctx.io.write_fmt(format_args!("pci: devices={}\r\n", list.len()));
        if list.is_empty() {
            ctx.io.write_str("pci: no devices\r\n");
            return;
        }

        fn walk_caps(
            bus: u8,
            slot: u8,
            function: u8,
        ) -> (bool, bool, Option<(u8, u8)>) {
            // Returns (has_msi, has_msix, pcie_link(gen, width)).
            let status = crate::pci::config_read_u16(bus, slot, function, 0x06);
            let has_caps = (status & (1 << 4)) != 0;
            if !has_caps {
                return (false, false, None);
            }

            let mut has_msi = false;
            let mut has_msix = false;
            let mut pcie_link: Option<(u8, u8)> = None;

            // Standard capability list is single-byte pointer at 0x34.
            let mut cap_ptr = crate::pci::config_read_u8(bus, slot, function, 0x34) & 0xFC;
            let mut iters = 0u8;
            while cap_ptr >= 0x40 && cap_ptr <= 0xFC && iters < 48 {
                iters = iters.wrapping_add(1);
                let cap_id = crate::pci::config_read_u8(bus, slot, function, cap_ptr as u16);
                let next = crate::pci::config_read_u8(bus, slot, function, (cap_ptr as u16) + 1) & 0xFC;

                match cap_id {
                    0x05 => has_msi = true,
                    0x11 => has_msix = true,
                    0x10 => {
                        // PCI Express Capability.
                        // Link Status is at cap+0x12 (16-bit): speed[3:0], width[9:4].
                        let link_status = crate::pci::config_read_u16(
                            bus,
                            slot,
                            function,
                            (cap_ptr as u16) + 0x12,
                        );
                        let speed = (link_status & 0x000F) as u8;
                        let width = ((link_status >> 4) & 0x003F) as u8;
                        if speed != 0 && width != 0 {
                            pcie_link = Some((speed, width));
                        }
                    }
                    _ => {}
                }

                if next == 0 || next == cap_ptr {
                    break;
                }
                cap_ptr = next;
            }

            (has_msi, has_msix, pcie_link)
        }

        for dev in list.iter() {
            let (bar0_lo, bar0_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            let irq_line = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x3C);
            let irq_pin = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x3D);
            let rev = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x08);
            let subsys_vid = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2C);
            let subsys_did = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2E);
            let (has_msi, has_msix, pcie_link) = walk_caps(dev.bus, dev.slot, dev.function);

            let name_suffix = if let Some(db) = pci_ids_db.as_deref() {
                if let Some((v, d)) = crate::pci::pciids::lookup_vendor_device_from_db(
                    db,
                    dev.vendor,
                    dev.device,
                ) {
                    let v = alloc::string::String::from(alloc::string::String::from_utf8_lossy(v).trim());
                    let d = alloc::string::String::from(alloc::string::String::from_utf8_lossy(d).trim());
                    alloc::format!(" name=\"{} {}\"", v, d)
                } else {
                    alloc::string::String::new()
                }
            } else {
                alloc::string::String::new()
            };

            if let Some(hi) = bar0_hi {
                ctx.io.write_fmt(format_args!(
                    "pci: {:02X}:{:02X}.{} vid=0x{:04X} did=0x{:04X} subsys=0x{:04X}:0x{:04X} cls={:02X}/{:02X}/{:02X} rev=0x{:02X} msi={} msix={} pcie={:?} bar0=0x{:08X}{:08X} irq_line={} irq_pin={}{}\r\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.vendor,
                    dev.device,
                    subsys_vid,
                    subsys_did,
                    dev.class,
                    dev.subclass,
                    dev.prog_if,
                    rev,
                    has_msi,
                    has_msix,
                    pcie_link,
                    hi,
                    bar0_lo,
                    irq_line,
                    irq_pin,
                    name_suffix,
                ));
            } else {
                ctx.io.write_fmt(format_args!(
                    "pci: {:02X}:{:02X}.{} vid=0x{:04X} did=0x{:04X} subsys=0x{:04X}:0x{:04X} cls={:02X}/{:02X}/{:02X} rev=0x{:02X} msi={} msix={} pcie={:?} bar0=0x{:08X} irq_line={} irq_pin={}{}\r\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.vendor,
                    dev.device,
                    subsys_vid,
                    subsys_did,
                    dev.class,
                    dev.subclass,
                    dev.prog_if,
                    rev,
                    has_msi,
                    has_msix,
                    pcie_link,
                    bar0_lo,
                    irq_line,
                    irq_pin,
                    name_suffix,
                ));
            }
        }
    });

    super::CommandAction::None
}
