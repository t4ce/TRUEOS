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
const INTEL_GUC_ACTION_MEMORY_CAT_ERROR: u32 = 0x6000;
const GUC_ID_UNKNOWN: u32 = u32::MAX;
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
const HWLRCA_PAGE_MASK: u32 = !0xFFF;
const REPORT_ENABLE_TIMEOUT: u8 = 1 << 0;
const REPORT_DISABLE_TIMEOUT: u8 = 1 << 1;
const REPORT_DEREGISTER_TIMEOUT: u8 = 1 << 2;
const REPORT_DISABLE_REJECTED: u8 = 1 << 0;
const REPORT_DEREGISTER_REJECTED: u8 = 1 << 1;

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
    DeviceFaulted,
    ContextRegistryFull,
    InvalidContext,
    OwnershipConflict,
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
            Self::DeviceFaulted => "device-faulted",
            Self::ContextRegistryFull => "context-registry-full",
            Self::InvalidContext => "invalid-context",
            Self::OwnershipConflict => "ownership-conflict",
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

/// Immutable ownership origin for one GuC registration generation. This tag
/// prevents an idempotent HWLRCA lookup from silently mixing direct backend
/// contexts with contexts owned by the mediated vGPU broker.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GucContextOrigin {
    Direct,
    Mediated,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GucContextFaultKind {
    MemoryCat,
    ContextReset,
    LifecycleProtocol,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GucSchedulerStatus {
    pub(crate) capacity: usize,
    pub(crate) registered: usize,
    pub(crate) enabled: usize,
    pub(crate) destroy_requested: usize,
    pub(crate) pending_enable: usize,
    pub(crate) pending_disable: usize,
    pub(crate) pending_deregister: usize,
    pub(crate) faulted: usize,
    pub(crate) owner_handoffs_pending: usize,
    pub(crate) submissions: u64,
    pub(crate) registrations: u64,
    pub(crate) deregistrations: u64,
    pub(crate) failures: u64,
    pub(crate) async_events: u64,
    pub(crate) async_event_errors: u64,
    pub(crate) memory_cat_faults: u64,
    pub(crate) unattributed_faults: u64,
    pub(crate) lifecycle_timeouts: u64,
    pub(crate) lifecycle_retries: u64,
    pub(crate) gt_faulted: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucContextStatus {
    pub(crate) token: GucContextToken,
    pub(crate) context_id: u32,
    pub(crate) engine: crate::gpu::physical::PhysicalEngineId,
    pub(crate) priority: crate::gpu::physical::PhysicalContextPriority,
    pub(crate) origin: GucContextOrigin,
    pub(crate) policy_enqueued: bool,
    pub(crate) enabled: bool,
    pub(crate) destroy_requested: bool,
    pub(crate) pending_enable: bool,
    pub(crate) pending_disable: bool,
    pub(crate) pending_deregister: bool,
    pub(crate) faulted: bool,
    pub(crate) owner_handoff_pending: bool,
    pub(crate) fault_kind: Option<GucContextFaultKind>,
    pub(crate) cat_hw_type: Option<u32>,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) submissions: u64,
}

#[derive(Copy, Clone)]
struct GucContextState {
    registered: bool,
    enabled: bool,
    /// The owning vGPU has begun teardown. Keep the ID, HWLRCA, and all
    /// backing quarantined and reject new work until DEREGISTER_CONTEXT_DONE.
    destroy_requested: bool,
    pending_enable: bool,
    pending_disable: bool,
    pending_deregister: bool,
    /// An exactly attributed CAT/reset must remove this context from GuC's
    /// runnable set even though its registration and backing stay pinned.
    /// Cleared only by the matching DISABLE mode-done event.
    fault_disable_required: bool,
    faulted: bool,
    owner_fault_reported: bool,
    owner_handoff_pending: bool,
    fault_kind: Option<GucContextFaultKind>,
    cat_hw_type: Option<u32>,
    lifecycle_timeout_reported: u8,
    lifecycle_reject_reported: u8,
    generation: u32,
    origin: GucContextOrigin,
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
        destroy_requested: false,
        pending_enable: false,
        pending_disable: false,
        pending_deregister: false,
        fault_disable_required: false,
        faulted: false,
        owner_fault_reported: false,
        owner_handoff_pending: false,
        fault_kind: None,
        cat_hw_type: None,
        lifecycle_timeout_reported: 0,
        lifecycle_reject_reported: 0,
        generation: 0,
        origin: GucContextOrigin::Direct,
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
    memory_cat_faults: u64,
    unattributed_faults: u64,
    lifecycle_timeouts: u64,
    lifecycle_retries: u64,
    gt_faulted: bool,
}

static CONTEXTS: Mutex<GucSubmissionState> = Mutex::new(GucSubmissionState {
    contexts: [GucContextState::EMPTY; MAX_GUC_CONTEXTS],
    serial: 0,
    registrations: 0,
    deregistrations: 0,
    failures: 0,
    async_events: 0,
    async_event_errors: 0,
    memory_cat_faults: 0,
    unattributed_faults: 0,
    lifecycle_timeouts: 0,
    lifecycle_retries: 0,
    gt_faulted: false,
});

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucContextFault {
    pub(crate) token: GucContextToken,
    pub(crate) engine: crate::gpu::physical::PhysicalEngineId,
    pub(crate) origin: GucContextOrigin,
    pub(crate) kind: GucContextFaultKind,
    /// Optional firmware telemetry. The value is hardware-defined and is not
    /// an engine selector or permission to program a reset register.
    pub(crate) hw_type: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucFaultSnapshot {
    pub(crate) gt_faulted: bool,
    pub(crate) unattributed_faults: u64,
    pub(crate) contexts: Vec<GucContextFault>,
}

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
        register_context(dev, engine, hwlrca_lo, hwlrca_hi, priority, GucContextOrigin::Direct)
    }

    pub(crate) fn register_mediated(
        &self,
        dev: crate::intel::Dev,
        engine: crate::gpu::physical::PhysicalEngineId,
        hwlrca_lo: u32,
        hwlrca_hi: u32,
        priority: crate::gpu::physical::PhysicalContextPriority,
    ) -> Result<GucContextToken, GucSubmissionError> {
        register_context(dev, engine, hwlrca_lo, hwlrca_hi, priority, GucContextOrigin::Mediated)
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

    pub(crate) fn fault_snapshot(&self) -> GucFaultSnapshot {
        fault_snapshot()
    }

    pub(crate) fn acknowledge_fault(&self, token: GucContextToken) -> bool {
        acknowledge_fault(token)
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
    origin: GucContextOrigin,
) -> Result<GucContextToken, GucSubmissionError> {
    if !ready() {
        return Err(GucSubmissionError::TransportNotReady);
    }

    let engine_abi = guc_engine_abi(dev, engine).ok_or(GucSubmissionError::InvalidContext)?;
    let hwlrca_lo = guc_hwlrca_descriptor(engine, priority, hwlrca_lo);
    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    if state.gt_faulted {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    if let Some((slot, context)) =
        state
            .contexts
            .iter()
            .copied()
            .enumerate()
            .find(|(_, context)| {
                context.registered
                    && hwlrca_backing_page(context.hwlrca_lo) == hwlrca_backing_page(hwlrca_lo)
                    && context.hwlrca_hi == hwlrca_hi
            })
    {
        if context.destroy_requested
            || context.pending_disable
            || context.pending_deregister
            || context.faulted
        {
            state.failures = state.failures.saturating_add(1);
            return Err(GucSubmissionError::InvalidContext);
        }
        if context.engine != engine {
            state.failures = state.failures.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: context-owner conflict=1 context_id={} current_engine={:?}:{} requested_engine={:?}:{} hwlrca_page=0x{:08X}:0x{:08X} action=reject-cross-engine-hwlrca-alias\n",
                context_id(slot),
                context.engine.class,
                context.engine.instance,
                engine.class,
                engine.instance,
                hwlrca_hi,
                hwlrca_backing_page(hwlrca_lo),
            );
            return Err(GucSubmissionError::OwnershipConflict);
        }
        if context.origin != origin {
            state.failures = state.failures.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: context-owner conflict=1 engine={} context_id={} current_origin={:?} requested_origin={:?} action=reject-cross-origin-hwlrca-alias\n",
                engine_abi.name,
                context_id(slot),
                context.origin,
                origin,
            );
            return Err(GucSubmissionError::OwnershipConflict);
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
        let token = context.token(slot);
        drain_g2h_events(&mut state);
        if state.gt_faulted {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        if state.contexts[slot].faulted {
            return Ok(token);
        }
        if !program_context_priority(dev, context_id(slot), engine_abi, priority) {
            state.failures = state.failures.saturating_add(1);
            // The registration predates this policy retry. Preserve its token
            // as ownership evidence; submit will reject until policy succeeds.
            return Ok(token);
        }
        state.contexts[slot].policy_enqueued = true;
        drain_g2h_events(&mut state);
        if state.gt_faulted {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        return Ok(token);
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
    drain_g2h_events(&mut state);
    if state.gt_faulted {
        return Err(GucSubmissionError::DeviceFaulted);
    }
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
        destroy_requested: false,
        pending_enable: false,
        pending_disable: false,
        pending_deregister: false,
        fault_disable_required: false,
        faulted: false,
        owner_fault_reported: false,
        owner_handoff_pending: false,
        fault_kind: None,
        cat_hw_type: None,
        lifecycle_timeout_reported: 0,
        lifecycle_reject_reported: 0,
        generation,
        origin,
        engine,
        priority,
        policy_enqueued: false,
        hwlrca_lo,
        hwlrca_hi,
        submissions: 0,
    };
    state.registrations = state.registrations.saturating_add(1);
    let token = GucContextToken::new(slot, generation);
    // REGISTER's transport wait may have queued G2H events. Install the slot
    // first so an exact CAT can be attributed, then drain and refuse the
    // follow-up policy H2G if that registration or the whole GT faulted.
    drain_g2h_events(&mut state);
    if state.gt_faulted {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    if state.contexts[slot].faulted {
        // REGISTER was accepted, so the caller owns this generation even when
        // firmware immediately faulted it. Return the token as ownership
        // evidence; submit remains blocked and the broker will synchronously
        // map the pending exact fault to this one owner.
        return Ok(token);
    }
    if !program_context_priority(dev, context_id, engine_abi, priority) {
        state.failures = state.failures.saturating_add(1);
        // REGISTER is already accepted. Returning its token keeps ownership
        // representable even though submit remains fenced by policy_enqueued.
        return Ok(token);
    }
    state.contexts[slot].policy_enqueued = true;
    drain_g2h_events(&mut state);
    if state.gt_faulted {
        return Err(GucSubmissionError::DeviceFaulted);
    }
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
    if state.gt_faulted {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    let (slot, generation) = token.parts().ok_or(GucSubmissionError::InvalidContext)?;
    let Some(context) = state.contexts.get(slot).copied() else {
        return Err(GucSubmissionError::InvalidContext);
    };
    if !context.registered || context.generation != generation {
        return Err(GucSubmissionError::InvalidContext);
    }
    if context.destroy_requested
        || context.pending_disable
        || context.pending_deregister
        || context.faulted
    {
        state.failures = state.failures.saturating_add(1);
        return Err(GucSubmissionError::InvalidContext);
    }
    let engine_abi =
        guc_engine_abi(dev, context.engine).ok_or(GucSubmissionError::InvalidContext)?;
    if !context.policy_enqueued {
        // REGISTER may have succeeded while the immediately following policy
        // H2G was transiently rejected. The broker already owns this exact
        // generation, so retry only the missing idempotent policy operation at
        // the next submission boundary instead of stranding the context.
        drain_g2h_events(&mut state);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        if !program_context_priority(dev, context_id(slot), engine_abi, context.priority) {
            state.failures = state.failures.saturating_add(1);
            drain_g2h_events(&mut state);
            if fault_requires_retention(&state, slot) {
                return Err(GucSubmissionError::DeviceFaulted);
            }
            return Err(GucSubmissionError::PolicyEnqueueRejected);
        }
        state.contexts[slot].policy_enqueued = true;
        drain_g2h_events(&mut state);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
    }
    let context_id = (slot + 1) as u32;
    // Revalidate immediately at the CTB publication boundary. The post-send
    // drain below catches events ingested while the request was in flight.
    drain_g2h_events(&mut state);
    if fault_requires_retention(&state, slot) {
        return Err(GucSubmissionError::DeviceFaulted);
    }
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
            drain_g2h_events(&mut state);
            if fault_requires_retention(&state, slot) {
                return Err(GucSubmissionError::DeviceFaulted);
            }
            return Err(GucSubmissionError::ScheduleRejected);
        }
        state.contexts[slot].enabled = true;
        state.contexts[slot].pending_enable = true;
        state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
        state.serial = state.serial.wrapping_add(1).max(1);
        let serial = state.serial;
        drain_g2h_events(&mut state);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
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
        drain_g2h_events(&mut state);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        return Err(GucSubmissionError::ScheduleRejected);
    }

    state.contexts[slot].submissions = state.contexts[slot].submissions.saturating_add(1);
    state.serial = state.serial.wrapping_add(1).max(1);
    let serial = state.serial;
    drain_g2h_events(&mut state);
    if fault_requires_retention(&state, slot) {
        return Err(GucSubmissionError::DeviceFaulted);
    }
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
        // Allocation of a newer generation is only possible after GuC sent
        // DEREGISTER_CONTEXT_DONE for every preceding generation. A delayed
        // owner retry may therefore acknowledge its older token safely even
        // when this slot already serves another context.
        return if generation_precedes(generation, context.generation) {
            Ok(())
        } else {
            Err(GucSubmissionError::InvalidContext)
        };
    }
    // A matching token can be destroyed repeatedly after its completion. This
    // is required when a previous call returned pending and its G2H arrived
    // before the caller retried.
    if !context.registered {
        return Ok(());
    }
    // Exact CAT/reset containment owns its narrowly scoped DISABLE in the G2H
    // drain path. Destruction must never follow that with DEREGISTER: the
    // generation-tagged ID and all backing remain quarantined until a real GT
    // reset/reboot establishes a new boundary.
    if fault_requires_retention(&state, slot) {
        state.contexts[slot].destroy_requested = true;
        return Err(GucSubmissionError::DeviceFaulted);
    }
    let engine_abi =
        guc_engine_abi(dev, context.engine).ok_or(GucSubmissionError::InvalidContext)?;
    let context_id = (slot + 1) as u32;
    // This bit closes the gap between a timed-out enable/disable completion
    // and the caller's retry. Even if no transition is currently pending,
    // submit/register may no longer revive a context whose owner is tearing
    // it down.
    state.contexts[slot].destroy_requested = true;

    if state.contexts[slot].pending_deregister {
        let completed = wait_for_transition(&mut state, slot, GucPendingTransition::Deregister);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        if completed {
            return Ok(());
        }
        note_lifecycle_timeout(
            &mut state,
            slot,
            engine_abi.name,
            context_id,
            "deregister-done",
            REPORT_DEREGISTER_TIMEOUT,
        );
        return Err(GucSubmissionError::DeregisterPending);
    }

    if state.contexts[slot].pending_enable {
        let completed = wait_for_transition(&mut state, slot, GucPendingTransition::Enable);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        if !completed {
            note_lifecycle_timeout(
                &mut state,
                slot,
                engine_abi.name,
                context_id,
                "enable-done",
                REPORT_ENABLE_TIMEOUT,
            );
            return Err(GucSubmissionError::DisablePending);
        }
    }

    drain_g2h_events(&mut state);
    if fault_requires_retention(&state, slot) {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    if state.contexts[slot].enabled {
        let disabled = crate::intel::guc_ctb::send_hxg_fast_action(
            dev,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET,
            &[context_id, GUC_CONTEXT_DISABLE],
        );
        if !disabled.accepted {
            note_lifecycle_rejection(
                &mut state,
                slot,
                engine_abi.name,
                context_id,
                "disable",
                REPORT_DISABLE_REJECTED,
            );
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
    if state.contexts[slot].pending_disable {
        let completed = wait_for_transition(&mut state, slot, GucPendingTransition::Disable);
        if fault_requires_retention(&state, slot) {
            return Err(GucSubmissionError::DeviceFaulted);
        }
        if !completed {
            note_lifecycle_timeout(
                &mut state,
                slot,
                engine_abi.name,
                context_id,
                "disable-done",
                REPORT_DISABLE_TIMEOUT,
            );
            return Err(GucSubmissionError::DisablePending);
        }
    }

    drain_g2h_events(&mut state);
    if fault_requires_retention(&state, slot) {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    let deregistered = crate::intel::guc_ctb::send_hxg_fast_action(
        dev,
        INTEL_GUC_ACTION_DEREGISTER_CONTEXT,
        &[context_id],
    );
    if !deregistered.accepted {
        note_lifecycle_rejection(
            &mut state,
            slot,
            engine_abi.name,
            context_id,
            "deregister",
            REPORT_DEREGISTER_REJECTED,
        );
        return Err(GucSubmissionError::DeregisterRejected);
    }
    state.contexts[slot].pending_deregister = true;
    crate::log!(
        "intel/guc-submit: deregister enqueued=1 engine={} context_id={} token=0x{:X} pending_deregister=1 completion_event=deregister-context-done hxg=fast-request\n",
        engine_abi.name,
        context_id,
        token.raw()
    );
    let completed = wait_for_transition(&mut state, slot, GucPendingTransition::Deregister);
    if fault_requires_retention(&state, slot) {
        return Err(GucSubmissionError::DeviceFaulted);
    }
    if completed {
        Ok(())
    } else {
        note_lifecycle_timeout(
            &mut state,
            slot,
            engine_abi.name,
            context_id,
            "deregister-done",
            REPORT_DEREGISTER_TIMEOUT,
        );
        Err(GucSubmissionError::DeregisterPending)
    }
}

fn fault_requires_retention(state: &GucSubmissionState, slot: usize) -> bool {
    context_fault_requires_retention(
        state.gt_faulted,
        state
            .contexts
            .get(slot)
            .is_some_and(|context| context.faulted),
    )
}

const fn context_fault_requires_retention(gt_faulted: bool, context_faulted: bool) -> bool {
    gt_faulted || context_faulted
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
    slot: usize,
    engine: &'static str,
    context_id: u32,
    transition: &'static str,
    report_bit: u8,
) {
    if state.contexts[slot].lifecycle_timeout_reported & report_bit != 0 {
        state.lifecycle_retries = state.lifecycle_retries.saturating_add(1);
        return;
    }
    state.contexts[slot].lifecycle_timeout_reported |= report_bit;
    state.failures = state.failures.saturating_add(1);
    state.lifecycle_timeouts = state.lifecycle_timeouts.saturating_add(1);
    crate::log_warn!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle pending=1 engine={} context_id={} transition={} first_timeout=1 action=retain-context-id-and-backing\n",
        engine,
        context_id,
        transition,
    );
}

fn note_lifecycle_rejection(
    state: &mut GucSubmissionState,
    slot: usize,
    engine: &'static str,
    context_id: u32,
    transition: &'static str,
    report_bit: u8,
) {
    if state.contexts[slot].lifecycle_reject_reported & report_bit != 0 {
        state.lifecycle_retries = state.lifecycle_retries.saturating_add(1);
        return;
    }
    state.contexts[slot].lifecycle_reject_reported |= report_bit;
    state.failures = state.failures.saturating_add(1);
    crate::log_warn!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle enqueue=0 engine={} context_id={} transition={} first_rejection=1 action=retain-context-id-and-backing\n",
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
        let attribution_errors = result
            .malformed_messages
            .saturating_add(result.dropped_events);
        if attribution_errors != 0 {
            // A dropped or undecodable G2H message could be the only CAT event
            // naming its owner. Once that evidence is gone, continuing H2G on
            // a guessed context would violate isolation, so loss is global.
            state.gt_faulted = true;
            state.unattributed_faults =
                state.unattributed_faults.saturating_add(attribution_errors);
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/guc-submit: g2h transport_error=1 malformed={} dropped_events={} unsolicited_responses={} attribution_lost={} gt_faulted={} action=retain-pending-contexts-and-reject-new-h2g\n",
            result.malformed_messages,
            result.dropped_events,
            result.unsolicited_responses,
            attribution_errors,
            state.gt_faulted as u8,
        );
    }
    // An exact CAT/reset does not authorize a whole-GT fence. It does require
    // one narrowly scoped lifecycle action: order a DISABLE for that exact
    // GuC id before a clean peer is admitted again. This runs after the G2H
    // queue lock has been released, so publishing the H2G cannot invert CTB
    // lock order. Rejected publications remain required and are retried by
    // the boot-owned fault pump or the next scheduler boundary.
    if let Some(dev) = crate::intel::claimed_device() {
        enqueue_exact_fault_disables(dev, state);
    }
    if !context_registry_invariants_hold(state) {
        state.async_event_errors = state.async_event_errors.saturating_add(1);
        state.failures = state.failures.saturating_add(1);
        let newly_faulted = !state.gt_faulted;
        state.gt_faulted = true;
        if newly_faulted {
            state.unattributed_faults = state.unattributed_faults.saturating_add(1);
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/guc-submit: lifecycle invariant=0 gt_faulted=1 newly_faulted={} action=quarantine-registry-and-reject-new-h2g\n",
            newly_faulted as u8,
        );
    }
}

fn process_g2h_event(state: &mut GucSubmissionState, event: crate::intel::guc_ctb::CtbG2hEvent) {
    match event.action {
        INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_DONE => {
            if event.payload_len != 2 || event.truncated() {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done");
                return;
            }
            let (Some(context_id), Some(runnable_state)) = (event.payload(0), event.payload(1))
            else {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "sched-context-mode-done-context-id");
                return;
            };
            let context = state.contexts[slot];
            if !context.registered {
                note_malformed_lifecycle_event(
                    state,
                    event,
                    "sched-context-mode-done-unregistered",
                );
                return;
            }
            let transition = match runnable_state {
                GUC_CONTEXT_ENABLE
                    if pending_mode_matches(
                        context.pending_enable,
                        context.pending_disable,
                        runnable_state,
                    ) =>
                {
                    state.contexts[slot].pending_enable = false;
                    "enable"
                }
                GUC_CONTEXT_DISABLE
                    if pending_mode_matches(
                        context.pending_enable,
                        context.pending_disable,
                        runnable_state,
                    ) =>
                {
                    state.contexts[slot].pending_disable = false;
                    if context.faulted {
                        state.contexts[slot].fault_disable_required = false;
                    }
                    "disable"
                }
                GUC_CONTEXT_ENABLE if context.faulted && !context.pending_enable => {
                    // An exact CAT/reset supersedes any ENABLE that had
                    // already reached the CTB. GuC mode-done carries no
                    // request sequence, so consume this late completion as
                    // stale while keeping the local context non-runnable and
                    // its exact DISABLE required/pending.
                    crate::log_trace!(
                        target: "gpgpu";
                        "intel/guc-submit: lifecycle stale=1 context_id={} transition=enable faulted=1 runnable_local=0 containment_disable_required={} pending_disable={} action=ignore-superseded-mode-done\n",
                        context_id,
                        context.fault_disable_required as u8,
                        context.pending_disable as u8,
                    );
                    return;
                }
                _ => {
                    // Do not clear either transition on a contradictory event.
                    // Retaining the pending bit keeps the ID and backing fenced.
                    note_exact_lifecycle_fault(
                        state,
                        slot,
                        event,
                        "sched-context-mode-done-state-mismatch",
                    );
                    return;
                }
            };
            crate::log_trace!(
                target: "gpgpu";
                "intel/guc-submit: lifecycle complete=1 context_id={} transition={} runnable_state={} action=0x{:04X}\n",
                context_id,
                transition,
                runnable_state,
                event.action,
            );
            if runnable_state == GUC_CONTEXT_DISABLE && context.faulted {
                crate::log_error!(
                    target: "gpgpu";
                    "intel/guc-submit: exact-context containment_complete=1 context_id={} token=0x{:X} runnable=0 registered=1 id_retained=1 backing_retained=1 deregister=0 gt_faulted={}\n",
                    context_id,
                    context.token(slot).raw(),
                    state.gt_faulted as u8,
                );
            }
        }
        INTEL_GUC_ACTION_DEREGISTER_CONTEXT_DONE => {
            if event.payload_len != 1 || event.truncated() {
                note_malformed_lifecycle_event(state, event, "deregister-context-done");
                return;
            }
            let Some(context_id) = event.payload(0) else {
                note_malformed_lifecycle_event(state, event, "deregister-context-done");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "deregister-context-done-context-id");
                return;
            };
            let context = state.contexts[slot];
            if context.registered && (context.faulted || state.gt_faulted) {
                crate::log_error!(
                    target: "gpgpu";
                    "intel/guc-submit: deregister completion_accepted=0 context_id={} token=0x{:X} faulted={} gt_faulted={} action=retain-context-id-and-backing-until-reset\n",
                    context_id,
                    context.token(slot).raw(),
                    context.faulted as u8,
                    state.gt_faulted as u8,
                );
                return;
            }
            if !context.registered
                || !context.destroy_requested
                || !context.pending_deregister
                || context.enabled
                || context.pending_enable
                || context.pending_disable
            {
                if context.registered {
                    note_exact_lifecycle_fault(
                        state,
                        slot,
                        event,
                        "deregister-context-done-unexpected",
                    );
                } else {
                    note_malformed_lifecycle_event(
                        state,
                        event,
                        "deregister-context-done-unexpected",
                    );
                }
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
            if event.payload_len == 0 || event.truncated() {
                note_malformed_lifecycle_event(state, event, "context-reset");
                return;
            }
            let Some(context_id) = event.payload(0) else {
                note_malformed_lifecycle_event(state, event, "context-reset");
                return;
            };
            let Some(slot) = context_slot(context_id) else {
                note_malformed_lifecycle_event(state, event, "context-reset-context-id");
                return;
            };
            let context = state.contexts[slot];
            if !context.registered {
                note_malformed_lifecycle_event(state, event, "context-reset-unregistered");
                return;
            }
            let newly_reported =
                mark_exact_context_fault(state, slot, GucContextFaultKind::ContextReset, None);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: context-reset=1 context_id={} engine={:?}:{} duplicate={} action=defer-exact-context-owner-handoff\n",
                context_id,
                context.engine.class,
                context.engine.instance,
                (!newly_reported) as u8,
            );
        }
        INTEL_GUC_ACTION_MEMORY_CAT_ERROR => {
            if !(event.payload_len == 1 || event.payload_len == 2) || event.truncated() {
                state.memory_cat_faults = state.memory_cat_faults.saturating_add(1);
                state.async_event_errors = state.async_event_errors.saturating_add(1);
                note_unattributed_memory_cat(
                    state,
                    event.payload(0).unwrap_or(GUC_ID_UNKNOWN),
                    event.payload(1),
                    "malformed-or-truncated",
                );
                return;
            }
            let Some(context_id) = event.payload(0) else {
                state.memory_cat_faults = state.memory_cat_faults.saturating_add(1);
                state.async_event_errors = state.async_event_errors.saturating_add(1);
                note_unattributed_memory_cat(
                    state,
                    GUC_ID_UNKNOWN,
                    event.payload(1),
                    "missing-context-id",
                );
                return;
            };
            let hw_type = event.payload(1);
            state.memory_cat_faults = state.memory_cat_faults.saturating_add(1);

            if context_id == GUC_ID_UNKNOWN {
                note_unattributed_memory_cat(state, context_id, hw_type, "guc-id-unknown");
                return;
            }
            let Some(slot) = context_slot(context_id) else {
                note_unattributed_memory_cat(state, context_id, hw_type, "outside-local-registry");
                return;
            };
            let context = state.contexts[slot];
            if !context.registered {
                note_unattributed_memory_cat(state, context_id, hw_type, "unregistered-local-id");
                return;
            }

            // Capture the faulting page before GuC's exact-context
            // containment can replace the live RCS register image.  CAT is
            // asynchronous to the submitter, so waiting for the ordinary
            // retirement poll to fail loses this evidence when a completion
            // cookie happened to retire just ahead of the fault event.
            if !context.faulted {
                log_memory_cat_registers(context_id);
            }
            let newly_reported =
                mark_exact_context_fault(state, slot, GucContextFaultKind::MemoryCat, hw_type);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: memory-cat-error=1 context_id={} engine={:?}:{} hw_type=0x{:08X} duplicate={} action=defer-exact-context-containment\n",
                context_id,
                context.engine.class,
                context.engine.instance,
                hw_type.unwrap_or(u32::MAX),
                (!newly_reported) as u8,
            );
        }
        INTEL_GUC_ACTION_ENGINE_FAILURE_NOTIFICATION => {
            state.failures = state.failures.saturating_add(1);
            let newly_faulted = !state.gt_faulted;
            state.gt_faulted = true;
            state.unattributed_faults = state.unattributed_faults.saturating_add(1);
            crate::log_error!(
                target: "gpgpu";
                "intel/guc-submit: engine-failure=1 class=0x{:X} instance=0x{:X} reason=0x{:08X} payload_len={} truncated={} newly_gt_faulted={} action=reject-all-h2g-no-context-guess\n",
                event.payload(0).unwrap_or(u32::MAX),
                event.payload(1).unwrap_or(u32::MAX),
                event.payload(2).unwrap_or(u32::MAX),
                event.payload_len,
                event.truncated() as u8,
                newly_faulted as u8,
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

fn log_memory_cat_registers(context_id: u32) {
    const RCS_RING_BASE: usize = 0x2000;
    const RCS_RING_ACTHD_UDW: usize = RCS_RING_BASE + 0x5C;
    const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
    const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
    const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
    const RCS_RING_BBADDR: usize = RCS_RING_BASE + 0x140;
    const RCS_RING_BBADDR_UDW: usize = RCS_RING_BASE + 0x168;
    const GEN12_FAULT_TLB_DATA0: usize = 0xCEB8;
    const GEN12_FAULT_TLB_DATA1: usize = 0xCEBC;
    const GEN12_RING_FAULT_REG: usize = 0xCEC4;

    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };
    let fault = crate::intel::mmio_read(dev, GEN12_RING_FAULT_REG);
    let data0 = crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA0);
    let data1 = crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA1);
    let fault_gpu = (u64::from(data1 & 0xF) << 44) | (u64::from(data0) << 12);
    let acthd_lo = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
    let acthd_hi = crate::intel::mmio_read(dev, RCS_RING_ACTHD_UDW);
    let bbaddr_lo = crate::intel::mmio_read(dev, RCS_RING_BBADDR);
    let bbaddr_hi = crate::intel::mmio_read(dev, RCS_RING_BBADDR_UDW);
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: memory-cat-snapshot context_id={} fault=0x{:08X} valid={} type={} source_id={} engine_id={} address_space={} fault_gpu=0x{:016X} data0=0x{:08X} data1=0x{:08X} acthd=0x{:08X}{:08X} bbaddr=0x{:08X}{:08X} ipeir=0x{:08X} ipehr=0x{:08X}\n",
        context_id,
        fault,
        fault & 1,
        (fault >> 1) & 0x3,
        (fault >> 3) & 0xFF,
        (fault >> 12) & 0x1F,
        if data1 & (1 << 4) != 0 { "ggtt" } else { "ppgtt" },
        fault_gpu,
        data0,
        data1,
        acthd_hi,
        acthd_lo,
        bbaddr_hi,
        bbaddr_lo,
        crate::intel::mmio_read(dev, RCS_RING_IPEIR),
        crate::intel::mmio_read(dev, RCS_RING_IPEHR),
    );
}

fn note_unattributed_memory_cat(
    state: &mut GucSubmissionState,
    context_id: u32,
    hw_type: Option<u32>,
    reason: &'static str,
) {
    let newly_faulted = !state.gt_faulted;
    state.gt_faulted = true;
    state.unattributed_faults = state.unattributed_faults.saturating_add(1);
    if newly_faulted {
        state.failures = state.failures.saturating_add(1);
    }
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: memory-cat-error=1 context_id=0x{:08X} hw_type=0x{:08X} attributed=0 reason={} duplicate={} action=wedge-gt-no-context-guess\n",
        context_id,
        hw_type.unwrap_or(u32::MAX),
        reason,
        (!newly_faulted) as u8,
    );
}

fn note_malformed_lifecycle_event(
    state: &mut GucSubmissionState,
    event: crate::intel::guc_ctb::CtbG2hEvent,
    reason: &'static str,
) {
    state.async_event_errors = state.async_event_errors.saturating_add(1);
    state.failures = state.failures.saturating_add(1);
    let newly_faulted = !state.gt_faulted;
    state.gt_faulted = true;
    state.unattributed_faults = state.unattributed_faults.saturating_add(1);
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle event_valid=0 action=0x{:04X} payload_len={} truncated={} payload0=0x{:08X} payload1=0x{:08X} reason={} newly_gt_faulted={} action_on_error=wedge-gt-owner-evidence-unavailable\n",
        event.action,
        event.payload_len,
        event.truncated() as u8,
        event.payload(0).unwrap_or(u32::MAX),
        event.payload(1).unwrap_or(u32::MAX),
        reason,
        newly_faulted as u8,
    );
}

