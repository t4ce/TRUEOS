// Gen12 GT cache-policy ownership and frequency-state diagnostics.
//
// Firmware owns the lower half of the shared MOCS table. Lumen owns only the
// upper half and uses one entry from that range; display/compositor clients
// retain their firmware-supplied lower-half entries unchanged.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const GEN12_GLOBAL_MOCS_BASE: usize = 0x4000;
const GEN12_GLOBAL_MOCS_ENTRIES: usize = 64;
const GEN12_LNCFCMOCS_BASE: usize = 0xB020;
const GEN12_LNCFCMOCS_REGISTERS: usize = GEN12_GLOBAL_MOCS_ENTRIES / 2;
const GEN12_LUMEN_MOCS_FIRST_INDEX: usize = GEN12_GLOBAL_MOCS_ENTRIES / 2;
const GEN12_LUMEN_L3CC_FIRST_REGISTER: usize = GEN12_LNCFCMOCS_REGISTERS / 2;
pub(super) const GEN12_LUMEN_MOCS_INDEX: u32 = 49;
const GEN12_MOCS_DEFAULT_CONTROL: u32 = 0x0037;
const GEN12_MOCS_DEFAULT_L3CC: u16 = 0x0030;
const FNV1A_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;

const GEN12_RPNSWREQ: usize = 0xA008;
const GEN12_GT0_PERF_LIMIT_REASONS: usize = 0x1381A8;
const GEN12_RPSTAT1: usize = 0x1381B4;
const GEN12_RP_STATE_CAP: usize = 0x145998;
const GEN10_FREQ_INFO_REC: usize = 0x145EF0;
const GEN12_CAGF_MASK: u32 = 0x1FF;
const GEN12_CAGF_SHIFT: u32 = 11;
const GEN9_SW_REQ_UNSLICE_RATIO_SHIFT: u32 = 23;
const GEN9_SW_REQ_UNSLICE_RATIO_MASK: u32 = GEN12_CAGF_MASK << GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
const GEN12_RP0_CAP_MASK: u32 = 0xFF;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GEN12_GT0_PERF_LIMIT_REASONS_MASK: u32 = 0x0DE3;
static GEN12_LUMEN_GT_BOOST_ACTIVE: AtomicBool = AtomicBool::new(false);
static GEN12_LUMEN_GT_PREVIOUS_REQUEST: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Gen12GlobalGtPowerMarker {
    pub(crate) generation: u64,
    pub(crate) active: bool,
    pub(crate) requested_mhz: u32,
    pub(crate) actual_mhz: u32,
    pub(crate) rp0_mhz: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Gen12GlobalGtPowerState {
    active: bool,
    generation: u64,
    transient_boost: Option<Gen12TransientGtBoostLease>,
    next_transient_boost_token: u64,
    saved_request: u32,
    boost_ratio: u32,
    marker: Gen12GlobalGtPowerMarker,
}

static GEN12_GLOBAL_GT_POWER_STATE: Mutex<Gen12GlobalGtPowerState> =
    Mutex::new(Gen12GlobalGtPowerState {
        active: false,
        generation: 0,
        transient_boost: None,
        next_transient_boost_token: 0,
        saved_request: 0,
        boost_ratio: 0,
        marker: Gen12GlobalGtPowerMarker {
            generation: 0,
            active: false,
            requested_mhz: 0,
            actual_mhz: 0,
            rp0_mhz: 0,
        },
    });

/// A short-lived caller owns a global GT enable only when this exact marker
/// generation remains active.  The token distinguishes a stale timer from a
/// later lease even if the generation counter eventually wraps.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct Gen12TransientGtBoostLease {
    pub(super) token: u64,
    pub(super) generation: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Gen12TransientGtBoostStart {
    Armed(Gen12TransientGtBoostLease),
    AlreadyActive,
}

const fn expected_mocs_control_table() -> [u32; GEN12_GLOBAL_MOCS_ENTRIES] {
    let mut table = [GEN12_MOCS_DEFAULT_CONTROL; GEN12_GLOBAL_MOCS_ENTRIES];
    table[3] = 0x0005;
    table[4] = 0x0005;
    table[5] = 0x0037;
    table[6] = 0x0017;
    table[7] = 0x0017;
    table[8] = 0x0027;
    table[9] = 0x0027;
    table[10] = 0x0077;
    table[11] = 0x0077;
    table[12] = 0x0057;
    table[13] = 0x0057;
    table[14] = 0x0067;
    table[15] = 0x0067;
    table[16] = 0x4005;
    table[17] = 0x4005;
    table[18] = 0x0006_0037;
    table[19] = 0x0737;
    table[20] = 0x0337;
    table[21] = 0x0137;
    table[22] = 0x03B7;
    table[23] = 0x07B7;
    table[48] = 0x0037;
    table[49] = 0x0005;
    table[50] = 0x0037;
    table[51] = 0x0005;
    table[60] = 0x0037;
    table[61] = 0x0005;
    table[62] = 0x0037;
    table[63] = 0x0037;
    table
}

const fn expected_mocs_l3cc_table() -> [u16; GEN12_GLOBAL_MOCS_ENTRIES] {
    let mut table = [GEN12_MOCS_DEFAULT_L3CC; GEN12_GLOBAL_MOCS_ENTRIES];
    table[3] = 0x0010;
    table[4] = 0x0030;
    table[5] = 0x0010;
    table[6] = 0x0010;
    table[7] = 0x0030;
    table[8] = 0x0010;
    table[9] = 0x0030;
    table[10] = 0x0010;
    table[11] = 0x0030;
    table[12] = 0x0010;
    table[13] = 0x0030;
    table[14] = 0x0010;
    table[15] = 0x0030;
    table[16] = 0x0010;
    table[17] = 0x0030;
    table[18] = 0x0030;
    table[19] = 0x0030;
    table[20] = 0x0030;
    table[21] = 0x0030;
    table[22] = 0x0030;
    table[23] = 0x0030;
    table[48] = 0x0030;
    table[49] = 0x0030;
    table[50] = 0x0010;
    table[51] = 0x0010;
    table[60] = 0x0010;
    table[61] = 0x0030;
    table[62] = 0x0010;
    table[63] = 0x0010;
    table
}

const GEN12_EXPECTED_MOCS_CONTROL: [u32; GEN12_GLOBAL_MOCS_ENTRIES] = expected_mocs_control_table();
const GEN12_EXPECTED_MOCS_L3CC: [u16; GEN12_GLOBAL_MOCS_ENTRIES] = expected_mocs_l3cc_table();

const fn expected_packed_l3cc(register: usize) -> u32 {
    GEN12_EXPECTED_MOCS_L3CC[register * 2] as u32
        | ((GEN12_EXPECTED_MOCS_L3CC[register * 2 + 1] as u32) << 16)
}

const _: () = {
    assert!(GEN12_EXPECTED_MOCS_CONTROL[4] == 0x0005);
    assert!(expected_packed_l3cc(2) == 0x0010_0030);
    assert!(GEN12_EXPECTED_MOCS_CONTROL[GEN12_LUMEN_MOCS_INDEX as usize] == 0x0005);
    assert!(GEN12_EXPECTED_MOCS_L3CC[GEN12_LUMEN_MOCS_INDEX as usize] == 0x0030);
    assert!(GEN12_LUMEN_MOCS_INDEX as usize >= GEN12_LUMEN_MOCS_FIRST_INDEX);
};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Gen12MocsReadback {
    pub(super) available: bool,
    pub(super) accepted: bool,
    pub(super) lumen_half_accepted: bool,
    pub(super) global_mismatches: u32,
    pub(super) l3cc_mismatches: u32,
    pub(super) lumen_global_mismatches: u32,
    pub(super) lumen_l3cc_mismatches: u32,
    pub(super) first_global_index: u32,
    pub(super) first_global_observed: u32,
    pub(super) first_global_expected: u32,
    pub(super) first_l3cc_register: u32,
    pub(super) first_l3cc_observed: u32,
    pub(super) first_l3cc_expected: u32,
    pub(super) global_index4: u32,
    pub(super) l3cc_pair2: u32,
    pub(super) global_lumen_index: u32,
    pub(super) l3cc_lumen_pair: u32,
    pub(super) global_fingerprint: u64,
    pub(super) l3cc_fingerprint: u64,
    pub(super) resident_global_fingerprint: u64,
    pub(super) resident_l3cc_fingerprint: u64,
    pub(super) lumen_global_fingerprint: u64,
    pub(super) lumen_l3cc_fingerprint: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Gen12LumenMocsInitReport {
    pub(super) available: bool,
    pub(super) accepted: bool,
    pub(super) residents_preserved: bool,
    pub(super) before: Gen12MocsReadback,
    pub(super) after: Gen12MocsReadback,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[expect(dead_code, reason = "raw ratio fields retained for GT diagnostics")]
pub(crate) struct Gen12GtStateSnapshot {
    pub(crate) available: bool,
    pub(crate) actual_ratio: u32,
    pub(crate) actual_mhz: u32,
    pub(crate) requested_ratio: u32,
    pub(crate) requested_mhz: u32,
    pub(crate) rp0_mhz: u32,
    pub(crate) rpe_mhz: u32,
    pub(crate) rpn_mhz: u32,
    pub(crate) throttle_reasons: u32,
    pub(crate) throttle_reasons_raw: u32,
    pub(crate) rpstat1_raw: u32,
    pub(crate) rpnswreq_raw: u32,
}

/// Turn-scoped ownership of the Gen9+ unslice frequency request.
///
/// Lumen restores the exact request bits it observed on entry. If another
/// owner changes the request while the turn is active, drop deliberately
/// leaves that newer request untouched.
#[must_use = "keep the guard alive for the complete Lumen inference turn"]
pub(crate) struct Gen12LumenGtBoost {
    dev: super::Dev,
    previous_request: u32,
    previous_ratio: u32,
    boost_ratio: u32,
    active: bool,
}

impl Drop for Gen12LumenGtBoost {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let observed = super::mmio_read(self.dev, GEN12_RPNSWREQ);
        let observed_ratio =
            (observed & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
        // Serialize the restore against an F12 transition. A global enable
        // that arrives here either observes the restored predecessor or wins
        // first and prevents this lower-priority scope from restoring it.
        let global_power_state = GEN12_GLOBAL_GT_POWER_STATE.lock();
        let global_power_active = global_power_state.active;
        let restored = !global_power_active && observed_ratio == self.boost_ratio;
        let final_request = if restored {
            (observed & !GEN9_SW_REQ_UNSLICE_RATIO_MASK)
                | (self.previous_request & GEN9_SW_REQ_UNSLICE_RATIO_MASK)
        } else {
            observed
        };
        if restored {
            super::mmio_write(self.dev, GEN12_RPNSWREQ, final_request);
            core::sync::atomic::compiler_fence(Ordering::SeqCst);
        }
        let final_ratio = (super::mmio_read(self.dev, GEN12_RPNSWREQ)
            & GEN9_SW_REQ_UNSLICE_RATIO_MASK)
            >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
        crate::log_info!(
            target: "gpgpu";
            "intel/lumen-gt-boost: stage=end restored={} previous_ratio={} previous_mhz={} boost_ratio={} boost_mhz={} observed_ratio={} final_ratio={} final_mhz={} global_power_active={} ownership=turn-scoped conflict_policy=preserve-newer-request+defer-to-global-power\n",
            restored as u8,
            self.previous_ratio,
            ratio_to_mhz(self.previous_ratio),
            self.boost_ratio,
            ratio_to_mhz(self.boost_ratio),
            observed_ratio,
            final_ratio,
            ratio_to_mhz(final_ratio),
            global_power_active as u8,
        );
        drop(global_power_state);
        self.active = false;
        GEN12_LUMEN_GT_PREVIOUS_REQUEST.store(0, Ordering::Release);
        GEN12_LUMEN_GT_BOOST_ACTIVE.store(false, Ordering::Release);
    }
}

fn gt_state_registers_available(dev: super::Dev) -> bool {
    [
        GEN12_RPNSWREQ,
        GEN12_GT0_PERF_LIMIT_REASONS,
        GEN12_RPSTAT1,
        GEN12_RP_STATE_CAP,
        GEN10_FREQ_INFO_REC,
    ]
    .into_iter()
    .all(|offset| {
        offset
            .checked_add(core::mem::size_of::<u32>())
            .is_some_and(|end| end <= dev.mmio_len)
    })
}

fn mocs_registers_available(dev: super::Dev) -> bool {
    GEN12_GLOBAL_MOCS_BASE
        .checked_add(GEN12_GLOBAL_MOCS_ENTRIES * core::mem::size_of::<u32>())
        .is_some_and(|end| end <= dev.mmio_len)
        && GEN12_LNCFCMOCS_BASE
            .checked_add(GEN12_LNCFCMOCS_REGISTERS * core::mem::size_of::<u32>())
            .is_some_and(|end| end <= dev.mmio_len)
}

fn fingerprint_u32(mut fingerprint: u64, value: u32) -> u64 {
    for byte in value.to_le_bytes() {
        fingerprint ^= u64::from(byte);
        fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
    }
    fingerprint
}

pub(super) fn read_mocs(dev: super::Dev) -> Gen12MocsReadback {
    if !mocs_registers_available(dev) {
        return Gen12MocsReadback::default();
    }

    let mut readback = Gen12MocsReadback {
        available: true,
        global_fingerprint: FNV1A_OFFSET_BASIS,
        l3cc_fingerprint: FNV1A_OFFSET_BASIS,
        resident_global_fingerprint: FNV1A_OFFSET_BASIS,
        resident_l3cc_fingerprint: FNV1A_OFFSET_BASIS,
        lumen_global_fingerprint: FNV1A_OFFSET_BASIS,
        lumen_l3cc_fingerprint: FNV1A_OFFSET_BASIS,
        ..Gen12MocsReadback::default()
    };
    let mut index = 0usize;
    while index < GEN12_GLOBAL_MOCS_ENTRIES {
        let observed = super::mmio_read(dev, GEN12_GLOBAL_MOCS_BASE + index * 4);
        let expected = GEN12_EXPECTED_MOCS_CONTROL[index];
        readback.global_fingerprint = fingerprint_u32(readback.global_fingerprint, observed);
        if index < GEN12_LUMEN_MOCS_FIRST_INDEX {
            readback.resident_global_fingerprint =
                fingerprint_u32(readback.resident_global_fingerprint, observed);
        } else {
            readback.lumen_global_fingerprint =
                fingerprint_u32(readback.lumen_global_fingerprint, observed);
        }
        if observed != expected {
            if readback.global_mismatches == 0 {
                readback.first_global_index = index as u32;
                readback.first_global_observed = observed;
                readback.first_global_expected = expected;
            }
            readback.global_mismatches = readback.global_mismatches.saturating_add(1);
            if index >= GEN12_LUMEN_MOCS_FIRST_INDEX {
                readback.lumen_global_mismatches =
                    readback.lumen_global_mismatches.saturating_add(1);
            }
        }
        if index == 4 {
            readback.global_index4 = observed;
        }
        if index == GEN12_LUMEN_MOCS_INDEX as usize {
            readback.global_lumen_index = observed;
        }
        index += 1;
    }

    let mut register = 0usize;
    while register < GEN12_LNCFCMOCS_REGISTERS {
        let observed = super::mmio_read(dev, GEN12_LNCFCMOCS_BASE + register * 4);
        let expected = expected_packed_l3cc(register);
        readback.l3cc_fingerprint = fingerprint_u32(readback.l3cc_fingerprint, observed);
        if register < GEN12_LUMEN_L3CC_FIRST_REGISTER {
            readback.resident_l3cc_fingerprint =
                fingerprint_u32(readback.resident_l3cc_fingerprint, observed);
        } else {
            readback.lumen_l3cc_fingerprint =
                fingerprint_u32(readback.lumen_l3cc_fingerprint, observed);
        }
        if observed != expected {
            if readback.l3cc_mismatches == 0 {
                readback.first_l3cc_register = register as u32;
                readback.first_l3cc_observed = observed;
                readback.first_l3cc_expected = expected;
            }
            readback.l3cc_mismatches = readback.l3cc_mismatches.saturating_add(1);
            if register >= GEN12_LUMEN_L3CC_FIRST_REGISTER {
                readback.lumen_l3cc_mismatches = readback.lumen_l3cc_mismatches.saturating_add(1);
            }
        }
        if register == 2 {
            readback.l3cc_pair2 = observed;
        }
        if register == GEN12_LUMEN_MOCS_INDEX as usize / 2 {
            readback.l3cc_lumen_pair = observed;
        }
        register += 1;
    }
    readback.accepted = readback.global_mismatches == 0 && readback.l3cc_mismatches == 0;
    readback.lumen_half_accepted =
        readback.lumen_global_mismatches == 0 && readback.lumen_l3cc_mismatches == 0;
    readback
}

pub(super) fn init_lumen_mocs(dev: super::Dev) -> Gen12LumenMocsInitReport {
    let before = read_mocs(dev);
    if !before.available {
        return Gen12LumenMocsInitReport::default();
    }

    let mut index = GEN12_LUMEN_MOCS_FIRST_INDEX;
    while index < GEN12_GLOBAL_MOCS_ENTRIES {
        super::mmio_write(
            dev,
            GEN12_GLOBAL_MOCS_BASE + index * 4,
            GEN12_EXPECTED_MOCS_CONTROL[index],
        );
        index += 1;
    }
    let mut register = GEN12_LUMEN_L3CC_FIRST_REGISTER;
    while register < GEN12_LNCFCMOCS_REGISTERS {
        super::mmio_write(dev, GEN12_LNCFCMOCS_BASE + register * 4, expected_packed_l3cc(register));
        register += 1;
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    let after = read_mocs(dev);
    let residents_preserved = after.available
        && before.resident_global_fingerprint == after.resident_global_fingerprint
        && before.resident_l3cc_fingerprint == after.resident_l3cc_fingerprint;
    Gen12LumenMocsInitReport {
        available: after.available,
        accepted: residents_preserved && after.lumen_half_accepted,
        residents_preserved,
        before,
        after,
    }
}

pub(crate) const fn ratio_to_mhz(ratio: u32) -> u32 {
    // Gen9+ hardware opcodes are in 16.67 MHz units.
    (ratio.saturating_mul(50).saturating_add(1)) / 3
}

const fn rp_cap_50mhz_to_request_ratio(cap: u32) -> u32 {
    cap.saturating_mul(3)
}

const _: () = {
    assert!(rp_cap_50mhz_to_request_ratio(31) == 93);
    assert!(ratio_to_mhz(rp_cap_50mhz_to_request_ratio(31)) == 1_550);
};

pub(super) fn global_gt_power_marker() -> Gen12GlobalGtPowerMarker {
    GEN12_GLOBAL_GT_POWER_STATE.lock().marker
}

fn fused_rp0_ratio(dev: super::Dev) -> Result<u32, &'static str> {
    if !gt_state_registers_available(dev) {
        return Err("gt-frequency-registers-unavailable");
    }

    let state_cap = super::mmio_read(dev, GEN12_RP_STATE_CAP);
    let rp0_ratio = rp_cap_50mhz_to_request_ratio(state_cap & GEN12_RP0_CAP_MASK);
    if rp0_ratio == 0 || rp0_ratio > GEN12_CAGF_MASK {
        return Err("invalid-fused-rp0");
    }
    Ok(rp0_ratio)
}

fn transition_global_gt_power_mode_locked(
    dev: super::Dev,
    state: &mut Gen12GlobalGtPowerState,
    rp0_ratio: u32,
) -> Result<Gen12GlobalGtPowerMarker, &'static str> {
    let observed_before = super::mmio_read(dev, GEN12_RPNSWREQ);
    let observed_before_ratio =
        (observed_before & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
    let next_active = !state.active;
    let lumen_active = GEN12_LUMEN_GT_BOOST_ACTIVE.load(Ordering::Acquire);
    let restore_owned_request =
        !next_active && !lumen_active && observed_before_ratio == state.boost_ratio;

    if next_active {
        // If Lumen already owns the temporary RP0 request, remember the
        // request below it. The global mode then survives Lumen's scope and
        // can still restore the true predecessor when F12 turns it off.
        state.saved_request = if lumen_active {
            GEN12_LUMEN_GT_PREVIOUS_REQUEST.load(Ordering::Acquire)
        } else {
            observed_before
        };
        state.boost_ratio = rp0_ratio;
        let boost_request = (observed_before & !GEN9_SW_REQ_UNSLICE_RATIO_MASK)
            | (rp0_ratio << GEN9_SW_REQ_UNSLICE_RATIO_SHIFT);
        super::mmio_write(dev, GEN12_RPNSWREQ, boost_request);
    } else if restore_owned_request {
        // Restore only the ratio field owned by this mode. Preserve all other
        // resident firmware/request bits exactly as observed at toggle-off.
        let restore_request = (observed_before & !GEN9_SW_REQ_UNSLICE_RATIO_MASK)
            | (state.saved_request & GEN9_SW_REQ_UNSLICE_RATIO_MASK);
        super::mmio_write(dev, GEN12_RPNSWREQ, restore_request);
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);

    let observed_after = super::mmio_read(dev, GEN12_RPNSWREQ);
    let observed_after_ratio =
        (observed_after & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
    let expected_after_ratio = if next_active {
        rp0_ratio
    } else if restore_owned_request {
        (state.saved_request & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT
    } else {
        observed_before_ratio
    };
    if observed_after_ratio != expected_after_ratio {
        return Err(if next_active {
            "rp0-request-readback-mismatch"
        } else {
            "restore-request-readback-mismatch"
        });
    }

    state.active = next_active;
    state.generation = state.generation.wrapping_add(1).max(1);
    state.marker = Gen12GlobalGtPowerMarker {
        generation: state.generation,
        active: state.active,
        requested_mhz: ratio_to_mhz(observed_after_ratio),
        actual_mhz: ratio_to_mhz(actual_ratio(dev)),
        rp0_mhz: ratio_to_mhz(rp0_ratio),
    };
    let marker = state.marker;
    crate::log_info!(
        target: "render";
        "intel/gt-power-mode: marker={} active={} accepted=1 requested_mhz={} actual_mhz={} rp0_mhz={} previous_mhz={} lumen_active={} ownership=global-ui4-f12 pcoded-safety=retained spirit_signal=published-after-request-readback\n",
        marker.generation,
        marker.active as u8,
        marker.requested_mhz,
        marker.actual_mhz,
        marker.rp0_mhz,
        ratio_to_mhz(observed_before_ratio),
        lumen_active as u8,
    );
    Ok(marker)
}

pub(super) fn toggle_global_gt_power_mode(
    dev: super::Dev,
) -> Result<Gen12GlobalGtPowerMarker, &'static str> {
    let rp0_ratio = fused_rp0_ratio(dev)?;
    let mut state = GEN12_GLOBAL_GT_POWER_STATE.lock();
    let marker = transition_global_gt_power_mode_locked(dev, &mut state, rp0_ratio)?;
    // A human F12 decision always wins over any pending bounded boost.  Its
    // timer will observe the missing lease and cannot undo this transition.
    state.transient_boost = None;
    Ok(marker)
}

pub(super) fn begin_transient_global_gt_boost(
    dev: super::Dev,
) -> Result<Gen12TransientGtBoostStart, &'static str> {
    let rp0_ratio = fused_rp0_ratio(dev)?;
    let mut state = GEN12_GLOBAL_GT_POWER_STATE.lock();
    if state.active {
        return Ok(Gen12TransientGtBoostStart::AlreadyActive);
    }

    let marker = transition_global_gt_power_mode_locked(dev, &mut state, rp0_ratio)?;
    debug_assert!(marker.active);
    state.next_transient_boost_token = state.next_transient_boost_token.wrapping_add(1).max(1);
    let lease = Gen12TransientGtBoostLease {
        token: state.next_transient_boost_token,
        generation: marker.generation,
    };
    state.transient_boost = Some(lease);
    Ok(Gen12TransientGtBoostStart::Armed(lease))
}

/// Expire a bounded boost only if the global state has not changed since the
/// boost itself enabled it.  In particular, an F12 toggle clears the lease and
/// changes the marker generation before this can reach the hardware.
pub(super) fn expire_transient_global_gt_boost(
    dev: super::Dev,
    lease: Gen12TransientGtBoostLease,
) -> Result<bool, &'static str> {
    let rp0_ratio = fused_rp0_ratio(dev)?;
    let mut state = GEN12_GLOBAL_GT_POWER_STATE.lock();
    if !transient_boost_owns_current_state(&state, lease) {
        return Ok(false);
    }

    let marker = transition_global_gt_power_mode_locked(dev, &mut state, rp0_ratio)?;
    debug_assert!(!marker.active);
    state.transient_boost = None;
    Ok(true)
}

fn transient_boost_owns_current_state(
    state: &Gen12GlobalGtPowerState,
    lease: Gen12TransientGtBoostLease,
) -> bool {
    state.transient_boost == Some(lease)
        && state.active
        && state.marker.active
        && state.marker.generation == lease.generation
}

pub(super) fn begin_lumen_gt_boost(dev: super::Dev) -> Option<Gen12LumenGtBoost> {
    let global_power_state = GEN12_GLOBAL_GT_POWER_STATE.lock();
    if global_power_state.active {
        return None;
    }
    if !gt_state_registers_available(dev)
        || GEN12_LUMEN_GT_BOOST_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return None;
    }

    let previous_request = super::mmio_read(dev, GEN12_RPNSWREQ);
    GEN12_LUMEN_GT_PREVIOUS_REQUEST.store(previous_request, Ordering::Release);
    let previous_ratio =
        (previous_request & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
    let state_cap = super::mmio_read(dev, GEN12_RP_STATE_CAP);
    let rp0_ratio = rp_cap_50mhz_to_request_ratio(state_cap & GEN12_RP0_CAP_MASK);
    if rp0_ratio == 0 || rp0_ratio > GEN12_CAGF_MASK {
        GEN12_LUMEN_GT_PREVIOUS_REQUEST.store(0, Ordering::Release);
        GEN12_LUMEN_GT_BOOST_ACTIVE.store(false, Ordering::Release);
        return None;
    }
    let boost_ratio = core::cmp::max(previous_ratio, rp0_ratio);
    let boost_request = (previous_request & !GEN9_SW_REQ_UNSLICE_RATIO_MASK)
        | (boost_ratio << GEN9_SW_REQ_UNSLICE_RATIO_SHIFT);
    super::mmio_write(dev, GEN12_RPNSWREQ, boost_request);
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    let observed_request = super::mmio_read(dev, GEN12_RPNSWREQ);
    let observed_ratio =
        (observed_request & GEN9_SW_REQ_UNSLICE_RATIO_MASK) >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT;
    if observed_ratio != boost_ratio {
        GEN12_LUMEN_GT_PREVIOUS_REQUEST.store(0, Ordering::Release);
        GEN12_LUMEN_GT_BOOST_ACTIVE.store(false, Ordering::Release);
        crate::log_warn!(
            target: "gpgpu";
            "intel/lumen-gt-boost: stage=begin accepted=0 previous_ratio={} requested_ratio={} observed_ratio={} ownership=turn-scoped action=continue-at-firmware-frequency\n",
            previous_ratio,
            boost_ratio,
            observed_ratio,
        );
        return None;
    }
    drop(global_power_state);
    crate::log_info!(
        target: "gpgpu";
        "intel/lumen-gt-boost: stage=begin accepted=1 previous_ratio={} previous_mhz={} requested_ratio={} requested_mhz={} actual_ratio={} actual_mhz={} rp0_policy=turn-scoped restore=on-drop\n",
        previous_ratio,
        ratio_to_mhz(previous_ratio),
        boost_ratio,
        ratio_to_mhz(boost_ratio),
        actual_ratio(dev),
        ratio_to_mhz(actual_ratio(dev)),
    );
    Some(Gen12LumenGtBoost {
        dev,
        previous_request,
        previous_ratio,
        boost_ratio,
        active: true,
    })
}

pub(super) fn actual_ratio(dev: super::Dev) -> u32 {
    if !gt_state_registers_available(dev) {
        return 0;
    }
    (super::mmio_read(dev, GEN12_RPSTAT1) >> GEN12_CAGF_SHIFT) & GEN12_CAGF_MASK
}

pub(super) fn read(dev: super::Dev) -> Gen12GtStateSnapshot {
    if !gt_state_registers_available(dev) {
        return Gen12GtStateSnapshot::default();
    }
    let rpstat1_raw = super::mmio_read(dev, GEN12_RPSTAT1);
    let rpnswreq_raw = super::mmio_read(dev, GEN12_RPNSWREQ);
    let state_cap = super::mmio_read(dev, GEN12_RP_STATE_CAP);
    let frequency_info = super::mmio_read(dev, GEN10_FREQ_INFO_REC);
    let throttle_reasons_raw = super::mmio_read(dev, GEN12_GT0_PERF_LIMIT_REASONS);
    let actual_ratio = (rpstat1_raw >> GEN12_CAGF_SHIFT) & GEN12_CAGF_MASK;
    let requested_ratio = (rpnswreq_raw >> GEN9_SW_REQ_UNSLICE_RATIO_SHIFT) & GEN12_CAGF_MASK;
    Gen12GtStateSnapshot {
        available: true,
        actual_ratio,
        actual_mhz: ratio_to_mhz(actual_ratio),
        requested_ratio,
        requested_mhz: ratio_to_mhz(requested_ratio),
        rp0_mhz: (state_cap & 0xFF).saturating_mul(50),
        rpe_mhz: ((frequency_info >> 8) & 0xFF).saturating_mul(50),
        rpn_mhz: ((state_cap >> 16) & 0xFF).saturating_mul(50),
        throttle_reasons: throttle_reasons_raw & GEN12_GT0_PERF_LIMIT_REASONS_MASK,
        throttle_reasons_raw,
        rpstat1_raw,
        rpnswreq_raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_state(lease: Gen12TransientGtBoostLease) -> Gen12GlobalGtPowerState {
        Gen12GlobalGtPowerState {
            active: true,
            generation: lease.generation,
            transient_boost: Some(lease),
            next_transient_boost_token: lease.token,
            saved_request: 0,
            boost_ratio: 0,
            marker: Gen12GlobalGtPowerMarker {
                generation: lease.generation,
                active: true,
                requested_mhz: 0,
                actual_mhz: 0,
                rp0_mhz: 0,
            },
        }
    }

    #[test]
    fn transient_expiry_requires_the_exact_owned_generation() {
        let lease = Gen12TransientGtBoostLease {
            token: 7,
            generation: 41,
        };
        let state = active_state(lease);
        assert!(transient_boost_owns_current_state(&state, lease));
        assert!(!transient_boost_owns_current_state(
            &state,
            Gen12TransientGtBoostLease {
                token: 8,
                generation: 41,
            },
        ));
        assert!(!transient_boost_owns_current_state(
            &state,
            Gen12TransientGtBoostLease {
                token: 7,
                generation: 42,
            },
        ));
    }

    #[test]
    fn transient_expiry_cannot_override_a_manual_f12_transition() {
        let lease = Gen12TransientGtBoostLease {
            token: 9,
            generation: 12,
        };
        let mut state = active_state(lease);
        // Manual F12 changes both the generation and the pending ownership.
        state.active = false;
        state.marker.active = false;
        state.marker.generation = 13;
        state.transient_boost = None;
        assert!(!transient_boost_owns_current_state(&state, lease));
    }
}
