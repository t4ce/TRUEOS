// Read-only Gen12 GT cache-policy and frequency-state diagnostics.
//
// Keep this module observational: Lumen diagnostics may sample these registers,
// but must not program shared GT cache, power, or frequency policy. The display
// compositor and inference workloads use the same integrated GT.

const GEN12_GLOBAL_MOCS_BASE: usize = 0x4000;
const GEN12_GLOBAL_MOCS_ENTRIES: usize = 64;
const GEN12_LNCFCMOCS_BASE: usize = 0xB020;
const GEN12_LNCFCMOCS_REGISTERS: usize = GEN12_GLOBAL_MOCS_ENTRIES / 2;
const GEN12_MOCS_DEFAULT_CONTROL: u32 = 0x0037;
const GEN12_MOCS_DEFAULT_L3CC: u16 = 0x0030;

const GEN12_RPNSWREQ: usize = 0xA008;
const GEN12_GT0_PERF_LIMIT_REASONS: usize = 0x1381A8;
const GEN12_RPSTAT1: usize = 0x1381B4;
const GEN12_RP_STATE_CAP: usize = 0x145998;
const GEN10_FREQ_INFO_REC: usize = 0x145EF0;
const GEN12_CAGF_MASK: u32 = 0x1FF;
const GEN12_CAGF_SHIFT: u32 = 11;
const GEN9_SW_REQ_UNSLICE_RATIO_SHIFT: u32 = 23;
const GEN12_GT0_PERF_LIMIT_REASONS_MASK: u32 = 0x0DE3;

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

const GEN12_EXPECTED_MOCS_CONTROL: [u32; GEN12_GLOBAL_MOCS_ENTRIES] =
    expected_mocs_control_table();
const GEN12_EXPECTED_MOCS_L3CC: [u16; GEN12_GLOBAL_MOCS_ENTRIES] =
    expected_mocs_l3cc_table();

const fn expected_packed_l3cc(register: usize) -> u32 {
    GEN12_EXPECTED_MOCS_L3CC[register * 2] as u32
        | ((GEN12_EXPECTED_MOCS_L3CC[register * 2 + 1] as u32) << 16)
}

const _: () = {
    assert!(GEN12_EXPECTED_MOCS_CONTROL[4] == 0x0005);
    assert!(expected_packed_l3cc(2) == 0x0010_0030);
};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Gen12MocsReadback {
    pub(super) available: bool,
    pub(super) accepted: bool,
    pub(super) global_mismatches: u32,
    pub(super) l3cc_mismatches: u32,
    pub(super) first_global_index: u32,
    pub(super) first_global_observed: u32,
    pub(super) first_global_expected: u32,
    pub(super) first_l3cc_register: u32,
    pub(super) first_l3cc_observed: u32,
    pub(super) first_l3cc_expected: u32,
    pub(super) global_index4: u32,
    pub(super) l3cc_pair2: u32,
    pub(super) global_fingerprint: u64,
    pub(super) l3cc_fingerprint: u64,
}

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
        global_fingerprint: 0xCBF2_9CE4_8422_2325,
        l3cc_fingerprint: 0xCBF2_9CE4_8422_2325,
        ..Gen12MocsReadback::default()
    };
    let mut index = 0usize;
    while index < GEN12_GLOBAL_MOCS_ENTRIES {
        let observed = super::mmio_read(dev, GEN12_GLOBAL_MOCS_BASE + index * 4);
        let expected = GEN12_EXPECTED_MOCS_CONTROL[index];
        readback.global_fingerprint = fingerprint_u32(readback.global_fingerprint, observed);
        if observed != expected {
            if readback.global_mismatches == 0 {
                readback.first_global_index = index as u32;
                readback.first_global_observed = observed;
                readback.first_global_expected = expected;
            }
            readback.global_mismatches = readback.global_mismatches.saturating_add(1);
        }
        if index == 4 {
            readback.global_index4 = observed;
        }
        index += 1;
    }

    let mut register = 0usize;
    while register < GEN12_LNCFCMOCS_REGISTERS {
        let observed = super::mmio_read(dev, GEN12_LNCFCMOCS_BASE + register * 4);
        let expected = expected_packed_l3cc(register);
        readback.l3cc_fingerprint = fingerprint_u32(readback.l3cc_fingerprint, observed);
        if observed != expected {
            if readback.l3cc_mismatches == 0 {
                readback.first_l3cc_register = register as u32;
                readback.first_l3cc_observed = observed;
                readback.first_l3cc_expected = expected;
            }
            readback.l3cc_mismatches = readback.l3cc_mismatches.saturating_add(1);
        }
        if register == 2 {
            readback.l3cc_pair2 = observed;
        }
        register += 1;
    }
    readback.accepted = readback.global_mismatches == 0 && readback.l3cc_mismatches == 0;
    readback
}

pub(crate) const fn ratio_to_mhz(ratio: u32) -> u32 {
    // Gen9+ hardware opcodes are in 16.67 MHz units.
    (ratio.saturating_mul(50).saturating_add(1)) / 3
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
