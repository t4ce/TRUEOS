//! OS menu installation controllers for local and downloaded payloads.
use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::shell3::{MatrixTarget, matrix_target_print_line as print_matrix_target_line};
use crate::shell3::matrix_target_interrupted;

pub(crate) fn submit_install_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
) {
    submit_install_mode_to_target(
        spawner,
        target,
        disk,
        crate::disc::install::pxeproc::InstallMode::Regular,
    );
}

pub(crate) fn submit_install_mode_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
    mode: crate::disc::install::pxeproc::InstallMode,
) {
    let Some(bootx64) = crate::limine::install_bootx64_bytes() else {
        print_matrix_target_line(
            &target,
            "install: missing boot payload module `trueos.install.bootx64`",
        );
        return;
    };
    let Some(kernel) = crate::limine::install_kernel_bytes() else {
        print_matrix_target_line(&target, "install: missing install kernel payload");
        return;
    };

    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!("install: starting on disk id={} ({})", info.id.raw(), info.id).as_str(),
    );

    match install_command_task(target.clone(), disk, bootx64, kernel, mode) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {

            print_matrix_target_line(&target, "install: spawn failed");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn install_command_task(
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
    bootx64: &'static [u8],
    kernel: &'static [u8],
    mode: crate::disc::install::pxeproc::InstallMode,
) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        let info = disk.info();
        log(alloc::format!(
            "install: target id={} ({}) blocks={} bs={} writable={} label={:?}",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
        )
        .as_str());
        if interrupted() {
            log("install: interrupted before disk probe");
            return;
        }

        let (status, err) = crate::r::disc::detect::detect_physical_disk_detail(disk).await;
        log(alloc::format!(
            "install: target status={}{}",
            status.short(),
            match (&status, err) {
                (crate::r::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        )
        .as_str());
        if interrupted() {
            log("install: interrupted before install");
            return;
        }

        let bootx64_ok = bootx64.get(0..2) == Some(b"MZ");
        let kernel_ok = kernel.get(0..4) == Some(b"\x7FELF");
        log(alloc::format!(
            "install: payload BOOTX64.EFI={} bytes (mz={}), TRUEOS.elf={} bytes (elf={})",
            bootx64.len(),
            bootx64_ok,
            kernel.len(),
            kernel_ok
        )
        .as_str());
        if !bootx64_ok || !kernel_ok {
            log("install: refusing to install (payload format looks wrong)");
            return;
        }

        log("install: installing current local payload onto selected disk");
        log(alloc::format!("install: boot mode={mode:?}").as_str());
        match crate::disc::install::install_bootable_uefi_gpt_mode_with_log(
            disk,
            bootx64,
            kernel,
            mode,
            &mut |line| log(line),
        )
        .await
        {
            Ok(()) => match crate::r::fs::trueosfs::remount_root_async(disk).await {
                Ok(Some(_)) => log("install: ok"),
                Ok(None) => log("install: failed to remount TRUEOSFS"),
                Err(e) => log(alloc::format!("install: remount failed ({:?})", e).as_str()),
            },
            Err(e) => log(alloc::format!("install: failed ({:?})", e).as_str()),
        }
    }
    .await;

}

pub(crate) fn submit_online_install_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
) {
    submit_online_install_mode_to_target(
        spawner,
        target,
        disk,
        crate::disc::install::pxeproc::InstallMode::Regular,
    );
}

pub(crate) fn submit_online_install_mode_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
    mode: crate::disc::install::pxeproc::InstallMode,
) {
    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!("install online: starting on disk id={} ({})", info.id.raw(), info.id)
            .as_str(),
    );

    match online_install_command_task(target.clone(), disk, mode) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {

            print_matrix_target_line(&target, "install online: spawn failed");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn online_install_command_task(
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
    mode: crate::disc::install::pxeproc::InstallMode,
) {
    let task_target = target.clone();
    async move {
        const ISO_URL: &str = "https://trueos.eu/TrueOS.lz4";

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
            "install online: downloaded payload={} bytes (lz4_magic={})",
            payload.len(),
            payload.starts_with(&crate::r::lz4::MAGIC)
        )
        .as_str());

        if !payload.starts_with(&crate::r::lz4::MAGIC) {
            log("install online: refused (payload is not an LZ4 frame)");
            return;
        }

        let iso = match crate::iso9660::decode_update_bundle(payload).await {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("install online: extract failed ({:?})", e).as_str());
                return;
            }
        };
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
        log(alloc::format!("install online: boot mode={mode:?}").as_str());
        match crate::disc::install::install_bootable_uefi_gpt_mode_with_log(
            disk,
            bootx64,
            kernel,
            mode,
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

}

