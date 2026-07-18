
extern crate alloc;

use alloc::string::String;
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};

const THERMAL_WATCH_RECEIVERS: usize = 8;
const THERMAL_CORE_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;

#[derive(Clone, Copy, Debug)]
pub struct ThermalCaps {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_eist: bool,
    pub has_dts: bool,
    pub has_ptm: bool,
    pub has_turbo_boost: bool,
    pub has_turbo_boost3: bool,
    pub has_hw_coord_feedback: bool,
    pub dts_irq_thresholds: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThermalDomain {
    Core,
    Package,
}

impl ThermalDomain {
    pub fn short_name(self) -> &'static str {
        match self {
            ThermalDomain::Core => "core",
            ThermalDomain::Package => "pkg",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ThermalDomain::Core => "per-core digital thermal sensor",
            ThermalDomain::Package => "package thermal sensor",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ThermalSample {
    pub domain: ThermalDomain,
    pub msr: u32,
    pub raw: u64,
    pub reading_valid: bool,
    pub delta_to_tjmax_celsius: u8,
    pub temperature_celsius: Option<i16>,
    pub thermal_status: bool,
    pub thermal_log: bool,
    pub prochot_status: bool,
    pub prochot_log: bool,
    pub critical_status: bool,
    pub critical_log: bool,
    pub threshold1_status: bool,
    pub threshold1_log: bool,
    pub threshold2_status: bool,
    pub threshold2_log: bool,
    pub power_limit_status: bool,
    pub power_limit_log: bool,
    pub current_limit_status: bool,
    pub current_limit_log: bool,
    pub cross_domain_limit_status: bool,
    pub cross_domain_limit_log: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CoreThermalSample {
    pub slot: usize,
    pub online: bool,
    pub completed: bool,
    pub core_kind: u8,
    pub executor_spawned: Option<usize>,
    pub executor_ready: Option<usize>,
    pub hlt_now: Option<bool>,
    pub hlt_recent_active: Option<u8>,
    pub source: &'static str,
    pub age_ms: Option<u64>,
    pub stale: bool,
    pub perf_status_raw: Option<u64>,
    pub perf_ratio: Option<u8>,
    pub aperf_delta: Option<u64>,
    pub mperf_delta: Option<u64>,
    pub effective_permille: Option<u64>,
    pub sample: Option<ThermalSample>,
}

#[derive(Clone, Debug)]
pub struct ThermalSnapshot {
    pub update_count: u64,
    pub last_update_ms: u64,
    pub cpuid_supported: bool,
    pub sample_valid: bool,
    pub sampling_scope: &'static str,
    pub tj_max_celsius: Option<u8>,
    pub total_cpus: usize,
    pub online_cpus: usize,
    pub completed_cpus: usize,
    pub busy_aps: usize,
    pub timed_out: bool,
    pub package: Option<ThermalSample>,
    pub cores: [Option<CoreThermalSample>; THERMAL_CORE_LIMIT],
}

impl ThermalSnapshot {
    pub const fn empty() -> Self {
        Self {
            update_count: 0,
            last_update_ms: 0,
            cpuid_supported: false,
            sample_valid: false,
            sampling_scope: "none",
            tj_max_celsius: None,
            total_cpus: 0,
            online_cpus: 0,
            completed_cpus: 0,
            busy_aps: 0,
            timed_out: false,
            package: None,
            cores: [None; THERMAL_CORE_LIMIT],
        }
    }

    pub const fn has_data(&self) -> bool {
        self.sample_valid
    }
}

static THERMAL_WATCH: Watch<
    crate::wait::EmbassySpinRawMutex,
    ThermalSnapshot,
    THERMAL_WATCH_RECEIVERS,
> = Watch::new_with(ThermalSnapshot::empty());

pub type ThermalReceiver<'a> =
    WatchReceiver<'a, crate::wait::EmbassySpinRawMutex, ThermalSnapshot, THERMAL_WATCH_RECEIVERS>;

pub fn init() {}

pub fn caps() -> Option<&'static ThermalCaps> {
    None
}

pub fn supported_cpuid_only() -> bool {
    false
}

pub fn latest_snapshot() -> ThermalSnapshot {
    THERMAL_WATCH
        .try_get()
        .unwrap_or_else(ThermalSnapshot::empty)
}

pub fn latest_snapshot_text() -> String {
    String::from("thermal snapshot\ncpuid_supported=false\nsample_valid=false\n")
}

pub fn subscribe() -> Option<ThermalReceiver<'static>> {
    THERMAL_WATCH.receiver()
}

pub fn anon_snapshot() -> ThermalSnapshot {
    let mut receiver = THERMAL_WATCH.anon_receiver();
    receiver.try_get().unwrap_or_else(ThermalSnapshot::empty)
}

pub fn refresh_snapshot_once() -> ThermalSnapshot {
    let snapshot = ThermalSnapshot::empty();
    THERMAL_WATCH.sender().send(snapshot.clone());
    snapshot
}

pub fn poll_current_core_passive(_hard_sleep_ticks: u64) {}

pub unsafe fn probe_local() -> Option<(Option<u8>, ThermalSample, Option<ThermalSample>)> {
    None
}

#[embassy_executor::task]
pub async fn thermal_service() {
    THERMAL_WATCH.sender().send(ThermalSnapshot::empty());
}
