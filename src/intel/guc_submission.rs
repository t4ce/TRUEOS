//! GuC-owned physical scheduling for TRUEOS RCS contexts.
//!
//! This module deliberately knows nothing about users, guests, WebGPU, or
//! presentation policy.  It owns the GuC context-id namespace and turns an
//! already-built HW LRC into an explicitly registered physical context.  The
//! generic GPU broker above this module associates those physical contexts
//! with mediated virtual devices and projects their submissions onto virtual
//! timelines.

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

const INTEL_GUC_ACTION_SCHED_CONTEXT: u32 = 0x1000;
const INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET: u32 = 0x1001;
const INTEL_GUC_ACTION_REGISTER_CONTEXT: u32 = 0x4502;
const INTEL_GUC_ACTION_DEREGISTER_CONTEXT: u32 = 0x4503;
const GUC_CONTEXT_DISABLE: u32 = 0;
const GUC_CONTEXT_ENABLE: u32 = 1;
const CONTEXT_REGISTRATION_FLAG_KMD: u32 = 1;
const GUC_RENDER_CLASS: u32 = 0;
const RCS0_SUBMIT_MASK: u32 = 1;
const MAX_GUC_RCS_CONTEXTS: usize = 32;

/// Generation-tagged reference to one GuC context registration.
///
/// The low half is the one-based scheduler slot and the high half is its
/// generation.  A token from a destroyed/reused slot is therefore rejected.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct GucContextToken(u64);

