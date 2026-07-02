use alloc::string::String;
use core::fmt::Write;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use embassy_time::{Duration as EmbassyDuration, Timer};
use raw_cpuid::CpuId;
use spin::Once;
use x86_64::registers::model_specific::Msr;

const MSR_IA32_THERM_STATUS: u32 = 0x19C;
const MSR_IA32_PERF_STATUS: u32 = 0x198;
const MSR_IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const MSR_IA32_MPERF: u32 = 0xE7;
const MSR_IA32_APERF: u32 = 0xE8;
const THERMAL_SERVICE_SAMPLE_PERIOD_MS: u64 = 1000;
const THERMAL_PASSIVE_CORE_SAMPLE_PERIOD_MS: u64 = 60_000;
const THERMAL_PASSIVE_CORE_SAMPLE_SLACK_MS: u64 = 30_000;
const THERMAL_WATCH_RECEIVERS: usize = 8;
const THERMAL_CORE_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const THERM_STATUS_READING_VALID: u64 = 1 << 31;
const PASSIVE_PRESENT_NONE: u8 = 0;
const PASSIVE_PRESENT_SAMPLED: u8 = 1;

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

static CAPS: Once<Option<ThermalCaps>> = Once::new();

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
static PASSIVE_CORE_RAW: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_SAMPLE_MS: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_NEXT_DUE_MS: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_PRESENT: [AtomicU8; THERMAL_CORE_LIMIT] =
    [const { AtomicU8::new(PASSIVE_PRESENT_NONE) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_PERF_STATUS: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_APERF_DELTA: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_MPERF_DELTA: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_EFFECTIVE_PERMILLE: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_PREV_APERF: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];
static PASSIVE_CORE_PREV_MPERF: [AtomicU64; THERMAL_CORE_LIMIT] =
    [const { AtomicU64::new(0) }; THERMAL_CORE_LIMIT];

pub type ThermalReceiver<'a> =
    WatchReceiver<'a, crate::wait::EmbassySpinRawMutex, ThermalSnapshot, THERMAL_WATCH_RECEIVERS>;

pub fn init() {
    CAPS.call_once(detect_caps_cpuid_only);
}

pub fn caps() -> Option<&'static ThermalCaps> {
    CAPS.get().and_then(|caps| caps.as_ref())
}

pub fn supported_cpuid_only() -> bool {
    init();
    caps()
        .map(|caps| caps.vendor_intel && caps.has_msr && caps.has_dts)
        .unwrap_or(false)
}

pub fn latest_snapshot() -> ThermalSnapshot {
    THERMAL_WATCH
        .try_get()
        .unwrap_or_else(ThermalSnapshot::empty)
}

pub fn latest_snapshot_text() -> String {
    format_snapshot_text(&latest_snapshot())
}

pub fn subscribe() -> Option<ThermalReceiver<'static>> {
    THERMAL_WATCH.receiver()
}

pub fn anon_snapshot() -> ThermalSnapshot {
    let mut receiver = THERMAL_WATCH.anon_receiver();
    receiver.try_get().unwrap_or_else(ThermalSnapshot::empty)
}

pub fn refresh_snapshot_once() -> ThermalSnapshot {
    let sender = THERMAL_WATCH.sender();
    let previous = latest_snapshot();
    let cpuid_supported = supported_cpuid_only();
    let now_ms = service_now_ms();
    let snapshot = if cpuid_supported {
        unsafe { probe_all(previous.update_count.saturating_add(1), now_ms) }
    } else {
        let mut snapshot = ThermalSnapshot::empty();
        snapshot.update_count = previous.update_count.saturating_add(1);
        snapshot.last_update_ms = now_ms;
        snapshot.cpuid_supported = false;
        snapshot
    };
    sender.send(snapshot.clone());
    snapshot
}

/// Probes Intel thermal MSRs on the current CPU.
///
/// # Safety
/// Caller must ensure the current hardware implements the queried thermal MSRs
/// and that the exception path is safe if the firmware/CPU rejects a read.
pub unsafe fn probe_local() -> Option<(Option<u8>, ThermalSample, Option<ThermalSample>)> {
    if !supported_cpuid_only() {
        return None;
    }
    let tj_max = read_tj_max_celsius();
    let core = read_sample(ThermalDomain::Core, MSR_IA32_THERM_STATUS, tj_max);
    let package = caps()
        .filter(|caps| caps.has_ptm)
        .map(|_| read_sample(ThermalDomain::Package, MSR_IA32_PACKAGE_THERM_STATUS, tj_max));
    Some((tj_max, core, package))
}

