use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::{
    MatrixTarget, matrix_target_interrupted, print_matrix_target_line, set_matrix_target_active,
};

pub(crate) fn submit_online_install_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
) {
    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!("install online: starting on disk id={} ({})", info.id.raw(), info.id)
            .as_str(),
    );

    set_matrix_target_active(&target, true);
    match online_install_command_task(target.clone(), disk) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(&target, "install online: spawn failed");
        }
    }
}

pub(crate) fn submit_live_update_to_target(spawner: &Spawner, target: MatrixTarget) {
    print_matrix_target_line(
        &target,
        "update live: step=01/20 command-accepted mode=RAM-only disk-install=disabled",
    );

    set_matrix_target_active(&target, true);
    match live_update_command_task(target.clone(), *spawner) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(&target, "update live: spawn failed");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn online_install_command_task(target: MatrixTarget, disk: crate::disc::block::DeviceHandle) {
    let task_target = target.clone();
    async move {
        const ISO_URL: &str = "https://trueos.eu/TrueOS.7z";

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        let info = disk.info();
        log("install online: waiting for net");
        crate::r::readiness::wait_for(
            crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        )
        .await;
        if interrupted() {
            log("install online: interrupted before download");
            return;
        }

        log(alloc::format!(
            "install online: target id={} ({}) blocks={} bs={} writable={} label={:?}",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
        )
        .as_str());
        if interrupted() {
            log("install online: interrupted before disk probe");
            return;
        }

        let (status, err) = crate::r::disc::detect::detect_physical_disk_detail(disk).await;
        log(alloc::format!(
            "install online: target status={}{}",
            status.short(),
            match (&status, err) {
                (crate::r::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        )
        .as_str());
        log(alloc::format!("install online: download {}", ISO_URL).as_str());
        if interrupted() {
            log("install online: interrupted before download");
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
                log(alloc::format!("install online: download failed ({})", e).as_str());
                return;
            }
        };
        if interrupted() {
            log("install online: interrupted after download");
            return;
        }

        log(alloc::format!(
            "install online: downloaded payload={} bytes (7z_magic={})",
            payload.len(),
            crate::z7::looks_like_7z(payload.as_slice())
        )
        .as_str());

        if !crate::z7::looks_like_7z(payload.as_slice()) {
            log("install online: refused (payload is not a 7z archive)");
            return;
        }

        let iso = match crate::z7::extract_file_to_vec(payload.as_slice(), "trueos.iso") {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("install online: extract failed ({:?})", e).as_str());
                return;
            }
        };
        drop(payload);
        let iso_view = iso.as_slice();
        if interrupted() {
            log("install online: interrupted before install");
            return;
        }

        log(alloc::format!(
            "install online: extracted trueos.iso bytes={} (iso9660_magic={})",
            iso_view.len(),
            crate::iso9660::looks_like_iso9660(iso_view)
        )
        .as_str());

        if !crate::iso9660::looks_like_iso9660(iso_view) {
            log("install online: refused (extracted data is not an ISO9660 image)");
            return;
        }

        let bootx64 = match crate::iso9660::file_slice(iso_view, "/EFI/BOOT/BOOTX64.EFI") {
            Ok(v) => v,
            Err(_) => {
                let efi_img = match crate::iso9660::file_slice(iso_view, "/efi.img") {
                    Ok(v) => v,
                    Err(e) => {
                        log(alloc::format!("install online: ISO missing efi.img ({:?})", e)
                            .as_str());
                        return;
                    }
                };
                match crate::efi_img::bootx64_from_efi_img(efi_img) {
                    Some(v) => v,
                    None => {
                        log("install online: efi.img missing EFI/BOOT/BOOTX64.EFI");
                        return;
                    }
                }
            }
        };

        let kernel = match crate::iso9660::file_slice(iso_view, "/TRUEOS.elf") {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("install online: ISO missing TRUEOS.elf ({:?})", e).as_str());
                return;
            }
        };

        let bootx64_ok = bootx64.get(0..2) == Some(b"MZ");
        let kernel_ok = kernel.get(0..4) == Some(b"\x7FELF");
        log(alloc::format!(
            "install online: BOOTX64.EFI={} bytes (mz={}), TRUEOS.elf={} bytes (elf={})",
            bootx64.len(),
            bootx64_ok,
            kernel.len(),
            kernel_ok
        )
        .as_str());
        if !bootx64_ok || !kernel_ok {
            log("install online: refusing to install (payload format looks wrong)");
            return;
        }

        log("install online: installing onto selected disk");
        match crate::disc::install::install_bootable_uefi_gpt_with_log(
            disk,
            bootx64,
            kernel,
            &mut |line| log(line),
        )
        .await
        {
            Ok(()) => match crate::r::fs::trueosfs::remount_root_async(disk).await {
                Ok(Some(_)) => log("install online: ok"),
                Ok(None) => log("install online: failed to remount TRUEOSFS"),
                Err(e) => log(alloc::format!("install online: remount failed ({:?})", e).as_str()),
            },
            Err(e) => log(alloc::format!("install online: failed ({:?})", e).as_str()),
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}

#[trueos_executor::task]
async fn live_update_command_task(target: MatrixTarget, spawner: Spawner) {
    let task_target = target.clone();
    async move {
        const LIVE_ISO_URL: &str = "https://trueos.eu/TrueOS.7z";

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        log("update live: step=02a/20 waiting for IPv4 network");
        crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
        if interrupted() {
            log("update live: interrupted before download");
            return;
        }
        log("update live: step=02b/20 readiness-satisfied net=v4; checkpoint storage is conditional");

        log(alloc::format!("update live: step=03/20 download-begin {}", LIVE_ISO_URL).as_str());
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
            "update live: step=04a/20 download-complete bytes={} archive=7z transport=https-rustls",
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
        log("update live: step=04b/20 ISO9660-verified");
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
            "update live: step=05/20 candidate-ELF-extracted bytes={} disk-install=skipped",
            kernel.len(),
        )
        .as_str());
        drop(iso);

        match crate::live_update::stage_and_swap(
            kernel,
            spawner,
            task_target.clone(),
            crate::live_update::NonReplicatableVmPolicy::DiscardAtCommit,
        )
        .await
        {
            Ok(never) => match never {},
            Err(error) => {
                log(alloc::format!("update live: failed ({})", error).as_str());
            }
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}
