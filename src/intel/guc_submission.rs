//! GuC-owned physical scheduling for TRUEOS engine contexts.
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
const INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_DONE: u32 = 0x1002;
const INTEL_GUC_ACTION_CONTEXT_RESET_NOTIFICATION: u32 = 0x1008;
const INTEL_GUC_ACTION_ENGINE_FAILURE_NOTIFICATION: u32 = 0x1009;
const INTEL_GUC_ACTION_HOST2GUC_UPDATE_CONTEXT_POLICIES: u32 = 0x100B;
const INTEL_GUC_ACTION_REGISTER_CONTEXT: u32 = 0x4502;
const INTEL_GUC_ACTION_DEREGISTER_CONTEXT: u32 = 0x4503;
const INTEL_GUC_ACTION_DEREGISTER_CONTEXT_DONE: u32 = 0x4600;
const GUC_CONTEXT_DISABLE: u32 = 0;
const GUC_CONTEXT_ENABLE: u32 = 1;
const CONTEXT_REGISTRATION_FLAG_KMD: u32 = 1;
const GUC_RENDER_CLASS: u32 = 0;
const GUC_VIDEO_CLASS: u32 = 1;
const GUC_BLITTER_CLASS: u32 = 3;
const ENGINE_LOGICAL_INSTANCE_0_SUBMIT_MASK: u32 = 1;
const GUC_CONTEXT_POLICIES_KLV_ID_EXECUTION_QUANTUM: u32 = 0x2001;
const GUC_CONTEXT_POLICIES_KLV_ID_PREEMPTION_TIMEOUT: u32 = 0x2002;
const GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY: u32 = 0x2003;
const GUC_KLV_DWORD_LEN: u32 = 1;
const GUC_CONTEXT_EXECUTION_QUANTUM_US: u32 = 1_000;
const GUC_CONTEXT_PREEMPTION_TIMEOUT_US: u32 = 7_500_000;
const GUC_CLIENT_PRIORITY_KMD_HIGH: u32 = 0;
const GUC_CLIENT_PRIORITY_KMD_NORMAL: u32 = 2;
const GEN12_HW_CONTEXT_PRIORITY_SHIFT: u32 = 9;
const GEN12_HW_CONTEXT_PRIORITY_MASK: u32 = 0b11 << GEN12_HW_CONTEXT_PRIORITY_SHIFT;
const GEN12_HW_CONTEXT_PRIORITY_NORMAL: u32 = 0b01 << GEN12_HW_CONTEXT_PRIORITY_SHIFT;
const GEN12_HW_CONTEXT_PRIORITY_HIGH: u32 = 0b10 << GEN12_HW_CONTEXT_PRIORITY_SHIFT;
const MAX_GUC_CONTEXTS: usize = 32;
const GUC_LIFECYCLE_POLL_ITERS: usize = 8_192;

#[derive(Copy, Clone)]
struct GucEngineAbi {
    class: u32,
    submit_mask: u32,
    name: &'static str,
}

fn guc_engine_abi(
    dev: crate::intel::Dev,
    engine: crate::gpu::physical::PhysicalEngineId,
) -> Option<GucEngineAbi> {
    match (engine.class, engine.instance) {
        (crate::gpu::physical::EngineClass::RenderCompute, 0) => Some(GucEngineAbi {
            class: GUC_RENDER_CLASS,
            submit_mask: ENGINE_LOGICAL_INSTANCE_0_SUBMIT_MASK,
            name: "rcs0",
        }),
        (crate::gpu::physical::EngineClass::VideoDecode, physical @ (0 | 2)) => {
            let logical = crate::intel::media_vdbox_logical_instance(dev, physical)?;
            Some(GucEngineAbi {
                class: GUC_VIDEO_CLASS,
                submit_mask: 1u32.checked_shl(u32::from(logical))?,
                name: if physical == 0 { "vcs0" } else { "vcs2" },
            })
        }
        (crate::gpu::physical::EngineClass::Copy, 0) => Some(GucEngineAbi {
            class: GUC_BLITTER_CLASS,
            submit_mask: ENGINE_LOGICAL_INSTANCE_0_SUBMIT_MASK,
            name: "bcs0",
        }),
        _ => None,
    }
}

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
    PriorityConflict,
    PolicyEnqueueRejected,
    ScheduleRejected,
    DisableRejected,
    DisablePending,
    DeregisterRejected,
    DeregisterPending,
}

