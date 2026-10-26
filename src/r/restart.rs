//! Blueprint restart policy for cold boots and live kernel replacement.
//!
//! These paths are intentionally exclusive. A power-on, reset, or ACPI reboot
//! starts local Blueprints from AppDB, falling back to Appstore pullbot. A live update
//! leaves applications closed unless the legacy checkpoint feature is enabled.

use alloc::{string::String, vec::Vec};

use serde::Deserialize;
use trueos_executor::Spawner;
use trueos_time::{Duration, Timer};

#[cfg(feature = "legacy_vm_checkpoint")]
const VM_STORE_READY_TIMEOUT_MS: u64 = 30_000;
#[cfg(feature = "legacy_vm_checkpoint")]
const VM_STORE_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(feature = "legacy_vm_checkpoint")]
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
    fn pullbot_script(&self) -> Result<String, String> {
        let selector = self.online_selector.as_deref()
            .unwrap_or_else(|| self.archive.strip_suffix(".bp").unwrap_or(&self.archive));
        if selector.is_empty() || selector.starts_with('-')
            || selector.chars().any(|c| c.is_whitespace() || c.is_control() || c == '§') {
            return Err("invalid online selector".into());
        }
        if self.instance.is_some() || !self.args.is_empty() {
            return Err("Appstore pullbot does not support instance or args".into());
        }
        match self.launch_script.as_deref() {
            Some(script) if !script.trim().is_empty() => {
                let script = script.trim();
                if script.chars().any(|c| c.is_control()) {
                    return Err("Appstore pullbot requires a single-line launch script".into());
                }
                Ok(alloc::format!("pull --headless {selector} -- {script}\n"))
            }
            _ => Ok(alloc::format!("pull --headless {selector}\n")),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupAlias {
    name: String,
    blueprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdStartConfiguration {
    #[serde(default)]
    aliases: Vec<StartupAlias>,
    autostart: Vec<ColdStartBlueprint>,
    /// Application settings are consumed by guests through vFile:startup.
    #[serde(default, rename = "solara")]
    _solara: serde_json::Value,
}

/// Resolve an application alias from the same embedded startup manifest used by
/// cold-start Blueprint launch. Returning owned data keeps the JSON parser's
/// allocation independent from the caller's task lifetime.
pub(crate) fn startup_alias_blueprint(alias: &str) -> Option<String> {
    super::startup_config::blueprint(alias)
}
pub(crate) fn startup_alias_names() -> Vec<String> { super::startup_config::aliases() }

pub(crate) fn embedded_startup_manifest() -> &'static [u8] { COLD_START_BLUEPRINTS_JSON }
pub(crate) fn validate_startup_manifest(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let config: ColdStartConfiguration = serde_json::from_slice(bytes).map_err(|e| alloc::format!("{e}"))?;
    let mut aliases = Vec::new();
    for alias in config.aliases {
        if alias.name.is_empty() || alias.name == "Aka" || alias.name.chars().any(|c| c.is_whitespace() || c.is_control() || c == '§')
            || alias.blueprint.is_empty() || alias.blueprint.chars().any(|c| c.is_whitespace() || c.is_control())
            || aliases.iter().any(|(name,_)| name == &alias.name) {
            return Err("invalid or duplicate alias".into());
        }
        aliases.push((alias.name, alias.blueprint));
    }
    Ok(aliases)
}

#[trueos_executor::task]
pub(crate) async fn restart_apps_task(spawner: Spawner) {
    #[cfg(not(feature = "legacy_vm_checkpoint"))]
    let _ = spawner;
    if RestartPolicy::active() == RestartPolicy::ColdStart {
        crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;
        super::startup_config::reload().await;
        let manifest = super::startup_config::manifest();
        let config: ColdStartConfiguration =
            match serde_json::from_slice(&manifest) {
                Ok(config) => config,
                Err(error) => {
                    crate::log!("restart: invalid startup configuration error={}\n", error);
                    return;
                }
            };
        for app in config.autostart {
            if app.action != ColdStartAction::Launch {
                continue;
            }
            Timer::after(Duration::from_millis(app.settle_ms)).await;
            let result = (|| {
                if app.online_selector.is_none() {
                    if let Some(bytes) = crate::app_db::get(&app.archive)? {
                        let mut request = crate::hv::launcher::LaunchRequest::new(app.archive.clone(), bytes);
                        request.args = app.args.clone();
                        request.script = app.launch_script.clone();
                        if let Some(instance) = &app.instance {
                            request.instance = crate::hv::BlueprintInstanceRequest::named(instance.clone());
                        }
                        return crate::hv::launcher::enqueue(request, None).map(|_| ());
                    }
                }
                let script = app.pullbot_script()?;
                let bytes = crate::app_db::get("apps.bp")?
                    .ok_or_else(|| String::from("Appstore builtin unavailable"))?;
                let mut request = crate::hv::launcher::LaunchRequest::new("apps.bp".into(), bytes);
                request.script = Some(script);
                crate::hv::launcher::enqueue(request, None).map(|_| ())
            })();
            match result {
                Ok(()) => crate::log!("restart: cold-start queued archive={} slot={}\n", app.archive, app.slot),
                Err(error) => crate::log!("restart: cold-start failed archive={} error={}\n", app.archive, error),
            }
        }
        return;
    }

    let Some(plan) = crate::live_update::warm_vm_restart_plan().await else {
        return;
    };
    #[cfg(not(feature = "legacy_vm_checkpoint"))]
    {
        if !plan.entries.is_empty() {
            crate::log!("restart: checkpoint handoff ignored entries={} feature=disabled\n", plan.entries.len());
        }
        crate::live_update::mark_post_boot_uplift_complete(plan.generation);
    }
    #[cfg(feature = "legacy_vm_checkpoint")]
    restore_warm_checkpoints(spawner, plan).await;
}

#[cfg(feature = "legacy_vm_checkpoint")]
async fn restore_warm_checkpoints(spawner: Spawner, plan: crate::live_update::WarmVmRestartPlan) {
    // No startup manifest, online launch, or fresh proof app runs on this path.
    if !plan.entries.iter().any(|entry| entry.resume) {
        crate::live_update::mark_post_boot_uplift_complete(plan.generation);
        return;
    }
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
    for entry in plan.entries {
        // A stopped/paused app must not be relaunched merely because a checkpoint exists.
        if !entry.resume {
            continue;
        }
        if entry.replicate {
            let vm_id = entry.vm_id;
            let name = entry.checkpoint_name;
            let _ = crate::hv::eject(vm_id);
            match crate::hv::try_begin_restore(vm_id) {
                Ok(true) => {}
                Ok(false) => {
                    crate::log!(
                        "restart: vm{} replica already queued checkpoint={}\n",
                        vm_id,
                        name
                    );
                    continue;
                }
                Err(error) => {
                    crate::log!(
                        "restart: vm{} replica admission failed checkpoint={} error={error:?}\n",
                        vm_id,
                        name,
                    );
                    continue;
                }
            }
            let image = match crate::hv::store::load_replication_async(name.as_str()).await {
                Ok(image) => image,
                Err(error) => {
                    crate::log!(
                        "restart: vm{} replication checkpoint load failed checkpoint={} error={error:?}\n",
                        vm_id,
                        name,
                    );
                    crate::hv::finish_restore(vm_id);
                    continue;
                }
            };
            if image.source_vm_id != vm_id {
                crate::log!(
                    "restart: vm{} replication source mismatch checkpoint={} source_vm={} action=reject\n",
                    vm_id,
                    name,
                    image.source_vm_id,
                );
                crate::hv::finish_restore(vm_id);
                continue;
            }
            let relaunch = match crate::hv::decode_blueprint_relaunch_state(image.relaunch()) {
                Ok(relaunch) => relaunch,
                Err(error) => {
                    crate::log!(
                        "restart: vm{} replication relaunch decode failed checkpoint={} error={}\n",
                        vm_id,
                        name,
                        error,
                    );
                    crate::hv::finish_restore(vm_id);
                    continue;
                }
            };
            let checkpoint = crate::hv::BlueprintReplicationCheckpoint {
                operation: 0,
                version: image.checkpoint_version,
                bytes: image.checkpoint().to_vec(),
            };
            match crate::hv::start_blueprint_replica_vm(vm_id, &spawner, relaunch, checkpoint) {
                Ok(()) => crate::log!(
                    "restart: vm{} fresh-Hull replica queued checkpoint={} generation={} original_running={}\n",
                    vm_id,
                    name,
                    plan.generation,
                    entry.resume as u8,
                ),
                Err(error) => {
                    crate::log!(
                        "restart: vm{} fresh-Hull replica failed checkpoint={} error={error:?}\n",
                        vm_id,
                        name,
                    );
                    crate::hv::finish_restore(vm_id);
                    continue;
                }
            }
            crate::hv::finish_restore(vm_id);
            continue;
        }
        let vm_id = entry.vm_id;
        let name = entry.checkpoint_name;
        let _ = crate::hv::eject(vm_id);
        match crate::hv::try_begin_restore(vm_id) {
            Ok(true) => {}
            Ok(false) => {
                crate::log!("restart: vm{} restore already queued checkpoint={}\n", vm_id, name);
                continue;
            }
            Err(error) => {
                crate::log!(
                    "restart: vm{} restore admission failed checkpoint={} error={error:?}\n",
                    vm_id,
                    name,
                );
                continue;
            }
        }

        crate::log!(
            "restart: vm{} checkpoint load begin checkpoint={} generation={}\n",
            vm_id,
            name,
            plan.generation,
        );
        let image = match crate::hv::store::load_persistent_async(name.as_str()).await {
            Ok(image) => image,
            Err(error) => {
                crate::log!(
                    "restart: vm{} checkpoint load failed checkpoint={} error={error:?}\n",
                    vm_id,
                    name,
                );
                crate::hv::finish_restore(vm_id);
                continue;
            }
        };
        crate::log!(
            "restart: vm{} checkpoint load complete checkpoint={} snapshot={} guest_heap={} hull_rw={} blueprint={}\n",
            vm_id,
            name,
            image.snapshot().len(),
            image.guest_heap().len(),
            image.hull_rw().len(),
            image.blueprint().len(),
        );
        if image.source_vm_id != vm_id {
            crate::log!(
                "restart: vm{} checkpoint source mismatch checkpoint={} source_vm={} action=reject\n",
                vm_id,
                name,
                image.source_vm_id,
            );
            crate::hv::finish_restore(vm_id);
            continue;
        }
        if let Err(error) =
            crate::hv::store::save_bytes_async(vm_id, image.snapshot().to_vec()).await
        {
            crate::log!(
                "restart: vm{} warm-store seed failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            );
            crate::hv::finish_restore(vm_id);
            continue;
        }
        if let Err(error) = crate::hv::restore_persistent_image(vm_id, &image, None) {
            crate::log!(
                "restart: vm{} envelope load failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            );
            crate::hv::finish_restore(vm_id);
            continue;
        }

        match crate::hv::start(vm_id, &spawner, None) {
            Ok(()) => crate::log!(
                "restart: vm{} load queued checkpoint={} generation={} resume=1\n",
                vm_id,
                name,
                plan.generation,
            ),
            Err(error) => crate::log!(
                "restart: vm{} loaded but resume failed checkpoint={} error={error:?}\n",
                vm_id,
                name,
            ),
        }
        crate::hv::finish_restore(vm_id);
    }
    crate::live_update::mark_post_boot_uplift_complete(plan.generation);
}

/// Public, read-only startup configuration for Blueprint application settings.
pub(crate) fn startup_manifest() -> alloc::sync::Arc<[u8]> {
    super::startup_config::manifest()
}