#[embassy_executor::task]
pub async fn thermal_service() {
    crate::log_info!(
        target: "boot";
        "thermal: service online sample_ms={} passive_core_ms={} passive_slack_ms={}\n",
        THERMAL_SERVICE_SAMPLE_PERIOD_MS,
        THERMAL_PASSIVE_CORE_SAMPLE_PERIOD_MS,
        THERMAL_PASSIVE_CORE_SAMPLE_SLACK_MS
    );

    loop {
        let _ = refresh_snapshot_once();
        Timer::after(EmbassyDuration::from_millis(THERMAL_SERVICE_SAMPLE_PERIOD_MS)).await;
    }
}

/// Opportunistically samples the current AP's core-local thermal MSR.
///
/// This is a passive runtime hook, not an Embassy task. It never arms a timer
/// and never returns a wake deadline; it can only run while the AP is awake for
/// some other reason.
pub fn poll_current_core_passive(_hard_sleep_ticks: u64) {
    if !supported_cpuid_only() {
        return;
    }

    let slot = crate::percpu::current_slot_via_cpuid();
    if slot == 0 || slot >= THERMAL_CORE_LIMIT {
        return;
    }

    let now_ms = service_now_ms();
    let next_due_ms = PASSIVE_CORE_NEXT_DUE_MS[slot].load(Ordering::Acquire);
    if next_due_ms != 0 && !passive_sample_window_open(now_ms, next_due_ms) {
        return;
    }

    let tj_max = read_tj_max_celsius();
    let sample = read_sample(ThermalDomain::Core, MSR_IA32_THERM_STATUS, tj_max);
    let perf_status = read_perf_status_if_supported().unwrap_or(0);
    let (aperf_delta, mperf_delta, effective_permille) = read_aperf_mperf_delta(slot);
    PASSIVE_CORE_RAW[slot].store(sample.raw, Ordering::Release);
    PASSIVE_CORE_PERF_STATUS[slot].store(perf_status, Ordering::Release);
    PASSIVE_CORE_APERF_DELTA[slot].store(aperf_delta, Ordering::Release);
    PASSIVE_CORE_MPERF_DELTA[slot].store(mperf_delta, Ordering::Release);
    PASSIVE_CORE_EFFECTIVE_PERMILLE[slot].store(effective_permille, Ordering::Release);
    PASSIVE_CORE_SAMPLE_MS[slot].store(now_ms, Ordering::Release);
    PASSIVE_CORE_PRESENT[slot].store(PASSIVE_PRESENT_SAMPLED, Ordering::Release);
    PASSIVE_CORE_NEXT_DUE_MS[slot]
        .store(now_ms.saturating_add(THERMAL_PASSIVE_CORE_SAMPLE_PERIOD_MS), Ordering::Release);
}

fn passive_sample_window_open(now_ms: u64, next_due_ms: u64) -> bool {
    if now_ms >= next_due_ms {
        return true;
    }

    let early_window_ms = next_due_ms.saturating_sub(THERMAL_PASSIVE_CORE_SAMPLE_SLACK_MS);
    now_ms >= early_window_ms
}

fn read_perf_status_if_supported() -> Option<u64> {
    caps()
        .filter(|caps| caps.vendor_intel && caps.has_msr && caps.has_eist)
        .map(|_| unsafe { Msr::new(MSR_IA32_PERF_STATUS).read() })
}

fn perf_ratio_from_status(raw: u64) -> u8 {
    ((raw >> 8) & 0xff) as u8
}

fn read_aperf_mperf_delta(slot: usize) -> (u64, u64, u64) {
    if !caps()
        .map(|caps| caps.vendor_intel && caps.has_msr && caps.has_hw_coord_feedback)
        .unwrap_or(false)
    {
        return (0, 0, 0);
    }

    let aperf = unsafe { Msr::new(MSR_IA32_APERF).read() };
    let mperf = unsafe { Msr::new(MSR_IA32_MPERF).read() };
    let prev_aperf = PASSIVE_CORE_PREV_APERF[slot].swap(aperf, Ordering::AcqRel);
    let prev_mperf = PASSIVE_CORE_PREV_MPERF[slot].swap(mperf, Ordering::AcqRel);
    if prev_aperf == 0 || prev_mperf == 0 {
        return (0, 0, 0);
    }

    let aperf_delta = aperf.wrapping_sub(prev_aperf);
    let mperf_delta = mperf.wrapping_sub(prev_mperf);
    let effective_permille = if mperf_delta == 0 {
        0
    } else {
        ((aperf_delta as u128).saturating_mul(1000) / mperf_delta as u128) as u64
    };
    (aperf_delta, mperf_delta, effective_permille)
}

