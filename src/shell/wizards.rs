use embassy_executor::Spawner;
use embassy_time::Instant;

use crate::ecma48;
use crate::shell::{CommandAction, ShellBackend, ShellIo, PROMPT_RGB};
use crate::shell::table::{Table, TableColumn};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallWizardStage {
    SelectDisk,
    FormatSelectDisk,
    UpdateSelectDisk,
    FileSelectMount,
    BenchSelectDisk,
    NetbenchSelectNic,
}

#[derive(Copy, Clone)]
pub enum PendingAction {
    AcpiReset,
    AcpiState(u8),
    FormatConfirm { disc_id: u32 },
    InstallConfirm { disc_id: u32 },
    UpdateConfirm { disc_id: u32 },
}

#[derive(Clone)]
pub enum ShellMode {
    Idle,
    Wizard(InstallWizardStage),
    Confirm(PendingAction),
    Wait {
        action: PendingAction,
        deadline: Instant,
    },
}

impl Default for ShellMode {
    fn default() -> Self {
        Self::Idle
    }
}

pub enum InputResult {
    Handled,
    Transition(ShellMode),
    ProcessCommand,
    RunAction(CommandAction),
}

impl ShellMode {
    pub async fn process_input(
        &self,
        io: &dyn ShellBackend,
        line: &str,
        spawner: &Spawner,
    ) -> InputResult {
        match self {
            ShellMode::Idle => InputResult::ProcessCommand,
            ShellMode::Wait { .. } => {
                io.write_str("\r\n");
                InputResult::Transition(ShellMode::Idle)
            }
            ShellMode::Confirm(action) => {
                let s = line.trim();
                match action {
                    PendingAction::FormatConfirm { disc_id } => {
                        if s.is_empty() {
                            InputResult::RunAction(CommandAction::DoFormat { disc_id: *disc_id })
                        } else {
                            io.write_str("\r\nformat: cancelled\r\n");
                            InputResult::Transition(ShellMode::Idle)
                        }
                    }
                    PendingAction::InstallConfirm { disc_id } => {
                        if s.is_empty() {
                            InputResult::RunAction(CommandAction::DoInstall { disc_id: *disc_id })
                        } else {
                            io.write_str("\r\ninstall: cancelled\r\n");
                            InputResult::Transition(ShellMode::Idle)
                        }
                    }
                    PendingAction::UpdateConfirm { disc_id } => {
                        if s.is_empty() {
                            InputResult::RunAction(CommandAction::DoUpdate { disc_id: *disc_id })
                        } else {
                            io.write_str("\r\nupdate: cancelled\r\n");
                            InputResult::Transition(ShellMode::Idle)
                        }
                    }
                    _ => InputResult::Transition(ShellMode::Idle),
                }
            }
            ShellMode::Wizard(stage) => {
                handle_wizard_input_internal(io, line, spawner, *stage).await
            }
        }
    }
}

fn should_cancel(s: &str) -> bool {
    s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit")
}

