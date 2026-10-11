//! Native NET menu over the existing NIC, DNS, ICMP and IRC services.
use super::table::TlbTable;
use super::{
    MatrixTarget,
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use crate::r::net::VNet;
use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet as api;

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("net", frontend)? else {
        return Ok(());
    };
    let token = match menu_task(target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            return Err("NET menu task pool is full.".into());
        }
    };
    spawner.spawn(token);
    Ok(())
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
fn append_r8125_details(lines: &mut Vec<String>, specific_index: Option<usize>) {
    for snapshot in crate::net::r8125::snapshots() {
        let Some(index) =
            crate::net::find_device_by_bdf(snapshot.bus, snapshot.slot, snapshot.function)
        else {
            continue;
        };
        if let Some(target) = specific_index
            && index != target
        {
            continue;
        }

        lines.push((alloc::format!(
                "rtl8125 dev={} bdf={:02x}:{:02x}.{} family={} xid={:03x} pci-rev={:02x} subsys={:04x}:{:04x} fw-hint={} initial-tcr=0x{:08x}",
                index,
                snapshot.bus,
                snapshot.slot,
                snapshot.function,
                snapshot.family,
                snapshot.xid,
                snapshot.revision,
                snapshot.subsystem_vendor,
                snapshot.subsystem_device,
                snapshot.firmware_hint,
                snapshot.initial_tcr
            )
            .as_str()).into());
        lines.push((alloc::format!(
                "  filter rcr=0x{:08x} own={} broadcast={} multicast={} promiscuous={} mar=0x{:016x}",
                snapshot.rcr,
                snapshot.accepts_own_mac() as u8,
                snapshot.accepts_broadcast() as u8,
                snapshot.accepts_multicast() as u8,
                snapshot.promiscuous() as u8,
                snapshot.multicast_hash
            )
            .as_str()).into());
        lines.push((alloc::format!(
                "  control cplus=0x{:04x} mcu=0x{:02x}->0x{:02x} oob={} cfg3=0x{:02x} cfg5=0x{:02x}",
                snapshot.cplus,
                snapshot.mcu_before,
                snapshot.mcu_after,
                ((snapshot.mcu_after & (1 << 7)) != 0) as u8,
                snapshot.config3,
                snapshot.config5
            )
            .as_str()).into());
    }
}
fn nic_snapshot(width: usize, selector: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    let specific_index = match selector {
        Some(sel) => match parse_device_selector(sel) {
            Some(index) => Some(index),
            None => {
                lines.push(("net: usage `net nic [index|vid:pid|bb:dd.f]`").into());
                return lines;
            }
        },
        None => None,
    };

    let count = crate::net::device_count();
    if count == 0 {
        lines.push(("net nic: no nics").into());
        return lines;
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
    let table = TlbTable::with_width(&headers, width.saturating_sub(2));
    table.emit_header(|text| lines.push(text.into()));

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
        let vidpid = if let Some((vid, pid)) = crate::net::device_id_at(index) {
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

        table
            .emit_row::<&str, _>(&[&idx, &bdf, &vidpid, iface, &mac, &ipv4, mode, &ipv6], |text| {
                lines.push(text.into())
            });
    }

    table.emit_footer(|text| lines.push(text.into()));
    append_r8125_details(&mut lines, specific_index);
    lines
}

const MAX_OUTPUT_LINES: usize = 256;
struct Job {
    lines: Mutex<Vec<String>>,
    running: AtomicBool,
    cancelled: AtomicBool,
}
impl Job {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            lines: Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
            cancelled: AtomicBool::new(false),
        })
    }
    fn line(&self, text: &str) {
        let mut lines = self.lines.lock();
        if lines.len() == MAX_OUTPUT_LINES {
            lines.remove(0);
        }
        lines.push(
            text.chars()
                .take(1024)
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect(),
        );
    }
    fn active(&self, lease: &super::MatrixSlotLease) -> bool {
        !self.cancelled.load(Ordering::Acquire) && super::matrix_target::matrix_slot_is_live(lease)
    }
}
async fn ping(
    target: String,
    device_index: Option<usize>,
    job: Arc<Job>,
    lease: super::MatrixSlotLease,
) {
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

    if !job.active(&lease) {
        return;
    }
    let ip = match ip_res {
        Ok(addr) => addr,
        Err(err) => {
            let msg = alloc::format!("net icmp: resolve failed {:?}", err);
            job.line(msg.as_str());
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
    job.line(header.as_str());

    let vnet = match device_index {
        Some(index) => VNet::open(index),
        None => VNet::open_primary(),
    };
    let Some(vnet) = vnet else {
        job.line("net icmp: no network device");
        return;
    };

    let mut seq = 1u16;
    for _ in 0..4 {
        if !job.active(&lease) {
            return;
        }
        let payload = [0u8; 56];
        if vnet
            .submit(api::Command::IcmpEcho {
                target: ip,
                seq,
                data: api::ByteBuf::from_slice_trunc(&payload),
            })
            .is_err()
        {
            job.line("net icmp: send failed");
        }

        let deadline = Instant::now() + EmbassyDuration::from_secs(2);
        let mut got = false;
        while Instant::now() < deadline && job.active(&lease) {
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
                job.line(msg.as_str());
                got = true;
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        if !got {
            let msg = alloc::format!("net icmp: request seq={} timeout", seq);
            job.line(msg.as_str());
        }

        seq = seq.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(1000)).await;
    }
}
async fn irc(server: String, chan: Option<String>, job: Arc<Job>, lease: super::MatrixSlotLease) {
    use crate::r::net::cli::irc::{IRC_DEFAULT_PORT, IrcSession};

    let msg = alloc::format!("irc: connecting {}:{}", server.as_str(), IRC_DEFAULT_PORT);
    job.line(msg.as_str());

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
            job.line(msg.as_str());
            return;
        }
    };

    if let Err(e) = session
        .register("trueos", "trueos", "TrueOS kernel", 15_000)
        .await
    {
        let msg = alloc::format!("irc: register failed {:?}", e);
        job.line(msg.as_str());
        let _ = session.quit("bye", 1_000).await;
        return;
    }
    job.line("irc: registered");

    if let Some(ch) = chan.as_ref() {
        let msg = alloc::format!("irc: joining {}", ch.as_str());
        job.line(msg.as_str());
        if let Err(e) = session.join(ch.as_str()) {
            let msg = alloc::format!("irc: join failed {:?}", e);
            job.line(msg.as_str());
        }
    }

    if !job.active(&lease) {
        let _ = session.quit("bye", 1_000).await;
        return;
    }
    let deadline = Instant::now() + EmbassyDuration::from_secs(10);
    while Instant::now() < deadline && job.active(&lease) {
        match session.recv_line(500).await {
            Ok(Some(l)) => job.line(l.as_str()),
            Ok(None) => {}
            Err(_) => break,
        }
    }

    let _ = session.quit("bye", 2_000).await;
    job.line("irc: done");
}