unsafe fn probe_all(update_count: u64, now_ms: u64) -> ThermalSnapshot {
    let Some((tj_max, bsp_core, package)) = (unsafe { probe_local() }) else {
        let mut snapshot = ThermalSnapshot::empty();
        snapshot.update_count = update_count;
        snapshot.last_update_ms = now_ms;
        snapshot.cpuid_supported = supported_cpuid_only();
        return snapshot;
    };

    let total = crate::smp::cpu_count().min(THERMAL_CORE_LIMIT);
    let mut snapshot = ThermalSnapshot {
        update_count,
        last_update_ms: now_ms,
        cpuid_supported: true,
        sample_valid: bsp_core.reading_valid || package.map(|s| s.reading_valid).unwrap_or(false),
        sampling_scope: "package+bsp-local+passive-ap-cache",
        tj_max_celsius: tj_max,
        total_cpus: total,
        online_cpus: 0,
        completed_cpus: 0,
        busy_aps: 0,
        timed_out: false,
        package,
        cores: [None; THERMAL_CORE_LIMIT],
    };

    if total == 0 {
        return snapshot;
    }

    let bsp_perf_status = read_perf_status_if_supported();
    snapshot.cores[0] = Some(CoreThermalSample {
        slot: 0,
        online: true,
        completed: true,
        core_kind: crate::workers::core_kind_for_slot(0),
        executor_spawned: executor_spawned_for_slot(0),
        executor_ready: executor_ready_for_slot(0),
        hlt_now: crate::smp::read(0).map(|info| info.hlt_now),
        hlt_recent_active: hlt_recent_active_for_slot(0),
        source: "bsp-live",
        age_ms: Some(0),
        stale: false,
        perf_status_raw: bsp_perf_status,
        perf_ratio: bsp_perf_status.map(perf_ratio_from_status),
        aperf_delta: None,
        mperf_delta: None,
        effective_permille: None,
        sample: Some(bsp_core),
    });
    snapshot.online_cpus = 1;
    snapshot.completed_cpus = 1;

    // Per-core IA32_THERM_STATUS is CPU-local. Most APs can be parked in HLT,
    // so the low-rate background service deliberately avoids SMP mailbox
    // requests here; otherwise a thermal snapshot can leave AP mailboxes busy
    // and produce misleading no-reply rows. Package temperature is still useful
    // and is readable from BSP.
    for slot in 1..total {
        let Some(info) = crate::smp::read(slot) else {
            continue;
        };
        if !info.online {
            continue;
        }
        snapshot.online_cpus = snapshot.online_cpus.saturating_add(1);
        let cached = passive_cached_core_sample(slot, tj_max, now_ms);
        if cached.sample.is_some() {
            snapshot.completed_cpus = snapshot.completed_cpus.saturating_add(1);
            snapshot.sample_valid = true;
        }
        snapshot.cores[slot] = Some(cached);
    }

    snapshot
}

