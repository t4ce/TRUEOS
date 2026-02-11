use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::shell::shellcube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};

pub(crate) mod ecma48;

pub(crate) mod shellcube;
pub(crate) mod shellqjs;
pub(crate) mod txtedt;

pub(crate) mod cmd;

pub(crate) mod matrix;
pub(crate) mod statusbar;

mod crlf;

mod interface;
pub(crate) use interface::{ShellBackend, ShellIo};

pub(crate) mod backends;
pub(crate) use backends::{NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND};

pub(crate) mod uart1_com1;

struct Utf8Decoder {
    buf: [u8; 4],
    len: usize,
    need: usize,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self {
            buf: [0u8; 4],
            len: 0,
            need: 0,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.need = 0;
    }

    fn push(&mut self, b: u8) -> Option<char> {
        if self.len == 0 {
            if b < 0x80 {
                return Some(b as char);
            }
            let need = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => {
                    // Invalid leading byte.
                    return None;
                }
            };
            self.buf[0] = b;
            self.len = 1;
            self.need = need;
            return None;
        }

        // Continuation byte.
        if (b & 0xC0) != 0x80 {
            self.clear();
            return None;
        }

        self.buf[self.len] = b;
        self.len += 1;
        if self.len < self.need {
            return None;
        }

        let s = core::str::from_utf8(&self.buf[..self.need]).ok()?;
        let ch = s.chars().next()?;
        self.clear();
        Some(ch)
    }
}

const PROMPT_RGB: (u8, u8, u8) = (255, 55, 255);
const MATRIX_RUNNING_GLYPH: char = '⣿';
const DEFAULT_TERM_COLS: usize = 80;
const DEFAULT_TERM_ROWS: usize = 24;
const NETBENCH_UPDATE_MS: u64 = 250;
const NETBENCH_URL: &str = "http://ipv4.download.thinkbroadband.com/100MB.zip";

const NETBENCH_IDLE: u8 = 0;
const NETBENCH_RUNNING: u8 = 1;
const NETBENCH_DONE: u8 = 2;
const NETBENCH_ABORTED: u8 = 3;
const NETBENCH_FAILED: u8 = 4;

static NETBENCH_STATE: AtomicU8 = AtomicU8::new(NETBENCH_IDLE);
static NETBENCH_ABORT_REQ: AtomicBool = AtomicBool::new(false);
static NETBENCH_BYTES: AtomicU64 = AtomicU64::new(0);
static NETBENCH_START_TICK: AtomicU64 = AtomicU64::new(0);
static NETBENCH_END_TICK: AtomicU64 = AtomicU64::new(0);
static NETBENCH_FAIL_CODE: AtomicU8 = AtomicU8::new(0);
static NETBENCH_STATUS_SLOT: AtomicU8 = AtomicU8::new(u8::MAX);

#[inline]
fn netbench_fail_text(code: u8) -> &'static str {
    match code {
        1 => "bad url",
        2 => "dns",
        3 => "open vnet",
        4 => "open tcp",
        5 => "tcp open timeout",
        6 => "tcp open failed",
        7 => "tcp send failed",
        8 => "timeout",
        9 => "response too large",
        10 => "io",
        _ => "unknown",
    }
}

#[inline]
fn write_prompt(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
}

#[inline]
fn write_prompt_for_state(
    io: &dyn ShellIo,
    pending_action: Option<PendingAction>,
    install_wizard: Option<InstallWizardStage>,
) {
    // When we are actively asking the user for wizard/confirmation input,
    // avoid printing any prompt prefix so the user's typed input starts at
    // column 0 (less confusing in transcripts).
    if pending_action.is_some() || install_wizard.is_some() {
        return;
    }

    write_prompt(io);
}

#[inline]
fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    if prefix.len() > s.len() {
        return false;
    }
    s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn handle_tab_completion(
    io: &dyn ShellIo,
    line: &mut String<128>,
    term_cols: usize,
    term_rows: usize,
    pending_action: Option<PendingAction>,
    install_wizard: Option<InstallWizardStage>,
) {
    if pending_action.is_some() || install_wizard.is_some() {
        return;
    }

    let mut input_buf: String<128> = String::new();
    let _ = input_buf.push_str(line.as_str());
    let input = input_buf.as_str();
    if input.is_empty() {
        return;
    }

    // If the command token is already present, Tab shows that command's usage line.
    if input.chars().any(|ch| ch.is_whitespace()) {
        let cmd = input.split_whitespace().next().unwrap_or("");
        if !cmd.is_empty() {
            let mut usage: String<192> = String::new();
            if crate::shell::cmd::usage_text_for_name(cmd, &mut usage) {
                write_overlay_hint(io, term_cols, term_rows, usage.as_str());
            }
        }
        return;
    }

    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmd::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    let mut shown: heapless::Vec<&'static str, 5> = heapless::Vec::new();
    let mut match_count = 0usize;
    let mut unique: Option<&'static str> = None;

    for name in cmds.iter().copied() {
        if starts_with_ignore_ascii_case(name, input) {
            match_count += 1;
            if unique.is_none() {
                unique = Some(name);
            }
            if shown.len() < 5 {
                let _ = shown.push(name);
            }
        }
    }

    if match_count == 0 {
        return;
    }

    if match_count == 1 {
        let target = unique.unwrap_or(input);
        if target.len() > input.len() {
            let suffix = &target[input.len()..];
            for ch in suffix.chars() {
                if line.push(ch).is_err() {
                    break;
                }
                io.write_char(ch);
            }
        }
        if !line.as_str().ends_with(' ') {
            if line.push(' ').is_ok() {
                io.write_char(' ');
            }
        }
        let mut usage: String<192> = String::new();
        if crate::shell::cmd::usage_text_for_name(target, &mut usage) {
            write_overlay_hint(io, term_cols, term_rows, usage.as_str());
        } else {
            write_overlay_hint(io, term_cols, term_rows, "");
        }
        return;
    }

    let mut msg: String<192> = String::new();
    let _ = msg.push_str("matches: ");
    for (idx, name) in shown.iter().enumerate() {
        if idx != 0 {
            let _ = msg.push(' ');
        }
        let _ = msg.push_str(name);
    }
    if match_count > shown.len() {
        let _ = msg.push_str(" ...");
    }
    write_overlay_hint(io, term_cols, term_rows, msg.as_str());
}

#[inline]
fn write_overlay_hint(io: &dyn ShellIo, term_cols: usize, term_rows: usize, text: &str) {
    if term_cols == 0 || term_rows < 2 {
        return;
    }

    let mut clipped: String<256> = String::new();
    let mut cols = 0usize;
    for ch in text.chars() {
        if cols >= term_cols {
            break;
        }
        if clipped.push(ch).is_err() {
            break;
        }
        cols += 1;
    }

    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows - 1, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(clipped.as_str());
    io.write_str(crate::ecma48::RESTORE_CURSOR);
    refresh_status_bar(io, term_cols, term_rows);
}

