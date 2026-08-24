//! Blueprint restart policy for cold boots and live kernel replacement.
//!
//! These paths are intentionally exclusive. A power-on, reset, or ACPI reboot
//! starts the configured Blueprints. A live update restores only the VMs named
//! by the warm handoff and never also applies the cold-start list.

use alloc::{string::String, vec::Vec};

use trueos_executor::Spawner;
use trueos_time::{Duration, Timer};

const QUICK_START_SETTLE: Duration = Duration::from_millis(250);
const NORMAL_START_SETTLE: Duration = Duration::from_millis(750);
const VM_STORE_READY_TIMEOUT_MS: u64 = 30_000;
const VM_STORE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const VM_RESUME_SETTLE: Duration = Duration::from_millis(150);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColdStartAction {
    Skip,
    Launch,
}

#[derive(Clone, Copy)]
struct ColdStartBlueprint {
    action: ColdStartAction,
    archive: &'static str,
    online_selector: Option<&'static str>,
    slot: &'static str,
    args: &'static [&'static str],
    launch_script: Option<&'static str>,
    settle: Duration,
}

impl ColdStartBlueprint {
    const fn with_action(
        action: ColdStartAction,
        archive: &'static str,
        slot: &'static str,
        settle: Duration,
    ) -> Self {
        Self {
            action,
            archive,
            online_selector: None,
            slot,
            args: &[],
            launch_script: None,
            settle,
        }
    }

    const fn skip(archive: &'static str, slot: &'static str, settle: Duration) -> Self {
        Self::with_action(ColdStartAction::Skip, archive, slot, settle)
    }

    #[expect(
        dead_code,
        reason = "launch is the deliberate configuration edit; the checked-in profile may skip all entries"
    )]
    const fn launch(archive: &'static str, slot: &'static str, settle: Duration) -> Self {
        Self::with_action(ColdStartAction::Launch, archive, slot, settle)
    }

    const fn online_as(mut self, selector: &'static str) -> Self {
        self.online_selector = Some(selector);
        self
    }

    const fn with_args(mut self, args: &'static [&'static str]) -> Self {
        self.args = args;
        self
    }

    const fn with_launch_script(mut self, script: &'static str) -> Self {
        self.launch_script = Some(script);
        self
    }

    fn label(self) -> &'static str {
        self.archive.strip_suffix(".bp").unwrap_or(self.archive)
    }

    fn online_selector(self) -> &'static str {
        self.online_selector.unwrap_or_else(|| self.label())
    }
}

// This is the only cold-start edit surface. Change `skip` to `launch` for the
// Blueprints wanted after a power loss, reset, or ACPI reboot.
const COLD_START_BLUEPRINTS: &[ColdStartBlueprint] = &[
    ColdStartBlueprint::skip("swarm.bp", "swm", QUICK_START_SETTLE).online_as("swarm"),
    ColdStartBlueprint::skip("img.bp", "img", QUICK_START_SETTLE)
        .online_as("img")
        .with_launch_script(
            "show kernel:logo center nohit\nshow kernel:intel-graphics bottom-left\nshow kernel:bgrt bottom-right",
        ),
    ColdStartBlueprint::skip("horizon.bp", "hor", QUICK_START_SETTLE),
    ColdStartBlueprint::skip("mandelbrot.bp", "man", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("flags.bp", "flg", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("weather.bp", "wth", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("chart.bp", "chr", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("hello_world.bp", "h_w", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("websys.bp", "fs", NORMAL_START_SETTLE),
    ColdStartBlueprint::skip("bat.bp", "bat", NORMAL_START_SETTLE)
        .with_args(&["--help"]),
];

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

    for blueprint in COLD_START_BLUEPRINTS.iter().copied() {
        if blueprint.action == ColdStartAction::Skip {
            crate::log!(
                "restart: cold-start skipped archive={} slot={}\n",
                blueprint.archive,
                blueprint.slot,
            );
            continue;
        }

        Timer::after(blueprint.settle).await;

        let target = crate::shell2::matrix_target_for_slot_name(
            crate::shell2::OUTPUT_SYSTEM_MASK,
            blueprint.slot,
        );
        let local_args = blueprint.args.iter().copied().map(String::from).collect();
        let local = match blueprint.launch_script {
            Some(script) => {
                crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_with_launch_script_async(
                    target.clone(),
                    blueprint.archive,
                    String::from(script),
                )
                .await
            }
            None => {
                crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_default_async(
                    target.clone(),
                    blueprint.archive,
                    local_args,
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
        let submitted = match blueprint.launch_script {
            Some(script) => crate::shell2::submit_online_launch_script_to_target(
                &spawner, target, selector, script,
            ),
            None => {
                let mut args = Vec::with_capacity(blueprint.args.len().saturating_add(1));
                args.push(String::from(selector));
                args.extend(blueprint.args.iter().copied().map(String::from));
                crate::shell2::submit_online_to_target(&spawner, target, args)
            }
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
    match crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_default_async(
        target,
        "weave_hello.bp",
        Vec::new(),
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
