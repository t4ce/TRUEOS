use crate::disc::{block, install as disc_install, layout};
use crate::shell::ShellIo;

pub(crate) enum PendingInstall {
    Install { raw_id: u32, mode: InstallMode },
    Format { raw_id: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallMode {
    /// Auto-detect based on disk layout.
    Auto,
    /// Force migrate-in-place (superfloppy -> partitioned).
    Migrate,
    /// Force fresh install (will erase/repartition).
    Fresh,
}

pub(crate) fn handle_install_command(io: &dyn ShellIo, args: &str) -> Option<PendingInstall> {
    let args = args.trim();

    if args.is_empty() {
        io.write_str("install: BIOS/MBR install (DESTRUCTIVE)\r\n");
        io.write_str("usage: install <disc_id> auto         (auto-detect)\r\n");
        io.write_str("       install <disc_id> migrate      (force migrate)\r\n");
        io.write_str("       install <disc_id> fresh        (force fresh)\r\n");
        io.write_str("       install <disc_id> format       (format FAT superfloppy @ LBA0)\r\n");
        io.write_str("example: install 1 auto\r\n");
        io.write_str("example: install 1 migrate\r\n");
        io.write_str("example: install 1 fresh\r\n");
        io.write_str("example: install 1 format\r\n");
        io.write_str("available disks:\r\n");
        for h in block::device_handles().into_iter() {
            let info = h.info();
            io.write_fmt(format_args!(
                "  id={} ({}) kind={:?} blocks={} bs={} writable={} label={:?}\r\n",
                info.id.raw(),
                info.id,
                info.kind,
                info.block_count,
                info.block_size,
                info.writable,
                info.label
            ));
        }
        return None;
    }

    let mut parts = args.split_whitespace();
    let id_str = match parts.next() {
        Some(s) => s,
        None => {
            io.write_str("install: missing args\r\n");
            return None;
        }
    };
    let mode_str = match parts.next() {
        Some(s) => s,
        None => {
            io.write_str("install: missing mode (auto|migrate|fresh|format)\r\n");
            return None;
        }
    };

    // Supported forms:
    //   install <id> auto|migrate|fresh|format
    // (Intentionally does NOT support: install <mode> <id>)
    enum Op {
        Install(InstallMode),
        Format,
    }

    let op = if mode_str.eq_ignore_ascii_case("auto") {
        Op::Install(InstallMode::Auto)
    } else if mode_str.eq_ignore_ascii_case("migrate") {
        Op::Install(InstallMode::Migrate)
    } else if mode_str.eq_ignore_ascii_case("fresh") {
        Op::Install(InstallMode::Fresh)
    } else if mode_str.eq_ignore_ascii_case("format") {
        Op::Format
    } else {
        io.write_str("install: invalid mode (auto|migrate|fresh|format)\r\n");
        return None;
    };

    if parts.next().is_some() {
        io.write_str("install: too many args\r\n");
        return None;
    }

    let raw_id = match parse_disc_id_raw(id_str) {
        Some(v) => v,
        None => {
            io.write_str("install: invalid id (use decimal like '1' or 'disc001')\r\n");
            return None;
        }
    };

    let target = block::device_handles().into_iter().find(|h| h.id().raw() == raw_id);
    let Some(handle) = target else {
        io.write_str("install: no such device\r\n");
        return None;
    };

    let info = handle.info();
    io.write_fmt(format_args!(
        "install: target id={} ({}) label={:?} blocks={} bs={}\r\n",
        info.id.raw(),
        info.id,
        info.label,
        info.block_count,
        info.block_size
    ));
    match op {
        Op::Format => {
            io.write_str("install: format will create an empty FAT superfloppy (FAT @ LBA0).\r\n");
            io.write_str("install: this does NOT zero the whole disk, but it WILL destroy existing filesystem contents.\r\n");
        }
        Op::Install(mode) => {
            if matches!(mode, InstallMode::Migrate) {
                io.write_str(
                    "install: migrate shifts a FAT superfloppy (FAT-at-LBA0) forward by 1MiB into a partition.\r\n",
                );
                io.write_str(
                    "install: it validates the last 1MiB is free; otherwise it aborts to avoid data loss.\r\n",
                );
                io.write_str("install: still destructive; always back up.\r\n");
            } else if matches!(mode, InstallMode::Fresh) {
                io.write_str("install: fresh will ERASE the disk.\r\n");
            } else {
                io.write_str("install: auto will detect superfloppy vs MBR and choose safely.\r\n");
            }
        }
    }
    io.write_str("install: press Enter to proceed, Space to abort\r\n");

    Some(match op {
        Op::Format => PendingInstall::Format { raw_id: info.id.raw() },
        Op::Install(mode) => PendingInstall::Install {
            raw_id: info.id.raw(),
            mode,
        },
    })
}

pub(crate) fn run_install(io: &dyn ShellIo, raw_id: u32, mode: InstallMode) {
    let target = block::device_handles().into_iter().find(|h| h.id().raw() == raw_id);
    let Some(handle) = target else {
        io.write_str("install: no such device\r\n");
        return;
    };

    let info = handle.info();
    io.write_fmt(format_args!(
        "install: installing Limine BIOS + TRUEOS to id={} ({})...\r\n",
        info.id.raw(),
        info.id
    ));

    const SPINNER: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    let mut spinner_idx: usize = 0;
    let mut tick = || {
        let ch = SPINNER[spinner_idx];
        spinner_idx = (spinner_idx + 1) % SPINNER.len();
        io.write_str("\rinstall: working ");
        io.write_char(ch);
    };

    let mut status = |args: core::fmt::Arguments<'_>| {
        io.write_str("\r\n");
        io.write_fmt(args);
        io.write_str("\r\n");
    };

    let chosen = match mode {
        InstallMode::Fresh => InstallMode::Fresh,
        InstallMode::Migrate => InstallMode::Migrate,
        InstallMode::Auto => {
            match layout::probe_fat_volume(handle) {
                Ok(layout::FatVolumeLayout::FatAtLba0 { whole_disk: true, .. }) => InstallMode::Migrate,
                Ok(layout::FatVolumeLayout::FatAtLba0 { whole_disk: false, .. }) => {
                    io.write_str("install: auto: FAT-at-LBA0 but BPB size != disk size; refusing (not a whole-disk superfloppy)\r\n");
                    return;
                }
                Ok(layout::FatVolumeLayout::MbrPartition { .. }) => InstallMode::Fresh,
                Err(layout::ProbeError::UnsupportedBlockSize(bs)) => {
                    io.write_fmt(format_args!("install: auto: unsupported block size {}\r\n", bs));
                    return;
                }
                Err(layout::ProbeError::DeviceIo(e)) => {
                    io.write_fmt(format_args!("install: auto: probe I/O failed: {:?}\r\n", e));
                    return;
                }
                Err(layout::ProbeError::UnknownLayout) => {
                    io.write_str("install: auto: unknown layout (expected FAT@LBA0 or MBR+FAT); refusing\r\n");
                    return;
                }
            }
        }
    };

    let res = match chosen {
        InstallMode::Migrate => disc_install::install_bios_mbr_migrate_superfloppy_with_progress_and_status(
            handle,
            &mut tick,
            &mut status,
        ),
        InstallMode::Fresh | InstallMode::Auto => disc_install::install_bios_mbr_with_progress_and_status(
            handle,
            &mut tick,
            &mut status,
        ),
    };

    io.write_str("\r\n");

    match res {
        Ok(()) => io.write_str("install: ok\r\n"),
        Err(e) => io.write_fmt(format_args!("install: failed: {:?}\r\n", e)),
    }
}

pub(crate) fn run_format_superfloppy(io: &dyn ShellIo, raw_id: u32) {
    let target = block::device_handles().into_iter().find(|h| h.id().raw() == raw_id);
    let Some(handle) = target else {
        io.write_str("install: no such device\r\n");
        return;
    };

    let info = handle.info();
    io.write_fmt(format_args!(
        "install: formatting FAT superfloppy on id={} ({})...\r\n",
        info.id.raw(),
        info.id
    ));

    const SPINNER: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    let mut spinner_idx: usize = 0;
    let mut tick = || {
        let ch = SPINNER[spinner_idx];
        spinner_idx = (spinner_idx + 1) % SPINNER.len();
        io.write_str("\rinstall: working ");
        io.write_char(ch);
    };

    let mut status = |args: core::fmt::Arguments<'_>| {
        io.write_str("\r\n");
        io.write_fmt(args);
        io.write_str("\r\n");
    };

    let res = disc_install::format_superfloppy_fat_with_progress_and_status(handle, &mut tick, &mut status);

    io.write_str("\r\n");
    match res {
        Ok(()) => io.write_str("install: format ok\r\n"),
        Err(e) => io.write_fmt(format_args!("install: format failed: {:?}\r\n", e)),
    }
}

fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}
