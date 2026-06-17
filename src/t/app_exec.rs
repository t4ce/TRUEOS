//! Admission/accounting for app-originated execution vessels.
//!
//! This is intentionally not a thread scheduler. Embassy futures and VM hulls
//! carry resume state; this layer only decides whether app-requested execution
//! surfaces may enter TRUEOS carrier lanes.

use core::sync::atomic::{AtomicUsize, Ordering};

const PTHREAD_CONTINUATION_FLOOR_PER_VM: usize = 4;
const APP_WORK_LOG_LIMIT: usize = 32;

static VM_PTHREAD_LIVE: [AtomicUsize; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicUsize::new(0) }; crate::allcaps::hv::VM_ID_LIMIT];
static ADMISSION_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppWorkKind {
    Pthread,
}

impl AppWorkKind {
    const fn limit_per_vm(self) -> usize {
        match self {
            Self::Pthread => PTHREAD_CONTINUATION_FLOOR_PER_VM,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Pthread => "pthread",
        }
    }

    fn live_counter(self, vm_id: u8) -> Option<&'static AtomicUsize> {
        match self {
            Self::Pthread => VM_PTHREAD_LIVE.get(vm_id as usize),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    InvalidVm,
    BudgetExhausted { live: usize, limit: usize },
}

pub struct AppWorkPermit {
    vm_id: Option<u8>,
    kind: AppWorkKind,
    released: bool,
}

impl AppWorkPermit {
    fn release_inner(&mut self) {
        if self.released {
            return;
        }
        self.released = true;

        let Some(vm_id) = self.vm_id else {
            return;
        };
        let Some(counter) = self.kind.live_counter(vm_id) else {
            return;
        };
        let prev = counter.fetch_sub(1, Ordering::AcqRel);
        if prev == 0 {
            counter.store(0, Ordering::Release);
        }
    }
}

impl Drop for AppWorkPermit {
    fn drop(&mut self) {
        self.release_inner();
    }
}

pub fn admit_current_app_work(
    kind: AppWorkKind,
    request_id: usize,
) -> Result<AppWorkPermit, AdmissionError> {
    let vm_id = crate::hv::current_guest_execution_context_vm_id();
    admit_app_work_for_vm(kind, vm_id, request_id)
}

pub fn admit_app_work_for_vm(
    kind: AppWorkKind,
    vm_id: Option<u8>,
    request_id: usize,
) -> Result<AppWorkPermit, AdmissionError> {
    let Some(vm_id) = vm_id else {
        return Ok(AppWorkPermit {
            vm_id: None,
            kind,
            released: false,
        });
    };
    let Some(counter) = kind.live_counter(vm_id) else {
        log_admission(kind, vm_id, request_id, "reject-invalid-vm", 0, 0);
        return Err(AdmissionError::InvalidVm);
    };

    let limit = kind.limit_per_vm();
    loop {
        let live = counter.load(Ordering::Acquire);
        if live >= limit {
            log_admission(kind, vm_id, request_id, "reject-budget", live, limit);
            return Err(AdmissionError::BudgetExhausted { live, limit });
        }
        if counter
            .compare_exchange(live, live + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            log_admission(kind, vm_id, request_id, "admit", live + 1, limit);
            return Ok(AppWorkPermit {
                vm_id: Some(vm_id),
                kind,
                released: false,
            });
        }
    }
}

fn log_admission(
    kind: AppWorkKind,
    vm_id: u8,
    request_id: usize,
    action: &'static str,
    live: usize,
    limit: usize,
) {
    let seq = ADMISSION_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if seq >= APP_WORK_LOG_LIMIT {
        return;
    }
    crate::log_info!(
        target: "service";
        "app-exec: {} kind={} vm={} request={} live={} limit={}\n",
        action,
        kind.name(),
        vm_id,
        request_id,
        live,
        limit
    );
}