fn mark_exact_context_fault(
    state: &mut GucSubmissionState,
    slot: usize,
    kind: GucContextFaultKind,
    hw_type: Option<u32>,
) -> bool {
    let newly_reported = !state.contexts[slot].owner_fault_reported;
    {
        let context = &mut state.contexts[slot];
        let first_exact_fault = !context.faulted;
        context.faulted = true;
        context.destroy_requested = true;
        context.owner_fault_reported = true;
        // A pending DEREGISTER is reachable only after a completed DISABLE,
        // so no second mode change is needed at that already non-runnable
        // boundary. Its local registration is still retained below.
        if first_exact_fault && !context.pending_deregister {
            context.fault_disable_required = true;
        }
        // DISABLE supersedes an outstanding ENABLE after exact attribution.
        // GuC mode-done has no request sequence, so a delayed ENABLE event is
        // accepted below as stale without ever making the context runnable in
        // local state again.
        context.pending_enable = false;
        if newly_reported {
            context.owner_handoff_pending = true;
        }
        if matches!(kind, GucContextFaultKind::MemoryCat) || context.fault_kind.is_none() {
            context.fault_kind = Some(kind);
        }
        if matches!(kind, GucContextFaultKind::MemoryCat) {
            context.cat_hw_type = hw_type;
        }
    }
    if newly_reported {
        state.failures = state.failures.saturating_add(1);
    }
    newly_reported
}

