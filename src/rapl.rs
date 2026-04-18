#![allow(dead_code)]

use core::arch::x86_64::__cpuid;

use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Once;
use x86_64::registers::model_specific::Msr;

const MSR_RAPL_POWER_UNIT: u32 = 0x606;
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
const MSR_DRAM_ENERGY_STATUS: u32 = 0x619;
const MSR_PP0_ENERGY_STATUS: u32 = 0x639;
const MSR_PP1_ENERGY_STATUS: u32 = 0x641;
const MSR_PLATFORM_ENERGY_STATUS: u32 = 0x64D;
const RAPL_SERVICE_PERIOD_SECS: u64 = 1;
const RAPL_WATCH_RECEIVERS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct RaplCaps {
    pub vendor_intel: bool,
    pub has_msr: bool,
}

static CAPS: Once<Option<RaplCaps>> = Once::new();

#[derive(Clone, Copy, Debug)]
pub struct RaplUnits {
    pub power_watts: f64,
    pub energy_joules: f64,
    pub time_seconds: f64,
    pub power_raw_shift: u8,
    pub energy_raw_shift: u8,
    pub time_raw_shift: u8,
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

pub fn init() {
    CAPS.call_once(detect_caps_cpuid_only);
}

pub fn caps() -> Option<&'static RaplCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn supported_cpuid_only() -> bool {
    caps()
        .map(|caps| caps.vendor_intel && caps.has_msr)
        .unwrap_or(false)
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

/// Probes Intel RAPL MSRs on the current CPU.
///
/// This is intentionally opt-in because an RDMSR to an unsupported register
/// raises #GP. On this codebase's current early-boot paths that can still turn
/// into a reboot, so keep reads explicit and deliberate.
///
/// # Safety
/// Caller must ensure the current machine/firmware actually implements the
/// queried RAPL MSRs and that the exception path is safe if it does not.
pub unsafe fn probe_local() -> Option<RaplProbe> {
    init();
    if !supported_cpuid_only() {
        return None;
    }

    let units_raw = unsafe { Msr::new(MSR_RAPL_POWER_UNIT).read() };
    let units = decode_units(units_raw);

    Some(RaplProbe {
        units,
        package: read_sample(RaplDomain::Package, MSR_PKG_ENERGY_STATUS, units),
        core: read_sample(RaplDomain::Core, MSR_PP0_ENERGY_STATUS, units),
        graphics: read_sample(RaplDomain::Graphics, MSR_PP1_ENERGY_STATUS, units),
        dram: read_sample(RaplDomain::Dram, MSR_DRAM_ENERGY_STATUS, units),
        platform: read_sample(RaplDomain::Platform, MSR_PLATFORM_ENERGY_STATUS, units),
    })
}

/// Logs a one-shot local RAPL probe for Intel package/domain energy counters.
///
/// This helper is intentionally not called from boot yet; it exists so we can
/// wire it in later without redoing the RAPL decode logic.
///
/// # Safety
/// Same requirements as [`probe_local`].
pub unsafe fn log_local_probe() -> bool {
    let Some(probe) = (unsafe { probe_local() }) else {
        return false;
    };

    crate::log!(
        "rapl: units power=2^-{}W ({:.9}) energy=2^-{}J ({:.9}) time=2^-{}s ({:.9})\n",
        probe.units.power_raw_shift,
        probe.units.power_watts,
        probe.units.energy_raw_shift,
        probe.units.energy_joules,
        probe.units.time_raw_shift,
        probe.units.time_seconds,
    );

    for sample in probe.samples() {
        crate::log!(
            "rapl: domain={} desc={} msr=0x{:03X} raw=0x{:08X} joules={:.6}\n",
            sample.domain.short_name(),
            sample.domain.description(),
            sample.msr,
            sample.raw,
            sample.joules,
        );
    }

    true
}

#[embassy_executor::task]
pub async fn raple_service() {
    let sender = RAPL_WATCH.sender();

    crate::log!("rapl: service online period_s={}\n", RAPL_SERVICE_PERIOD_SECS);

    loop {
        let cpuid_supported = supported_cpuid_only();
        let latest = if cpuid_supported {
            unsafe { probe_local() }
        } else {
            None
        };

        let snapshot = RaplSnapshot {
            update_count: latest_snapshot().update_count.saturating_add(1),
            last_update_ms: service_now_ms(),
            cpuid_supported,
            sample_valid: latest.is_some(),
            latest,
        };

        sender.send(snapshot);
        Timer::after(EmbassyDuration::from_secs(RAPL_SERVICE_PERIOD_SECS)).await;
    }
}

pub fn wraparound_delta_joules(raw_start: u32, raw_end: u32, joules_per_tick: f64) -> f64 {
    let delta_ticks = raw_end.wrapping_sub(raw_start) as u64;
    (delta_ticks as f64) * joules_per_tick
}

fn detect_caps_cpuid_only() -> Option<RaplCaps> {
    let r0 = __cpuid(0x0);
    let vendor_intel = r0.ebx == 0x756e6547 && r0.edx == 0x49656e69 && r0.ecx == 0x6c65746e;

    let r1 = __cpuid(0x1);
    let has_msr = (r1.edx & (1 << 5)) != 0;

    Some(RaplCaps {
        vendor_intel,
        has_msr,
    })
}

fn decode_units(raw: u64) -> RaplUnits {
    let power_raw_shift = (raw & 0x0f) as u8;
    let energy_raw_shift = ((raw >> 8) & 0x1f) as u8;
    let time_raw_shift = ((raw >> 16) & 0x0f) as u8;

    RaplUnits {
        power_watts: 1.0 / ((1u64 << power_raw_shift) as f64),
        energy_joules: 1.0 / ((1u64 << energy_raw_shift) as f64),
        time_seconds: 1.0 / ((1u64 << time_raw_shift) as f64),
        power_raw_shift,
        energy_raw_shift,
        time_raw_shift,
    }
}

fn read_sample(domain: RaplDomain, msr: u32, units: RaplUnits) -> RaplSample {
    let raw = unsafe { Msr::new(msr).read() } as u32;
    RaplSample {
        domain,
        msr,
        raw,
        joules: (raw as f64) * units.energy_joules,
    }
}

fn service_now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}