impl GucSubmissionError {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::TransportNotReady => "transport-not-ready",
            Self::ContextRegistryFull => "context-registry-full",
            Self::InvalidContext => "invalid-context",
            Self::RegisterRejected => "register-rejected",
            Self::PriorityConflict => "priority-conflict",
            Self::PolicyEnqueueRejected => "policy-enqueue-rejected",
            Self::ScheduleRejected => "schedule-rejected",
            Self::DisableRejected => "disable-rejected",
            Self::DisablePending => "disable-pending",
            Self::DeregisterRejected => "deregister-rejected",
            Self::DeregisterPending => "deregister-pending",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucPhysicalSubmission {
    pub(crate) context: GucContextToken,
    pub(crate) serial: u64,
    pub(crate) h2g_publish_sequence: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GucSchedulerStatus {
    pub(crate) capacity: usize,
    pub(crate) registered: usize,
    pub(crate) enabled: usize,
    pub(crate) pending_enable: usize,
    pub(crate) pending_disable: usize,
    pub(crate) pending_deregister: usize,
    pub(crate) submissions: u64,
    pub(crate) registrations: u64,
    pub(crate) deregistrations: u64,
    pub(crate) failures: u64,
    pub(crate) async_events: u64,
    pub(crate) async_event_errors: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucContextStatus {
    pub(crate) token: GucContextToken,
    pub(crate) context_id: u32,
    pub(crate) engine: crate::gpu::physical::PhysicalEngineId,
    pub(crate) priority: crate::gpu::physical::PhysicalContextPriority,
    pub(crate) policy_enqueued: bool,
    pub(crate) enabled: bool,
    pub(crate) pending_enable: bool,
    pub(crate) pending_disable: bool,
    pub(crate) pending_deregister: bool,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) submissions: u64,
}

#[derive(Copy, Clone)]
struct GucContextState {
    registered: bool,
    enabled: bool,
    pending_enable: bool,
    pending_disable: bool,
    pending_deregister: bool,
    faulted: bool,
    generation: u32,
    engine: crate::gpu::physical::PhysicalEngineId,
    priority: crate::gpu::physical::PhysicalContextPriority,
    policy_enqueued: bool,
    hwlrca_lo: u32,
    hwlrca_hi: u32,
    submissions: u64,
}

impl GucContextState {
    const EMPTY: Self = Self {
        registered: false,
        enabled: false,
        pending_enable: false,
        pending_disable: false,
        pending_deregister: false,
        faulted: false,
        generation: 0,
        engine: crate::gpu::physical::PhysicalEngineId::RCS0,
        priority: crate::gpu::physical::PhysicalContextPriority::KernelNormal,
        policy_enqueued: false,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
        submissions: 0,
    };

    fn token(self, slot: usize) -> GucContextToken {
        GucContextToken::new(slot, self.generation)
    }
}

struct GucSubmissionState {
    contexts: [GucContextState; MAX_GUC_CONTEXTS],
    serial: u64,
    registrations: u64,
    deregistrations: u64,
    failures: u64,
    async_events: u64,
    async_event_errors: u64,
}

static CONTEXTS: Mutex<GucSubmissionState> = Mutex::new(GucSubmissionState {
    contexts: [GucContextState::EMPTY; MAX_GUC_CONTEXTS],
    serial: 0,
    registrations: 0,
    deregistrations: 0,
    failures: 0,
    async_events: 0,
    async_event_errors: 0,
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
        engine: crate::gpu::physical::PhysicalEngineId,
        hwlrca_lo: u32,
        hwlrca_hi: u32,
        priority: crate::gpu::physical::PhysicalContextPriority,
    ) -> Result<GucContextToken, GucSubmissionError> {
        register_context(dev, engine, hwlrca_lo, hwlrca_hi, priority)
    }

    pub(crate) fn submit(
        &self,
        dev: crate::intel::Dev,
        token: GucContextToken,
    ) -> Result<GucPhysicalSubmission, GucSubmissionError> {
        submit_context(dev, token)
    }

    pub(crate) fn destroy(
        &self,
        dev: crate::intel::Dev,
        token: GucContextToken,
    ) -> Result<(), GucSubmissionError> {
        destroy_context(dev, token)
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
pub(crate) fn register_context(
    dev: crate::intel::Dev,
    engine: crate::gpu::physical::PhysicalEngineId,
    hwlrca_lo: u32,
    hwlrca_hi: u32,
    priority: crate::gpu::physical::PhysicalContextPriority,
) -> Result<GucContextToken, GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let engine_abi = guc_engine_abi(dev, engine).ok_or(GucSubmissionError::InvalidContext)?;
    let hwlrca_lo = guc_hwlrca_descriptor(engine, priority, hwlrca_lo);
    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    if let Some((slot, context)) =
        state
            .contexts
            .iter()
            .copied()
            .enumerate()
            .find(|(_, context)| {
                context.registered
                    && context.engine == engine
                    && context.hwlrca_lo == hwlrca_lo
                    && context.hwlrca_hi == hwlrca_hi
            })
    {
        if context.pending_disable || context.pending_deregister || context.faulted {
            state.failures = state.failures.saturating_add(1);
            return Err(GucSubmissionError::InvalidContext);
        }
        if context.priority != priority {
            state.failures = state.failures.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: context-policy conflict=1 engine={} context_id={} current_priority={} requested_priority={} action=reject-priority-mutation\n",
                engine_abi.name,
                context_id(slot),
                guc_priority_name(context.priority),
                guc_priority_name(priority),
            );
            return Err(GucSubmissionError::PriorityConflict);
        }
        if context.policy_enqueued {
            return Ok(context.token(slot));
        }
        if !program_context_priority(dev, context_id(slot), engine_abi, priority) {
            state.failures = state.failures.saturating_add(1);
            return Err(GucSubmissionError::PolicyEnqueueRejected);
        }
        state.contexts[slot].policy_enqueued = true;
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
    let register = crate::intel::guc_ctb::send_hxg_fast_action(
        dev,
        INTEL_GUC_ACTION_REGISTER_CONTEXT,
        &[
            CONTEXT_REGISTRATION_FLAG_KMD,
            context_id,
            engine_abi.class,
            engine_abi.submit_mask,
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
            "intel/guc-submit: register accepted=0 engine={} context_id={} class={} submit_mask=0x{:X} hwlrca=0x{:08X}:0x{:08X} response=0x{:08X} type={} error={} g2h_poll_iters={}\n",
            engine_abi.name,
            context_id,
            engine_abi.class,
            engine_abi.submit_mask,
            hwlrca_hi,
            hwlrca_lo,
            register.response,
            register.response_type,
            register.error,
            register.g2h_poll_iters
        );
        return Err(GucSubmissionError::RegisterRejected);
    }

    state.contexts[slot] = GucContextState {
        registered: true,
        enabled: false,
        pending_enable: false,
        pending_disable: false,
        pending_deregister: false,
        faulted: false,
        generation,
        engine,
        priority,
        policy_enqueued: false,
        hwlrca_lo,
        hwlrca_hi,
        submissions: 0,
    };
    state.registrations = state.registrations.saturating_add(1);
    let token = GucContextToken::new(slot, generation);
    if !program_context_priority(dev, context_id, engine_abi, priority) {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::PolicyEnqueueRejected);
    }
    state.contexts[slot].policy_enqueued = true;
    crate::log!(
        "intel/guc-submit: register enqueued=1 engine={} context_id={} token=0x{:X} class={} submit_mask=0x{:X} hwlrca=0x{:08X}:0x{:08X} abi=v1 single_lrc=1 priority={} priority_abi={} hwlrca_priority_bits=0x{:03X} policy_enqueued=1 hxg=fast-request\n",
        engine_abi.name,
        context_id,
        token.raw(),
        engine_abi.class,
        engine_abi.submit_mask,
        hwlrca_hi,
        hwlrca_lo,
        guc_priority_name(priority),
        guc_priority_abi(priority),
        hwlrca_lo & GEN12_HW_CONTEXT_PRIORITY_MASK,
    );
    Ok(token)
}

/// Notify GuC that a registered LRC has a new ring tail.
///
/// The caller has already written/flushed the LRC and ring.  GuC acceptance is
/// a physical admission serial, not GPU completion; the broker completes its
/// virtual timeline only when the caller's marker/fence retires.
pub(crate) fn submit_context(
    dev: crate::intel::Dev,
    token: GucContextToken,
) -> Result<GucPhysicalSubmission, GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    let (slot, generation) = token.parts().ok_or(GucSubmissionError::InvalidContext)?;
    let Some(context) = state.contexts.get(slot).copied() else {
        return Err(GucSubmissionError::InvalidContext);
    };
    if !context.registered || context.generation != generation {
        return Err(GucSubmissionError::InvalidContext);
    }
    if context.pending_disable || context.pending_deregister || context.faulted {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::InvalidContext);
    }
    if !context.policy_enqueued {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::PolicyEnqueueRejected);
    }
    let engine_abi =
        guc_engine_abi(dev, context.engine).ok_or(GucSubmissionError::InvalidContext)?;
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
            log_schedule_rejected(
                engine_abi.name,
                context_id,
                INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET,
                scheduled,
            );
            return Err(GucSubmissionError::ScheduleRejected);
        }
        state.contexts[slot].enabled = true;
        state.contexts[slot].pending_enable = true;
        state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
        state.serial = state.serial.wrapping_add(1).max(1);
        let serial = state.serial;
        crate::log_trace!(
            target: "gpgpu";
            "intel/guc-submit: schedule enqueued=1 engine={} context_id={} token=0x{:X} serial={} action=0x{:04X} hxg=fast-request pending_enable=1 completion_event=sched-context-mode-done submission_owner=guc\n",
            engine_abi.name,
            context_id,
            token.raw(),
            serial,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET
        );
        return Ok(GucPhysicalSubmission {
            context: token,
            serial,
            h2g_publish_sequence: scheduled.h2g_publish_sequence,
        });
    };
    let scheduled = crate::intel::guc_ctb::send_hxg_fast_action(dev, action, args);
    if !scheduled.accepted {
        state.failures = state.failures.saturating_add(1);
        log_schedule_rejected(engine_abi.name, context_id, action, scheduled);
        return Err(GucSubmissionError::ScheduleRejected);
    }

    state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
    state.serial = state.serial.wrapping_add(1).max(1);
    let serial = state.serial;
    crate::log_trace!(
        target: "gpgpu";
        "intel/guc-submit: schedule enqueued=1 engine={} context_id={} token=0x{:X} serial={} action=0x{:04X} hxg=fast-request submission_owner=guc\n",
        engine_abi.name,
        context_id,
        token.raw(),
        serial,
        action
    );
    Ok(GucPhysicalSubmission {
        context: token,
        serial,
        h2g_publish_sequence: scheduled.h2g_publish_sequence,
    })
}

