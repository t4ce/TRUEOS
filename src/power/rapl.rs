use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use embassy_time::{Duration as EmbassyDuration, Timer};
use raw_cpuid::CpuId;
use spin::{Mutex, Once};
use x86_64::registers::model_specific::Msr;

const MSR_RAPL_POWER_UNIT: u32 = 0x606;
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
const MSR_DRAM_ENERGY_STATUS: u32 = 0x619;
const MSR_PP0_ENERGY_STATUS: u32 = 0x639;
const MSR_PP1_ENERGY_STATUS: u32 = 0x641;
const MSR_PLATFORM_ENERGY_STATUS: u32 = 0x64D;
const RAPL_SERVICE_SAMPLE_PERIOD_MS: u64 = 100;
const RAPL_TRUEOSFS_PERSIST_PERIOD_MS: u64 = 10_000;
const RAPL_TRUEOSFS_PATH: &str = "rapl.txt";
pub const RAPL_HISTORY_MAX_BYTES: usize = 5 * 1024 * 1024;
const RAPL_HISTORY_TRIM_BYTES: usize = 1024 * 1024;
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
    pub interval_ms: u64,
    pub cpuid_supported: bool,
    pub sample_valid: bool,
    pub previous: Option<RaplProbe>,
    pub latest: Option<RaplProbe>,
}

impl RaplSnapshot {
    pub const fn empty() -> Self {
        Self {
            update_count: 0,
            last_update_ms: 0,
            interval_ms: 0,
            cpuid_supported: false,
            sample_valid: false,
            previous: None,
            latest: None,
        }
    }

    pub const fn has_data(self) -> bool {
        self.sample_valid
    }
}

static RAPL_WATCH: Watch<crate::wait::EmbassySpinRawMutex, RaplSnapshot, RAPL_WATCH_RECEIVERS> =
    Watch::new_with(RaplSnapshot::empty());
static RAPL_HISTORY: Mutex<Vec<u8>> = Mutex::new(Vec::new());

pub type RaplReceiver<'a> =
    WatchReceiver<'a, crate::wait::EmbassySpinRawMutex, RaplSnapshot, RAPL_WATCH_RECEIVERS>;

pub fn init() {
    CAPS.call_once(detect_caps_cpuid_only);
}

pub fn caps() -> Option<&'static RaplCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn supported_cpuid_only() -> bool {
    init();
    caps()
        .map(|caps| caps.vendor_intel && caps.has_msr)
        .unwrap_or(false)
}

pub fn latest_snapshot() -> RaplSnapshot {
    RAPL_WATCH.try_get().unwrap_or(RaplSnapshot::empty())
}

pub fn latest_snapshot_text() -> String {
    format_snapshot_text(latest_snapshot())
}

pub fn history_len() -> usize {
    RAPL_HISTORY.lock().len()
}

pub fn copy_history_slice(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let history = RAPL_HISTORY.lock();
    if offset >= history.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), history.len() - offset);
    out[..n].copy_from_slice(&history[offset..offset + n]);
    n
}

pub fn subscribe() -> Option<RaplReceiver<'static>> {
    RAPL_WATCH.receiver()
}

pub fn anon_snapshot() -> RaplSnapshot {
    let mut receiver = RAPL_WATCH.anon_receiver();
    receiver.try_get().unwrap_or(RaplSnapshot::empty())
}

pub fn refresh_snapshot_once() -> RaplSnapshot {
    let sender = RAPL_WATCH.sender();
    let previous_snapshot = latest_snapshot();
    let cpuid_supported = supported_cpuid_only();
    let latest = if cpuid_supported {
        unsafe { probe_local() }
    } else {
        None
    };
    let now_ms = service_now_ms();
    let previous = if previous_snapshot.sample_valid {
        previous_snapshot.latest
    } else {
        None
    };
    let interval_ms = if previous.is_some() && latest.is_some() {
        now_ms.saturating_sub(previous_snapshot.last_update_ms)
    } else {
        0
    };

    let snapshot = RaplSnapshot {
        update_count: previous_snapshot.update_count.saturating_add(1),
        last_update_ms: now_ms,
        interval_ms,
        cpuid_supported,
        sample_valid: latest.is_some(),
        previous,
        latest,
    };

    sender.send(snapshot);
    snapshot
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
    crate::log_info!(
        target: "boot";
        "rapl: service online sample_ms={} persist_ms={} path={}\n",
        RAPL_SERVICE_SAMPLE_PERIOD_MS,
        RAPL_TRUEOSFS_PERSIST_PERIOD_MS,
        RAPL_TRUEOSFS_PATH
    );

    let mut next_persist_ms = service_now_ms().saturating_add(RAPL_TRUEOSFS_PERSIST_PERIOD_MS);
    loop {
        let snapshot = refresh_snapshot_once();
        append_snapshot_to_history(snapshot);
        let now_ms = snapshot.last_update_ms;
        if now_ms >= next_persist_ms {
            persist_history_to_trueosfs().await;
            next_persist_ms = now_ms.saturating_add(RAPL_TRUEOSFS_PERSIST_PERIOD_MS);
        }
        Timer::after(EmbassyDuration::from_millis(RAPL_SERVICE_SAMPLE_PERIOD_MS)).await;
    }
}

