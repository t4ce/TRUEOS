//! Asynchronous Blueprint admission shared by host callers.
//! Presentation owners may observe a launch; unattended callers pass `None`.
use alloc::{boxed::Box, string::String, vec::Vec};
use trueos_time::{Duration, Timer};

pub(crate) struct LaunchRequest {
    pub archive: String,
    pub bytes: Vec<u8>,
    pub args: Vec<String>,
    pub script: Option<String>,
    pub instance: super::BlueprintInstanceRequest,
    pub console_target: Option<crate::shell3::MatrixTarget>,
    pub console_surface: super::BlueprintConsoleSurface,
}
impl LaunchRequest {
    pub fn new(archive: String, bytes: Vec<u8>) -> Self {
        Self { archive, bytes, args: Vec::new(), script: None,
            instance: super::BlueprintInstanceRequest::default(), console_target: None,
            console_surface: super::BlueprintConsoleSurface::Text }
    }
}

/// Callbacks must be synchronous and must not enqueue another launch during
/// admission. `is_live` cancels pending work; owners handle killing active VMs.
pub(crate) trait LaunchObserver: Send + Sync {
    fn is_live(&self) -> bool { true }
    fn starting(&self, _vm: u8) {}
    fn started(&self, _vm: u8) {}
    fn finished(&self, _vm: Option<u8>, _error: Option<&str>) {}
}

static ADMISSION: spin::Mutex<()> = spin::Mutex::new(());

/// Validate and queue a launch without requiring a shell or terminal slot.
/// Success means queued, not yet started. Failures after admission are reported
/// to the observer, or the Blueprint log for unattended launches.
pub(crate) fn enqueue(request: LaunchRequest, observer: Option<Box<dyn LaunchObserver>>) -> Result<(), String> {
    let required = super::blueprint::prebind_required_readiness(&request.bytes)?;
    let worker = crate::workers::pick_background_spawner().ok_or("apps: no background worker")?;
    let task = launch_task(request, required, observer).map_err(|_| String::from("apps: launch task pool exhausted"))?;
    worker.spawn(task);
    Ok(())
}

#[trueos_executor::task(pool_size = 64)]
async fn launch_task(request: LaunchRequest, required: u32, observer: Option<Box<dyn LaunchObserver>>) {
    let live = || observer.as_ref().map_or(true, |owner| owner.is_live());
    let finish = |vm, error: Option<String>| {
        if let Some(owner) = &observer { owner.finished(vm, error.as_deref()); }
        else if let Some(error) = error { crate::log_os::blueprint_important_line(format_args!("hv launcher: {}: {}\n", request.archive, error)); }
    };
    let deadline = trueos_time::Instant::now() + Duration::from_secs(30);
    while crate::r::readiness::mask() & required != required {
        if !live() { finish(None, None); return; }
        if trueos_time::Instant::now() >= deadline {
            finish(None, Some(alloc::format!("{}: readiness timeout mask=0x{required:x}", request.archive))); return;
        }
        Timer::after(Duration::from_millis(25)).await;
    }
    let spawner = unsafe { trueos_executor::Spawner::for_current_executor().await };
    let result = {
        let _admission = ADMISSION.lock();
        if !live() { finish(None, None); return; }
        match super::first_free_vm_id() {
            Some(vm) => {
                if let Some(owner) = &observer { owner.starting(vm); }
                let result = super::start_blueprint_app_vm(vm, &spawner, request.archive.clone(), request.bytes,
                    request.args, request.script, request.instance, request.console_target, request.console_surface);
                result.map(|()| {
                    let run = super::vm_run_generation(vm);
                    if let Some(owner) = &observer { owner.started(vm); }
                    (vm, run)
                }).map_err(|error| alloc::format!("{error:?}"))
            }
            None => Err(String::from("no free VM")),
        }
    };
    let (vm, run) = match result { Ok(value) => value, Err(error) => { finish(None, Some(error)); return; } };
    loop {
        if !live() { finish(Some(vm), None); return; }
        let state = super::vm_state(vm);
        if super::vm_run_generation(vm) != run || (!state.running && !state.starting) { break; }
        Timer::after(Duration::from_millis(100)).await;
    }
    finish(Some(vm), None);
}