/// Disable and unregister one GuC context.
///
/// Both mode changes and deregistration complete through asynchronous G2H
/// events. The registry slot remains live (and therefore quarantines the GuC
/// ID and caller-owned backing) until the matching completion event is
/// observed. A timeout is reported as pending, never treated as completion.
pub(crate) fn destroy_context(
    dev: crate::intel::Dev,
    token: GucContextToken,
) -> Result<(), GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    let (slot, generation) = token.parts().ok_or(GucSubmissionError::InvalidContext)?;
    let Some(context) = state.contexts.get(slot).copied() else {
        return Err(GucSubmissionError::InvalidContext);
    };
    if context.generation != generation {
        return Err(GucSubmissionError::InvalidContext);
    }
    // A matching token can be destroyed repeatedly after its completion. This
    // is required when a previous call returned pending and its G2H arrived
    // before the caller retried.
    if !context.registered {
        return Ok(());
    }
    let engine_abi =
        guc_engine_abi(dev, context.engine).ok_or(GucSubmissionError::InvalidContext)?;
    let context_id = (slot + 1) as u32;

    if state.contexts[slot].pending_deregister {
        if wait_for_transition(&mut state, slot, GucPendingTransition::Deregister) {
            return Ok(());
        }
        note_lifecycle_timeout(
            &mut state,
            engine_abi.name,
            context_id,
            "deregister-done",
        );
        return Err(GucSubmissionError::DeregisterPending);
    }

    if state.contexts[slot].pending_enable
        && !wait_for_transition(&mut state, slot, GucPendingTransition::Enable)
    {
        note_lifecycle_timeout(&mut state, engine_abi.name, context_id, "enable-done");
        return Err(GucSubmissionError::DisablePending);
    }

    if state.contexts[slot].enabled {
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
        state.contexts[slot].pending_disable = true;
        crate::log_trace!(
            target: "gpgpu";
            "intel/guc-submit: disable enqueued=1 engine={} context_id={} token=0x{:X} pending_disable=1 completion_event=sched-context-mode-done hxg=fast-request\n",
            engine_abi.name,
            context_id,
            token.raw(),
        );
    }
    if state.contexts[slot].pending_disable
        && !wait_for_transition(&mut state, slot, GucPendingTransition::Disable)
    {
        note_lifecycle_timeout(&mut state, engine_abi.name, context_id, "disable-done");
        return Err(GucSubmissionError::DisablePending);
    }

    let deregistered = crate::intel::guc_ctb::send_hxg_fast_action(
        dev,
        INTEL_GUC_ACTION_DEREGISTER_CONTEXT,
        &[context_id],
    );
    if !deregistered.accepted {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::DeregisterRejected);
    }
    state.contexts[slot].pending_deregister = true;
    crate::log!(
        "intel/guc-submit: deregister enqueued=1 engine={} context_id={} token=0x{:X} pending_deregister=1 completion_event=deregister-context-done hxg=fast-request\n",
        engine_abi.name,
        context_id,
        token.raw()
    );
    if wait_for_transition(&mut state, slot, GucPendingTransition::Deregister) {
        Ok(())
    } else {
        note_lifecycle_timeout(
            &mut state,
            engine_abi.name,
            context_id,
            "deregister-done",
        );
        Err(GucSubmissionError::DeregisterPending)
    }
}

