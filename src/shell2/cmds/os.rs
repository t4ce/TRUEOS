//! Compact operating-system administration TUI launcher.
//!
//! The Blueprint owns only presentation and selection. Privileged work stays
//! here: its exit reason is parsed as a narrow action, the disk is resolved
//! again after the terminal lease is released, and the existing install/live
//! update implementation performs the operation.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use trueos_executor::{Spawner, task};
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line,
};

const OS_ARCHIVE: &str = "os.bp";
const OS_VM_START_TIMEOUT_MS: u64 = 30_000;
const OS_VM_EXIT_TIMEOUT_MS: u64 = 5_000;

static OS_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn disk_argument(choice: &super::tlb_helper::DiskChoice) -> String {
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

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let rest = rest.trim();
    if matches!(rest, "help" | "-h" | "--help") {
        print_shell_line(
            io,
            "os: install TRUEOS onto a selected disk or live-update the running kernel",
        );
        return ParseOutcome::Handled;
    }
    if !rest.is_empty() {
        print_shell_line(io, "os: no arguments expected; use `os`");
        return ParseOutcome::Handled;
    }

    let disks = super::tlb_helper::collect_top_level_disk_choices();
    let app_args: Vec<String> = disks.iter().map(disk_argument).collect();
    let generation = OS_INSTANCE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    let instance_name = alloc::format!("os-admin-{generation}");
    let target = matrix_target_for_backend(io);
    match os_admin_task(*spawner, target, app_args, instance_name) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "os: administration TUI task unavailable"),
    }
    ParseOutcome::Handled
}

#[task(pool_size = 1)]
async fn os_admin_task(
    spawner: Spawner,
    target: MatrixTarget,
    app_args: Vec<String>,
    instance_name: String,
) {
    if let Err(error) = super::run::submit_archive_name_to_target_from_app_db_with_instance_async(
        target.clone(),
        OS_ARCHIVE,
        app_args,
        crate::hv::BlueprintInstanceRequest::named(instance_name.clone()),
    )
    .await
    {
        print_matrix_target_line(
            &target,
            alloc::format!("os: could not launch {OS_ARCHIVE}: {error}").as_str(),
        );
        return;
    }

    let start_deadline = Instant::now()
        .as_millis()
        .saturating_add(OS_VM_START_TIMEOUT_MS);
    let vm_id = loop {
        if let Some(vm_id) = crate::hv::named_app_instance_vms(OS_ARCHIVE)
            .into_iter()
            .find_map(|(vm_id, name)| (name == instance_name).then_some(vm_id))
        {
            break vm_id;
        }
        if Instant::now().as_millis() >= start_deadline {
            print_matrix_target_line(&target, "os: administration TUI launch timed out");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    };

    let active_deadline = Instant::now()
        .as_millis()
        .saturating_add(OS_VM_START_TIMEOUT_MS);
    let mut observed_active = false;
    let reason = loop {
        if let Some(reason) = crate::hv::blueprint_console_exit_reason(vm_id) {
            break reason;
        }
        let state = crate::hv::vm_state(vm_id);
        if state.running || state.starting {
            observed_active = true;
        } else if observed_active {
            print_matrix_target_line(&target, "os: administration TUI exited without an action");
            return;
        } else if Instant::now().as_millis() >= active_deadline {
            print_matrix_target_line(&target, "os: administration TUI start timed out");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    };

    let exit_deadline = Instant::now()
        .as_millis()
        .saturating_add(OS_VM_EXIT_TIMEOUT_MS);
    while {
        let state = crate::hv::vm_state(vm_id);
        state.running || state.starting
    } && Instant::now().as_millis() < exit_deadline
    {
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
    if {
        let state = crate::hv::vm_state(vm_id);
        state.running || state.starting
    } {
        let _ = crate::hv::stop(vm_id);
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }

    dispatch_admin_action(&spawner, &target, reason.as_str());
}

fn dispatch_admin_action(spawner: &Spawner, target: &MatrixTarget, reason: &str) {
    if matches!(reason, "os:quit" | "os:cancel") {
        return;
    }
    if reason == "os:update:live" {
        super::update::submit_live_update_to_target(spawner, target.clone());
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
    let Some(raw_id) = super::tlb_helper::parse_disc_id_raw(raw_id) else {
        print_matrix_target_line(target, "os: selected disk id is invalid");
        return;
    };
    let Some(disk) = super::tlb_helper::select_top_level_disk(raw_id) else {
        print_matrix_target_line(target, "os: selected disk is no longer available");
        return;
    };

    match source {
        "local" => super::install::submit_install_to_target(spawner, target.clone(), disk),
        "online" => super::update::submit_online_install_to_target(spawner, target.clone(), disk),
        _ => print_matrix_target_line(target, "os: rejected unknown install source"),
    }
}
