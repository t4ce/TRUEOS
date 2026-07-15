//! GuC-owned submission for TRUEOS's serialized kernel RCS contexts.
//!
//! GuC submission ABI v1 (GuC 70+) registers a single LRC directly. Work
//! queues are only populated for multi-LRC contexts, so the current font and
//! GPGPU batches can retain their distinct ring/LRC layouts while GuC owns
//! scheduling.

use spin::Mutex;

const INTEL_GUC_ACTION_SCHED_CONTEXT: u32 = 0x1000;
const INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET: u32 = 0x1001;
const INTEL_GUC_ACTION_REGISTER_CONTEXT: u32 = 0x4502;
const GUC_CONTEXT_ENABLE: u32 = 1;
const CONTEXT_REGISTRATION_FLAG_KMD: u32 = 1;
const GUC_RENDER_CLASS: u32 = 0;
const RCS0_SUBMIT_MASK: u32 = 1;
const MAX_KERNEL_RCS_CONTEXTS: usize = 4;

#[derive(Copy, Clone)]
struct RcsContextState {
    registered: bool,
    enabled: bool,
    hwlrca_lo: u32,
    hwlrca_hi: u32,
}

impl RcsContextState {
    const EMPTY: Self = Self {
        registered: false,
        enabled: false,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
    };
}

#[derive(Copy, Clone)]
struct RcsSubmissionState {
    contexts: [RcsContextState; MAX_KERNEL_RCS_CONTEXTS],
    serial: u64,
}

static RCS: Mutex<RcsSubmissionState> = Mutex::new(RcsSubmissionState {
    contexts: [RcsContextState::EMPTY; MAX_KERNEL_RCS_CONTEXTS],
    serial: 0,
});

pub(crate) fn ready() -> bool {
    crate::intel::guc::ready() && crate::intel::guc_ctb::enabled()
}

/// Notify GuC that the serialized kernel RCS LRC has a new ring tail.
///
/// The caller must finish writing and flushing the LRC and ring first. The
/// marker/fence already emitted by each caller remains its completion source.
pub(crate) fn submit_rcs_lrc(dev: crate::intel::Dev, hwlrca_lo: u32, hwlrca_hi: u32) -> bool {
    if !ready() {
        crate::log!(
            "intel/guc-submit: rejected engine=rcs0 reason=transport-not-ready guc_ready={} ctb_ready={}\n",
            crate::intel::guc::ready() as u8,
            crate::intel::guc_ctb::enabled() as u8
        );
        return false;
    }

    let mut state = RCS.lock();
    let existing = state.contexts.iter().position(|context| {
        context.registered && context.hwlrca_lo == hwlrca_lo && context.hwlrca_hi == hwlrca_hi
    });
    let Some(slot) = existing.or_else(|| {
        state
            .contexts
            .iter()
            .position(|context| !context.registered)
    }) else {
        crate::log!(
            "intel/guc-submit: rejected engine=rcs0 reason=context-registry-full capacity={} requested_hwlrca=0x{:08X}:0x{:08X}\n",
            MAX_KERNEL_RCS_CONTEXTS,
            hwlrca_hi,
            hwlrca_lo,
        );
        return false;
    };
    let context_id = (slot + 1) as u32;

    if !state.contexts[slot].registered {
        // GuC submission ABI v1 single-LRC registration. WQ fields are zero;
        // they are used for parallel/multi-LRC registrations only.
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
            return false;
        }
        state.contexts[slot] = RcsContextState {
            registered: true,
            enabled: false,
            hwlrca_lo,
            hwlrca_hi,
        };
        crate::log!(
            "intel/guc-submit: register accepted=1 engine=rcs0 context_id={} class={} submit_mask=0x{:X} hwlrca=0x{:08X}:0x{:08X} abi=v1 single_lrc=1\n",
            context_id,
            GUC_RENDER_CLASS,
            RCS0_SUBMIT_MASK,
            hwlrca_hi,
            hwlrca_lo
        );
    }

    let (action, args): (u32, &[u32]) = if state.contexts[slot].enabled {
        (INTEL_GUC_ACTION_SCHED_CONTEXT, &[context_id])
    } else {
        (INTEL_GUC_ACTION_SCHED_CONTEXT_MODE_SET, &[context_id, GUC_CONTEXT_ENABLE])
    };
    let scheduled = crate::intel::guc_ctb::send_hxg_action(dev, action, args);
    if !scheduled.accepted {
        crate::log!(
            "intel/guc-submit: schedule accepted=0 engine=rcs0 context_id={} action=0x{:04X} response=0x{:08X} type={} error={} g2h_poll_iters={}\n",
            context_id,
            action,
            scheduled.response,
            scheduled.response_type,
            scheduled.error,
            scheduled.g2h_poll_iters
        );
        return false;
    }

    state.contexts[slot].enabled = true;
    state.serial = state.serial.wrapping_add(1);
    crate::log_trace!(
        target: "gpgpu";
        "intel/guc-submit: schedule accepted=1 engine=rcs0 context_id={} serial={} action=0x{:04X} submission_owner=guc\n",
        context_id,
        state.serial,
        action
    );
    true
}