#[derive(Copy, Clone)]
enum GucPendingTransition {
    Enable,
    Disable,
    Deregister,
}

fn wait_for_transition(
    state: &mut GucSubmissionState,
    slot: usize,
    transition: GucPendingTransition,
) -> bool {
    for _ in 0..GUC_LIFECYCLE_POLL_ITERS {
        drain_g2h_events(state);
        let Some(context) = state.contexts.get(slot) else {
            return false;
        };
        let pending = match transition {
            GucPendingTransition::Enable => context.pending_enable,
            GucPendingTransition::Disable => context.pending_disable,
            GucPendingTransition::Deregister => context.pending_deregister,
        };
        if !pending {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn note_lifecycle_timeout(
    state: &mut GucSubmissionState,
    engine: &'static str,
    context_id: u32,
    transition: &'static str,
) {
    state.failures = state.failures.saturating_add(1);
    state.async_event_errors = state.async_event_errors.saturating_add(1);
    crate::log_warn!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle pending=1 engine={} context_id={} transition={} action=retain-context-id-and-backing\n",
        engine,
        context_id,
        transition,
    );
}

fn drain_g2h_events(state: &mut GucSubmissionState) {
    let result = crate::intel::guc_ctb::poll_g2h_events(|event| {
        process_g2h_event(state, event);
    });
    state.async_events = state.async_events.saturating_add(result.events as u64);
    let transport_errors = result
        .malformed_messages
        .saturating_add(result.dropped_events)
        .saturating_add(result.unsolicited_responses);
    if transport_errors != 0 {
        state.async_event_errors = state.async_event_errors.saturating_add(transport_errors);
        state.failures = state.failures.saturating_add(transport_errors);
        crate::log_error!(
            target: "gpgpu";
            "intel/guc-submit: g2h transport_error=1 malformed={} dropped_events={} unsolicited_responses={} action=retain-pending-contexts\n",
            result.malformed_messages,
            result.dropped_events,
            result.unsolicited_responses,
        );
    }
    if !context_registry_invariants_hold(state) {
        state.async_event_errors = state.async_event_errors.saturating_add(1);
        state.failures = state.failures.saturating_add(1);
        crate::log_error!(
            target: "gpgpu";
            "intel/guc-submit: lifecycle invariant=0 action=quarantine-registry\n",
        );
    }
}

fn process_g2h_event(
    state: &mut GucSubmissionState,
    event: crate::intel::guc_ctb::CtbG2hEvent,
) {
    match event.action {
        INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_DONE => {
            let (Some(context_id), Some(mode_status)) = (event.payload(0), event.payload(1)) else {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done-context-id");
                return;
            };
            let context = state.contexts[slot];
            if !context.registered {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done-unregistered");
                return;
            }
            let transition = if context.pending_enable {
                state.contexts[slot].pending_enable = false;
                "enable"
            } else if context.pending_disable {
                state.contexts[slot].pending_disable = false;
                "disable"
            } else {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done-unexpected");
                return;
            };
            crate::log_trace!(
                target: "gpgpu";
                "intel/guc-submit: lifecycle complete=1 context_id={} transition={} mode_status=0x{:08X} action=0x{:04X}\n",
                context_id,
                transition,
                mode_status,
                event.action,
            );
        }
        INTEL_GUC_ACTION_DEREGISTER_CONTEXT_DONE => {
            let Some(context_id) = event.payload(0) else {
                note_malformed_lifecycle_event(state, event, "deregister-context-done");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "deregister-context-done-context-id");
                return;
            };
            let context = state.contexts[slot];
            if !context.registered || !context.pending_deregister {
                note_malformed_lifecycle_event(state, event, "deregister-context-done-unexpected");
                return;
            }
            let token = context.token(slot);
            let generation = context.generation;
            let engine = context.engine;
            state.contexts[slot] = GucContextState {
                generation,
                ..GucContextState::EMPTY
            };
            state.deregistrations = state.deregistrations.saturating_add(1);
            crate::log!(
                "intel/guc-submit: deregister complete=1 engine={:?} context_id={} token=0x{:X} action=0x{:04X} id_reusable=1 backing_release_safe=1\n",
                engine,
                context_id,
                token.raw(),
                event.action,
            );
        }
        INTEL_GUC_ACTION_CONTEXT_RESET_NOTIFICATION => {
            let Some(context_id) = event.payload(0) else {
                note_malformed_lifecycle_event(state, event, "context-reset");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "context-reset-context-id");
                return;
            };
            if !state.contexts[slot].registered {
                note_malformed_lifecycle_event(state, event, "context-reset-unregistered");
                return;
            }
            state.contexts[slot].faulted = true;
            state.failures = state.failures.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: context-reset=1 context_id={} action=quarantine-context\n",
                context_id,
            );
        }
        INTEL_GUC_ACTION_ENGINE_FAILURE_NOTIFICATION => {
            state.failures = state.failures.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: engine-failure=1 class=0x{:X} instance=0x{:X} reason=0x{:08X} payload_len={} truncated={}\n",
                event.payload(0).unwrap_or(u32::MAX),
                event.payload(1).unwrap_or(u32::MAX),
                event.payload(2).unwrap_or(u32::MAX),
                event.payload_len,
                event.truncated() as u8,
            );
        }
        _ => {
            state.async_event_errors = state.async_event_errors.saturating_add(1);
            crate::log_warn!(
                target: "gpgpu";
                "intel/guc-submit: g2h event=unhandled action=0x{:04X} payload_len={} truncated={} payload0=0x{:08X}\n",
                event.action,
                event.payload_len,
                event.truncated() as u8,
                event.payload(0).unwrap_or(0),
            );
        }
    }
}