const LABELS: [&str; 6] = [
    "Network interfaces",
    "Inspect one interface",
    "ICMP ping",
    "IRC connection test",
    "Hostname",
    "Return",
];
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Nic,
    Ping,
    Irc,
    Hostname,
}
impl Field {
    fn prompt(self) -> &'static str {
        match self {
            Self::Nic => "Interface: index, vid:pid or bb:dd.f",
            Self::Ping => "Target [interface]",
            Self::Irc => "Host [#channel]",
            Self::Hostname => "Hostname (empty shows current)",
        }
    }
}
enum Page {
    Home,
    Field(Field),
    Details(Option<String>),
    Output,
}
struct View {
    page: Page,
    selected: usize,
    scroll: usize,
    value: String,
    after_cr: bool,
    input: Input,
    screen: Screen,
    content: Vec<String>,
    job: Option<Arc<Job>>,
    message: String,
}
impl View {
    fn new() -> Self {
        Self {
            page: Page::Home,
            selected: 0,
            scroll: 0,
            value: String::new(),
            after_cr: false,
            input: Input::default(),
            screen: Screen::default(),
            content: Vec::new(),
            job: None,
            message: String::new(),
        }
    }
    fn home(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancelled.store(true, Ordering::Release);
        }
        self.page = Page::Home;
        self.value.clear();
        self.scroll = 0;
        self.input = Input::default();
    }
    fn layout(&self, rows: usize) -> (usize, usize, usize) {
        let first = if rows >= 10 { 4 } else { 3 };
        let visible = rows.saturating_sub(first).min(LABELS.len());
        let offset = self.selected.saturating_sub(visible.saturating_sub(1));
        (first, visible, offset)
    }
    fn choose(&mut self, target: &MatrixTarget, cols: usize) {
        self.message.clear();
        self.value.clear();
        self.scroll = 0;
        self.page = match self.selected {
            0 => {
                self.content = nic_snapshot(cols, None);
                Page::Details(None)
            }
            1 => Page::Field(Field::Nic),
            2 => Page::Field(Field::Ping),
            3 => Page::Field(Field::Irc),
            4 => Page::Field(Field::Hostname),
            _ => {
                tui::native_return(target);
                return;
            }
        };
    }
    fn submit(&mut self, field: Field, target: &MatrixTarget, cols: usize) {
        let value = core::mem::take(&mut self.value);
        let parts: Vec<&str> = value.split_whitespace().collect();
        if field == Field::Hostname {
            if parts.len() > 1
                || parts.first().is_some_and(|name| {
                    name.len() > 63
                        || name.starts_with('-')
                        || name.ends_with('-')
                        || !name
                            .bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
                })
            {
                self.message = "Use a hostname of 1–63 letters, digits or hyphens.".into();
                return;
            }
            if let Some(name) = parts.first() {
                crate::net::adapter::set_hostname(name);
            }
            self.home();
            self.message = format!("Hostname: {}", crate::net::adapter::get_hostname());
            return;
        }
        if field == Field::Nic {
            if parts.len() != 1 || parse_device_selector(parts[0]).is_none() {
                self.message = "Choose an interface by index, vid:pid or bb:dd.f.".into();
                return;
            }
            self.content = nic_snapshot(cols, Some(parts[0]));
            self.page = Page::Details(Some(parts[0].into()));
            return;
        }
        if parts.is_empty() || parts.len() > 2 || parts[0].len() > 128 {
            self.message = format!("Enter {}.", field.prompt());
            return;
        }
        let device = if field == Field::Ping {
            match parts.get(1) {
                Some(selector) => match parse_device_selector(selector) {
                    Some(index) if index < crate::net::device_count() => Some(index),
                    _ => {
                        self.message = "No matching network interface.".into();
                        return;
                    }
                },
                None => None,
            }
        } else {
            None
        };
        if field == Field::Irc
            && parts
                .get(1)
                .is_some_and(|channel| channel.len() > 64 || !channel.starts_with('#'))
        {
            self.message = "Use an IRC channel beginning with # (up to 64 characters).".into();
            return;
        }
        let host = parts[0].into();
        let channel = parts.get(1).map(|text| (*text).into());
        let job = Job::new();
        self.job = Some(job.clone());
        self.page = Page::Output;
        self.scroll = 0;
        let lease = super::matrix_target_slot_lease(target);
        crate::wait::spawn_local_detached(async move {
            if job.active(&lease) {
                if field == Field::Ping {
                    ping(host, device, job.clone(), lease).await;
                } else {
                    irc(host, channel, job.clone(), lease).await;
                }
            }
            job.running.store(false, Ordering::Release);
        });
    }
    fn input(&mut self, target: &MatrixTarget, bytes: &[u8], cols: usize, rows: usize, now: u64) {
        if let Page::Field(field) = self.page {
            // Forms accept navigation letters literally and never consume
            // multiple submissions from one CRLF or mouse packet.
            for &byte in bytes {
                if byte == 27 || self.input.in_sequence() {
                    for action in self.input.feed(&[byte], now) {
                        if action == Action::Quit || action == Action::Click(1) {
                            self.home();
                            return;
                        }
                    }
                    continue;
                }
                if byte == b'\n' && self.after_cr {
                    self.after_cr = false;
                    continue;
                }
                self.after_cr = byte == b'\r';
                match byte {
                    b'\r' | b'\n' => {
                        self.submit(field, target, cols);
                        return;
                    }
                    3 => {
                        self.home();
                        return;
                    }
                    8 | 127 => {
                        self.value.pop();
                    }
                    32..=126 if self.value.len() < 194 => self.value.push(byte as char),
                    _ => {}
                }
            }
            if self.input.timeout(now) == Some(Action::Quit) {
                self.home();
            }
            return;
        }
        let mut actions = self.input.feed(bytes, now);
        if let Some(action) = self.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            if matches!(self.page, Page::Home) {
                match action {
                    Action::Up => self.selected = (self.selected + LABELS.len() - 1) % LABELS.len(),
                    Action::Down => self.selected = (self.selected + 1) % LABELS.len(),
                    Action::Choose => self.choose(target, cols),
                    Action::Quit | Action::Click(1) => {
                        tui::native_return(target);
                        return;
                    }
                    Action::Click(row) => {
                        let (first, visible, offset) = self.layout(rows);
                        if (first..first + visible).contains(&row) {
                            self.selected = offset + row - first;
                            self.choose(target, cols);
                        }
                    }
                }
            } else {
                match action {
                    Action::Up => self.scroll = self.scroll.saturating_sub(1),
                    Action::Down => self.scroll = self.scroll.saturating_add(1),
                    Action::Choose | Action::Quit | Action::Click(1) => {
                        self.home();
                        return;
                    }
                    _ => {}
                }
            }
            // Remaining keys belong to the page that received this packet.
            if !matches!(self.page, Page::Home) || !tui::native_visible(target) {
                return;
            }
        }
    }
    fn frame(&mut self, cols: usize, rows: usize, now: u64) -> Vec<String> {
        let mut lines = alloc::vec![String::new();rows];
        if rows < 5 {
            return lines;
        }
        lines[0] = "NET  network tools".into();
        lines[1] = if matches!(self.page, Page::Field(_)) {
            "Enter submit   Esc Return".into()
        } else if matches!(self.page, Page::Home) {
            "↑/↓ j/k select   Enter choose   Esc/q Return   Mouse choose".into()
        } else {
            "↑/↓ j/k scroll   Enter/Esc Return   Mouse wheel scroll".into()
        };
        match &self.page {
            Page::Home => {
                lines[2] = if self.message.is_empty() {
                    format!("Hostname: {}", crate::net::adapter::get_hostname())
                } else {
                    self.message.clone()
                };
                let (first, visible, offset) = self.layout(rows);
                for index in 0..visible {
                    let selected = index + offset;
                    lines[first + index] = format!(
                        "  {} {}",
                        if selected == self.selected {
                            "›"
                        } else {
                            " "
                        },
                        LABELS[selected]
                    );
                }
            }
            Page::Field(field) => {
                lines[2] = field.prompt().into();
                lines[3] = format!("> {}", self.value);
                lines[4] = self.message.clone();
            }
            Page::Details(_) | Page::Output => {
                let content = if let Some(job) = &self.job {
                    lines[2] = if job.running.load(Ordering::Acquire) {
                        format!("{} Working…", helper::spinner(now))
                    } else {
                        "Complete".into()
                    };
                    job.lines.lock().clone()
                } else {
                    lines[2] = "Network interfaces".into();
                    self.content.clone()
                };
                let visible = rows - 3;
                self.scroll = self.scroll.min(content.len().saturating_sub(visible));
                for (index, line) in content
                    .into_iter()
                    .skip(self.scroll)
                    .take(visible)
                    .enumerate()
                {
                    lines[3 + index] = line;
                }
            }
        }
        let _ = cols;
        lines
    }
}
impl Drop for View {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancelled.store(true, Ordering::Release);
        }
    }
}
#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut view = View::new();
    let mut geometry = (0, 0);
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            Timer::after(EmbassyDuration::from_millis(20)).await;
            continue;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let (cols, rows) = (surface.cols as usize, surface.rows as usize);
        if geometry != (cols, rows) {
            if let Page::Details(selector) = &view.page {
                view.content = nic_snapshot(cols, selector.as_deref());
            }
            geometry = (cols, rows);
        }
        let now = crate::chronos::monotonic_nanos();
        view.input(&target, &bytes, cols, rows, now);
        if tui::native_visible(&target) {
            let lines = view.frame(cols, rows, now);
            view.screen.paint(&target, &lines, cols, rows);
        }
        Timer::after(EmbassyDuration::from_millis(20)).await;
    }
}
