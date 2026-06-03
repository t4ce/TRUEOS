use core::sync::atomic::{AtomicU32, Ordering};

const SIMD16_LANES: usize = 16;
const REQUESTED_EUS: u32 = 32;
const RESULT_MARKER: u32 = 0xC0DE_7616;
const INPUT_A: [f32; SIMD16_LANES] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
];
const INPUT_B: [f32; SIMD16_LANES] = [
    16.0, 15.0, 14.0, 13.0, 12.0, 11.0, 10.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0,
];

static SUBMIT_SEQ: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Eu32Simd16ProbeStatus {
    NoIntelDevice,
    GuCNotReady,
    RenderWalkerNotLinked,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Eu32Simd16ProbeReport {
    pub(crate) seq: u32,
    pub(crate) requested_eus: u32,
    pub(crate) simd_width: u32,
    pub(crate) lane_mask: u32,
    pub(crate) result_marker: u32,
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) guc_ready: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) cpu_add_bits: [u32; SIMD16_LANES],
    pub(crate) cpu_mul_bits: [u32; SIMD16_LANES],
    pub(crate) add_checksum: u32,
    pub(crate) mul_checksum: u32,
    pub(crate) status: Eu32Simd16ProbeStatus,
    pub(crate) reason: &'static str,
    pub(crate) next_packet: &'static str,
}

impl Eu32Simd16ProbeStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NoIntelDevice => "no-intel-device",
            Self::GuCNotReady => "guc-not-ready",
            Self::RenderWalkerNotLinked => "render-gpgpu-walker-not-linked",
        }
    }
}

pub(crate) fn run_eu32_simd16_probe() -> Eu32Simd16ProbeReport {
    let seq = SUBMIT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    let mut report = base_report(seq);

    let Some(dev) = super::claimed_device() else {
        report.status = Eu32Simd16ProbeStatus::NoIntelDevice;
        report.reason = "intel-display-device-not-claimed";
        log_probe(report);
        return report;
    };

    report.device_id = dev.device_id;
    report.revision_id = dev.revision_id;
    report.guc_ready = super::guc_ready();
    if !report.guc_ready {
        report.status = Eu32Simd16ProbeStatus::GuCNotReady;
        report.reason = "guc-required-before-render-compute-submit";
        log_probe(report);
        return report;
    }

    report.status = Eu32Simd16ProbeStatus::RenderWalkerNotLinked;
    report.reason = "needs-rcs-state-base-interface-descriptor-gpgpu-walker-eu-isa";
    log_probe(report);
    report
}

fn base_report(seq: u32) -> Eu32Simd16ProbeReport {
    let mut cpu_add_bits = [0u32; SIMD16_LANES];
    let mut cpu_mul_bits = [0u32; SIMD16_LANES];
    let mut add_checksum = 0xA16D_0010u32;
    let mut mul_checksum = 0xB16D_0010u32;

    for lane in 0..SIMD16_LANES {
        let add = INPUT_A[lane] + INPUT_B[lane];
        let mul = INPUT_A[lane] * INPUT_B[lane];
        cpu_add_bits[lane] = add.to_bits();
        cpu_mul_bits[lane] = mul.to_bits();
        add_checksum = add_checksum.rotate_left(5) ^ cpu_add_bits[lane] ^ lane as u32;
        mul_checksum = mul_checksum.rotate_left(7) ^ cpu_mul_bits[lane] ^ ((lane as u32) << 16);
    }

    Eu32Simd16ProbeReport {
        seq,
        requested_eus: REQUESTED_EUS,
        simd_width: SIMD16_LANES as u32,
        lane_mask: 0x0000_FFFF,
        result_marker: RESULT_MARKER,
        device_id: 0,
        revision_id: 0,
        guc_ready: false,
        submitted: false,
        retired: false,
        cpu_add_bits,
        cpu_mul_bits,
        add_checksum,
        mul_checksum,
        status: Eu32Simd16ProbeStatus::RenderWalkerNotLinked,
        reason: "not-run",
        next_packet: "GPGPU_WALKER",
    }
}

fn log_probe(report: Eu32Simd16ProbeReport) {
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: eu32-simd16-probe seq={} status={} submitted={} retired={} device=0x{:04X} rev=0x{:02X} guc_ready={} eus={} simd={} lane_mask=0x{:08X} marker=0x{:08X} add_checksum=0x{:08X} mul_checksum=0x{:08X} next={} reason={}\n",
        report.seq,
        report.status.as_str(),
        report.submitted as u8,
        report.retired as u8,
        report.device_id,
        report.revision_id,
        report.guc_ready as u8,
        report.requested_eus,
        report.simd_width,
        report.lane_mask,
        report.result_marker,
        report.add_checksum,
        report.mul_checksum,
        report.next_packet,
        report.reason
    );
}