impl GucContextToken {
    const fn new(slot: usize, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (slot as u64 + 1))
    }

    const fn parts(self) -> Option<(usize, u32)> {
        let one_based = self.0 as u32;
        if one_based == 0 {
            return None;
        }
        Some(((one_based - 1) as usize, (self.0 >> 32) as u32))
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GucSubmissionError {
    TransportNotReady,
    ContextRegistryFull,
    InvalidContext,
    RegisterRejected,
    ScheduleRejected,
    DisableRejected,
    DeregisterRejected,
}

impl GucSubmissionError {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::TransportNotReady => "transport-not-ready",
            Self::ContextRegistryFull => "context-registry-full",
            Self::InvalidContext => "invalid-context",
            Self::RegisterRejected => "register-rejected",
            Self::ScheduleRejected => "schedule-rejected",
            Self::DisableRejected => "disable-rejected",
            Self::DeregisterRejected => "deregister-rejected",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucPhysicalSubmission {
    pub(crate) context: GucContextToken,
    pub(crate) serial: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GucSchedulerStatus {
    pub(crate) capacity: usize,
    pub(crate) registered: usize,
    pub(crate) enabled: usize,
    pub(crate) submissions: u64,
    pub(crate) registrations: u64,
    pub(crate) deregistrations: u64,
    pub(crate) failures: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucContextStatus {
    pub(crate) token: GucContextToken,
    pub(crate) context_id: u32,
    pub(crate) enabled: bool,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) submissions: u64,
}

#[derive(Copy, Clone)]
struct RcsContextState {
    registered: bool,
    enabled: bool,
    generation: u32,
    hwlrca_lo: u32,
    hwlrca_hi: u32,
    submissions: u64,
}

impl RcsContextState {
    const EMPTY: Self = Self {
        registered: false,
        enabled: false,
        generation: 0,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
        submissions: 0,
    };

    fn token(self, slot: usize) -> GucContextToken {
        GucContextToken::new(slot, self.generation)
    }
}

struct RcsSubmissionState {
    contexts: [RcsContextState; MAX_GUC_RCS_CONTEXTS],
    serial: u64,
    registrations: u64,
    deregistrations: u64,
    failures: u64,
}

static RCS: Mutex<RcsSubmissionState> = Mutex::new(RcsSubmissionState {
    contexts: [RcsContextState::EMPTY; MAX_GUC_RCS_CONTEXTS],
    serial: 0,
    registrations: 0,
    deregistrations: 0,
    failures: 0,
});

/// Explicit physical scheduler owned by the Intel backend.
pub(crate) struct IntelGucScheduler;

pub(crate) static INTEL_GUC_SCHEDULER: IntelGucScheduler = IntelGucScheduler;

impl IntelGucScheduler {
    pub(crate) fn ready(&self) -> bool {
        ready()
    }

    pub(crate) fn register(
        &self,
        dev: crate::intel::Dev,
        hwlrca_lo: u32,
        hwlrca_hi: u32,
    ) -> Result<GucContextToken, GucSubmissionError> {
        register_rcs_context(dev, hwlrca_lo, hwlrca_hi)
    }

    pub(crate) fn submit(
        &self,
        dev: crate::intel::Dev,
        token: GucContextToken,
    ) -> Result<GucPhysicalSubmission, GucSubmissionError> {
        submit_rcs_context(dev, token)
    }

    pub(crate) fn destroy(
        &self,
        dev: crate::intel::Dev,
        token: GucContextToken,
    ) -> Result<(), GucSubmissionError> {
        destroy_rcs_context(dev, token)
    }

    pub(crate) fn status(&self) -> GucSchedulerStatus {
        scheduler_status()
    }

    pub(crate) fn contexts(&self) -> Vec<GucContextStatus> {
        context_status()
    }
}

pub(crate) fn ready() -> bool {
    crate::intel::guc::ready() && crate::intel::guc_ctb::enabled()
}

/// Register one stable HWLRCA with GuC and return its physical context token.
/// Re-registering the same live HWLRCA is idempotent.
pub(crate) fn register_rcs_context(
    dev: crate::intel::Dev,
    hwlrca_lo: u32,
    hwlrca_hi: u32,
) -> Result<GucContextToken, GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let mut state = RCS.lock();
    if let Some((slot, context)) =
        state
            .contexts
            .iter()
            .copied()
            .enumerate()
            .find(|(_, context)| {
                context.registered
                    && context.hwlrca_lo == hwlrca_lo
                    && context.hwlrca_hi == hwlrca_hi
            })
    {
        return Ok(context.token(slot));
    }

    let Some(slot) = state
        .contexts
        .iter()
        .position(|context| !context.registered)
    else {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::ContextRegistryFull);
    };
    let context_id = (slot + 1) as u32;
    let generation = state.contexts[slot].generation.wrapping_add(1).max(1);
    let register = crate::intel::guc_ctb::send_hxg_action(
        dev,
        INTEL_GUC_ACTION_REGISTER_CONTEXT,
        &[
            CONTEXT_REGISTRATION_FLAG_KMD,
            context_id,
            GUC_RENDER_CLASS,
            RCS0_SUBMIT_MASK,
            0,
            0,
            0,
            0,
            0,
            hwlrca_lo,
            hwlrca_hi,
        ],
    );
    if !register.accepted {
        state.failures = state.failures.saturating_add(1);
        crate::log!(
            "intel/guc-submit: register accepted=0 engine=rcs0 context_id={} hwlrca=0x{:08X}:0x{:08X} response=0x{:08X} type={} error={} g2h_poll_iters={}\n",
            context_id,
            hwlrca_hi,
            hwlrca_lo,
            register.response,
            register.response_type,
            register.error,
            register.g2h_poll_iters
        );
        return Err(GucSubmissionError::RegisterRejected);
    }

    state.contexts[slot] = RcsContextState {
        registered: true,
        enabled: false,
        generation,
        hwlrca_lo,
        hwlrca_hi,
        submissions: 0,
    };
    state.registrations = state.registrations.saturating_add(1);
    let token = GucContextToken::new(slot, generation);
    crate::log!(
        "intel/guc-submit: register accepted=1 engine=rcs0 context_id={} token=0x{:X} class={} submit_mask=0x{:X} hwlrca=0x{:08X}:0x{:08X} abi=v1 single_lrc=1\n",
        context_id,
        token.raw(),
        GUC_RENDER_CLASS,
        RCS0_SUBMIT_MASK,
        hwlrca_hi,
        hwlrca_lo
    );
    Ok(token)
}

/// Notify GuC that a registered LRC has a new ring tail.
///
/// The caller has already written/flushed the LRC and ring.  GuC acceptance is
/// a physical admission serial, not GPU completion; the broker completes its
/// virtual timeline only when the caller's marker/fence retires.
pub(crate) fn submit_rcs_context(
    dev: crate::intel::Dev,
    token: GucContextToken,
) -> Result<GucPhysicalSubmission, GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let mut state = RCS.lock();
    let (slot, generation) = token.parts().ok_or(GucSubmissionError::InvalidContext)?;
    let Some(context) = state.contexts.get(slot).copied() else {
        return Err(GucSubmissionError::InvalidContext);
    };
    if !context.registered || context.generation != generation {
        return Err(GucSubmissionError::InvalidContext);
    }
    let context_id = (slot + 1) as u32;
    let (action, args): (u32, &[u32]) = if context.enabled {
        (INTEL_GUC_ACTION_SCHED_CONTEXT, core::slice::from_ref(&context_id))
    } else {
        // Keep the fixed local array alive for the duration of send below.
        let enable_args = [context_id, GUC_CONTEXT_ENABLE];
        let scheduled = crate::intel::guc_ctb::send_hxg_fast_action(
            dev,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET,
            &enable_args,
        );
        if !scheduled.accepted {
            state.failures = state.failures.saturating_add(1);
            log_schedule_rejected(context_id, INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET, scheduled);
            return Err(GucSubmissionError::ScheduleRejected);
        }
        state.contexts[slot].enabled = true;
        state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
        state.serial = state.serial.wrapping_add(1).max(1);
        let serial = state.serial;
        crate::log_info!(
            target: "gpgpu";
            "intel/guc-submit: schedule enqueued=1 engine=rcs0 context_id={} token=0x{:X} serial={} action=0x{:04X} hxg=fast-request completion_event=sched-context-mode-done submission_owner=guc\n",
            context_id,
            token.raw(),
            serial,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET
        );
        return Ok(GucPhysicalSubmission {
            context: token,
            serial,
        });
    };
    let scheduled = crate::intel::guc_ctb::send_hxg_fast_action(dev, action, args);
    if !scheduled.accepted {
        state.failures = state.failures.saturating_add(1);
        log_schedule_rejected(context_id, action, scheduled);
        return Err(GucSubmissionError::ScheduleRejected);
    }

    state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
    state.serial = state.serial.wrapping_add(1).max(1);
    let serial = state.serial;
    crate::log_trace!(
        target: "gpgpu";
        "intel/guc-submit: schedule enqueued=1 engine=rcs0 context_id={} token=0x{:X} serial={} action=0x{:04X} hxg=fast-request submission_owner=guc\n",
        context_id,
        token.raw(),
        serial,
        action
    );
    Ok(GucPhysicalSubmission {
        context: token,
        serial,
    })
}

