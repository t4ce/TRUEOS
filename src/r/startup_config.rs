//! One validated runtime startup manifest shared by every shell and guest.
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::disc::block::{self, DeviceHandle};
use trueos_time::{Duration, Timer};

pub(crate) const PATH: &str = "apps/common/startup.json";
const MAX_BYTES: u64 = 1024 * 1024;
struct Configuration {
    bytes: Arc<[u8]>,
    aliases: Vec<(String, String)>,
}
static CURRENT: spin::Mutex<Option<Configuration>> = spin::Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(1);
static DIRTY: AtomicBool = AtomicBool::new(true);
static STARTED: AtomicBool = AtomicBool::new(false);

fn current() -> spin::MutexGuard<'static, Option<Configuration>> {
    let mut guard = CURRENT.lock();
    if guard.is_none() {
        let bytes = super::restart::embedded_startup_manifest();
        *guard = Some(Configuration { bytes: Arc::from(bytes),
            aliases: super::restart::validate_startup_manifest(bytes).unwrap_or_default() });
    }
    guard
}
pub(crate) fn manifest() -> Arc<[u8]> { current().as_ref().unwrap().bytes.clone() }
pub(crate) fn generation() -> u64 { GENERATION.load(Ordering::Acquire) }
pub(crate) fn aliases() -> Vec<String> { current().as_ref().unwrap().aliases.iter().map(|(name,_)|name.clone()).collect() }
pub(crate) fn blueprint(name: &str) -> Option<String> {
    current().as_ref().unwrap().aliases.iter().find(|(alias,_)|alias == name).map(|(_,bp)|bp.clone())
}
fn publish(bytes: Vec<u8>) -> Result<bool, String> {
    let mut guard = current();
    if guard.as_ref().unwrap().bytes.as_ref() == bytes.as_slice() { return Ok(false); }
    let aliases = super::restart::validate_startup_manifest(&bytes)?;
    *guard = Some(Configuration { bytes: Arc::from(bytes), aliases });
    GENERATION.fetch_add(1, Ordering::Release);
    drop(guard);
    crate::shell3::service::notify_work();
    Ok(true)
}
fn changed(_: block::DiscId, _: &str) { DIRTY.store(true, Ordering::Release); }
fn roots() -> Vec<DeviceHandle> {
    // The existing /common sandbox export resolves on the primary mounted root.
    super::fs::trueosfs::primary_root_id().and_then(block::device_handle).into_iter().collect()
}

/// Used at boot too, before evaluating autostart. Runtime updates never rerun it.
pub(crate) async fn reload() {
    for disk in roots() {
        let Ok(Some(info)) = super::fs::trueosfs::file_info_async(disk, PATH).await else { continue; };
        if info.data_len > MAX_BYTES { return; }
        let Ok(Some(bytes)) = super::fs::trueosfs::file_out_async(disk, PATH).await else { return; };
        match publish(bytes) {
            Ok(true) => crate::log_info!(target: "service"; "startup: runtime configuration refreshed disc={} path={}\n", disk.id().raw(), PATH),
            Err(error) => crate::log_warn!(target: "service"; "startup: rejected runtime configuration; keeping last valid version: {}\n", error),
            _ => {},
        }
        return;
    }
}
pub(crate) fn start() {
    if STARTED.swap(true, Ordering::AcqRel) { return; }
    super::fs::file_watch::subscribe(PATH, changed);
    let token = watcher_task();
    if let (Some(worker), Ok(token)) = (crate::workers::pick_background_spawner(), token) { worker.spawn(token); }
    else { STARTED.store(false, Ordering::Release); }
}
#[trueos_executor::task(pool_size = 1)]
async fn watcher_task() {
    let mut ticks = 0;
    let mut known_roots = Vec::new();
    loop {
        // Periodic discovery covers roots mounted after the embedded seed.
        let dirty = DIRTY.swap(false, Ordering::AcqRel);
        let mounted: Vec<_> = if ticks == 0 { roots().iter().map(|disk| disk.id()).collect() } else { known_roots.clone() };
        if dirty || mounted != known_roots { reload().await; }
        known_roots = mounted;
        ticks = (ticks + 1) % 20;
        Timer::after(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn edit_path() -> Result<String, String> {
    let roots = roots();
    for disk in &roots {
        if super::fs::trueosfs::file_info_async(*disk, PATH).await.map_err(|e|alloc::format!("Aka: {e:?}"))?.is_some() {
            if disk.info().is_read_only() { return Err("Aka: configuration root is read-only".into()); }
            return Ok(String::from("/common/startup.json"));
        }
    }
    let disk = roots.into_iter().find(|disk| !disk.info().is_read_only()).ok_or("Aka: no writable TRUEOSFS root")?;
    super::fs::trueosfs::dir_create_all_async(disk, "apps/common").await.map_err(|e|alloc::format!("Aka: {e:?}"))?;
    let bytes = manifest();
    if !super::fs::trueosfs::file_in_async(disk, PATH, &bytes).await.map_err(|e|alloc::format!("Aka: {e:?}"))? {
        return Err("Aka: cannot create startup.json".into());
    }
    Ok(String::from("/common/startup.json"))
}
