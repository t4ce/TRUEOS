//! Host controller for the OS Blueprint; selection stays in the guest.
use alloc::{string::String, vec::Vec};
use trueos_executor::Spawner;
use crate::shell3::{MatrixTarget, matrix_target_print_line as print_matrix_target_line};

pub(super) fn arguments() -> Vec<String> {
    let mut args: Vec<String> = super::table::collect_top_level_disk_choices()
        .iter().map(disk_argument).collect();
    args.extend(non_replicatable_vm_arguments());
    args
}

pub(crate) fn matrix_target_interrupted(target: &MatrixTarget) -> bool {
    !super::matrix_target::matrix_slot_is_live(&super::matrix_target_slot_lease(target))
}

pub(super) fn complete(target: MatrixTarget, reason: String) {
    // ACPI's SMI command port and firmware runtime calls belong on the BSP.
    let worker = if matches!(reason.as_str(), "os:shutdown" | "os:reboot") {
        crate::workers::spawner_for_slot(0)
    } else {
        crate::workers::pick_background_spawner()
    };
    let Some(worker) = worker else {
        print_matrix_target_line(&target, "os: no administration worker available");
        return;
    };
    match action_task(target.clone(), reason) {
        Ok(token) => worker.spawn(token),
        Err(_) => print_matrix_target_line(&target, "os: administration task pool exhausted"),
    }
}

#[trueos_executor::task(pool_size = 4)]
async fn action_task(target: MatrixTarget, reason: String) {
    let spawner = unsafe { Spawner::for_current_executor().await };
    if !matrix_target_interrupted(&target) {
        dispatch_admin_action(&spawner, &target, &reason);
    }
}

fn disk_argument(choice: &super::table::DiskChoice) -> String {
    let clean = |value: String| {
        value
            .chars()
            .map(|ch| {
                if matches!(ch, '|' | '\n' | '\r') {
                    ' '
                } else {
                    ch
                }
            })
            .collect::<String>()
    };
    alloc::format!(
        "disk={}|{}|{}|{}|{}|{}",
        choice.raw_id(),
        clean(alloc::format!("{}", choice.handle.id())),
        clean(choice.size_text()),
        choice.mode_text(),
        clean(choice.status_text()),
        clean(choice.label_text()),
    )
}

fn non_replicatable_vm_arguments() -> Vec<String> {
    let clean = |value: String| {
        value
            .chars()
            .map(|ch| {
                if matches!(ch, '|' | '\n' | '\r') {
                    ' '
                } else {
                    ch
                }
            })
            .collect::<String>()
    };
    let mut arguments = Vec::new();
    for vm_index in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let Ok(vm_id) = u8::try_from(vm_index) else {
            continue;
        };
        let state = crate::hv::vm_state(vm_id);
        if state.supported && (state.running || state.starting) && !state.replicatable {
            let label = crate::hv::app_vm_display_label(vm_id)
                .unwrap_or_else(|| alloc::format!("vm{}", vm_id));
            arguments.push(alloc::format!("nonrep={}|{}", vm_id, clean(label)));
        }
    }
    arguments
}

fn dispatch_admin_action(spawner: &Spawner, target: &MatrixTarget, reason: &str) {
    if matches!(reason, "os:quit" | "os:cancel") {
        return;
    }
    if reason == "os:shutdown" {
        match crate::efi::acpi::facp::enter_named_sleep_state(5) {
            Ok(()) => {}
            Err(error) => print_matrix_target_line(
                target,
                alloc::format!("os: shutdown failed ({error:?})").as_str(),
            ),
        }
        return;
    }
    if reason == "os:reboot" {
        match crate::efi::acpi::facp::reset_system() {
            Ok(()) => {}
            Err(error) => print_matrix_target_line(
                target,
                alloc::format!("os: reboot failed ({error:?})").as_str(),
            ),
        }
        return;
    }
    if reason == "os:update:live" {
        super::os_update::submit_live_update_to_target(spawner, target.clone());
        return;
    }
    if reason == "os:update:lan" {
        super::os_update::submit_live_update_from_url_to_target(
            spawner,
            target.clone(),
            super::os_update::LAN_UPDATE_URL,
        );
        return;
    }

    let Some(rest) = reason.strip_prefix("os:install:") else {
        print_matrix_target_line(target, "os: rejected unknown administration action");
        return;
    };
    let Some((source, raw_id)) = rest.split_once(':') else {
        print_matrix_target_line(target, "os: rejected malformed install action");
        return;
    };
    let Some(raw_id) = super::table::parse_disc_id_raw(raw_id) else {
        print_matrix_target_line(target, "os: selected disk id is invalid");
        return;
    };
    let Some(disk) = super::table::select_top_level_disk(raw_id) else {
        print_matrix_target_line(target, "os: selected disk is no longer available");
        return;
    };

    match source {
        "local" => crate::disc::install::controller::submit_install_to_target(spawner, target.clone(), disk),
        "online" => crate::disc::install::controller::submit_online_install_to_target(spawner, target.clone(), disk),
        "pxeproc-local" => crate::disc::install::controller::submit_install_mode_to_target(
            spawner,
            target.clone(),
            disk,
            crate::disc::install::pxeproc::InstallMode::Pxeproc,
        ),
        "pxeproc-online" => crate::disc::install::controller::submit_online_install_mode_to_target(
            spawner,
            target.clone(),
            disk,
            crate::disc::install::pxeproc::InstallMode::Pxeproc,
        ),
        _ => print_matrix_target_line(target, "os: rejected unknown install source"),
    }
}
