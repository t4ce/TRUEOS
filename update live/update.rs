use core::str::SplitWhitespace;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, matrix_target_interrupted,
    print_matrix_target_line, print_shell_line, set_matrix_target_active,
};

pub(crate) fn print_update_disk_table(io: &'static dyn ShellBackend2) {
    let choices = super::tlb_helper::collect_top_level_disk_choices();
    super::tlb_helper::print_disk_choice_table(io, "update", "disk selection", choices.as_slice());
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(arg) = args.next() else {
        print_update_disk_table(io);
        print_shell_line(
            io,
            "update: run `update <disk-id>` for a persistent install or `update live` for a RAM-only generation swap",
        );
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        print_shell_line(io, "update: usage `update <disk-id>|live`");
        return ParseOutcome::Handled;
    }

    if arg == "live" {
        submit_live_update(spawner, io);
        return ParseOutcome::Handled;
    }

    let Some(raw_id) = super::tlb_helper::parse_disc_id_raw(arg) else {
        print_shell_line(io, "update: invalid disk id (or use `update live`)");
        print_update_disk_table(io);
        return ParseOutcome::Handled;
    };
    let Some(disk) = super::tlb_helper::select_top_level_disk(raw_id) else {
        print_shell_line(io, "update: no such top-level disk");
        print_update_disk_table(io);
        return ParseOutcome::Handled;
    };

    submit_update(spawner, io, disk);
    ParseOutcome::Handled
}

pub(crate) fn submit_update(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    disk: crate::disc::block::DeviceHandle,
) {
    let target = matrix_target_for_backend(io);
    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!("update: starting on disk id={} ({})", info.id.raw(), info.id).as_str(),
    );

    set_matrix_target_active(&target, true);
    match update_command_task(target.clone(), disk) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "update: spawn failed");
        }
    }
}

pub(crate) fn submit_live_update(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    let target = matrix_target_for_backend(io);
    print_matrix_target_line(
        &target,
        "update live: starting RAM-only generation replacement; no kernel disk install will run",
    );

    set_matrix_target_active(&target, true);
    match live_update_command_task(target.clone(), *spawner) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "update live: spawn failed");
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn update_command_task(target: MatrixTarget, disk: crate::disc::block::DeviceHandle) {
    let task_target = target.clone();
    async move {
        const ISO_URL: &str = "http://trueos.eu/TrueOS.7z";

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        let info = disk.info();
        log("update: waiting for net");
        crate::r::readiness::wait_for(
            crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        )
        .await;
        if interrupted() {
            log("update: interrupted before download");
            return;
        }

        log(alloc::format!(
            "update: target id={} ({}) blocks={} bs={} writable={} label={:?}",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
        )
        .as_str());
        if interrupted() {
            log("update: interrupted before disk probe");
            return;
        }

        let (status, err) = crate::r::disc::detect::detect_physical_disk_detail(disk).await;
        log(alloc::format!(
            "update: target status={}{}",
            status.short(),
            match (&status, err) {
                (crate::r::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        )
        .as_str());
        if !matches!(status, crate::r::disc::detect::DiscStatus::Trueos { .. }) {
            log("update: install before update");
            return;
        }

        log(alloc::format!("update: download {}", ISO_URL).as_str());
        if interrupted() {
            log("update: interrupted before download");
            return;
        }

        let payload = match crate::surfer::html_shack::fetch_bytes_via_pool(
            ISO_URL,
            120_000,
            128 * 1024 * 1024,
        )
        .await
        {
            Ok(fetch) => fetch.bytes,
            Err(e) => {
                log(alloc::format!("update: download failed ({})", e).as_str());
                return;
            }
        };
        if interrupted() {
            log("update: interrupted after download");
            return;
        }

        log(alloc::format!(
            "update: downloaded payload={} bytes (7z_magic={})",
            payload.len(),
            crate::z7::looks_like_7z(payload.as_slice())
        )
        .as_str());

        if !crate::z7::looks_like_7z(payload.as_slice()) {
            log("update: refused (payload is not a 7z archive)");
            return;
        }

        let iso = match crate::z7::extract_file_to_vec(payload.as_slice(), "trueos.iso") {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("update: extract failed ({:?})", e).as_str());
                return;
            }
        };
        drop(payload);
        let iso_view = iso.as_slice();
        if interrupted() {
            log("update: interrupted before install");
            return;
        }

        log(alloc::format!(
            "update: extracted trueos.iso bytes={} (iso9660_magic={})",
            iso_view.len(),
            crate::iso9660::looks_like_iso9660(iso_view)
        )
        .as_str());

        if !crate::iso9660::looks_like_iso9660(iso_view) {
            log("update: refused (extracted data is not an ISO9660 image)");
            return;
        }

        let bootx64 = match crate::iso9660::file_slice(iso_view, "/EFI/BOOT/BOOTX64.EFI") {
            Ok(v) => v,
            Err(_) => {
                let efi_img = match crate::iso9660::file_slice(iso_view, "/efi.img") {
                    Ok(v) => v,
                    Err(e) => {
                        log(alloc::format!("update: ISO missing efi.img ({:?})", e).as_str());
                        return;
                    }
                };
                match crate::efi_img::bootx64_from_efi_img(efi_img) {
                    Some(v) => v,
                    None => {
                        log("update: efi.img missing EFI/BOOT/BOOTX64.EFI");
                        return;
                    }
                }
            }
        };

        let kernel = match crate::iso9660::file_slice(iso_view, "/TRUEOS.elf") {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("update: ISO missing TRUEOS.elf ({:?})", e).as_str());
                return;
            }
        };

        let bootx64_ok = bootx64.get(0..2) == Some(b"MZ");
        let kernel_ok = kernel.get(0..4) == Some(b"\x7FELF");
        log(alloc::format!(
            "update: BOOTX64.EFI={} bytes (mz={}), TRUEOS.elf={} bytes (elf={})",
            bootx64.len(),
            bootx64_ok,
            kernel.len(),
            kernel_ok
        )
        .as_str());
        if !bootx64_ok || !kernel_ok {
            log("update: refusing to install (payload format looks wrong)");
            return;
        }

        log("update: installing onto selected TRUEOS disk");
        match crate::disc::install::install_bootable_uefi_gpt_with_log(
            disk,
            bootx64,
            kernel,
            &mut |line| log(line),
        )
        .await
        {
            Ok(()) => match crate::r::fs::trueosfs::remount_root_async(disk).await {
                Ok(Some(_)) => log("update: ok"),
                Ok(None) => log("update: failed to remount TRUEOSFS"),
                Err(e) => log(alloc::format!("update: remount failed ({:?})", e).as_str()),
            },
            Err(e) => log(alloc::format!("update: failed ({:?})", e).as_str()),
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}