fn passive_cached_core_sample(slot: usize, tj_max: Option<u8>, now_ms: u64) -> CoreThermalSample {
    if PASSIVE_CORE_PRESENT[slot].load(Ordering::Acquire) != PASSIVE_PRESENT_SAMPLED {
        return CoreThermalSample {
            slot,
            online: true,
            completed: false,
            core_kind: crate::workers::core_kind_for_slot(slot as u32),
            executor_spawned: executor_spawned_for_slot(slot),
            executor_ready: executor_ready_for_slot(slot),
            hlt_now: crate::smp::read(slot).map(|info| info.hlt_now),
            hlt_recent_active: hlt_recent_active_for_slot(slot),
            source: "passive-never-awake",
            age_ms: None,
            stale: true,
            perf_status_raw: None,
            perf_ratio: None,
            aperf_delta: None,
            mperf_delta: None,
            effective_permille: None,
            sample: None,
        };
    }

    let sample_ms = PASSIVE_CORE_SAMPLE_MS[slot].load(Ordering::Acquire);
    let raw = PASSIVE_CORE_RAW[slot].load(Ordering::Acquire);
    let perf_status_raw = zero_to_none(PASSIVE_CORE_PERF_STATUS[slot].load(Ordering::Acquire));
    let aperf_delta = zero_to_none(PASSIVE_CORE_APERF_DELTA[slot].load(Ordering::Acquire));
    let mperf_delta = zero_to_none(PASSIVE_CORE_MPERF_DELTA[slot].load(Ordering::Acquire));
    let effective_permille =
        zero_to_none(PASSIVE_CORE_EFFECTIVE_PERMILLE[slot].load(Ordering::Acquire));
    let age_ms = now_ms.saturating_sub(sample_ms);
    let stale_after =
        THERMAL_PASSIVE_CORE_SAMPLE_PERIOD_MS.saturating_add(THERMAL_PASSIVE_CORE_SAMPLE_SLACK_MS);
    CoreThermalSample {
        slot,
        online: true,
        completed: true,
        core_kind: crate::workers::core_kind_for_slot(slot as u32),
        executor_spawned: executor_spawned_for_slot(slot),
        executor_ready: executor_ready_for_slot(slot),
        hlt_now: crate::smp::read(slot).map(|info| info.hlt_now),
        hlt_recent_active: hlt_recent_active_for_slot(slot),
        source: "passive-cache",
        age_ms: Some(age_ms),
        stale: age_ms > stale_after,
        perf_status_raw,
        perf_ratio: perf_status_raw.map(perf_ratio_from_status),
        aperf_delta,
        mperf_delta,
        effective_permille,
        sample: Some(decode_sample(ThermalDomain::Core, MSR_IA32_THERM_STATUS, raw, tj_max)),
    }
}

fn executor_spawned_for_slot(slot: usize) -> Option<usize> {
    crate::workers::spawner_for_slot(slot as u32).map(|spawner| spawner.spawned_task_count())
}

fn executor_ready_for_slot(slot: usize) -> Option<usize> {
    crate::workers::spawner_for_slot(slot as u32).map(|spawner| spawner.ready_task_count())
}

fn hlt_recent_active_for_slot(slot: usize) -> Option<u8> {
    crate::smp::hlt_history_text(slot).map(|history| {
        history
            .bytes()
            .filter(|byte| *byte == b'!')
            .count()
            .min(u8::MAX as usize) as u8
    })
}

fn zero_to_none(value: u64) -> Option<u64> {
    if value == 0 { None } else { Some(value) }
}

fn detect_caps_cpuid_only() -> Option<ThermalCaps> {
    let cpuid = CpuId::new();
    let vendor_intel = cpuid
        .get_vendor_info()
        .map(|vendor| vendor.as_str() == "GenuineIntel")
        .unwrap_or(false);
    let features = cpuid.get_feature_info();
    let has_msr = features
        .as_ref()
        .map(|features| features.has_msr())
        .unwrap_or(false);
    let has_eist = vendor_intel
        && features
            .as_ref()
            .map(|features| features.has_eist())
            .unwrap_or(false);
    let thermal = cpuid.get_thermal_power_info();
    let has_dts = vendor_intel && thermal.as_ref().map(|info| info.has_dts()).unwrap_or(false);
    let has_ptm = vendor_intel && thermal.as_ref().map(|info| info.has_ptm()).unwrap_or(false);
    let has_turbo_boost = vendor_intel
        && thermal
            .as_ref()
            .map(|info| info.has_turbo_boost())
            .unwrap_or(false);
    let has_turbo_boost3 = vendor_intel
        && thermal
            .as_ref()
            .map(|info| info.has_turbo_boost3())
            .unwrap_or(false);
    let has_hw_coord_feedback = vendor_intel
        && thermal
            .as_ref()
            .map(|info| info.has_hw_coord_feedback())
            .unwrap_or(false);
    let dts_irq_thresholds = thermal
        .as_ref()
        .map(|info| info.dts_irq_threshold())
        .unwrap_or(0);

    Some(ThermalCaps {
        vendor_intel,
        has_msr,
        has_eist,
        has_dts,
        has_ptm,
        has_turbo_boost,
        has_turbo_boost3,
        has_hw_coord_feedback,
        dts_irq_thresholds,
    })
}

fn read_tj_max_celsius() -> Option<u8> {
    let raw = unsafe { Msr::new(MSR_IA32_TEMPERATURE_TARGET).read() };
    let tj_max = ((raw >> 16) & 0xff) as u8;
    if tj_max == 0 { None } else { Some(tj_max) }
}