pub fn wraparound_delta_joules(raw_start: u32, raw_end: u32, joules_per_tick: f64) -> f64 {
    let delta_ticks = raw_end.wrapping_sub(raw_start) as u64;
    (delta_ticks as f64) * joules_per_tick
}

fn detect_caps_cpuid_only() -> Option<RaplCaps> {
    let cpuid = CpuId::new();
    let vendor_intel = cpuid
        .get_vendor_info()
        .map(|vendor| vendor.as_str() == "GenuineIntel")
        .unwrap_or(false);
    let has_msr = cpuid
        .get_feature_info()
        .map(|features| features.has_msr())
        .unwrap_or(false);

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

async fn persist_history_to_trueosfs() {
    if !crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
        return;
    }
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return;
    };

    let history = history_bytes_snapshot();
    match crate::r::fs::trueosfs::file_in_async(disk, RAPL_TRUEOSFS_PATH, &history).await {
        Ok(true) => {}
        Ok(false) => {
            crate::log_info!(
                target: "rapl";
                "rapl: persist skipped path={} reason=no-space-or-fs\n",
                RAPL_TRUEOSFS_PATH
            );
        }
        Err(err) => {
            crate::log_info!(
                target: "rapl";
                "rapl: persist failed path={} err={:?}\n",
                RAPL_TRUEOSFS_PATH,
                err
            );
        }
    }
}

fn append_snapshot_to_history(snapshot: RaplSnapshot) {
    let text = format_snapshot_text(snapshot);
    let mut history = RAPL_HISTORY.lock();
    history.extend_from_slice(text.as_bytes());
    if !history.ends_with(b"\n") {
        history.push(b'\n');
    }
    history.push(b'\n');
    trim_history_if_needed(&mut history);
}

fn history_bytes_snapshot() -> Vec<u8> {
    RAPL_HISTORY.lock().clone()
}

fn trim_history_if_needed(history: &mut Vec<u8>) {
    if history.len() <= RAPL_HISTORY_MAX_BYTES {
        return;
    }

    let overshoot = history.len().saturating_sub(RAPL_HISTORY_MAX_BYTES);
    let mut drop_len = core::cmp::max(RAPL_HISTORY_TRIM_BYTES, overshoot);
    drop_len = core::cmp::min(drop_len, history.len());

    if drop_len < history.len() {
        if let Some(extra) = history[drop_len..].iter().position(|byte| *byte == b'\n') {
            drop_len = core::cmp::min(history.len(), drop_len.saturating_add(extra + 1));
        }
    }

    history.drain(..drop_len);
}

fn format_snapshot_text(snapshot: RaplSnapshot) -> String {
    let mut out = String::new();
    let caps = caps().copied();
    let _ = writeln!(out, "rapl snapshot");
    let _ = writeln!(out, "update_count={}", snapshot.update_count);
    let _ = writeln!(out, "last_update_ms={}", snapshot.last_update_ms);
    let _ = writeln!(out, "interval_ms={}", snapshot.interval_ms);
    let _ = writeln!(out, "intel_cpuid={}", caps.map(|caps| caps.vendor_intel).unwrap_or(false));
    let _ = writeln!(out, "msr_cpuid={}", caps.map(|caps| caps.has_msr).unwrap_or(false));
    let _ = writeln!(out, "cpuid_supported={}", snapshot.cpuid_supported);
    let _ = writeln!(out, "sample_valid={}", snapshot.sample_valid);

    let Some(probe) = snapshot.latest else {
        return out;
    };

    let _ = writeln!(
        out,
        "units power=2^-{}W ({:.9}) energy=2^-{}J ({:.9}) time=2^-{}s ({:.9})",
        probe.units.power_raw_shift,
        probe.units.power_watts,
        probe.units.energy_raw_shift,
        probe.units.energy_joules,
        probe.units.time_raw_shift,
        probe.units.time_seconds,
    );
    let _ = writeln!(out, "domain,description,msr,raw,joules,delta_joules,watts,state");
    let interval_seconds = snapshot.interval_ms as f64 / 1000.0;
    for sample in probe.samples() {
        let previous_sample = snapshot
            .previous
            .and_then(|probe| sample_for_domain(probe, sample.domain));
        let delta_joules =
            previous_sample.and_then(|earlier| sample.delta_joules_since(earlier, probe.units));
        let watts = previous_sample.and_then(|earlier| {
            sample.average_power_watts_since(earlier, probe.units, interval_seconds)
        });
        let state = if sample.raw == 0 {
            "zero/absent?"
        } else if watts.is_some() {
            "active"
        } else {
            "sampled"
        };
        let _ = writeln!(
            out,
            "{},{},0x{:03X},0x{:08X},{:.6},{},{},{}",
            sample.domain.short_name(),
            sample.domain.description(),
            sample.msr,
            sample.raw,
            sample.joules,
            delta_joules
                .map(|delta| alloc::format!("{:.6}", delta))
                .unwrap_or_else(|| String::from("-")),
            watts
                .map(|watts| alloc::format!("{:.3}", watts))
                .unwrap_or_else(|| String::from("-")),
            state
        );
    }

    out
}

fn sample_for_domain(probe: RaplProbe, domain: RaplDomain) -> Option<RaplSample> {
    probe
        .samples()
        .iter()
        .copied()
        .find(|sample| sample.domain == domain)
}

fn service_now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}