#[inline]
fn write_right_aligned(io: &dyn ShellIo, row: usize, term_cols: usize, text: &str) {
    if term_cols == 0 || text.is_empty() {
        return;
    }
    let len = crate::ecma48::visible_width(text);
    let col = term_cols.saturating_sub(len).saturating_add(1);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
    io.write_str(text);
}

#[inline]
fn output_bottom_row(term_rows: usize) -> usize {
    // row 1: banner, row 2: prompt, row term_rows-1: hint, row term_rows: status
    core::cmp::max(3, term_rows.saturating_sub(2))
}

#[inline]
fn apply_shell_scroll_region(io: &dyn ShellIo, term_rows: usize) {
    let top = 3usize;
    let bottom = output_bottom_row(term_rows);
    io.write_fmt(format_args!("\x1b[{};{}r", top, bottom));
}

#[inline]
fn append_output_cursor(io: &dyn ShellIo, term_rows: usize) {
    let row = output_bottom_row(term_rows);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, 1)));
    io.write_str("\r\n");
}

#[inline]
fn write_banner(io: &dyn ShellIo, term_cols: usize) {
    // Row 1: Banner + matrix symbols on the right.
    io.write_fmt(format_args!("{}\n", crate::ecma48::bold("TRUE OS")));
    crate::matrix::refresh_matrix_symbols(io, term_cols);

    write_prompt(io);

    io.write_str(crate::ecma48::SAVE_CURSOR);

    // Banner command list: derive from the registry (single source of truth).
    // Keep it sorted for stable UI.
    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmd::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    for (idx, cmd) in cmds.iter().enumerate() {
        // Start at row 2 so row 1 stays reserved for banner + symbols.
        let row = idx + 2;
        write_right_aligned(io, row, term_cols, cmd);
    }
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn refresh_status_bar(io: &dyn ShellIo, term_cols: usize, term_rows: usize) {
    if term_cols == 0 || term_rows == 0 {
        return;
    }

    #[inline]
    fn indicator_rgb(code: u8) -> (u8, u8, u8) {
        match code {
            1 => (230, 70, 70),   // red
            2 => (70, 210, 90),   // green
            3 => (230, 190, 70),  // yellow
            4 => (70, 130, 230),  // blue
            5 => (80, 200, 210),  // cyan
            6 => (200, 90, 210),  // magenta
            7 => (210, 210, 210), // white
            _ => (90, 90, 90),    // off/idle
        }
    }

    fn fit_10(src: &str) -> heapless::String<10> {
        let mut out: heapless::String<10> = heapless::String::new();
        for ch in src.chars() {
            if out.push(ch).is_err() {
                break;
            }
        }
        while out.len() < 10 {
            let _ = out.push(' ');
        }
        out
    }

    let (indicators, left, right) = if let Some(s) = crate::shell::statusbar::snapshot_active() {
        (
            s.indicators,
            fit_10(s.left.as_str()),
            fit_10(s.right.as_str()),
        )
    } else {
        (
            [0u8; crate::shell::statusbar::INDICATOR_COUNT],
            fit_10(""),
            fit_10(""),
        )
    };

    // Layout (single row):
    // [5 colored indicators][space][left 10 chars] ... [right 10 chars]
    // Render as non-invasive overlay on the bottom row.
    let right_col = term_cols.saturating_sub(10).saturating_add(1);
    let left_col = 1usize;

    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, 1)));
    for _ in 0..term_cols {
        io.write_byte(b' ');
    }

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, left_col)));
    for c in indicators {
        io.write_fmt(format_args!("{}", crate::ecma48::color("o", indicator_rgb(c))));
    }
    io.write_byte(b' ');
    io.write_fmt(format_args!("{}", crate::ecma48::style(left.as_str()).dim().fg((210, 210, 210))));

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, right_col)));
    io.write_fmt(format_args!("{}", crate::ecma48::style(right.as_str()).bold().fg((255, 120, 200))));

    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

#[inline]
fn set_go_mode(io: &dyn ShellIo, go_mode: &mut bool, enable: bool) {
    let prev = *go_mode;
    if enable && !prev {
        io.write_str(crate::ecma48::HIDE_CURSOR);
    } else if !enable && prev {
        io.write_str(crate::ecma48::SHOW_CURSOR);
    }
    *go_mode = enable;
}

fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}

async fn print_install_disk_table(io: &dyn ShellIo) {
    io.write_str("install: disk detection stage\r\n");
    io.write_str("install: choose a disk id to continue (blank/q cancels)\r\n");
    io.write_str("\r\n");

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;
        io.write_fmt(format_args!(
            "  id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}\r\n",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    // Keep it short; this is mainly for debugging why detection fails.
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        ));
    }

    io.write_str("\r\n");
}

async fn print_update_disk_table(io: &dyn ShellIo) {
    io.write_str("update: disk detection stage\r\n");
    io.write_str("update: choose a disk id to continue (blank/q cancels)\r\n");
    io.write_str("\r\n");

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;
        io.write_fmt(format_args!(
            "  id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}\r\n",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        ));
    }

    io.write_str("\r\n");
}

async fn print_format_disk_table(io: &dyn ShellIo) {
    io.write_str("format: disk selection stage\r\n");
    io.write_str("format: enter a disk id (blank/q cancels)\r\n");
    io.write_str("\r\n");

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;
        io.write_fmt(format_args!(
            "  id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}\r\n",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        ));
    }

    io.write_str("\r\n");
}

async fn print_bench_disk_table(io: &dyn ShellIo) {
    io.write_str("bench: TRUEOSFS disk selection\r\n");
    io.write_str("bench: enter a disk id (blank/q cancels)\r\n");
    io.write_str("\r\n");

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;
        if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
            continue;
        }
        io.write_fmt(format_args!(
            "  id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}\r\n",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        ));
    }

    io.write_str("\r\n");
}

async fn print_netbench_nic_table(io: &dyn ShellIo) {
    io.write_str("netbench: NIC selection\r\n");
    io.write_str("netbench: enter a nic id (blank/q cancels)\r\n");
    io.write_str("\r\n");

    let count = crate::net::device_count();
    if count == 0 {
        io.write_str("  (no nics)\r\n");
        io.write_str("\r\n");
        return;
    }

    for idx in 0..count {
        let mac = if idx == 0 {
            crate::net::mac_address()
        } else {
            crate::net::mac_address_at(idx)
        };
        match mac {
            Some([a, b, c, d, e, f]) => {
                io.write_fmt(format_args!(
                    "  id={} mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\r\n",
                    idx, a, b, c, d, e, f
                ));
            }
            None => {
                io.write_fmt(format_args!("  id={} mac=unavailable\r\n", idx));
            }
        }
    }

    io.write_str("\r\n");
}