const fn exact_fault_disable_should_enqueue(gt_faulted: bool, context: GucContextState) -> bool {
    !gt_faulted
        && context.registered
        && context.faulted
        && context.fault_disable_required
        && !context.pending_disable
        && !context.pending_deregister
}

/// Publish the only H2G operation permitted for an exactly faulted context.
///
/// This deliberately does not deregister, release, or mutate the generation,
/// HWLRCA, PPGTT ownership, or backing. A clean peer stays independently
/// schedulable because exact attribution never sets `gt_faulted`.
fn enqueue_exact_fault_disables(dev: crate::intel::Dev, state: &mut GucSubmissionState) {
    for slot in 0..state.contexts.len() {
        let context = state.contexts[slot];
        if !exact_fault_disable_should_enqueue(state.gt_faulted, context) {
            continue;
        }
        let Some(engine_abi) = guc_engine_abi(dev, context.engine) else {
            continue;
        };
        let context_id = context_id(slot);
        let disabled = crate::intel::guc_ctb::send_hxg_fast_action(
            dev,
            INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET,
            &[context_id, GUC_CONTEXT_DISABLE],
        );
        if !disabled.accepted {
            note_lifecycle_rejection(
                state,
                slot,
                engine_abi.name,
                context_id,
                "fault-containment-disable",
                REPORT_DISABLE_REJECTED,
            );
            continue;
        }
        state.contexts[slot].enabled = false;
        state.contexts[slot].pending_disable = true;
        crate::log_error!(
            target: "gpgpu";
            "intel/guc-submit: exact-context containment_enqueued=1 engine={} context_id={} token=0x{:X} pending_disable=1 completion_event=sched-context-mode-done registration_retained=1 backing_retained=1 deregister=0 gt_faulted=0 h2g_publish_sequence={}\n",
            engine_abi.name,
            context_id,
            context.token(slot).raw(),
            disabled.h2g_publish_sequence,
        );
    }
}

