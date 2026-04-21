#![allow(dead_code)]

use embassy_sync::watch::{Receiver as WatchReceiver, Watch};

// ARMTODO: RAPL is an Intel/x86 MSR-based energy telemetry path. A real
// non-x86 implementation would need platform-specific power/energy counters
// and service wiring instead of Intel CPUID + RAPL MSRs.

const RAPL_WATCH_RECEIVERS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct RaplCaps {
    pub vendor_intel: bool,
    pub has_msr: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RaplUnits {
    pub power_watts: f64,
    pub energy_joules: f64,
    pub time_seconds: f64,
    pub power_raw_shift: u8,
    pub energy_raw_shift: u8,
    pub time_raw_shift: u8,
}

impl RaplUnits {
    pub const fn empty() -> Self {
        Self {
            power_watts: 0.0,
            energy_joules: 0.0,
            time_seconds: 0.0,
            power_raw_shift: 0,
            energy_raw_shift: 0,
            time_raw_shift: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaplDomain {
    Package,
    Core,
    Graphics,
    Dram,
    Platform,
}

impl RaplDomain {
    pub fn short_name(self) -> &'static str {
        match self {
            RaplDomain::Package => "pkg",
            RaplDomain::Core => "pp0",
            RaplDomain::Graphics => "pp1",
            RaplDomain::Dram => "dram",
            RaplDomain::Platform => "psys",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            RaplDomain::Package => "package",
            RaplDomain::Core => "cores/pp0",
            RaplDomain::Graphics => "graphics/pp1",
            RaplDomain::Dram => "dram",
            RaplDomain::Platform => "platform",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RaplSample {
    pub domain: RaplDomain,
    pub msr: u32,
    pub raw: u32,
    pub joules: f64,
}

impl RaplSample {
    pub fn delta_joules_since(self, earlier: Self, units: RaplUnits) -> Option<f64> {
        if self.domain != earlier.domain {
            return None;
        }

        Some(wraparound_delta_joules(earlier.raw, self.raw, units.energy_joules))
    }

    pub fn average_power_watts_since(
        self,
        earlier: Self,
        units: RaplUnits,
        elapsed_seconds: f64,
    ) -> Option<f64> {
        if self.domain != earlier.domain || elapsed_seconds <= 0.0 {
            return None;
        }

        Some(self.delta_joules_since(earlier, units)? / elapsed_seconds)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RaplProbe {
    pub units: RaplUnits,
    pub package: RaplSample,
    pub core: RaplSample,
    pub graphics: RaplSample,
    pub dram: RaplSample,
    pub platform: RaplSample,
}

impl RaplProbe {
    pub fn samples(self) -> [RaplSample; 5] {
        [
            self.package,
            self.core,
            self.graphics,
            self.dram,
            self.platform,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RaplSnapshot {
    pub update_count: u64,
    pub last_update_ms: u64,
    pub cpuid_supported: bool,
    pub sample_valid: bool,
    pub latest: Option<RaplProbe>,
}

impl RaplSnapshot {
    pub const fn empty() -> Self {
        Self {
            update_count: 0,
            last_update_ms: 0,
            cpuid_supported: false,
            sample_valid: false,
            latest: None,
        }
    }

    pub const fn has_data(self) -> bool {
        self.sample_valid
    }
}

static RAPL_WATCH: Watch<crate::wait::EmbassySpinRawMutex, RaplSnapshot, RAPL_WATCH_RECEIVERS> =
    Watch::new_with(RaplSnapshot::empty());

pub type RaplReceiver<'a> =
    WatchReceiver<'a, crate::wait::EmbassySpinRawMutex, RaplSnapshot, RAPL_WATCH_RECEIVERS>;

pub fn init() {}

pub fn caps() -> Option<&'static RaplCaps> {
    None
}

pub fn supported_cpuid_only() -> bool {
    false
}

pub fn latest_snapshot() -> RaplSnapshot {
    RAPL_WATCH.try_get().unwrap_or(RaplSnapshot::empty())
}

pub fn subscribe() -> Option<RaplReceiver<'static>> {
    RAPL_WATCH.receiver()
}

pub fn anon_snapshot() -> RaplSnapshot {
    let mut receiver = RAPL_WATCH.anon_receiver();
    receiver.try_get().unwrap_or(RaplSnapshot::empty())
}

pub unsafe fn probe_local() -> Option<RaplProbe> {
    None
}

pub unsafe fn log_local_probe() -> bool {
    false
}

#[embassy_executor::task]
pub async fn raple_service() {
    let sender = RAPL_WATCH.sender();
    sender.send(RaplSnapshot::empty());
}

pub fn wraparound_delta_joules(raw_start: u32, raw_end: u32, joules_per_tick: f64) -> f64 {
    let delta_ticks = raw_end.wrapping_sub(raw_start) as u64;
    (delta_ticks as f64) * joules_per_tick
}