#[derive(Copy, Clone)]
pub(crate) enum PendingAction {
    AcpiReset,
    AcpiState(u8),
    FormatConfirm { disc_id: u32 },
    InstallConfirm { disc_id: u32 },
    UpdateConfirm { disc_id: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallWizardStage {
    SelectDisk,
    FormatSelectDisk,
    UpdateSelectDisk,
    FileSelectMount,
    BenchSelectDisk,
    NetbenchSelectNic,
}

pub(crate) enum CommandAction {
    None,
    Pending(PendingAction),
    ShowInstallDiskTable,
    ShowFormatDiskTable,
    ShowUpdateDiskTable,
    ShowFileMountTable,
    ShowBenchDiskTable,
    ShowNetbenchNicTable,
    EnterCube,
    EnterIco,
    EnterTxtEdt { filename: String<48>, slot_id: u8 },
    Mv { src: String<160>, dst: String<160> },
    Qjs { src: String<192> },
}

async fn print_trueosfs_mount_table(io: &dyn ShellIo) {
    io.write_str("\r\nfile: TRUEOSFS mounts\r\n");

    // `file` is user-facing and should be able to discover TRUEOSFS on already-present
    // disks (e.g. boot disk), not only those that arrived via hotplug.
    // Best-effort: probe all whole-disk devices and mount any TRUEOSFS roots we find.
    for disk in crate::disc::block::device_handles()
        .into_iter()
        .filter(|h| h.parent().is_none())
    {
        let _ = crate::v::fs::trueosfs::mount_root_async(disk).await;
    }

    let roots = crate::v::fs::trueosfs::list_roots();
    if roots.is_empty() {
        io.write_str("file: (none)\r\n");
        return;
    }
    for (idx, r) in roots.iter().enumerate() {
        io.write_fmt(format_args!(
            "file: {:>2}: {} (raw={} seq={})\r\n",
            idx,
            r.disk_id,
            r.disk_id.raw(),
            r.seq
        ));
    }
}

async fn print_trueosfs_tree_25(io: &dyn ShellIo, disk: crate::disc::block::DeviceHandle) {
    use alloc::string::String as AString;
    use alloc::vec::Vec;
    use core::fmt::Write;
    use trueos_math::{NodeId, Tree};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum FsKind {
        Root,
        Dir,
        File,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FsEntry {
        kind: FsKind,
        name: AString,
    }

    struct IoWriter<'a>(&'a dyn ShellIo);
    impl<'a> Write for IoWriter<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            self.0.write_str(s);
            Ok(())
        }
    }

    const MAX_PRINT: usize = 25;
    const CAP: usize = 128;

    let mut tree: Tree<FsEntry, CAP> = Tree::new();
    let Some(root) = tree.add_root(FsEntry {
        kind: FsKind::Root,
        name: AString::from("/"),
    }) else {
        io.write_str("file: tree alloc failed\r\n");
        return;
    };

    // BFS-ish expansion so the output is useful when we hit the entry limit.
    let mut queue: Vec<(NodeId, AString)> = Vec::new();
    queue.push((root, AString::new()));

    while let Some((parent, path)) = queue.pop() {
        if tree.len() >= MAX_PRINT {
            break;
        }

        let listing = match crate::v::fs::trueosfs::list_dir_async(disk, path.as_str()).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                io.write_str("file: not a TRUEOSFS disk\r\n");
                break;
            }
            Err(e) => {
                io.write_fmt(format_args!("file: list_dir failed ({:?})\r\n", e));
                break;
            }
        };

        for name in listing.lines() {
            if tree.len() >= MAX_PRINT {
                break;
            }
            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            let child_path = if path.is_empty() {
                AString::from(name)
            } else {
                let mut p = path.clone();
                p.push('/');
                p.push_str(name);
                p
            };

            let is_file = match crate::v::fs::trueosfs::file_exists_async(disk, child_path.as_str()).await {
                Ok(v) => v,
                Err(_) => false,
            };
            let kind = if is_file { FsKind::File } else { FsKind::Dir };

            let Some(node) = tree.add_child(
                parent,
                FsEntry {
                    kind: kind.clone(),
                    name: AString::from(name),
                },
            ) else {
                break;
            };

            if matches!(kind, FsKind::Dir) {
                // Push last so earlier siblings tend to appear first.
                queue.insert(0, (node, child_path));
            }
        }
    }

    let mut w = IoWriter(io);
    let _ = tree.write_ascii_tree(root, &mut w, MAX_PRINT, |e, w| {
        match e.kind {
            FsKind::Root => w.write_str("/")?,
            FsKind::Dir => {
                w.write_str(e.name.as_str())?;
                w.write_str("/")?;
            }
            FsKind::File => w.write_str(e.name.as_str())?,
        }
        Ok(())
    });
}

async fn run_bench_fs(io: &dyn ShellBackend, disk: crate::disc::block::DeviceHandle) {
    const BENCH_PATH: &str = "bench-lorem-100mb.txt";
    const BENCH_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
    const UPDATE_MS: u64 = 250;
    const PATTERN: &[u8] = b"10101010";
    const CONTROL_PERIOD_CHUNKS: u32 = 8;

    let Some(placement) = crate::v::fs::trueosfs::locate_async(disk).await.ok().flatten() else {
        io.write_str("bench: selected disk is not TRUEOSFS\r\n");
        return;
    };
    if !disk.supports_write() {
        io.write_str("bench: selected disk is read-only\r\n");
        return;
    }

    io.write_fmt(format_args!(
        "bench: target={} super_lba={} data_lba={} file=/{}\r\n",
        disk.id(),
        placement.super_lba,
        placement.data_lba,
        BENCH_PATH
    ));
    io.write_str("bench: writing 100MB fs stream (press any key to abort)\r\n");

    let info = disk.info();
    let bench_chunk_bytes = if info.max_transfer_bytes > 0 {
        let max_transfer = info.max_transfer_bytes as usize;
        core::cmp::max(4 * 1024, core::cmp::min(max_transfer, 1024 * 1024))
    } else {
        256 * 1024
    };

    let Some(stream_handle) = (match crate::v::fs::trueosfs::file_write_begin_async(
        disk,
        BENCH_PATH,
        BENCH_TOTAL_BYTES,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            io.write_fmt(format_args!("bench: begin failed ({:?})\r\n", e));
            return;
        }
    }) else {
        io.write_str("bench: begin failed (no space / no placement)\r\n");
        return;
    };

    let mut chunk: alloc::vec::Vec<u8> = alloc::vec![0u8; bench_chunk_bytes];
    if !PATTERN.is_empty() {
        let mut off = 0usize;
        while off < chunk.len() {
            let take = core::cmp::min(PATTERN.len(), chunk.len() - off);
            chunk[off..off + take].copy_from_slice(&PATTERN[..take]);
            off = off.saturating_add(take);
        }
    }

    let mut written: u64 = 0;
    let mut aborted = false;
    let mut write_err: Option<crate::disc::block::Error> = None;
    let mut finished_ok = false;
    let mut chunk_count: u32 = 0;

    let start_tick = embassy_time_driver::now();
    let mut next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);

    while written < BENCH_TOTAL_BYTES {
        let remaining = (BENCH_TOTAL_BYTES - written) as usize;
        let n = core::cmp::min(remaining, chunk.len());

        if let Err(e) = crate::v::fs::trueosfs::file_write_chunk_async(stream_handle, &chunk[..n]).await {
            write_err = Some(e);
            break;
        }
        written = written.saturating_add(n as u64);
        chunk_count = chunk_count.wrapping_add(1);

        if (chunk_count % CONTROL_PERIOD_CHUNKS) == 0 && io.read_byte().is_some() {
            aborted = true;
            break;
        }

        if Instant::now() >= next_update || written >= BENCH_TOTAL_BYTES {
            let now_tick = embassy_time_driver::now();
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bps = if elapsed_ms == 0 {
                0
            } else {
                written.saturating_mul(1000) / elapsed_ms
            };
            let kbps = bps / 1024;
            let total_kb = written / 1024;
            io.write_fmt(format_args!(
                "\rwrite speed: {} kb/sec | {} KB total   ",
                kbps,
                total_kb
            ));
            next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);
        }
    }

    if write_err.is_none() && !aborted {
        match crate::v::fs::trueosfs::file_write_finish_async(stream_handle).await {
            Ok(()) => finished_ok = true,
            Err(e) => {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
                write_err = Some(e);
            }
        }
    } else {
        let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
    }

    io.write_str("\r\n");
    if aborted {
        io.write_str("bench: aborted by key press\r\n");
    }
    if let Some(e) = write_err {
        io.write_fmt(format_args!("bench: write failed ({:?})\r\n", e));
    } else if finished_ok {
        io.write_str("bench: write complete\r\n");
    }

    let expected_absent = aborted || write_err.is_some() || !finished_ok;
    match crate::v::fs::trueosfs::file_delete_async(disk, BENCH_PATH).await {
        Ok(true) => io.write_str("bench: cleanup ok (deleted benchmark file)\r\n"),
        Ok(false) if expected_absent => {
            io.write_str("bench: cleanup: nothing to delete (expected for aborted/failed run)\r\n")
        }
        Ok(false) => io.write_str("bench: cleanup: benchmark file not present\r\n"),
        Err(e) => io.write_fmt(format_args!("bench: cleanup failed ({:?})\r\n", e)),
    }
}