fn note_exact_lifecycle_fault(
    state: &mut GucSubmissionState,
    slot: usize,
    event: crate::intel::guc_ctb::CtbG2hEvent,
    reason: &'static str,
) {
    let context = state.contexts[slot];
    state.async_event_errors = state.async_event_errors.saturating_add(1);
    let newly_reported =
        mark_exact_context_fault(state, slot, GucContextFaultKind::LifecycleProtocol, None);
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: lifecycle event_valid=0 action=0x{:04X} context_id={} token=0x{:X} payload_len={} truncated={} payload0=0x{:08X} payload1=0x{:08X} reason={} duplicate={} action_on_error=defer-exact-context-owner-handoff\n",
        event.action,
        context_id(slot),
        context.token(slot).raw(),
        event.payload_len,
        event.truncated() as u8,
        event.payload(0).unwrap_or(u32::MAX),
        event.payload(1).unwrap_or(u32::MAX),
        reason,
        (!newly_reported) as u8,
    );
}

const fn context_slot(context_id: u32) -> Option<usize> {
    if context_id == 0 || context_id as usize > MAX_GUC_CONTEXTS {
        None
    } else {
        Some(context_id as usize - 1)
    }
}

const fn generation_precedes(candidate: u32, current: u32) -> bool {
    let distance = current.wrapping_sub(candidate);
    candidate != 0 && current != 0 && distance != 0 && distance < (1u32 << 31)
}