fn note_malformed_lifecycle_event(
    state: &mut GucSubmissionState,
    event: crate::intel::guc_ctb::CtbG2hEvent,
    reason: &'static str,
) {
    state.async_event_errors = state.async_event_errors.saturating_add(1);
    state.failures = state.failures.saturating_add(1);
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle event_valid=0 action=0x{:04X} payload_len={} reason={} action_on_error=retain-context-id-and-backing\n",
        event.action,
        event.payload_len,
        reason,
    );
}

const fn context_slot(context_id: u32) -> Option<usize> {
    if context_id == 0 || context_id as usize > MAX_GUC_CONTEXTS {
        None
    } else {
        Some(context_id as usize - 1)
    }
}

fn context_registry_invariants_hold(state: &GucSubmissionState) -> bool {
    state.contexts.iter().all(|context| {
        let mutually_exclusive = !(context.pending_enable && context.pending_disable)
            && !(context.pending_deregister
                && (context.pending_enable || context.pending_disable || context.enabled));
        let empty_is_inert = context.registered
            || (!context.enabled
                && !context.pending_enable
                && !context.pending_disable
                && !context.pending_deregister);
        mutually_exclusive && empty_is_inert
    })
}

pub(crate) fn scheduler_status() -> GucSchedulerStatus {
    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    GucSchedulerStatus {
        capacity: MAX_GUC_CONTEXTS,
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
        pending_enable: state
            .contexts
            .iter()
            .filter(|context| context.pending_enable)
            .count(),
        pending_disable: state
            .contexts
            .iter()
            .filter(|context| context.pending_disable)
            .count(),
        pending_deregister: state
            .contexts
            .iter()
            .filter(|context| context.pending_deregister)
            .count(),
        submissions: state.serial,
        registrations: state.registrations,
        deregistrations: state.deregistrations,
        failures: state.failures,
        async_events: state.async_events,
        async_event_errors: state.async_event_errors,
    }
}