fn read_sample(domain: ThermalDomain, msr: u32, tj_max_celsius: Option<u8>) -> ThermalSample {
    let raw = unsafe { Msr::new(msr).read() };
    decode_sample(domain, msr, raw, tj_max_celsius)
}

fn decode_sample(
    domain: ThermalDomain,
    msr: u32,
    raw: u64,
    tj_max_celsius: Option<u8>,
) -> ThermalSample {
    let reading_valid = (raw & THERM_STATUS_READING_VALID) != 0;
    let delta_to_tjmax_celsius = ((raw >> 16) & 0x7f) as u8;
    let temperature_celsius = if reading_valid {
        tj_max_celsius.map(|tj_max| i16::from(tj_max) - i16::from(delta_to_tjmax_celsius))
    } else {
        None
    };

    ThermalSample {
        domain,
        msr,
        raw,
        reading_valid,
        delta_to_tjmax_celsius,
        temperature_celsius,
        thermal_status: bit(raw, 0),
        thermal_log: bit(raw, 1),
        prochot_status: bit(raw, 2),
        prochot_log: bit(raw, 3),
        critical_status: bit(raw, 4),
        critical_log: bit(raw, 5),
        threshold1_status: bit(raw, 6),
        threshold1_log: bit(raw, 7),
        threshold2_status: bit(raw, 8),
        threshold2_log: bit(raw, 9),
        power_limit_status: bit(raw, 10),
        power_limit_log: bit(raw, 11),
        current_limit_status: bit(raw, 12),
        current_limit_log: bit(raw, 13),
        cross_domain_limit_status: bit(raw, 14),
        cross_domain_limit_log: bit(raw, 15),
    }
}

fn bit(raw: u64, idx: u32) -> bool {
    (raw & (1u64 << idx)) != 0
}