/// GuC v70 SCHED_CONTEXT_MODE_DONE payload is `[context_id,
/// runnable_state]`, where runnable state uses the same 0/1 values as the
/// disable/enable request. A completion is useful only when exactly one local
/// transition is pending and it agrees with the firmware state.
const fn pending_mode_matches(
    pending_enable: bool,
    pending_disable: bool,
    runnable_state: u32,
) -> bool {
    match runnable_state {
        GUC_CONTEXT_ENABLE => pending_enable && !pending_disable,
        GUC_CONTEXT_DISABLE => pending_disable && !pending_enable,
        _ => false,
    }
}

fn context_registry_invariants_hold(state: &GucSubmissionState) -> bool {
    state.contexts.iter().all(|context| {
        let mutually_exclusive = !(context.pending_enable && context.pending_disable)
            && !(context.pending_deregister
                && (context.pending_enable || context.pending_disable || context.enabled));
        let empty_is_inert = context.registered
            || (!context.enabled
                && !context.destroy_requested
                && !context.pending_enable
                && !context.pending_disable
                && !context.pending_deregister
                && !context.fault_disable_required
                && !context.faulted
                && !context.owner_fault_reported
                && !context.owner_handoff_pending
                && context.fault_kind.is_none()
                && context.cat_hw_type.is_none()
                && context.lifecycle_timeout_reported == 0
                && context.lifecycle_reject_reported == 0);
        let teardown_is_quarantined = (!context.destroy_requested || context.registered)
            && (!context.pending_disable || context.destroy_requested)
            && (!context.pending_deregister || context.destroy_requested);
        let containment_is_quarantined = !context.owner_handoff_pending
            || (context.registered
                && context.faulted
                && context.destroy_requested
                && context.fault_kind.is_some());
        let fault_disable_is_scoped = !context.fault_disable_required
            || (context.registered
                && context.faulted
                && context.destroy_requested
                && !context.pending_deregister);
        let fault_telemetry_matches = context.cat_hw_type.is_none()
            || matches!(context.fault_kind, Some(GucContextFaultKind::MemoryCat));
        let lifecycle_reports_are_teardown_only = (context.lifecycle_timeout_reported == 0
            && context.lifecycle_reject_reported == 0)
            || (context.registered && context.destroy_requested);
        mutually_exclusive
            && empty_is_inert
            && teardown_is_quarantined
            && containment_is_quarantined
            && fault_disable_is_scoped
            && fault_telemetry_matches
            && lifecycle_reports_are_teardown_only
    })
}