async fn handle_wizard_input_internal(
    io: &dyn ShellBackend,
    line: &str,
    _spawner: &Spawner,
    stage: InstallWizardStage,
) -> InputResult {
    let mut s = line.trim();

    match stage {
        InstallWizardStage::SelectDisk => {
            if let Some(rest) = s.strip_prefix("install") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\ninstall: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
            if raw_id == 0 {
                io.write_str("\r\ninstall: invalid id\r\n");
                io.write_str("install: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                return InputResult::Handled;
            }
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
            let Some(handle) = target else {
                io.write_str("\r\ninstall: no such disk\r\n");
                return InputResult::Handled;
            };

            let info = handle.info();
            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
            io.write_fmt(format_args!(
                "\r\ninstall: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                info.id.raw(), info.id, info.block_count, info.block_size, info.writable, info.label, status.short(),
            ));
            io.write_str("install: DANGER: this may REPARTITION and FORMAT the disk\r\n");
            io.write_str("install: press Enter to confirm (any other key cancels)\r\n");

            InputResult::Transition(ShellMode::Confirm(PendingAction::InstallConfirm {
                disc_id: raw_id,
            }))
        }

        InstallWizardStage::UpdateSelectDisk => {
            if let Some(rest) = s.strip_prefix("update") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\nupdate: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
            if raw_id == 0 {
                io.write_str("\r\nupdate: invalid id\r\n");
                io.write_str("update: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                return InputResult::Handled;
            }
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
            let Some(handle) = target else {
                io.write_str("\r\nupdate: no such disk\r\n");
                return InputResult::Handled;
            };

            let info = handle.info();
            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
            io.write_fmt(format_args!(
                "\r\nupdate: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                info.id.raw(), info.id, info.block_count, info.block_size, info.writable, info.label, status.short(),
            ));

            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                io.write_str("update: refused (selected disk is not a TRUEOS disk)\r\n");
                io.write_str("update: use `install` for a fresh install\r\n");
                io.write_str("update: choose another disk id (or 'q' to cancel)\r\n");
                print_update_disk_table(io).await;
                return InputResult::Handled;
            }

            io.write_str(
                "update: downloads BOOTX64.EFI + TRUEOS.elf and refreshes ESP boot files\r\n",
            );
            io.write_str(
                "update: will NOT repartition/format (refuses if TRUEOSFS is not detected)\r\n",
            );
            io.write_str("update: press Enter to confirm (any other key cancels)\r\n");

            InputResult::Transition(ShellMode::Confirm(PendingAction::UpdateConfirm {
                disc_id: raw_id,
            }))
        }

        InstallWizardStage::FormatSelectDisk => {
            if let Some(rest) = s.strip_prefix("format") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\nformat: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
            if raw_id == 0 {
                io.write_str("\r\nformat: invalid id\r\n");
                io.write_str("format: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                return InputResult::Handled;
            }
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
            let Some(handle) = target else {
                io.write_str("\r\nformat: no such disk\r\n");
                return InputResult::Handled;
            };

            let info = handle.info();
            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
            io.write_fmt(format_args!(
                "\r\nformat: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                info.id.raw(), info.id, info.block_count, info.block_size, info.writable, info.label, status.short(),
            ));
            io.write_str("format: DANGER: this destroys all data on the disk\r\n");
            io.write_str("format: press Enter to confirm (any other key cancels)\r\n");

            InputResult::Transition(ShellMode::Confirm(PendingAction::FormatConfirm {
                disc_id: raw_id,
            }))
        }

        InstallWizardStage::FileSelectMount => {
            if let Some(rest) = s.strip_prefix("file") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\nfile: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            if s.eq_ignore_ascii_case("ls") || s.eq_ignore_ascii_case("list") {
                print_trueosfs_mount_table(io).await;
                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                return InputResult::Handled;
            }

            let roots = crate::v::fs::trueosfs::list_roots();
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
                    (
                        crate::disc::block::device_handles()
                            .into_iter()
                            .find(|h| h.parent().is_none() && h.id().raw() == raw_id),
                        Some(raw_id),
                    )
                }
            };

            let Some(handle) = handle else {
                io.write_str("\r\nfile: invalid mount id (try 'ls')\r\n");
                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                return InputResult::Handled;
            };

            io.write_fmt(format_args!(
                "\r\nfile: printing tree for {} (raw={})\r\n",
                handle.id(),
                shown_id.unwrap_or(handle.id().raw())
            ));

            print_trueosfs_tree_25(io, handle).await;
            io.write_str("\r\nfile: enter another mount index/id, 'ls', or 'q'\r\n");
            InputResult::Handled
        }

        InstallWizardStage::BenchSelectDisk => {
            if let Some(rest) = s.strip_prefix("bench") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\nbench: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
            if raw_id == 0 {
                io.write_str("\r\nbench: invalid id\r\n");
                io.write_str("bench: enter a TRUEOSFS disk id or 'q'\r\n");
                return InputResult::Handled;
            }

            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
            let Some(handle) = target else {
                io.write_str("\r\nbench: no such disk\r\n");
                return InputResult::Handled;
            };

            let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                io.write_fmt(format_args!(
                    "\r\nbench: refused (id={} is not TRUEOSFS; status={}{} )\r\n",
                    raw_id,
                    status.short(),
                    match (&status, err) {
                        (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) =>
                            alloc::format!(" err={:?}", e),
                        _ => alloc::string::String::new(),
                    }
                ));
                return InputResult::Handled;
            }

            InputResult::RunAction(CommandAction::RunBenchFs { disk_id: raw_id })
        }

        InstallWizardStage::NetbenchSelectNic => {
            if let Some(rest) = s.strip_prefix("netbench") {
                s = rest.trim();
            }
            if should_cancel(s) {
                io.write_str("\r\nnetbench: cancelled\r\n");
                return InputResult::Transition(ShellMode::Idle);
            }
            let nic_index = s.parse::<usize>().ok();
            let Some(nic_index) = nic_index else {
                io.write_str("\r\nnetbench: invalid nic id\r\n");
                io.write_str("netbench: enter a nic id or 'q'\r\n");
                return InputResult::Handled;
            };
            if nic_index >= crate::net::device_count() {
                io.write_str("\r\nnetbench: no such nic\r\n");
                return InputResult::Handled;
            }

            InputResult::RunAction(CommandAction::RunNetbench { nic_index })
        }
    }
}

