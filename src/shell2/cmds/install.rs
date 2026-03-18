use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

pub(crate) fn submit_install(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "install: no TRUEOSFS root mounted");
        return;
    };
    let Some(bootx64) = crate::limine::install_bootx64_bytes() else {
        print_shell_line(io, "install: missing boot payload module `trueos.install.bootx64`");
        return;
    };
    let Some(kernel) = crate::limine::install_kernel_bytes() else {
        print_shell_line(io, "install: missing install kernel payload");
        return;
    };

    let target = matrix_target_for_backend(io);
    let info = disk.info();
    print_matrix_target_line(
        &target,
        alloc::format!(
            "install: starting on mounted root disk id={} ({})",
            info.id.raw(),
            info.id
        )
        .as_str(),
    );

    set_matrix_target_active(&target, true);
    if spawner
        .spawn(install_command_task(target.clone(), disk, bootx64, kernel))
        .is_err()
    {
        set_matrix_target_active(&target, false);
        print_shell_line(io, "install: spawn failed");
    }
}

#[embassy_executor::task(pool_size = 2)]
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

        let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
        log(alloc::format!(
            "install: target status={}{}",
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        )
        .as_str());

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

        log("install: installing current local payload onto mounted TRUEOSFS root disk");
        match crate::disc::install::install_bootable_uefi_gpt_with_log(
            disk,
            bootx64,
            kernel,
            &mut |line| log(line),
        )
        .await
        {
            Ok(()) => log("install: ok"),
            Err(e) => log(alloc::format!("install: failed ({:?})", e).as_str()),
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}
