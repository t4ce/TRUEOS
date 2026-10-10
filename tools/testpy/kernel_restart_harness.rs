#![allow(dead_code, unused_imports)]
extern crate alloc;
extern crate self as trueos_executor;
extern crate self as trueos_time;
#[derive(Clone, Copy)]
pub struct Spawner;
pub struct Duration;
impl Duration {
    pub const fn from_millis(_: u64) -> Self {
        Self
    }
}
pub struct Timer;
impl Timer {
    pub async fn after(_: Duration) {}
}
pub struct Instant;
impl Instant {
    pub fn now() -> Self {
        Self
    }
    pub fn as_millis(&self) -> u64 {
        0
    }
}
#[macro_export]
macro_rules! log {($($a:tt)*)=>{{let _=format_args!($($a)*);}}}
use std::sync::Mutex;
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
fn event(s: impl Into<String>) {
    EVENTS.lock().unwrap().push(s.into());
}
mod app_db {
    pub fn get(s: &str) -> Result<Option<Vec<u8>>, String> {
        crate::event(format!("archive:{s}"));
        Ok(Some(vec![1]))
    }
}
mod r {
    pub mod readiness {
        pub const TRUEOSFS_ROOT_MOUNTED: u32 = 1;
        pub const UI4_COMPOSITOR_READY: u32 = 2;
        pub async fn wait_for(_: u32) {}
    }
}
mod live_update {
    use super::*;
    pub static WARM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub fn warm_boot_active() -> bool {
        WARM.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub struct WarmVmRestartEntry {
        pub vm_id: u8,
        pub checkpoint_name: String,
        pub resume: bool,
        pub replicate: bool,
    }
    pub struct WarmVmRestartPlan {
        pub generation: u64,
        pub entries: Vec<WarmVmRestartEntry>,
    }
    pub static PLAN: Mutex<Option<WarmVmRestartPlan>> = Mutex::new(None);
    pub async fn warm_vm_restart_plan() -> Option<WarmVmRestartPlan> {
        PLAN.lock().unwrap().take()
    }
    pub fn mark_post_boot_uplift_complete(_: u64) {
        event("complete")
    }
}
mod hv {
    use super::*;
    #[derive(Default)]
    pub struct BlueprintInstanceRequest;
    impl BlueprintInstanceRequest {
        pub fn named(_: String) -> Self {
            Self
        }
    }
    pub enum BlueprintConsoleSurface {
        Text,
    }
    pub struct BlueprintReplicationCheckpoint {
        pub operation: u64,
        pub version: u32,
        pub bytes: Vec<u8>,
    }
    pub mod blueprint {
        pub fn prebind_required_readiness(_: &[u8]) -> Result<u32, String> {
            Ok(0)
        }
    }
    pub fn first_free_vm_id() -> Option<u8> {
        Some(0)
    }
    pub fn start_blueprint_app_vm(
        id: u8,
        _: &Spawner,
        archive: String,
        _: Vec<u8>,
        _: Vec<String>,
        _: Option<String>,
        _: BlueprintInstanceRequest,
        target: Option<()>,
        _: BlueprintConsoleSurface,
    ) -> Result<(), String> {
        assert!(target.is_none());
        event(format!("cold:{archive}:{id}"));
        Ok(())
    }
    pub fn eject(id: u8) -> Result<(), String> {
        event(format!("eject:{id}"));
        Ok(())
    }
    pub fn try_begin_restore(_: u8) -> Result<bool, String> {
        Ok(true)
    }
    pub fn finish_restore(id: u8) {
        event(format!("finish:{id}"))
    }
    pub fn decode_blueprint_relaunch_state(_: &[u8]) -> Result<(), String> {
        Ok(())
    }
    pub fn start_blueprint_replica_vm(
        id: u8,
        _: &Spawner,
        _: (),
        _: BlueprintReplicationCheckpoint,
    ) -> Result<(), String> {
        event(format!("replica:{id}"));
        Ok(())
    }
    pub fn restore_persistent_image(_: u8, _: &store::Image, _: Option<()>) -> Result<(), String> {
        Ok(())
    }
    pub fn start(id: u8, _: &Spawner, _: Option<usize>) -> Result<(), String> {
        event(format!("resume:{id}"));
        Ok(())
    }
    pub mod store {
        use super::*;
        pub struct Image {
            pub source_vm_id: u8,
            pub checkpoint_version: u32,
        }
        impl Image {
            pub fn snapshot(&self) -> &[u8] {
                &[]
            }
            pub fn guest_heap(&self) -> &[u8] {
                &[]
            }
            pub fn hull_rw(&self) -> &[u8] {
                &[]
            }
            pub fn blueprint(&self) -> &[u8] {
                &[]
            }
            pub fn relaunch(&self) -> &[u8] {
                &[]
            }
            pub fn checkpoint(&self) -> &[u8] {
                &[]
            }
        }
        pub fn online() -> bool {
            true
        }
        pub async fn load_replication_async(s: &str) -> Result<Image, String> {
            event(format!("load:{s}"));
            Ok(Image {
                source_vm_id: s.parse().unwrap(),
                checkpoint_version: 1,
            })
        }
        pub async fn load_persistent_async(s: &str) -> Result<Image, String> {
            load_replication_async(s).await
        }
        pub async fn save_bytes_async(_: u8, _: Vec<u8>) -> Result<(), String> {
            Ok(())
        }
    }
}
mod restart {
    include!("restart.rs");
}
fn run(f: impl std::future::Future<Output = ()>) {
    use std::task::*;
    struct Wake;
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {}
    }
    let w = Waker::from(std::sync::Arc::new(Wake));
    let mut c = Context::from_waker(&w);
    assert!(std::pin::pin!(f).as_mut().poll(&mut c).is_ready());
}
fn setup(warm: bool, entries: Vec<live_update::WarmVmRestartEntry>) {
    EVENTS.lock().unwrap().clear();
    live_update::WARM.store(warm, std::sync::atomic::Ordering::Relaxed);
    *live_update::PLAN.lock().unwrap() = Some(live_update::WarmVmRestartPlan {
        generation: 1,
        entries,
    });
}
fn entry(id: u8, resume: bool, replicate: bool) -> live_update::WarmVmRestartEntry {
    live_update::WarmVmRestartEntry {
        vm_id: id,
        checkpoint_name: id.to_string(),
        resume,
        replicate,
    }
}
#[test]
fn warm_restores_only_running_apps_without_defaults() {
    setup(
        true,
        vec![
            entry(1, true, true),
            entry(2, false, true),
            entry(3, true, false),
            entry(4, false, false),
        ],
    );
    run(restart::restart_apps_task(Spawner));
    let e = EVENTS.lock().unwrap();
    assert!(e.contains(&"replica:1".into()));
    assert!(e.contains(&"resume:3".into()));
    assert!(!e.iter().any(|x| x.starts_with("cold:")
        || x.starts_with("archive:")
        || x == "eject:2"
        || x == "eject:4"));
    assert_eq!(e.last().unwrap(), "complete");
}
#[test]
fn warm_with_no_running_apps_launches_nothing() {
    setup(true, vec![entry(2, false, true)]);
    run(restart::restart_apps_task(Spawner));
    assert_eq!(*EVENTS.lock().unwrap(), vec!["complete"]);
}
#[test]
fn cold_keeps_configured_defaults_detached_from_shell() {
    setup(false, vec![]);
    run(restart::restart_apps_task(Spawner));
    let e = EVENTS.lock().unwrap();
    assert!(e.contains(&"cold:voxy.bp:0".into()));
    assert!(
        !e.iter()
            .any(|x| x == "complete" || x.starts_with("replica:") || x.starts_with("resume:"))
    );
}