pub(crate) fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}

fn write_inserted_line(io: &dyn ShellIo, args: core::fmt::Arguments) {
    // Insert Line at current cursor position (pushes everything down)
    io.write_str("\x1b[L");

    // Fix artifact on the line below (which was pushed down)
    io.write_str(ecma48::SAVE_CURSOR);
    io.write_str("\x1b[1B\r"); // Down 1, CR to Column 1
    io.write_str("\x1b[38;2;80;80;80m│\x1b[0m");
    io.write_str(ecma48::RESTORE_CURSOR);

    // Write scrollbar for the new line
    io.write_str("\x1b[38;2;80;80;80m│\x1b[0m");

    io.write_fmt(args);
    // Move to next line (which puts us on top of the pushed content, ready to insert again)
    io.write_str("\r\n");
}

pub(crate) async fn print_generic_disk_table(io: &dyn ShellIo, header: &str, filter_trueos: bool) {
    io.write_fmt(format_args!(
        "{} {}\r\n",
        crate::ecma48::style(header).bold().fg(PROMPT_RGB),
        crate::ecma48::dim("disk selection")
    ));
    io.write_fmt(format_args!(
        "{}\r\n\r\n",
        crate::ecma48::dim("choose a disk id to continue (blank/q cancels)")
    ));

    // Keep table cells ASCII-only (no ANSI sequences) so width/truncation stays predictable.
    // Alignment is handled by the output backend (ReverseOutput).
    let cols = [
        TableColumn { header: "ID", width: 6 },
        TableColumn { header: "Name", width: 10 },
        TableColumn { header: "Size", width: 10 },
        TableColumn { header: "Mode", width: 4 },
        TableColumn { header: "Status", width: 12 },
        TableColumn { header: "Label", width: 24 },
    ];

    let mut found = false;
    {
        let t = Table::new(&cols);
        t.print_header(io);

        for h in crate::disc::block::device_handles().into_iter() {
            if h.parent().is_some() {
                continue;
            }

            let info = h.info();
            let (status, _err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;

            if filter_trueos && !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                continue;
            }
            found = true;

            let total = info.block_count.saturating_mul(info.block_size as u64);
            let size = if total >= 1024 * 1024 * 1024 {
                alloc::format!("{}GB", total / (1024 * 1024 * 1024))
            } else if total >= 1024 * 1024 {
                alloc::format!("{}MB", total / (1024 * 1024))
            } else {
                alloc::format!("{}KB", total / 1024)
            };

            let mode = if info.writable { "RW" } else { "RO" };
            let label = info.label.as_deref().unwrap_or("-");

            t.print_row(
                io,
                [
                    alloc::format!("{}", info.id.raw()),
                    alloc::format!("{}", info.id),
                    size,
                    alloc::string::String::from(mode),
                    alloc::string::String::from(status.short()),
                    alloc::string::String::from(label),
                ],
            );
        }
    }

    if !found {
        io.write_str("(no suitable disks found)\r\n");
    }

    io.write_str("\r\n");
}


pub(crate) async fn print_install_disk_table(io: &dyn ShellIo) {
    print_generic_disk_table(io, "install", false).await;
}

pub(crate) async fn print_update_disk_table(io: &dyn ShellIo) {
    print_generic_disk_table(io, "update", false).await;
}

pub(crate) async fn print_format_disk_table(io: &dyn ShellIo) {
    print_generic_disk_table(io, "format", false).await;
}

pub(crate) async fn print_bench_disk_table(io: &dyn ShellIo) {
    print_generic_disk_table(io, "bench", true).await;
}