fn netbench_start(spawner: &Spawner, nic_index: usize) -> bool {
    if NETBENCH_STATE.load(Ordering::Relaxed) == NETBENCH_RUNNING {
        return false;
    }
    let old_slot = NETBENCH_STATUS_SLOT.swap(u8::MAX, Ordering::Relaxed);
    if old_slot != u8::MAX {
        let _ = crate::matrix::free_slot(old_slot);
    }
    if let Some(slot) = crate::matrix::alloc_slot("netbench") {
        NETBENCH_STATUS_SLOT.store(slot, Ordering::Relaxed);
        let _ = crate::shell::statusbar::set_active_slot(slot);
        let _ = crate::shell::statusbar::set_left(slot, "netbench");
        let _ = crate::shell::statusbar::set_right(slot, "starting");
        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
            let _ = crate::shell::statusbar::set_indicator(slot, i, 2);
        }
    }
    NETBENCH_ABORT_REQ.store(false, Ordering::Relaxed);
    NETBENCH_BYTES.store(0, Ordering::Relaxed);
    NETBENCH_FAIL_CODE.store(0, Ordering::Relaxed);
    NETBENCH_START_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
    NETBENCH_END_TICK.store(0, Ordering::Relaxed);
    NETBENCH_STATE.store(NETBENCH_RUNNING, Ordering::Relaxed);
    if spawner.spawn(netbench_worker_task(nic_index)).is_err() {
        NETBENCH_FAIL_CODE.store(10, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return false;
    }
    true
}

