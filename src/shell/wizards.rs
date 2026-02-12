use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};

use crate::shell::{ShellIo, PROMPT_RGB};
use crate::ecma48;

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

pub(crate) fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}

pub(crate) async fn print_generic_disk_table(io: &dyn ShellIo, header: &str, filter_trueos: bool) {
    let title = crate::ecma48::style(header).bold().fg(crate::shell::PROMPT_RGB);
    io.write_fmt(format_args!("{} {}\r\n", title, crate::ecma48::dim("disk detection stage")));
    io.write_fmt(format_args!("{}\r\n\r\n", crate::ecma48::dim("choose a disk id to continue (blank/q cancels)")));

    let mut found = false;

    // Header
    io.write_fmt(format_args!(
        "  {:<3} {:<8} {:<10} {:<6} {:<12} {}\r\n",
        "ID", "Name", "Size", "Mode", "Status", "Label"
    ));
    io.write_fmt(format_args!("  {}\r\n", crate::ecma48::dim("---------------------------------------------------------")));

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(h).await;
        
        if filter_trueos && !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
            continue;
        }
        found = true;

        let total = info.block_count.saturating_mul(info.block_size as u64);
        let (size_val, size_suffix) = if total >= 1024 * 1024 * 1024 {
            (total / (1024 * 1024 * 1024), "GB")
        } else if total >= 1024 * 1024 {
            (total / (1024 * 1024), "MB")
        } else {
            (total / 1024, "KB")
        };
        
        let status_color = match status {
            crate::v::disc::detect::DiscStatus::Trueos { .. } => (100, 255, 100),
            crate::v::disc::detect::DiscStatus::Unknown => (255, 100, 100),
            _ => (255, 200, 100),
        };

        io.write_fmt(format_args!(
            "  {:<3} {:<8} {:>4} {:<5} {:<6} {:<12} {}\r\n",
            crate::ecma48::bold(&alloc::format!("{}", info.id.raw())),
            info.id,
            size_val, size_suffix,
            if info.writable { "RW" } else { "RO" },
            crate::ecma48::color(status.short(), status_color),
            crate::ecma48::dim(info.label.unwrap_or("-"))
        ));

        if let Some(e) = err {
             io.write_fmt(format_args!(
                 "       {}\r\n", 
                 crate::ecma48::color(&alloc::format!("Error: {:?}", e), (255, 80, 80))
             ));
        }
    }

    if !found {
         io.write_str("  (no suitable disks found)\r\n");
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

pub(crate) async fn print_trueosfs_mount_table(io: &dyn ShellIo) {
    io.write_str("\r\nfile: TRUEOSFS mounts\r\n");

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

pub(crate) async fn print_trueosfs_tree_25(io: &dyn ShellIo, disk: crate::disc::block::DeviceHandle) {
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


#[inline]
pub(crate) fn write_prompt_for_state(
    io: &dyn ShellIo,
    pending_action: Option<PendingAction>,
    install_wizard: Option<InstallWizardStage>,
) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));

    if install_wizard.is_some() {
        io.write_fmt(format_args!("[{}] ", crate::ecma48::dim("id")));
    } else if let Some(action) = pending_action {
        let hint = match action {
            PendingAction::FormatConfirm { .. } |
            PendingAction::InstallConfirm { .. } |
            PendingAction::UpdateConfirm { .. } => "confirm",
            _ => "wait",
        };
        // Wait, format_args! macro needs a format string. The original code used io.write_fmt with extra brackets.
        // Let's copy it carefully.
        // io.write_fmt(format_args!("[{}] ", crate::ecma48::dim(hint)));
        // This looks correct.
        
        io.write_fmt(format_args!("[{}] ", crate::ecma48::dim(hint)));
    }
}