/// Disable and unregister one GuC context.  If either GuC action fails the
/// slot remains live, preventing its ID from being unsafely reused.
pub(crate) fn destroy_rcs_context(
    dev: crate::intel::Dev,
    token: GucContextToken,
) -> Result<(), GucSubmissionError> {
    let mut state = RCS.lock();
    let (slot, generation) = token.parts().ok_or(GucSubmissionError::InvalidContext)?;
    let Some(context) = state.contexts.get(slot).copied() else {
        return Err(GucSubmissionError::InvalidContext);
    };
    if !context.registered || context.generation != generation {
        return Err(GucSubmissionError::InvalidContext);
    }
    let context_id = (slot + 1) as u32;
    if context.enabled {
        let disabled = crate::intel::guc_ctb::send_hxg_fast_action(
            dev,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET,
            &[context_id, GUC_CONTEXT_DISABLE],
        );
        if !disabled.accepted {
            state.failures = state.failures.saturating_add(1);
            return Err(GucSubmissionError::DisableRejected);
        }
        state.contexts[slot].enabled = false;
    }
    let deregistered = crate::intel::guc_ctb::send_hxg_action(
        dev,
        INTEL_GUC_ACTION_DEREGISTER_CONTEXT,
        &[context_id],
    );
    if !deregistered.accepted {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::DeregisterRejected);
    }

    let retained_generation = state.contexts[slot].generation;
    state.contexts[slot] = RcsContextState {
        generation: retained_generation,
        ..RcsContextState::EMPTY
    };
    state.deregistrations = state.deregistrations.saturating_add(1);
    crate::log!(
        "intel/guc-submit: deregister accepted=1 engine=rcs0 context_id={} token=0x{:X}\n",
        context_id,
        token.raw()
    );
    Ok(())
}

pub(crate) fn scheduler_status() -> GucSchedulerStatus {
    let state = RCS.lock();
    GucSchedulerStatus {
        capacity: MAX_GUC_RCS_CONTEXTS,
        registered: state
            .contexts
            .iter()
            .filter(|context| context.registered)
            .count(),
        enabled: state
            .contexts
            .iter()
            .filter(|context| context.enabled)
            .count(),
        submissions: state.serial,
        registrations: state.registrations,
        deregistrations: state.deregistrations,
        failures: state.failures,
    }
}

pub(crate) fn context_status() -> Vec<GucContextStatus> {
    let state = RCS.lock();
    state
        .contexts
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, context)| context.registered)
        .map(|(slot, context)| GucContextStatus {
            token: context.token(slot),
            context_id: (slot + 1) as u32,
            enabled: context.enabled,
            hwlrca_lo: context.hwlrca_lo,
            hwlrca_hi: context.hwlrca_hi,
            submissions: context.submissions,
        })
        .collect()
}

fn log_schedule_rejected(
    context_id: u32,
    action: u32,
    scheduled: crate::intel::guc_ctb::CtbSendResult,
) {
    crate::log!(
        "intel/guc-submit: schedule enqueued=0 engine=rcs0 context_id={} action=0x{:04X} response=0x{:08X} type={} error={} g2h_poll_iters={}\n",
        context_id,
        action,
        scheduled.response,
        scheduled.response_type,
        scheduled.error,
        scheduled.g2h_poll_iters
    );
}