/// Copy sticky physical fault state without performing teardown or calling an
/// upper layer while the GuC registry lock is held. The caller may already
/// hold the broker lock in the canonical BROKER -> CONTEXTS order.
pub(crate) fn fault_snapshot() -> GucFaultSnapshot {
    let mut state = CONTEXTS.lock();
    drain_g2h_events(&mut state);
    GucFaultSnapshot {
        gt_faulted: state.gt_faulted,
        unattributed_faults: state.unattributed_faults,
        contexts: state
            .contexts
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, context)| context.registered && context.owner_handoff_pending)
            .filter_map(|(slot, context)| {
                context.fault_kind.map(|kind| GucContextFault {
                    token: context.token(slot),
                    engine: context.engine,
                    origin: context.origin,
                    kind,
                    hw_type: context.cat_hw_type,
                })
            })
            .collect(),
    }
}

/// Confirm that the host ownership layer consumed one exact fault record.
/// This is only a software owner-handoff acknowledgement: the context remains
/// faulted, destroy-requested, registered, and permanently non-reusable until
/// a full reset. No H2G request is sent here.
pub(crate) fn acknowledge_fault(token: GucContextToken) -> bool {
    let mut state = CONTEXTS.lock();
    let Some((slot, generation)) = token.parts() else {
        return false;
    };
    let Some(context) = state.contexts.get(slot).copied() else {
        return false;
    };
    if !context.registered
        || context.generation != generation
        || !context.faulted
        || !context.owner_handoff_pending
    {
        return false;
    }
    state.contexts[slot].owner_handoff_pending = false;
    crate::log_error!(
        target: "gpgpu";
        "intel/guc-submit: owner_handoff_recorded=1 fault_kind={:?} context_id={} token=0x{:X} engine={:?}:{} hardware_lifecycle=quarantined-until-reset h2g_sent=0\n",
        context.fault_kind,
        context_id(slot),
        token.raw(),
        context.engine.class,
        context.engine.instance,
    );
    true
}