#[embassy_executor::task(pool_size = 1)]
async fn netbench_worker_task(nic_index: usize) {
    use alloc::{string::String as AString, vec::Vec};
    use trueos_v::vnet as api;

    const OPEN_TIMEOUT_MS: u64 = 4000;
    const OVERALL_TIMEOUT_MS: u64 = 120000;
    const MAX_CAPTURE_BYTES: usize = 128 * 1024 * 1024;
    const IDLE_YIELD_US: u64 = 100;

    fn parse_http_url(url: &str) -> Option<(AString, u16, AString)> {
        let mut u = url.trim();
        if let Some(rest) = u.strip_prefix("http://") {
            u = rest;
        } else {
            return None;
        }
        let (hostport, path) = match u.split_once('/') {
            Some((a, b)) => (a, alloc::format!("/{}", b)),
            None => (u, alloc::string::String::from("/")),
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

    fn find_http_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| p + 4)
    }

    fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
        let mut i = 0usize;
        while i < headers.len() {
            let line_start = i;
            while i < headers.len() && headers[i] != b'\n' {
                i = i.saturating_add(1);
            }
            let mut line = &headers[line_start..i];
            if i < headers.len() && headers[i] == b'\n' {
                i = i.saturating_add(1);
            }
            if let Some((&b'\r', rest)) = line.split_last() {
                line = rest;
            }
            if line.is_empty() {
                continue;
            }
            let Some(colon) = line.iter().position(|b| *b == b':') else {
                continue;
            };
            let (k, mut v) = line.split_at(colon);
            v = v.get(1..).unwrap_or(&[]);
            if k.len() != name.len() {
                continue;
            }
            if !k
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
            {
                continue;
            }
            while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
                v = &v[1..];
            }
            return Some(v);
        }
        None
    }

    fn parse_content_length(headers: &[u8]) -> Option<usize> {
        let v = header_get_value(headers, b"content-length")?;
        let s = core::str::from_utf8(v).ok()?;
        s.trim().parse::<usize>().ok()
    }

    let Some((host, port, path)) = parse_http_url(NETBENCH_URL) else {
        NETBENCH_FAIL_CODE.store(1, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    };
    let ip = match crate::v::net::dns::resolve_ipv4_for_device(
        nic_index,
        host.as_str(),
        crate::v::net::dns::DnsConfig::default(),
    )
    .await
    {
        Ok(v) => v,
        Err(_e) => {
            NETBENCH_FAIL_CODE.store(2, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
    };

    let Some(vnet) = crate::v::net::VNet::open_with_event_queue_depth(nic_index, 4096) else {
        NETBENCH_FAIL_CODE.store(3, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    };
    if vnet
        .submit(api::Command::OpenTcpConnect {
            remote: api::EndpointV4 { addr: ip, port },
        })
        .is_err()
    {
        NETBENCH_FAIL_CODE.store(4, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    }

    let open_deadline = Instant::now() + EmbassyDuration::from_millis(OPEN_TIMEOUT_MS);
    let tcp_handle = loop {
        if NETBENCH_ABORT_REQ.load(Ordering::Relaxed) {
            NETBENCH_STATE.store(NETBENCH_ABORTED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
        if Instant::now() >= open_deadline {
            NETBENCH_FAIL_CODE.store(5, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
        if let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => break handle,
                api::Event::Error { msg: _ } => {
                    NETBENCH_FAIL_CODE.store(6, Ordering::Relaxed);
                    NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
                    NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
                    return;
                }
                _ => {}
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    };

    let request = alloc::format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path.as_str(),
        host.as_str()
    );
    if vnet
        .submit(api::Command::SendTcp {
            handle: tcp_handle,
            data: api::ByteBuf::from_slice_trunc(request.as_bytes()),
        })
        .is_err()
    {
        let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
        NETBENCH_FAIL_CODE.store(7, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    }

    let mut overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
    let mut header_bytes: Vec<u8> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut expected_len: Option<usize> = None;
    let mut received_bytes: usize = 0;
    let mut failed = false;
    let mut fail_code: u8 = 10;
    let mut closed = false;

    loop {
        if NETBENCH_ABORT_REQ.load(Ordering::Relaxed) {
            NETBENCH_STATE.store(NETBENCH_ABORTED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            break;
        }
        if Instant::now() >= overall_deadline {
            failed = true;
            fail_code = 8;
            break;
        }
        let mut got_event = false;
        while let Some(ev) = vnet.pop_event() {
            got_event = true;
            match ev {
                api::Event::TcpData { handle, data } if handle == tcp_handle => {
                    let bytes = data.as_slice();
                    if !header_done {
                        let room = MAX_CAPTURE_BYTES.saturating_sub(header_bytes.len());
                        if room == 0 {
                            failed = true;
                            fail_code = 9;
                            break;
                        }
                        let take = core::cmp::min(room, bytes.len());
                        header_bytes.extend_from_slice(&bytes[..take]);
                        if let Some(hend) = find_http_header_end(header_bytes.as_slice()) {
                            header_done = true;
                            expected_len = parse_content_length(&header_bytes[..hend]);
                            if let Some(cl) = expected_len {
                                let reserve = core::cmp::min(cl, MAX_CAPTURE_BYTES);
                                body.reserve(reserve);
                            }
                            if hend < header_bytes.len() {
                                let remain = header_bytes.split_off(hend);
                                let room2 = MAX_CAPTURE_BYTES.saturating_sub(body.len());
                                let take2 = core::cmp::min(room2, remain.len());
                                body.extend_from_slice(&remain[..take2]);
                                received_bytes = received_bytes.saturating_add(take2);
                                NETBENCH_BYTES.store(received_bytes as u64, Ordering::Relaxed);
                                if take2 < remain.len() {
                                    failed = true;
                                    fail_code = 9;
                                    break;
                                }
                                if let Some(cl) = expected_len {
                                    if body.len() >= cl {
                                        closed = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if take < bytes.len() && !header_done {
                            failed = true;
                            fail_code = 9;
                            break;
                        }
                    } else {
                        let room = MAX_CAPTURE_BYTES.saturating_sub(body.len());
                        if room == 0 {
                            failed = true;
                            fail_code = 9;
                            break;
                        }
                        let take = core::cmp::min(room, bytes.len());
                        body.extend_from_slice(&bytes[..take]);
                        received_bytes = received_bytes.saturating_add(take);
                        NETBENCH_BYTES.store(received_bytes as u64, Ordering::Relaxed);
                        if take < bytes.len() {
                            failed = true;
                            fail_code = 9;
                            break;
                        }
                        if let Some(cl) = expected_len {
                            if body.len() >= cl {
                                closed = true;
                                break;
                            }
                        }
                    }
                    overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
                }
                api::Event::Closed { handle } if handle == tcp_handle => {
                    closed = true;
                    break;
                }
                api::Event::Error { msg: _ } => {
                    failed = true;
                    fail_code = 10;
                    break;
                }
                _ => {}
            }
        }
        if failed {
            break;
        }
        if closed {
            break;
        }

        if !got_event {
            Timer::after(EmbassyDuration::from_micros(IDLE_YIELD_US)).await;
        }
    }

    let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
    if NETBENCH_STATE.load(Ordering::Relaxed) != NETBENCH_ABORTED {
        if let Some(cl) = expected_len {
            if body.len() > cl {
                body.truncate(cl);
            }
        }
        NETBENCH_BYTES.store(body.len() as u64, Ordering::Relaxed);
        if failed {
            NETBENCH_FAIL_CODE.store(fail_code, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        } else {
            NETBENCH_STATE.store(NETBENCH_DONE, Ordering::Relaxed);
        }
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
    }
}

#[embassy_executor::task(pool_size = 3)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend) {
    io.init();

    // Ensure the registry is populated before the shell starts.
    self::cmd::init_builtin_shell_commands();

    let mut term_cols: usize = DEFAULT_TERM_COLS;
    let mut term_rows: usize = DEFAULT_TERM_ROWS;

    write_banner(io, term_cols);
    apply_shell_scroll_region(io, term_rows);
    write_prompt(io);

    let mut line: String<128> = String::new();
    let mut utf8 = Utf8Decoder::new();
    let mut go_idx: usize = 0;
    let mut next_matrix_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut install_wizard: Option<InstallWizardStage> = None;
    let mut go_mode: bool = false;
    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.set_shape(WireShape::Cube);
    cube.reset();
    enter_cube_mode(io, &mut term_cols, &mut term_rows);

    // Treat CRLF as a single Enter (common on serial/USB bridges).
    let mut saw_cr: bool = false;
    let mut next_netbench_update: Instant = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
    let mut last_netbench_state: u8 = NETBENCH_STATE.load(Ordering::Relaxed);
    let mut next_status_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);

    loop {
        let netbench_state = NETBENCH_STATE.load(Ordering::Relaxed);
        if Instant::now() >= next_status_refresh {
            refresh_status_bar(io, term_cols, term_rows);
            next_status_refresh = Instant::now() + EmbassyDuration::from_millis(250);
        }
        if netbench_state == NETBENCH_RUNNING && Instant::now() >= next_netbench_update {
            let now_tick = embassy_time_driver::now();
            let start_tick = NETBENCH_START_TICK.load(Ordering::Relaxed);
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bytes = NETBENCH_BYTES.load(Ordering::Relaxed);
            let bps = if elapsed_ms == 0 {
                0
            } else {
                bytes.saturating_mul(1000) / elapsed_ms
            };
            let mut speed: heapless::String<10> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut speed, format_args!("{}kb/s", bps / 1024));
            let _ = crate::shell::statusbar::set_right_active(speed.as_str());
            next_netbench_update = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
        }

        if netbench_state != last_netbench_state {
            if netbench_state != NETBENCH_RUNNING {
                match netbench_state {
                    NETBENCH_DONE => {
                        let _ = crate::shell::statusbar::set_left_active("done");
                        let _ = crate::shell::statusbar::set_right_active("ok");
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 2);
                        }
                    }
                    NETBENCH_ABORTED => {
                        let _ = crate::shell::statusbar::set_left_active("aborted");
                        let _ = crate::shell::statusbar::set_right_active("stopped");
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 3);
                        }
                    }
                    NETBENCH_FAILED => {
                        let code = NETBENCH_FAIL_CODE.load(Ordering::Relaxed);
                        io.write_fmt(format_args!("netbench: failed ({})\r\n", netbench_fail_text(code)));
                        let _ = crate::shell::statusbar::set_left_active("failed");
                        let mut right: heapless::String<10> = heapless::String::new();
                        let _ = core::fmt::Write::write_fmt(&mut right, format_args!("e{}", code));
                        let _ = crate::shell::statusbar::set_right_active(right.as_str());
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 1);
                        }
                    }
                    _ => {}
                }
            }
            last_netbench_state = netbench_state;
        }

        if let Some(b) = io.read_byte() {
            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    term_cols = DEFAULT_TERM_COLS;
                    term_rows = DEFAULT_TERM_ROWS;
                    set_go_mode(io, &mut go_mode, false);
                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                    io.write_str(crate::ecma48::HOME);
                    write_banner(io, term_cols);
                    apply_shell_scroll_region(io, term_rows);
                    write_prompt(io);
                }
                continue;
            }
            match b {
                b'\r' | b'\n' | b' ' if matches!(pending_action, Some(PendingAction::AcpiReset | PendingAction::AcpiState(_))) => {
                    utf8.clear();
                    // Other pending actions: Enter/Space cancels.
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(io, &mut go_mode, false);
                    line.clear();
                    io.write_str("\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b if matches!(pending_action, Some(PendingAction::FormatConfirm { .. })) && b != b'\r' && b != b'\n' => {
                    // Destructive format confirmation: Enter confirms, any other key cancels.
                    utf8.clear();
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(io, &mut go_mode, false);
                    line.clear();
                    io.write_str("\r\nformat: cancelled\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b if matches!(pending_action, Some(PendingAction::InstallConfirm { .. })) && b != b'\r' && b != b'\n' => {
                    // Destructive install confirmation: Enter confirms, any other key cancels.
                    utf8.clear();
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(io, &mut go_mode, false);
                    line.clear();
                    io.write_str("\r\ninstall: cancelled\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b'\r' | b'\n' => {
                    utf8.clear();

                    // Confirmation gate for destructive `format`.
                    if let Some(PendingAction::FormatConfirm { disc_id }) = pending_action {
                        let do_format = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;
                        set_go_mode(io, &mut go_mode, false);

                        if do_format {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nformat: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            io.write_str("\r\nformat: creating 1 partition + TRUEOSFS...\r\n");

                            let parts = [crate::disc::install::gpt::GptPartitionSpec {
                                type_guid: crate::v::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
                                name: "TRUEOS",
                                size: crate::disc::install::gpt::PartitionSize::Remaining,
                                attributes: 0,
                            }];

                            let mut log = |msg: &str| {
                                io.write_str(msg);
                                io.write_str("\r\n");
                            };

                            let gpt_result = crate::disc::install::gpt::write_gpt_layout_with_log(
                                handle,
                                &parts,
                                &mut log,
                            )
                            .await;

                            match gpt_result {
                                Ok(_ranges) => {
                                    match crate::v::disc::partition::register_gpt_partitions(handle).await {
                                        Ok(reg) => {
                                            let Some(first) = reg.first() else {
                                                io.write_str("format: no partitions registered\r\n");
                                                write_prompt(io);
                                                continue;
                                            };

                                            let Some(part_handle) = crate::disc::block::device_handle(first.id) else {
                                                io.write_str("format: partition handle lookup failed\r\n");
                                                write_prompt(io);
                                                continue;
                                            };

                                            match crate::v::fs::trueosfs::format_blank_partition_async(part_handle).await {
                                                Ok(()) => {
                                                    let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
                                                    io.write_fmt(format_args!(
                                                        "format: ok (status now: {}{})\r\n",
                                                        status.short(),
                                                        match (&status, err) {
                                                            (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!("; err={:?}", e),
                                                            _ => alloc::string::String::new(),
                                                        }
                                                    ));
                                                }
                                                Err(e) => {
                                                    io.write_fmt(format_args!("format: TRUEOSFS failed ({:?})\r\n", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            io.write_fmt(format_args!("format: partition register failed ({:?})\r\n", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    io.write_fmt(format_args!("format: GPT write failed ({:?})\r\n", e));
                                }
                            }
                        } else {
                            io.write_str("\r\nformat: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Confirmation gate for destructive `install`.
                    if let Some(PendingAction::InstallConfirm { disc_id }) = pending_action {
                        let do_install = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;
                        set_go_mode(io, &mut go_mode, false);

                        if do_install {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\ninstall: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            let Some(kernel) = crate::limine::install_kernel_bytes() else {
                                io.write_str("\r\ninstall: kernel module missing\r\n");
                                io.write_str(
                                    "install: expected Limine module_string trueos.install.kernel\r\n",
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let Some(bootx64) = crate::limine::install_bootx64_bytes() else {
                                io.write_str("\r\ninstall: BOOTX64.EFI module missing\r\n");
                                io.write_str(
                                    "install: expected Limine module_string trueos.install.bootx64\r\n",
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            io.write_str("\r\ninstall: starting...\r\n");
                            match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", disc_id).as_str()) {
                                Some(slot) => {
                                    let _ = spawner.spawn(crate::matrix::install_matrix_job(
                                            slot,
                                            handle,
                                            bootx64,
                                            kernel,
                                        ),
                                    );
                                    io.write_fmt(format_args!(
                                        "install: started §{} (dump logs with § {})\r\n",
                                        slot + 1,
                                        slot + 1
                                    ));
                                    crate::matrix::refresh_matrix_symbols(io, term_cols);
                                }
                                None => {
                                    io.write_str("install: matrix full\r\n");
                                }
                            }
                        } else {
                            io.write_str("\r\ninstall: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Confirmation gate for `update` (network fetch + refresh installed boot files).
                    if let Some(PendingAction::UpdateConfirm { disc_id }) = pending_action {
                        let do_update = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;
                        set_go_mode(io, &mut go_mode, false);

                        if do_update {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nupdate: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            io.write_str("\r\nupdate: starting...\r\n");
                            match crate::matrix::alloc_slot(alloc::format!("update disc{:03}", disc_id).as_str()) {
                                Some(slot) => {
                                    let _ = spawner.spawn(crate::matrix::update_matrix_job(slot, handle),
                                    );
                                    io.write_fmt(format_args!(
                                        "update: started §{} (dump logs with § {})\r\n",
                                        slot + 1,
                                        slot + 1
                                    ));
                                    crate::matrix::refresh_matrix_symbols(io, term_cols);
                                }
                                None => {
                                    io.write_str("update: matrix full\r\n");
                                }
                            }
                        } else {
                            io.write_str("\r\nupdate: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Interactive install wizard consumes whole-line input (including empty line).
                    if pending_action.is_none() {
                        if let Some(InstallWizardStage::SelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `install 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("install") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\ninstall: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\ninstall: invalid id\r\n");
                                io.write_str("install: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\ninstall: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\ninstall: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));
                            io.write_str("install: DANGER: this may REPARTITION and FORMAT the disk\r\n");
                            io.write_str("install: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::InstallConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::UpdateSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `update 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("update") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nupdate: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nupdate: invalid id\r\n");
                                io.write_str("update: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nupdate: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\nupdate: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));

                            // Safety: `update` is intended to refresh boot files on an already-installed TRUEOS disk.
                            // Refuse to proceed if the disk doesn't already look like TRUEOSFS.
                            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                                io.write_str("update: refused (selected disk is not a TRUEOS disk)\r\n");
                                io.write_str("update: use `install` for a fresh install\r\n");
                                io.write_str("update: choose another disk id (or 'q' to cancel)\r\n");
                                print_update_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            io.write_str("update: downloads BOOTX64.EFI + TRUEOS.elf and refreshes ESP boot files\r\n");
                            io.write_str("update: will NOT repartition/format (refuses if TRUEOSFS is not detected)\r\n");
                            io.write_str("update: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::UpdateConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::FormatSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `format 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("format") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nformat: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nformat: invalid id\r\n");
                                io.write_str("format: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nformat: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\nformat: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));
                            io.write_str("format: DANGER: this destroys all data on the disk\r\n");
                            io.write_str("format: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::FormatConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::FileSelectMount) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `file 0` as well as just `0`.
                            if let Some(rest) = s.strip_prefix("file") {
                                s = rest.trim();
                            }

                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nfile: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            if s.eq_ignore_ascii_case("ls") || s.eq_ignore_ascii_case("list") {
                                line.clear();
                                print_trueosfs_mount_table(io).await;
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let roots = crate::v::fs::trueosfs::list_roots();

                            // Prefer the mount table index when it is in range.
                            let (handle, shown_id) = if let Ok(idx) = s.parse::<usize>() {
                                if let Some(r) = roots.get(idx) {
                                    (
                                        crate::disc::block::device_handle(r.disk_id),
                                        Some(r.disk_id.raw()),
                                    )
                                } else {
                                    (None, None)
                                }
                            } else {
                                let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                                if raw_id == 0 {
                                    (None, None)
                                } else {
                                    let target = crate::disc::block::device_handles()
                                        .into_iter()
                                        .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                                    (target, Some(raw_id))
                                }
                            };

                            line.clear();

                            let Some(handle) = handle else {
                                io.write_str("\r\nfile: invalid mount id (try 'ls')\r\n");
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            io.write_fmt(format_args!(
                                "\r\nfile: printing tree for {} (raw={})\r\n",
                                handle.id(),
                                shown_id.unwrap_or(handle.id().raw())
                            ));

                            print_trueosfs_tree_25(io, handle).await;
                            io.write_str("\r\nfile: enter another mount index/id, 'ls', or 'q'\r\n");
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::BenchSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            if let Some(rest) = s.strip_prefix("bench") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nbench: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nbench: invalid id\r\n");
                                io.write_str("bench: enter a TRUEOSFS disk id or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nbench: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
                            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                                io.write_fmt(format_args!(
                                    "\r\nbench: refused (id={} is not TRUEOSFS; status={}{} )\r\n",
                                    raw_id,
                                    status.short(),
                                    match (&status, err) {
                                        (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!(" err={:?}", e),
                                        _ => alloc::string::String::new(),
                                    }
                                ));
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            install_wizard = None;
                            run_bench_fs(io, handle).await;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::NetbenchSelectNic) = install_wizard {
                            let mut s = line.as_str().trim();
                            if let Some(rest) = s.strip_prefix("netbench") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nnetbench: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let nic_index = s.parse::<usize>().ok();
                            line.clear();
                            let Some(nic_index) = nic_index else {
                                io.write_str("\r\nnetbench: invalid nic id\r\n");
                                io.write_str("netbench: enter a nic id or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };
                            if nic_index >= crate::net::device_count() {
                                io.write_str("\r\nnetbench: no such nic\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            install_wizard = None;
                            if netbench_start(&spawner, nic_index) {
                                let slot = NETBENCH_STATUS_SLOT.load(Ordering::Relaxed);
                                io.write_fmt(format_args!(
                                    "\r\nnetbench: started nic={} §{} url={}\r\n",
                                    nic_index,
                                    slot.saturating_add(1),
                                    NETBENCH_URL
                                ));
                                let _ = crate::shell::statusbar::set_left_active("netbench");
                                let _ = crate::shell::statusbar::set_right_active("0kb/s");
                                for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                                    let _ = crate::shell::statusbar::set_indicator_active(i, 2);
                                }
                                next_netbench_update = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
                                last_netbench_state = NETBENCH_RUNNING;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            } else {
                                io.write_str("\r\nnetbench: already running\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            continue;
                        }
                    }

                    if line.is_empty() && pending_action.is_none() && go_mode {
                        set_go_mode(io, &mut go_mode, false);
                        io.write_str("\r\n");
                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }
                    if line.is_empty() && pending_action.is_none() {
                        // Empty command: do nothing (no newline, no prompt).
                        continue;
                    }
                    if !line.is_empty() {
                        // Enter uses the same prefix matching behavior as Tab:
                        // unique -> expand and execute, ambiguous -> show matches and stay in-place.
                        if pending_action.is_none() {
                            let mut input_buf: String<128> = String::new();
                            let _ = input_buf.push_str(line.as_str());
                            let input = input_buf.as_str();
                            if !input.chars().any(|ch| ch.is_whitespace()) {
                                let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
                                crate::shell::cmd::list_command_names(&mut cmds);
                                let mut matches = 0usize;
                                let mut unique: Option<&'static str> = None;
                                for name in cmds.iter().copied() {
                                    if starts_with_ignore_ascii_case(name, input) {
                                        matches += 1;
                                        if unique.is_none() {
                                            unique = Some(name);
                                        }
                                    }
                                }
                                if matches > 1 {
                                    utf8.clear();
                                    handle_tab_completion(
                                        io,
                                        &mut line,
                                        term_cols,
                                        term_rows,
                                        pending_action,
                                        install_wizard,
                                    );
                                    continue;
                                }
                                if matches == 1 {
                                    if let Some(full) = unique {
                                        if !full.eq_ignore_ascii_case(input) {
                                            line.clear();
                                            let _ = line.push_str(full);
                                        }
                                    }
                                }
                            }
                        }

                        append_output_cursor(io, term_rows);
                        let action = handle_line(
                            &line,
                            &spawner,
                            io,
                            &mut term_cols,
                            &mut term_rows,
                            &mut go_mode,
                            &mut install_wizard,
                        );
                        line.clear();
                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline = match action {
                                    PendingAction::AcpiReset |
                                    PendingAction::AcpiState(_) => {
                                        Some(Instant::now() + EmbassyDuration::from_secs(5))
                                    }
                                    PendingAction::FormatConfirm { .. } => None,
                                    PendingAction::InstallConfirm { .. } => None,
                                    PendingAction::UpdateConfirm { .. } => None,
                                };
                                set_go_mode(
                                    io,
                                    &mut go_mode,
                                    matches!(action, PendingAction::AcpiReset | PendingAction::AcpiState(_)),
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowInstallDiskTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_install_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowFormatDiskTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_format_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowUpdateDiskTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_update_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowFileMountTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_trueosfs_mount_table(io).await;
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowBenchDiskTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_bench_disk_table(io).await;
                                io.write_str("bench: enter TRUEOSFS disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowNetbenchNicTable => {
                                set_go_mode(io, &mut go_mode, false);
                                print_netbench_nic_table(io).await;
                                io.write_str("netbench: enter nic id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::Mv { src, dst } => {
                                set_go_mode(io, &mut go_mode, false);
                                match crate::surface::io::kfs::rename_async(src.as_str(), dst.as_str()).await {
                                    Ok(()) => io.write_str("mv: ok\r\n"),
                                    Err(e) => io.write_fmt(format_args!("mv: failed ({:?})\r\n", e)),
                                }
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::Qjs { src } => {
                                set_go_mode(io, &mut go_mode, false);
                                if trueos_qjs::async_fs::ensure_service_started(&spawner) {
                                } else {
                                    io.write_str("qjs: async fs service unavailable\r\n");
                                    write_prompt_for_state(io, pending_action, install_wizard);
                                    continue;
                                }
                                crate::shell::shellqjs::run(io, src.as_str()).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::EnterCube => {
                                cube_mode = true;
                                set_go_mode(io, &mut go_mode, false);
                                cube.set_shape(WireShape::Cube);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
                            }
                            CommandAction::EnterIco => {
                                cube_mode = true;
                                set_go_mode(io, &mut go_mode, false);
                                cube.set_shape(WireShape::Icosidodecahedron);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
                            }
                            CommandAction::EnterTxtEdt { filename, slot_id } => {
                                cube_mode = false;
                                set_go_mode(io, &mut go_mode, false);
                                let cols = term_cols;
                                let rows = term_rows;

                                // Edit the slot blob in-place (no auto-capture into a new slot).
                                let Some(buf) = crate::matrix::take_blob(slot_id) else {
                                    io.write_str("\r\ntxt: invalid slot\r\n");
                                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                                    io.write_str(crate::ecma48::HOME);
                                    write_banner(io, term_cols);
                                    apply_shell_scroll_region(io, term_rows);
                                    write_prompt(io);
                                    continue;
                                };

                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
                                let out_buf = crate::shell::txtedt::run(io, cols, rows, filename.as_str(), buf).await;
                                let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                                io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
                                crate::matrix::refresh_matrix_symbols(io, term_cols);

                                io.write_str(crate::ecma48::CLEAR_SCREEN);
                                io.write_str(crate::ecma48::HOME);
                                write_banner(io, term_cols);
                                apply_shell_scroll_region(io, term_rows);
                                write_prompt(io);
                            }
                            CommandAction::None => {
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                        }
                    }
                }
                0x08 | 0x7F => {
                    utf8.clear();
                    if !line.is_empty() {
                        line.pop();
                        io.write_str("\x08 \x08");
                    }
                }
                0x03 => {
                    utf8.clear();
                    line.clear();
                    io.write_str("^C\r\n");
                    write_prompt(io);
                }
                b'\t' => {
                    utf8.clear();
                    handle_tab_completion(
                        io,
                        &mut line,
                        term_cols,
                        term_rows,
                        pending_action,
                        install_wizard,
                    );
                }
                _ => {
                    if b >= 0x20 {
                        if let Some(ch) = utf8.push(b) {
                            if line.push(ch).is_ok() {
                                io.write_char(ch);
                            }
                        }
                    }
                }
            }
        } else {
            if cube_mode {
                cube.draw_frame(io);
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }

            // Keep header symbols in sync with background job state transitions.
            if Instant::now() >= next_matrix_refresh {
                crate::matrix::refresh_matrix_symbols(io, term_cols);
                next_matrix_refresh = Instant::now() + EmbassyDuration::from_millis(250);
            }

            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    set_go_mode(io, &mut go_mode, false);
                    pending_action = None;
                    pending_deadline = None;
                    match action {
                        PendingAction::AcpiReset => {
                            if crate::efi::acpi::facp::reset_system().is_err() {
                                io.write_str("\r\nacpi reset failed\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::AcpiState(level) => {
                            if crate::efi::acpi::facp::enter_named_sleep_state(level).is_err() {
                                io.write_fmt(format_args!("\r\nacpi s{} failed\r\n", level));
                                write_prompt(io);
                            }
                        }
                        PendingAction::FormatConfirm { .. } => {}
                        PendingAction::InstallConfirm { .. } => {}
                        PendingAction::UpdateConfirm { .. } => {}
                    }
                    continue;
                }
            }
            if go_mode {
                let ch = GO_CHARS[go_idx];
                go_idx = (go_idx + 1) % GO_CHARS.len();
                io.write_str("\r");
                write_prompt(io);
                io.write_char(ch);
                Timer::after(EmbassyDuration::from_millis(160)).await;
            } else {
                Timer::after(EmbassyDuration::from_millis(2)).await;
            }
        }
    }
}

fn handle_line(
    line: &str,
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    go_mode: &mut bool,
    install_wizard: &mut Option<InstallWizardStage>,
) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // New-style registered commands (typed args + validation + introspection).
    {
        let mut ctx = crate::shell::cmd::ShellCommandCtx {
            line: cmd,
            spawner,
            io,
            term_cols,
            term_rows,
            go_mode,
            install_wizard,
        };
        if let Some(action) = crate::shell::cmd::dispatch_line(&mut ctx) {
            return action;
        }
    }

    io.write_str("unknown: ");
    io.write_str(cmd);
    io.write_str("\r\n");
    CommandAction::None
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year: u32 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365u64 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month_idx = 0;
    while month_idx < month_lengths.len() {
        let len = month_lengths[month_idx] as u64;
        if days < len {
            let day = (days + 1) as u8;
            return (year, (month_idx + 1) as u8, day, hour, minute, second);
        }
        days -= len;
        month_idx += 1;
    }

    (year, 12, 31, hour, minute, second)
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub(crate) fn draw_corners(io: &dyn ShellIo, cols: usize, rows: usize) {
    if cols == 0 || rows == 0 {
        return;
    }
    io.write_str(crate::ecma48::SAVE_CURSOR);
    // top-right
    write_pos(io, 1, cols);
    io.write_byte(b'O');
    // bottom-left
    write_pos(io, rows, 1);
    io.write_byte(b'O');
    // bottom-right
    write_pos(io, rows, cols);
    io.write_byte(b'O');
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn write_pos(io: &dyn ShellIo, row: usize, col: usize) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
}

fn enter_cube_mode(io: &dyn ShellIo, term_cols: &mut usize, term_rows: &mut usize) {
    *term_cols = CUBE_COLS;
    *term_rows = CUBE_ROWS;
    draw_corners(io, CUBE_COLS, CUBE_ROWS);
    shellcube::enter_mode(io);
}
