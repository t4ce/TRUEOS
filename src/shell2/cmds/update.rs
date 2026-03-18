use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

pub(crate) fn submit_update(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "update: no TRUEOSFS root mounted");
        return;
    };

    let target = matrix_target_for_backend(io);
    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!(
            "update: starting on mounted root disk id={} ({})",
            info.id.raw(),
            info.id
        )
        .as_str(),
    );

    set_matrix_target_active(&target, true);
    if spawner.spawn(update_command_task(target.clone(), disk)).is_err() {
        set_matrix_target_active(&target, false);
        print_shell_line(io, "update: spawn failed");
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn update_command_task(target: MatrixTarget, disk: crate::disc::block::DeviceHandle) {
    let task_target = target.clone();
    async move {
        const ISO_URL: &str = "https://trueos.eu/TrueOS.7z";

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        let info = disk.info();
        log("update: waiting for net");
        crate::v::readiness::wait_for(crate::v::readiness::NET_CONFIGURED).await;

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

        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
        log(alloc::format!(
            "update: target status={}{}",
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        )
        .as_str());
        if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
            log("update: install before update");
            return;
        }

        log(alloc::format!("update: download {}", ISO_URL).as_str());

        let payload = match crate::v::net::https::fetch_https_body_progress_async(
            ISO_URL,
            120_000,
            128 * 1024 * 1024,
            &mut NoopProgress,
        )
        .await
        {
            Ok(b) => b,
            Err(e) => {
                log(alloc::format!("update: download failed ({:?})", e).as_str());
                return;
            }
        };

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

        let iso = match crate::z7::extract_single_file_to_vec(payload.as_slice()) {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("update: extract failed ({:?})", e).as_str());
                return;
            }
        };
        drop(payload);
        let iso_view = iso.as_slice();

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
            Err(e) => {
                log(alloc::format!("update: ISO missing BOOTX64.EFI ({:?})", e).as_str());
                return;
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

        log("update: installing onto mounted TRUEOSFS root disk");
        match crate::disc::install::install_bootable_uefi_gpt_with_log(
            disk,
            bootx64,
            kernel,
            &mut |line| log(line),
        )
        .await
        {
            Ok(()) => log("update: ok"),
            Err(e) => log(alloc::format!("update: failed ({:?})", e).as_str()),
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}

struct NoopProgress;

impl crate::v::net::https::FetchProgress for NoopProgress {
    fn on_progress(&mut self, _received: usize, _total: Option<usize>) {}
}
