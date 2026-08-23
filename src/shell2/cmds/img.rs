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

/// The unique non-autostart Blueprint launch after a successful warm handoff.
///
/// This proof is deliberately outside configured app autostart and its
/// restored-archive deduplication. It waits until checkpoint uplift has yielded
/// the restored VM slots, then starts one fresh named viewer. If an older proof
/// was never cleanly stopped and is itself restored, both instances may coexist
/// by design: each completed live boot remains independently visible.
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
    while !crate::live_update::post_boot_uplift_complete(generation) {
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }

    let instance_name = alloc::format!("live-update-{generation}");
    let mut rng = crate::tyche::soft_rng();
    let variant = rng.usize_below(crate::virtio_gpu_logo::LIVE_UPDATE_NOTICE_VARIANT_COUNT);
    let source = crate::virtio_gpu_logo::live_update_notice_source(variant);
    // Submit from the existing system/default slot. The archive launcher then
    // owns the one real `img` slot reservation; naming a synthetic source slot
    // here would leave an empty `lu-img` tab beside the proof viewer.
    let target = matrix_target_for_slot_name(OUTPUT_SYSTEM_MASK, "");
    let submitted = super::run::submit_archive_name_to_target_from_app_db_with_instance_waiving_readiness_noninteractive_async(
            target,
            IMG_ARCHIVE,
            alloc::vec![String::from(source)],
            crate::hv::BlueprintInstanceRequest::named(instance_name.clone()),
            crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
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
        if let Some(vm_id) = found {
            let state = crate::hv::vm_state(vm_id);
            // `lifecycle_ready` means Ready-for-checkpoint, not launched or
            // visible. Start the lifetime only after UI4 confirms this VM's
            // first frame crossed the physical SURFLIVE boundary.
            if state.running
                && !state.starting
                && !state.stop_requested
                && crate::ui4::owner_has_first_presentation(crate::ui4::WindowOwner::Vm(vm_id))
            {
                break Some(vm_id);
            }
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
        "live-update: notice visible generation={} vm={} variant={} lifetime=until-explicit-close source={}\n",
        generation,
        vm_id,
        variant,
        source,
    );
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