pub(crate) fn context_status() -> Vec<GucContextStatus> {
    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    state
        .contexts
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, context)| context.registered)
        .map(|(slot, context)| GucContextStatus {
            token: context.token(slot),
            context_id: (slot + 1) as u32,
            engine: context.engine,
            priority: context.priority,
            policy_enqueued: context.policy_enqueued,
            enabled: context.enabled,
            pending_enable: context.pending_enable,
            pending_disable: context.pending_disable,
            pending_deregister: context.pending_deregister,
            hwlrca_lo: context.hwlrca_lo,
            hwlrca_hi: context.hwlrca_hi,
            submissions: context.submissions,
        })
        .collect()
}

const fn context_id(slot: usize) -> u32 {
    (slot + 1) as u32
}

const fn guc_priority_abi(priority: crate::gpu::physical::PhysicalContextPriority) -> u32 {
    match priority {
        crate::gpu::physical::PhysicalContextPriority::KernelHigh => GUC_CLIENT_PRIORITY_KMD_HIGH,
        crate::gpu::physical::PhysicalContextPriority::KernelNormal => {
            GUC_CLIENT_PRIORITY_KMD_NORMAL
        }
    }
}

const fn guc_priority_name(
    priority: crate::gpu::physical::PhysicalContextPriority,
) -> &'static str {
    match priority {
        crate::gpu::physical::PhysicalContextPriority::KernelHigh => "kmd-high",
        crate::gpu::physical::PhysicalContextPriority::KernelNormal => "kmd-normal",
    }
}