pub(crate) async fn print_netbench_nic_table(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
    write_inserted_line(io, format_args!("netbench: NIC selection"));
    write_inserted_line(
        io,
        format_args!("netbench: enter a nic id (blank/q cancels)"),
    );
    write_inserted_line(io, format_args!(""));

    let count = crate::net::device_count();
    if count == 0 {
        write_inserted_line(io, format_args!("  (no nics)"));
        write_inserted_line(io, format_args!(""));
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
                write_inserted_line(
                    io,
                    format_args!(
                        "  id={} mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                        idx, a, b, c, d, e, f
                    ),
                );
            }
            None => {
                write_inserted_line(io, format_args!("  id={} mac=unavailable", idx));
            }
        }
    }

    write_inserted_line(io, format_args!(""));
}

pub(crate) async fn print_trueosfs_mount_table(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
    write_inserted_line(io, format_args!(""));
    write_inserted_line(io, format_args!("file: TRUEOSFS mounts"));

    for disk in crate::disc::block::device_handles()
        .into_iter()
        .filter(|h| h.parent().is_none())
    {
        // HARDENING: Ignore disks that are unresponsive or not TRUEOSFS.
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
        if err.is_some() {
            continue;
        }
        if let crate::v::disc::detect::DiscStatus::Trueos { .. } = status {
            let _ = crate::v::fs::trueosfs::mount_root_async(disk).await;
        }
    }

    let roots = crate::v::fs::trueosfs::list_roots();
    if roots.is_empty() {
        write_inserted_line(io, format_args!("file: (none)"));
        return;
    }
    for (idx, r) in roots.iter().enumerate() {
        write_inserted_line(
            io,
            format_args!(
                "file: {:>2}: {} (raw={} seq={})",
                idx,
                r.disk_id,
                r.disk_id.raw(),
                r.seq
            ),
        );
    }
}

pub(crate) async fn print_trueosfs_tree_25(
    io: &dyn ShellIo,
    disk: crate::disc::block::DeviceHandle,
) {
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

    struct IoWriter<'a> {
        io: &'a dyn ShellIo,
        buf: AString,
    }

    impl<'a> Write for IoWriter<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for ch in s.chars() {
                if ch == '\n' {
                    write_inserted_line(self.io, format_args!("{}", self.buf));
                    self.buf.clear();
                } else if ch != '\r' {
                    let _ = self.buf.push(ch);
                }
            }
            Ok(())
        }
    }

    impl<'a> Drop for IoWriter<'a> {
        fn drop(&mut self) {
            if !self.buf.is_empty() {
                write_inserted_line(self.io, format_args!("{}", self.buf));
            }
        }
    }

    const MAX_PRINT: usize = 25;
    const CAP: usize = 128;

    // Reset position before output
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));

    let mut tree: Tree<FsEntry, CAP> = Tree::new();
    let Some(root) = tree.add_root(FsEntry {
        kind: FsKind::Root,
        name: AString::from("/"),
    }) else {
        write_inserted_line(io, format_args!("file: tree alloc failed"));
        return;
    };

    let mut queue: Vec<(NodeId, AString)> = Vec::new();
    queue.push((root, AString::new()));

    while let Some((parent, path)) = queue.pop() {
        if tree.len() >= MAX_PRINT {
            break;
        }

        let listing = match crate::v::fs::trueosfs::list_dir_async(disk, path.as_str()).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                write_inserted_line(io, format_args!("file: not a TRUEOSFS disk"));
                break;
            }
            Err(e) => {
                write_inserted_line(io, format_args!("file: list_dir failed ({:?})", e));
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

            let is_file =
                match crate::v::fs::trueosfs::file_exists_async(disk, child_path.as_str()).await {
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
                queue.insert(0, (node, child_path));
            }
        }
    }

    let mut w = IoWriter {
        io,
        buf: AString::new(),
    };
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

#[inline]
pub(crate) fn write_prompt_for_state(io: &dyn ShellIo, mode: &ShellMode) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));

    match mode {
        ShellMode::Wizard(_) => {
            io.write_fmt(format_args!("[{}] ", crate::ecma48::dim("id")));
        }
        ShellMode::Confirm(action) => {
            let hint = match action {
                PendingAction::FormatConfirm { .. }
                | PendingAction::InstallConfirm { .. }
                | PendingAction::UpdateConfirm { .. } => "confirm",
                _ => "wait",
            };
            io.write_fmt(format_args!("[{}] ", crate::ecma48::dim(hint)));
        }
        _ => {}
    }
}
