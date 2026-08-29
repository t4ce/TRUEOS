//! Blueprint restart policy for cold boots and live kernel replacement.
//!
//! These paths are intentionally exclusive. A power-on, reset, or ACPI reboot
//! starts the configured Blueprints. A live update restores only the VMs named
//! by the warm handoff and never also applies the cold-start list.

use alloc::{string::String, vec::Vec};

use serde::Deserialize;
use trueos_executor::Spawner;
use trueos_time::{Duration, Timer};

const VM_STORE_READY_TIMEOUT_MS: u64 = 30_000;
const VM_STORE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const VM_RESUME_SETTLE: Duration = Duration::from_millis(150);
const COLD_START_BLUEPRINTS_JSON: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/startup.json"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RestartPolicy {
    ColdStart,
    LiveUpdateRestore,
}

impl RestartPolicy {
    fn active() -> Self {
        if crate::live_update::warm_boot_active() {
            Self::LiveUpdateRestore
        } else {
            Self::ColdStart
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ColdStartAction {
    Skip,
    Launch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdStartBlueprint {
    action: ColdStartAction,
    archive: String,
    online_selector: Option<String>,
    instance: Option<String>,
    slot: String,
    #[serde(default)]
    args: Vec<String>,
    launch_script: Option<String>,
    settle_ms: u64,
}

impl ColdStartBlueprint {
    fn label(&self) -> &str {
        self.archive.strip_suffix(".bp").unwrap_or(&self.archive)
    }

    fn online_selector(&self) -> &str {
        self.online_selector
            .as_deref()
            .unwrap_or_else(|| self.label())
    }

    fn instance_request(&self) -> crate::hv::BlueprintInstanceRequest {
        self.instance
            .as_ref()
            .map_or_else(crate::hv::BlueprintInstanceRequest::default, |name| {
                crate::hv::BlueprintInstanceRequest::named(name.clone())
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdStartConfiguration {
    blueprints: Vec<ColdStartBlueprint>,
}

#[trueos_executor::task]
pub(crate) async fn autostart_task(spawner: Spawner) {
    let policy = RestartPolicy::active();
    crate::log!("restart: selected policy={policy:?}\n");

    match policy {
        RestartPolicy::ColdStart => cold_start_blueprints(spawner).await,
        RestartPolicy::LiveUpdateRestore => restore_live_update_vms(spawner).await,
    }
}

async fn cold_start_blueprints(spawner: Spawner) {
    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;

    let config: ColdStartConfiguration = match serde_json::from_slice(COLD_START_BLUEPRINTS_JSON) {
        Ok(config) => config,
        Err(error) => {
            crate::log!("restart: cold-start configuration invalid error={error}\n");
            return;
        }
    };

    for blueprint in &config.blueprints {
        if blueprint.action == ColdStartAction::Skip {
            crate::log!(
                "restart: cold-start skipped archive={} slot={}\n",
                blueprint.archive,
                blueprint.slot,
            );
            continue;
        }

        Timer::after(Duration::from_millis(blueprint.settle_ms)).await;

        let target = crate::shell2::matrix_target_for_slot_name(
            crate::shell2::OUTPUT_SYSTEM_MASK,
            &blueprint.slot,
        );
        let local_args = blueprint.args.clone();
        let local = match blueprint.launch_script.as_deref() {
            Some(script) => {
                crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_with_instance_and_launch_script_async(
                    target.clone(),
                    &blueprint.archive,
                    local_args,
                    blueprint.instance_request(),
                    Some(String::from(script)),
                )
                .await
            }
            None => {
                crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_with_instance_async(
                    target.clone(),
                    &blueprint.archive,
                    local_args,
                    blueprint.instance_request(),
                )
                .await
            }
        };

        match local {
            Ok(source) => {
                crate::log!(
                    "restart: cold-start queued archive={} slot={} source={}\n",
                    blueprint.archive,
                    blueprint.slot,
                    source,
                );
                continue;
            }
            Err(error) => crate::log!(
                "restart: cold-start local miss archive={} slot={} source=app.db error={} fallback=online\n",
                blueprint.archive,
                blueprint.slot,
                error,
            ),
        }

        // app.db contains both built-ins and downloads. Only when it has no
        // usable local entry do we ask the online catalog to fetch and run it.
        let selector = blueprint.online_selector();
        let mut args = if let Some(name) = blueprint.instance.as_ref() {
            alloc::vec![String::from("new"), String::from(selector), name.clone()]
        } else {
            alloc::vec![String::from(selector)]
        };
        args.extend(blueprint.args.iter().cloned());
        let submitted = match blueprint.launch_script.as_deref() {
            Some(script) => crate::shell2::submit_online_args_with_launch_script_to_target(
                &spawner, target, args, script,
            ),
            None => crate::shell2::submit_online_to_target(&spawner, target, args),
        };
        match submitted {
            Ok(()) => crate::log!(
                "restart: cold-start queued archive={} slot={} source=online-fetch\n",
                blueprint.archive,
                blueprint.slot,
            ),
            Err(error) => crate::log!(
                "restart: cold-start failed archive={} slot={} source=online-fetch error={error:?}\n",
                blueprint.archive,
                blueprint.slot,
            ),
        }
    }
}

async fn restore_live_update_vms(spawner: Spawner) {
    let Some(plan) = crate::live_update::warm_vm_restart_plan().await else {
        return;
    };
    if plan.entries.is_empty() {
        crate::log!(
            "restart: live-update generation={} has no VM entries to restore\n",
            plan.generation,
        );
        crate::live_update::mark_post_boot_uplift_complete(plan.generation);
        return;
    }

    // Restored UI apps need their storage and compositor before hypervisor
    // load. The plan already contains the exact checkpoint and resume choice
    // for every VM; no cold-start Blueprint can enter this branch.
    crate::r::readiness::wait_for(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::UI4_COMPOSITOR_READY,
    )
    .await;
    let deadline = trueos_time::Instant::now()
        .as_millis()
        .saturating_add(VM_STORE_READY_TIMEOUT_MS);
    while !crate::hv::store::online() && trueos_time::Instant::now().as_millis() < deadline {
        Timer::after(VM_STORE_POLL_INTERVAL).await;
    }
    Timer::after(VM_RESUME_SETTLE).await;

    let entry_count = plan.entries.len();
    for entry in plan.entries {
        queue_live_update_vm_restore(&spawner, plan.generation, entry).await;
    }
    crate::live_update::mark_post_boot_uplift_complete(plan.generation);
    crate::log!(
        "restart: live-update restore complete entries={} cold_start_suppressed=1\n",
        entry_count,
    );
}

async fn queue_live_update_vm_restore(
    spawner: &Spawner,
    generation: u64,
    entry: crate::live_update::WarmVmRestartEntry,
) {
    let vm_id = entry.vm_id;
    let name = entry.checkpoint_name;
    let _ = crate::hv::eject(vm_id);
    match crate::hv::try_begin_restore(vm_id) {
        Ok(true) => {}
        Ok(false) => {
            crate::log!("restart: vm{} restore already queued checkpoint={}\n", vm_id, name);
            return;
        }
        Err(error) => {
            crate::log!(
                "restart: vm{} restore admission failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            );
            return;
        }
    }

    let image = match crate::hv::store::load_persistent_async(name.as_str()).await {
        Ok(image) => image,
        Err(error) => {
            crate::log!(
                "restart: vm{} checkpoint load failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            );
            crate::hv::finish_restore(vm_id);
            return;
        }
    };
    if let Err(error) = crate::hv::store::save_bytes_async(vm_id, image.snapshot.clone()).await {
        crate::log!(
            "restart: vm{} warm-store seed failed checkpoint={} error={error:?}\n",
            vm_id,
            name,
        );
        crate::hv::finish_restore(vm_id);
        return;
    }
    if let Err(error) = crate::hv::restore_persistent_image(vm_id, &image, None) {
        crate::log!(
            "restart: vm{} envelope load failed checkpoint={} error={error:?}\n",
            vm_id,
            name,
        );
        crate::hv::finish_restore(vm_id);
        return;
    }

    if entry.resume {
        match crate::hv::start(vm_id, spawner, None) {
            Ok(()) => crate::log!(
                "restart: vm{} load queued checkpoint={} generation={} resume=1\n",
                vm_id,
                name,
                generation,
            ),
            Err(error) => crate::log!(
                "restart: vm{} loaded but resume failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            ),
        }
    } else {
        crate::log!(
            "restart: vm{} load complete checkpoint={} generation={} resume=0\n",
            vm_id,
            name,
            generation,
        );
    }
    crate::hv::finish_restore(vm_id);
}

#[trueos_executor::task]
pub(crate) async fn weave_hello_autostart_task() {
    if RestartPolicy::active() != RestartPolicy::ColdStart {
        crate::log!("restart: weave-hello skipped policy=LiveUpdateRestore\n");
        return;
    }

    // Let the app-VM queue enter its receive loop. This Blueprint is a Limine
    // boot module, so it does not depend on TRUEOSFS being mounted.
    Timer::after(Duration::from_millis(250)).await;
    let target =
        crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_SYSTEM_MASK, "wve");
    match crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_with_instance_async(
        target,
        "weave_hello.bp",
        Vec::new(),
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
    {
        Ok(source) => crate::log!(
            "restart: weave-hello queued archive=weave_hello.bp slot=wve source={}\n",
            source,
        ),
        Err(error) => crate::log!(
            "restart: weave-hello failed archive=weave_hello.bp slot=wve error={}\n",
            error,
        ),
    }
}