#[embassy_executor::task]
async fn live_update_command_task(target: MatrixTarget, spawner: Spawner) {
    let task_target = target.clone();
    async move {
        const LIVE_ISO_URL: &str = "https://trueos.eu/TrueOS.7z";

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        log("update live: waiting for net and TRUEOSFS checkpoint storage");
        crate::r::readiness::wait_for(
            crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        )
        .await;
        if interrupted() {
            log("update live: interrupted before download");
            return;
        }

        log(alloc::format!("update live: download {}", LIVE_ISO_URL).as_str());
        let payload = match crate::surfer::html_shack::fetch_bytes_via_pool(
            LIVE_ISO_URL,
            120_000,
            128 * 1024 * 1024,
        )
        .await
        {
            Ok(fetch) => fetch.bytes,
            Err(error) => {
                log(alloc::format!("update live: download failed ({})", error).as_str());
                return;
            }
        };
        if interrupted() {
            log("update live: interrupted after download");
            return;
        }
        if !crate::z7::looks_like_7z(payload.as_slice()) {
            log("update live: refused (payload is not a 7z archive)");
            return;
        }
        log(alloc::format!(
            "update live: downloaded payload={} bytes (7z_magic=true)",
            payload.len(),
        )
        .as_str());

        let iso = match crate::z7::extract_file_to_vec(payload.as_slice(), "trueos.iso") {
            Ok(iso) => iso,
            Err(error) => {
                log(alloc::format!("update live: extract failed ({:?})", error).as_str());
                return;
            }
        };
        drop(payload);
        if !crate::iso9660::looks_like_iso9660(iso.as_slice()) {
            log("update live: refused (extracted data is not an ISO9660 image)");
            return;
        }
        if interrupted() {
            log("update live: interrupted before candidate extraction");
            return;
        }

        let kernel = match crate::iso9660::file_slice(iso.as_slice(), "/TRUEOS.elf") {
            Ok(kernel) if kernel.get(0..4) == Some(b"\x7FELF") => kernel.to_vec(),
            Ok(_) => {
                log("update live: refused (TRUEOS.elf has no ELF magic)");
                return;
            }
            Err(error) => {
                log(alloc::format!("update live: ISO missing TRUEOS.elf ({:?})", error).as_str());
                return;
            }
        };
        log(alloc::format!(
            "update live: candidate TRUEOS.elf={} bytes; disk install path skipped",
            kernel.len(),
        )
        .as_str());
        drop(iso);

        match crate::live_update::stage_and_swap(kernel, spawner, task_target.clone()).await {
            Ok(never) => match never {},
            Err(error) => {
                log(alloc::format!("update live: failed ({})", error).as_str());
            }
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}
