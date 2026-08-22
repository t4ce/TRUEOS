use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::{
    MatrixTarget, matrix_target_interrupted, print_matrix_target_line, set_matrix_target_active,
};

pub(crate) fn submit_install_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    disk: crate::disc::block::DeviceHandle,
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

    set_matrix_target_active(&target, true);
    match install_command_task(target.clone(), disk, bootx64, kernel) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
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
        match crate::disc::install::install_bootable_uefi_gpt_with_log(
            disk,
            bootx64,
            kernel,
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
    set_matrix_target_active(&target, false);
}