const fn context_priority_policy_args(
    context_id: u32,
    priority: crate::gpu::physical::PhysicalContextPriority,
) -> [u32; 7] {
    [
        context_id,
        (GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY << 16) | GUC_KLV_DWORD_LEN,
        guc_priority_abi(priority),
        (GUC_CONTEXT_POLICIES_KLV_ID_EXECUTION_QUANTUM << 16) | GUC_KLV_DWORD_LEN,
        GUC_CONTEXT_EXECUTION_QUANTUM_US,
        (GUC_CONTEXT_POLICIES_KLV_ID_PREEMPTION_TIMEOUT << 16) | GUC_KLV_DWORD_LEN,
        GUC_CONTEXT_PREEMPTION_TIMEOUT_US,
    ]
}

const _: () = {
    use crate::gpu::physical::PhysicalContextPriority::{KernelHigh, KernelNormal};

    let high = context_priority_policy_args(7, KernelHigh);
    let normal = context_priority_policy_args(7, KernelNormal);
    assert!(guc_priority_abi(KernelHigh) == 0);
    assert!(guc_priority_abi(KernelNormal) == 2);
    assert!(high[0] == 7);
    assert!(high[1] == 0x2003_0001);
    assert!(high[2] == GUC_CLIENT_PRIORITY_KMD_HIGH);
    assert!(high[3] == 0x2001_0001);
    assert!(high[4] == 1_000);
    assert!(high[5] == 0x2002_0001);
    assert!(high[6] == 7_500_000);
    assert!(normal[0] == 7);
    assert!(normal[1] == 0x2003_0001);
    assert!(normal[2] == GUC_CLIENT_PRIORITY_KMD_NORMAL);
    assert!(normal[3] == high[3]);
    assert!(normal[4] == high[4]);
    assert!(normal[5] == high[5]);
    assert!(normal[6] == high[6]);
};