fn format_snapshot_text(snapshot: &ThermalSnapshot) -> String {
    let mut out = String::new();
    let caps = caps().copied();
    let _ = writeln!(out, "thermal snapshot");
    let _ = writeln!(out, "update_count={}", snapshot.update_count);
    let _ = writeln!(out, "last_update_ms={}", snapshot.last_update_ms);
    let _ = writeln!(out, "intel_cpuid={}", caps.map(|caps| caps.vendor_intel).unwrap_or(false));
    let _ = writeln!(out, "msr_cpuid={}", caps.map(|caps| caps.has_msr).unwrap_or(false));
    let _ = writeln!(out, "eist_cpuid={}", caps.map(|caps| caps.has_eist).unwrap_or(false));
    let _ = writeln!(out, "dts_cpuid={}", caps.map(|caps| caps.has_dts).unwrap_or(false));
    let _ = writeln!(out, "ptm_cpuid={}", caps.map(|caps| caps.has_ptm).unwrap_or(false));
    let _ = writeln!(
        out,
        "turbo_boost_cpuid={}",
        caps.map(|caps| caps.has_turbo_boost).unwrap_or(false)
    );
    let _ = writeln!(
        out,
        "turbo_boost3_cpuid={}",
        caps.map(|caps| caps.has_turbo_boost3).unwrap_or(false)
    );
    let _ = writeln!(
        out,
        "aperf_mperf_cpuid={}",
        caps.map(|caps| caps.has_hw_coord_feedback).unwrap_or(false)
    );
    let _ = writeln!(
        out,
        "dts_irq_thresholds={}",
        caps.map(|caps| caps.dts_irq_thresholds).unwrap_or(0)
    );
    let _ = writeln!(out, "cpuid_supported={}", snapshot.cpuid_supported);
    let _ = writeln!(out, "sample_valid={}", snapshot.sample_valid);
    let _ = writeln!(out, "sampling_scope={}", snapshot.sampling_scope);
    let _ = writeln!(
        out,
        "passive_core_period_ms={} passive_core_slack_ms={}",
        THERMAL_PASSIVE_CORE_SAMPLE_PERIOD_MS, THERMAL_PASSIVE_CORE_SAMPLE_SLACK_MS
    );
    let _ = writeln!(out, "tj_max_celsius={}", fmt_opt_u8(snapshot.tj_max_celsius));
    let _ = writeln!(
        out,
        "cpus total={} online={} completed={} busy_aps={} timed_out={}",
        snapshot.total_cpus,
        snapshot.online_cpus,
        snapshot.completed_cpus,
        snapshot.busy_aps,
        snapshot.timed_out
    );

    if let Some(package) = snapshot.package {
        let _ = writeln!(
            out,
            "domain,description,msr,raw,temp_c,delta_to_tjmax,valid,thermal,prochot,critical,power_limit,current_limit,cross_domain,state"
        );
        write_sample_row(&mut out, package);
    } else {
        let _ = writeln!(out, "package=-");
    }

    let _ = writeln!(
        out,
        "slot,online,source,age_ms,kind,spawned,ready,hlt_now,hlt_active80,hlt_history,perf_ratio,perf_status,aperf_delta,mperf_delta,eff_permille,raw,temp_c,delta_to_tjmax,valid,thermal,prochot,critical,power_limit,current_limit,cross_domain,state"
    );
    for core in snapshot.cores.iter().flatten() {
        let hlt_history =
            crate::smp::hlt_history_text(core.slot).unwrap_or_else(|| String::from("-"));
        let Some(sample) = core.sample else {
            let _ = writeln!(
                out,
                "{},{},{},-,{},{},{},{},{},{},{},{},{},{},{},-,-,-,false,-,-,-,-,-,-,stale",
                core.slot,
                core.online,
                core.source,
                core_kind_name(core.core_kind),
                fmt_opt_usize(core.executor_spawned),
                fmt_opt_usize(core.executor_ready),
                fmt_opt_bool(core.hlt_now),
                fmt_opt_u8(core.hlt_recent_active),
                hlt_history,
                fmt_opt_u8(core.perf_ratio),
                fmt_opt_hex_u64(core.perf_status_raw),
                fmt_opt_u64(core.aperf_delta),
                fmt_opt_u64(core.mperf_delta),
                fmt_opt_u64(core.effective_permille),
            );
            continue;
        };
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},0x{:016X},{},{},{},{},{},{},{},{},{},{}",
            core.slot,
            core.online,
            core.source,
            fmt_opt_u64(core.age_ms),
            core_kind_name(core.core_kind),
            fmt_opt_usize(core.executor_spawned),
            fmt_opt_usize(core.executor_ready),
            fmt_opt_bool(core.hlt_now),
            fmt_opt_u8(core.hlt_recent_active),
            hlt_history,
            fmt_opt_u8(core.perf_ratio),
            fmt_opt_hex_u64(core.perf_status_raw),
            fmt_opt_u64(core.aperf_delta),
            fmt_opt_u64(core.mperf_delta),
            fmt_opt_u64(core.effective_permille),
            sample.raw,
            fmt_opt_i16(sample.temperature_celsius),
            sample.delta_to_tjmax_celsius,
            sample.reading_valid,
            sample.thermal_status,
            sample.prochot_status,
            sample.critical_status,
            sample.power_limit_status,
            sample.current_limit_status,
            sample.cross_domain_limit_status,
            if core.stale {
                "stale"
            } else {
                sample_state(sample)
            }
        );
    }

    out
}

fn core_kind_name(kind: u8) -> &'static str {
    match kind {
        crate::workers::CORE_KIND_PERF => "perf",
        crate::workers::CORE_KIND_EFF => "eff",
        _ => "unknown",
    }
}

fn write_sample_row(out: &mut String, sample: ThermalSample) {
    let _ = writeln!(
        out,
        "{},{},0x{:03X},0x{:016X},{},{},{},{},{},{},{},{},{},{}",
        sample.domain.short_name(),
        sample.domain.description(),
        sample.msr,
        sample.raw,
        fmt_opt_i16(sample.temperature_celsius),
        sample.delta_to_tjmax_celsius,
        sample.reading_valid,
        sample.thermal_status,
        sample.prochot_status,
        sample.critical_status,
        sample.power_limit_status,
        sample.current_limit_status,
        sample.cross_domain_limit_status,
        sample_state(sample)
    );
}

fn sample_state(sample: ThermalSample) -> &'static str {
    if sample.critical_status {
        "critical"
    } else if sample.prochot_status || sample.thermal_status {
        "hot"
    } else if sample.power_limit_status || sample.current_limit_status {
        "limited"
    } else if sample.reading_valid {
        "ok"
    } else {
        "invalid"
    }
}

fn fmt_opt_u8(value: Option<u8>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn fmt_opt_usize(value: Option<usize>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn fmt_opt_i16(value: Option<i16>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn fmt_opt_bool(value: Option<bool>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn fmt_opt_u64(value: Option<u64>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn fmt_opt_hex_u64(value: Option<u64>) -> String {
    value
        .map(|value| alloc::format!("0x{:016X}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn service_now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}