pub(crate) fn scheduler_status() -> GucSchedulerStatus {
    let state = CONTEXTS.lock();
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
        destroy_requested: state
            .contexts
            .iter()
            .filter(|context| context.destroy_requested)
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
        faulted: state
            .contexts
            .iter()
            .filter(|context| context.faulted)
            .count(),
        owner_handoffs_pending: state
            .contexts
            .iter()
            .filter(|context| context.owner_handoff_pending)
            .count(),
        submissions: state.serial,
        registrations: state.registrations,
        deregistrations: state.deregistrations,
        failures: state.failures,
        async_events: state.async_events,
        async_event_errors: state.async_event_errors,
        memory_cat_faults: state.memory_cat_faults,
        unattributed_faults: state.unattributed_faults,
        lifecycle_timeouts: state.lifecycle_timeouts,
        lifecycle_retries: state.lifecycle_retries,
        gt_faulted: state.gt_faulted,
    }
}

pub(crate) fn context_status() -> Vec<GucContextStatus> {
    let state = CONTEXTS.lock();
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
            origin: context.origin,
            policy_enqueued: context.policy_enqueued,
            enabled: context.enabled,
            destroy_requested: context.destroy_requested,
            pending_enable: context.pending_enable,
            pending_disable: context.pending_disable,
            pending_deregister: context.pending_deregister,
            faulted: context.faulted,
            owner_handoff_pending: context.owner_handoff_pending,
            fault_kind: context.fault_kind,
            cat_hw_type: context.cat_hw_type,
            hwlrca_lo: context.hwlrca_lo,
            hwlrca_hi: context.hwlrca_hi,
            submissions: context.submissions,
        })
        .collect()
}

