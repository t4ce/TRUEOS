//! Resident UI4 image viewer Blueprint launcher.
//!
//! `img` without arguments opens its VMX-minishell.  Supplying a path makes
//! that the first `show` command; the Blueprint remains alive afterwards so
//! further media can be opened without another VM launch.

use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, OUTPUT_SYSTEM_MASK, ShellBackend2, matrix_target_for_backend,
    matrix_target_for_slot_name, print_matrix_target_system_line, print_shell_line,
    submit_online_to_target,
};

const IMG_APP: &str = "img";
const IMG_ARCHIVE: &str = "img.bp";
const LIVE_UPDATE_SOURCE: &str = "kernel:live-update";
const LIVE_UPDATE_VISIBLE_MS: u64 = 3_000;
const LIVE_UPDATE_LAUNCH_TIMEOUT_MS: u64 = 30_000;

#[task(pool_size = 2)]
async fn launch_img(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    match super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        IMG_ARCHIVE,
        app_args.clone(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let mut online_args = Vec::with_capacity(app_args.len().saturating_add(1));
            online_args.push(String::from(IMG_APP));
            online_args.extend(app_args);
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "img: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("img: could not launch {IMG_ARCHIVE}: {error}").as_str(),
        ),
    }
}

/// Show an unmistakable, kernel-embedded proof after a successful warm handoff.
/// The named instance lets the timer close only this one-shot viewer even when
/// another `img` Blueprint is already open.
pub(crate) fn launch_live_update_notice(spawner: Spawner, generation: u64) {
    match live_update_notice_task(generation) {
        Ok(token) => spawner.spawn(token),
        Err(error) => crate::log_warn!(
            target: "global";
            "live-update: notice task unavailable generation={} error={:?}\n",
            generation,
            error,
        ),
    }
}

#[task(pool_size = 1)]
async fn live_update_notice_task(generation: u64) {
    let instance_name = alloc::format!("live-update-{generation}");
    let target = matrix_target_for_slot_name(OUTPUT_SYSTEM_MASK, "lu-img");
    let submitted = super::run::submit_archive_name_to_target_from_app_db_with_instance_async(
        target,
        IMG_ARCHIVE,
        alloc::vec![String::from(LIVE_UPDATE_SOURCE)],
        crate::hv::BlueprintInstanceRequest::named(instance_name.clone()),
    )
    .await;
    if let Err(error) = submitted {
        crate::log_warn!(
            target: "global";
            "live-update: notice launch rejected generation={} error={}\n",
            generation,
            error,
        );
        return;
    }

    let deadline = Instant::now()
        .as_millis()
        .saturating_add(LIVE_UPDATE_LAUNCH_TIMEOUT_MS);
    let vm_id = loop {
        let found = crate::hv::named_app_instance_vms(IMG_ARCHIVE)
            .into_iter()
            .find_map(|(vm_id, name)| (name == instance_name).then_some(vm_id));
        if let Some(vm_id) = found
            && crate::hv::vm_state(vm_id).lifecycle_ready
        {
            break Some(vm_id);
        }
        if Instant::now().as_millis() >= deadline {
            break None;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    };

    let Some(vm_id) = vm_id else {
        crate::log_warn!(
            target: "global";
            "live-update: notice launch timed out generation={}\n",
            generation,
        );
        return;
    };
    crate::log_info!(
        target: "global";
        "live-update: notice visible generation={} vm={} duration_ms={} source={}\n",
        generation,
        vm_id,
        LIVE_UPDATE_VISIBLE_MS,
        LIVE_UPDATE_SOURCE,
    );
    Timer::after(EmbassyDuration::from_millis(LIVE_UPDATE_VISIBLE_MS)).await;
    match crate::hv::stop(vm_id) {
        Ok(true) => crate::log_info!(
            target: "global";
            "live-update: notice closed generation={} vm={}\n",
            generation,
            vm_id,
        ),
        Ok(false) => crate::log_info!(
            target: "global";
            "live-update: notice already closed generation={} vm={}\n",
            generation,
            vm_id,
        ),
        Err(error) => crate::log_warn!(
            target: "global";
            "live-update: notice close failed generation={} vm={} error={:?}\n",
            generation,
            vm_id,
            error,
        ),
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let args = rest.split_whitespace().map(String::from).collect();
    match launch_img(*spawner, matrix_target_for_backend(io), args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "img: launch task unavailable"),
    }
    ParseOutcome::Handled
}
