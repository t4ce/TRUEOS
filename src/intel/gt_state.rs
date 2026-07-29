// Read-only Gen12 GT frequency-state diagnostics.
//
// Keep this module observational: Lumen diagnostics may sample these registers,
// but must not program shared GT cache, power, or frequency policy. The display
// compositor and inference workloads use the same integrated GT.

const GEN12_RPNSWREQ: usize = 0xA008;
const GEN12_GT0_PERF_LIMIT_REASONS: usize = 0x1381A8;
const GEN12_RPSTAT1: usize = 0x1381B4;
const GEN12_RP_STATE_CAP: usize = 0x145998;
const GEN10_FREQ_INFO_REC: usize = 0x145EF0;
const GEN12_CAGF_MASK: u32 = 0x1FF;
const GEN12_CAGF_SHIFT: u32 = 11;
const GEN9_SW_REQ_UNSLICE_RATIO_SHIFT: u32 = 23;
const GEN12_GT0_PERF_LIMIT_REASONS_MASK: u32 = 0x0DE3;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
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
    pub(crate) rpstat1_raw: u32,
    pub(crate) rpnswreq_raw: u32,
}

fn registers_available(dev: super::Dev) -> bool {
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

pub(crate) const fn ratio_to_mhz(ratio: u32) -> u32 {
    // Gen9+ hardware opcodes are in 16.67 MHz units.
    (ratio.saturating_mul(50).saturating_add(1)) / 3
}

pub(super) fn actual_ratio(dev: super::Dev) -> u32 {
    if !registers_available(dev) {
        return 0;
    }
    (super::mmio_read(dev, GEN12_RPSTAT1) >> GEN12_CAGF_SHIFT) & GEN12_CAGF_MASK
}

pub(super) fn read(dev: super::Dev) -> Gen12GtStateSnapshot {
    if !registers_available(dev) {
        return Gen12GtStateSnapshot::default();
    }
    let rpstat1_raw = super::mmio_read(dev, GEN12_RPSTAT1);
    let rpnswreq_raw = super::mmio_read(dev, GEN12_RPNSWREQ);
    let state_cap = super::mmio_read(dev, GEN12_RP_STATE_CAP);
    let frequency_info = super::mmio_read(dev, GEN10_FREQ_INFO_REC);
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
        throttle_reasons: super::mmio_read(dev, GEN12_GT0_PERF_LIMIT_REASONS)
            & GEN12_GT0_PERF_LIMIT_REASONS_MASK,
        rpstat1_raw,
        rpnswreq_raw,
    }
}