const fn context_id(slot: usize) -> u32 {
    (slot + 1) as u32
}

const fn hwlrca_backing_page(hwlrca_lo: u32) -> u32 {
    hwlrca_lo & HWLRCA_PAGE_MASK
}

/// Add the Gen12 RCS descriptor priority that is separate from GuC's policy
/// KLV. Bits 10:9 are available because HWLRCA is 4 KiB aligned. Copy and media
/// engines do not advertise the EU-priority descriptor capability, so their
/// descriptors are preserved byte-for-byte.
const fn guc_hwlrca_descriptor(
    engine: crate::gpu::physical::PhysicalEngineId,
    priority: crate::gpu::physical::PhysicalContextPriority,
    hwlrca_lo: u32,
) -> u32 {
    match engine.class {
        crate::gpu::physical::EngineClass::RenderCompute => {
            let descriptor_priority = match priority {
                crate::gpu::physical::PhysicalContextPriority::KernelHigh => {
                    GEN12_HW_CONTEXT_PRIORITY_HIGH
                }
                crate::gpu::physical::PhysicalContextPriority::KernelNormal => {
                    GEN12_HW_CONTEXT_PRIORITY_NORMAL
                }
            };
            (hwlrca_lo & !GEN12_HW_CONTEXT_PRIORITY_MASK) | descriptor_priority
        }
        crate::gpu::physical::EngineClass::VideoDecode
        | crate::gpu::physical::EngineClass::Copy => hwlrca_lo,
    }
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
    assert!(
        guc_hwlrca_descriptor(crate::gpu::physical::PhysicalEngineId::RCS0, KernelHigh, 0x8000)
            == 0x8400
    );
    assert!(
        guc_hwlrca_descriptor(crate::gpu::physical::PhysicalEngineId::RCS0, KernelNormal, 0x8000)
            == 0x8200
    );
    assert!(
        guc_hwlrca_descriptor(crate::gpu::physical::PhysicalEngineId::BCS0, KernelHigh, 0x8000)
            == 0x8000
    );
    assert!(hwlrca_backing_page(0x8200) == hwlrca_backing_page(0x8400));
    assert!(hwlrca_backing_page(0x8FFF) == 0x8000);
    assert!(hwlrca_backing_page(0x9000) != hwlrca_backing_page(0x8400));
    assert!(generation_precedes(1, 2));
    assert!(generation_precedes(u32::MAX, 1));
    assert!(!generation_precedes(2, 1));
    assert!(!generation_precedes(1, 1));
    assert!(pending_mode_matches(true, false, GUC_CONTEXT_ENABLE));
    assert!(pending_mode_matches(false, true, GUC_CONTEXT_DISABLE));
    assert!(!pending_mode_matches(true, false, GUC_CONTEXT_DISABLE));
    assert!(!pending_mode_matches(false, true, GUC_CONTEXT_ENABLE));
    assert!(!pending_mode_matches(false, false, GUC_CONTEXT_ENABLE));
    assert!(!pending_mode_matches(true, true, GUC_CONTEXT_ENABLE));
    assert!(!pending_mode_matches(true, false, 2));

    // Exact-context containment is local: the offender receives DISABLE,
    // while a clean peer remains schedulable and the GT stays live.
    let mut exact_fault = GucContextState::EMPTY;
    exact_fault.registered = true;
    exact_fault.faulted = true;
    exact_fault.destroy_requested = true;
    exact_fault.fault_disable_required = true;
    assert!(exact_fault_disable_should_enqueue(false, exact_fault));
    assert!(context_fault_requires_retention(false, true));
    assert!(!context_fault_requires_retention(false, false));
    assert!(context_fault_requires_retention(true, false));

    exact_fault.pending_disable = true;
    assert!(!exact_fault_disable_should_enqueue(false, exact_fault));
    exact_fault.pending_disable = false;
    exact_fault.pending_deregister = true;
    assert!(!exact_fault_disable_should_enqueue(false, exact_fault));
    exact_fault.pending_deregister = false;
    assert!(!exact_fault_disable_should_enqueue(true, exact_fault));
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
