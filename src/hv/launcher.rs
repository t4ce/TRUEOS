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
    pub frontend: Option<crate::shell3::tui::Frontend>,
    pub console_surface: super::BlueprintConsoleSurface,
}
impl LaunchRequest {
    pub fn new(archive: String, bytes: Vec<u8>) -> Self {
        Self {
            archive,
            bytes,
            args: Vec::new(),
            script: None,
            instance: super::BlueprintInstanceRequest::default(),
            frontend: None,
            console_surface: super::BlueprintConsoleSurface::Text,
        }
    }
}

/// Callbacks must be synchronous and must not enqueue another launch during
/// admission. `is_live` cancels pending work; owners handle killing active VMs.
pub(crate) trait LaunchObserver: Send + Sync {
    fn is_live(&self) -> bool {
        true
    }

    fn started(&self, _vm: u8) {}
    /// Observed launches retain their Matrix lifetime through teardown. The
    /// observer must discard it or transfer it to the resulting host action.
    /// Exit reasons are captured at retirement before the VM ID can be reused.
    fn finished(
        &self,
        _target: &crate::shell3::MatrixTarget,
        _vm: Option<u8>,
        _error: Option<&str>,
        _exit_reason: Option<&str>,
    ) {
    }
}

static ADMISSION: spin::Mutex<()> = spin::Mutex::new(());

/// Reserve one Matrix identity and queue a launch; a frontend is optional.
/// Success means queued, not yet started. Failures after admission are reported
/// to the observer, or the Blueprint log for unattended launches.
pub(crate) fn enqueue(
    request: LaunchRequest,
    observer: Option<Box<dyn LaunchObserver>>,
) -> Result<super::matrix::AppReceipt, String> {
    let prebind_started = trueos_time::Instant::now();
    let (required, unpacked) = super::blueprint::prebind_required_readiness(&request.bytes)?;
    crate::log_info!(target: "apps"; "launch phase=prebind elapsed_ms={} input_bytes={} output_bytes={} payload_reused=1\n",
        prebind_started.elapsed().as_millis(), request.bytes.len(), unpacked.len());
    let worker = crate::workers::pick_background_spawner().ok_or("apps: no background worker")?;
    let (target, receipt) = super::matrix::reserve(&request.archive, &request.bytes)?;
    if observer.is_some() {
        super::matrix::observe_completion(&target);
    }
    let frontend = request.frontend;
    if let Some(frontend) = frontend {
        if let Err(error) = crate::shell3::tui::attach(frontend, &target) {
            super::matrix::discard(&target);
            return Err(error);
        }
    }
    let task = match launch_task(request, target.clone(), required, unpacked, observer) {
        Ok(task) => task,
        Err(_) => {
            super::matrix::discard(&target);
            return Err("apps: launch task pool exhausted".into());
        }
    };
    // All fallible synchronous admission is complete. Do not park the old
    // native helper on attachment/task-pool failure, and do not let the new
    // guest run its one-shot terminal claim before this handoff commits.
    if let Some(frontend) = frontend {
        crate::shell3::tui::park_native_for_launch(frontend);
    }
    worker.spawn(task);
    Ok(receipt)
}

#[trueos_executor::task(pool_size = 64)]
async fn launch_task(
    request: LaunchRequest,
    target: crate::shell3::MatrixTarget,
    required: u32,
    unpacked: Vec<u8>,
    observer: Option<Box<dyn LaunchObserver>>,
) {
    let live = || {
        crate::shell3::matrix_target::matrix_slot_is_live(&crate::shell3::matrix_target_slot_lease(
            &target,
        )) && observer.as_ref().map_or(true, |owner| owner.is_live())
    };
    let finish = |vm, error: Option<String>, reason: Option<String>| {
        if let Some(owner) = &observer {
            owner.finished(&target, vm, error.as_deref(), reason.as_deref());
        }
        if let Some(error) = &error {
            crate::log_os::blueprint_important_line(format_args!(
                "hv launcher: {}: {}\n",
                request.archive, error
            ));
        }
        if vm.is_none() {
            super::matrix::discard(&target);
        }
    };
    let deadline = trueos_time::Instant::now() + Duration::from_secs(30);
    while crate::r::readiness::mask() & required != required {
        if !live() {
            finish(None, None, None);
            return;
        }
        if trueos_time::Instant::now() >= deadline {
            finish(
                None,
                Some(alloc::format!("{}: readiness timeout mask=0x{required:x}", request.archive)),
                None,
            );
            return;
        }
        Timer::after(Duration::from_millis(25)).await;
    }
    let spawner = unsafe { trueos_executor::Spawner::for_current_executor().await };
    let result = {
        let _admission = ADMISSION.lock();
        if !live() {
            finish(None, None, None);
            return;
        }
        match super::first_free_vm_id() {
            Some(vm) => {
                let result = super::start_prepared_blueprint_app_vm(
                    vm,
                    &spawner,
                    request.archive.clone(),
                    request.bytes,
                    Some(unpacked),
                    request.args,
                    request.script,
                    request.instance,
                    Some(target.clone()),
                    request.console_surface,
                );
                result
                    .map(|()| {
                        let run = super::vm_run_generation(vm);
                        if let Some(owner) = &observer {
                            owner.started(vm);
                        }
                        (vm, run)
                    })
                    .map_err(|error| alloc::format!("{error:?}"))
            }
            None => Err(String::from("no free VM")),
        }
    };
    let (vm, run) = match result {
        Ok(value) => value,
        Err(error) => {
            finish(None, Some(error), None);
            return;
        }
    };
    loop {
        if let Some(reason) = super::matrix::take_completion(&target) {
            finish(Some(vm), None, reason);
            return;
        }
        if !live() {
            finish(Some(vm), None, None);
            return;
        }
        let state = super::vm_state(vm);
        // Offline publication precedes Matrix retirement. Observed launches
        // must wait for that retirement snapshot, rather than race teardown.
        if observer.is_none()
            && (super::vm_run_generation(vm) != run || (!state.running && !state.starting))
        {
            break;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    finish(Some(vm), None, None);
}