fn program_context_priority(
    dev: crate::intel::Dev,
    context_id: u32,
    engine_abi: GucEngineAbi,
    priority: crate::gpu::physical::PhysicalContextPriority,
) -> bool {
    let priority_abi = guc_priority_abi(priority);
    let policy = crate::intel::guc_ctb::send_hxg_fast_action(
        dev,
        INTEL_GUC_ACTION_HOST2GUC_UPDATE_CONTEXT_POLICIES,
        &context_priority_policy_args(context_id, priority),
    );
    if policy.accepted {
        crate::log_trace!(
            target: "gpgpu";
            "intel/guc-submit: context-policy enqueued=1 engine={} context_id={} priority={} priority_abi={} action=0x{:04X} klv=0x{:04X} request=hxg-fast h2g_publish_sequence={} error={} execution_quantum_us={} preemption_timeout_us={} policy_klvs=0x{:04X},0x{:04X},0x{:04X}\n",
            engine_abi.name,
            context_id,
            guc_priority_name(priority),
            priority_abi,
            INTEL_GUC_ACTION_HOST2GUC_UPDATE_CONTEXT_POLICIES,
            GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY,
            policy.h2g_publish_sequence,
            policy.error,
            GUC_CONTEXT_EXECUTION_QUANTUM_US,
            GUC_CONTEXT_PREEMPTION_TIMEOUT_US,
            GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY,
            GUC_CONTEXT_POLICIES_KLV_ID_EXECUTION_QUANTUM,
            GUC_CONTEXT_POLICIES_KLV_ID_PREEMPTION_TIMEOUT,
        );
    } else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/guc-submit: context-policy enqueued=0 engine={} context_id={} priority={} priority_abi={} action=0x{:04X} klv=0x{:04X} request=hxg-fast h2g_publish_sequence={} error={} execution_quantum_us={} preemption_timeout_us={} policy_klvs=0x{:04X},0x{:04X},0x{:04X}\n",
            engine_abi.name,
            context_id,
            guc_priority_name(priority),
            priority_abi,
            INTEL_GUC_ACTION_HOST2GUC_UPDATE_CONTEXT_POLICIES,
            GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY,
            policy.h2g_publish_sequence,
            policy.error,
            GUC_CONTEXT_EXECUTION_QUANTUM_US,
            GUC_CONTEXT_PREEMPTION_TIMEOUT_US,
            GUC_CONTEXT_POLICIES_KLV_ID_SCHEDULING_PRIORITY,
            GUC_CONTEXT_POLICIES_KLV_ID_EXECUTION_QUANTUM,
            GUC_CONTEXT_POLICIES_KLV_ID_PREEMPTION_TIMEOUT,
        );
    }
    policy.accepted
}

fn log_schedule_rejected(
    engine: &'static str,
    context_id: u32,
    action: u32,
    scheduled: crate::intel::guc_ctb::CtbSendResult,
) {
    crate::log!(
        "intel/guc-submit: schedule enqueued=0 engine={} context_id={} action=0x{:04X} response=0x{:08X} type={} error={} g2h_poll_iters={}\n",
        engine,
        context_id,
        action,
        scheduled.response,
        scheduled.response_type,
        scheduled.error,
        scheduled.g2h_poll_iters
    );
}
