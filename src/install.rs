use crate::disc::{block, install as disc_install};
use crate::shell::ShellIo;

pub(crate) struct PendingInstall {
    pub(crate) raw_id: u32,
    pub(crate) migrate: bool,
}

pub(crate) fn handle_install_command(io: &dyn ShellIo, args: &str) -> Option<PendingInstall> {
    let args = args.trim();

    if args.is_empty() {
        io.write_str("install: BIOS/MBR install (DESTRUCTIVE)\r\n");
        io.write_str("usage: install <disc_id>\r\n");
        io.write_str("       install <disc_id> migrate\r\n");
        io.write_str("example: install 1\r\n");
        io.write_str("example: install 1 migrate\r\n");
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
    let first = match parts.next() {
        Some(s) => s,
        None => {
            io.write_str("install: missing args\r\n");
            return None;
        }
    };

    // Supported forms:
    //   install <id>
    //   install <id> migrate
    //   install migrate <id>
    let (mode_migrate, id_str) = if first.eq_ignore_ascii_case("migrate") {
        let id_str = match parts.next() {
            Some(s) => s,
            None => {
                io.write_str("install: missing id\r\n");
                return None;
            }
        };
        (true, id_str)
    } else {
        let second = parts.next();
        match second {
            Some(s2) if s2.eq_ignore_ascii_case("migrate") => (true, first),
            Some(_) => (false, first),
            None => (false, first),
        }
    };

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
    if mode_migrate {
        io.write_str(
            "install: migrate shifts a FAT superfloppy (FAT-at-LBA0) forward by 1MiB into a partition.\r\n",
        );
        io.write_str(
            "install: it validates the last 1MiB is free; otherwise it aborts to avoid data loss.\r\n",
        );
        io.write_str("install: still destructive; always back up.\r\n");
    } else {
        io.write_str("install: this will ERASE the disk.\r\n");
    }
    io.write_str("install: press Enter to proceed, Space to abort\r\n");

    Some(PendingInstall {
        raw_id: info.id.raw(),
        migrate: mode_migrate,
    })
}

pub(crate) fn run_install(io: &dyn ShellIo, raw_id: u32, mode_migrate: bool) {
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

    let res = if mode_migrate {
        disc_install::install_bios_mbr_migrate_superfloppy_with_progress_and_status(
            handle,
            &mut tick,
            &mut status,
        )
    } else {
        disc_install::install_bios_mbr_with_progress(handle, &mut tick)
    };

    io.write_str("\r\n");

    match res {
        Ok(()) => io.write_str("install: ok\r\n"),
        Err(e) => io.write_fmt(format_args!("install: failed: {:?}\r\n", e)),
    }
}

fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}
