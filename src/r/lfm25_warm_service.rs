//! Boot-time preparation of the fixed LFM2.5 Intel inference assets.
//!
//! The fleet has three independent, sealed jobs: tokenizer, source-F32
//! sidecar, and native Q8 image read/pack/GPU bind. Work is pinned only to
//! AP2+ performance executors. The large model is published once behind an
//! immutable `Arc`; this service never creates multiple decoder backends.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embassy_executor::SpawnError;
use embassy_time::{Duration, Timer};

const WARM_TASK_CAP: usize = 3;
const RETRY_BASE_MS: u64 = 1_000;
const RETRY_MAX_MS: u64 = 300_000;
const TOKENIZER_READY: u8 = 1 << 0;
const F32_READY: u8 = 1 << 1;
const MODEL_READY: u8 = 1 << 2;
const ALL_READY: u8 = TOKENIZER_READY | F32_READY | MODEL_READY;

static READY_MASK: AtomicU8 = AtomicU8::new(0);
static LIVE_MASK: AtomicU8 = AtomicU8::new(0);
static RETRY_ATTEMPTS: [AtomicU8; WARM_TASK_CAP] = [const { AtomicU8::new(0) }; WARM_TASK_CAP];
static READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
enum WarmRole {
    Tokenizer,
    F32Sidecar,
    PackedModel,
}

impl WarmRole {
    const fn label(self) -> &'static str {
        match self {
            Self::Tokenizer => "tokenizer",
            Self::F32Sidecar => "f32-sidecar",
            Self::PackedModel => "packed-model",
        }
    }

    const fn ready_bit(self) -> u8 {
        match self {
            Self::Tokenizer => TOKENIZER_READY,
            Self::F32Sidecar => F32_READY,
            Self::PackedModel => MODEL_READY,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Tokenizer => 0,
            Self::F32Sidecar => 1,
            Self::PackedModel => 2,
        }
    }
}

fn retry_delay_ms(attempt: u8) -> u64 {
    let shift = u32::from(attempt.saturating_sub(1).min(8));
    RETRY_BASE_MS
        .saturating_mul(1u64 << shift)
        .min(RETRY_MAX_MS)
}

fn log_failure(role: WarmRole, slot: u32, error: &impl core::fmt::Debug) -> u64 {
    let attempt = RETRY_ATTEMPTS[role.index()]
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let retry_ms = retry_delay_ms(attempt);
    crate::log_warn!(
        target: "r";
        "lfm25-warm: role failed role={} slot={} attempt={} retry_ms={} error={:?} policy=fail-closed-single-builder\n",
        role.label(),
        slot,
        attempt,
        retry_ms,
        error,
    );
    retry_ms
}

fn record_ready(role: WarmRole, slot: u32) {
    RETRY_ATTEMPTS[role.index()].store(0, Ordering::Release);
    let ready = READY_MASK.fetch_or(role.ready_bit(), Ordering::AcqRel) | role.ready_bit();
    LIVE_MASK.fetch_and(!role.ready_bit(), Ordering::AcqRel);
    crate::log_info!(
        target: "r";
        "lfm25-warm: role ready role={} slot={} ready_mask=0x{:02X}\n",
        role.label(),
        slot,
        ready,
    );
    if ready == ALL_READY
        && crate::r::lfm25_hybrid_cpu_backend::intel_igc_resident_assets_ready()
        && crate::r::lfm25_tokenizer::resident_ready()
        && READY_LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        crate::log_info!(
            target: "r";
            "lfm25-warm: resident ready roles=3 model_copies=1 placement=AP2+-pcore mutable_session_state=fresh\n"
        );
    }
}

#[embassy_executor::task(pool_size = WARM_TASK_CAP)]
async fn warm_asset_task(role: WarmRole, expected_slot: u32) {
    let actual_slot = crate::percpu::current_slot() as u32;
    let actual_kind = crate::workers::core_kind_for_slot(actual_slot);
    if actual_slot != expected_slot
        || !crate::workers::is_background_worker_slot(actual_slot)
        || actual_kind != crate::workers::CORE_KIND_PERF
    {
        crate::log_warn!(
            target: "r";
            "lfm25-warm: refused role={} expected_slot={} actual_slot={} core_kind={}\n",
            role.label(),
            expected_slot,
            actual_slot,
            actual_kind,
        );
        LIVE_MASK.fetch_and(!role.ready_bit(), Ordering::AcqRel);
        crate::r::spawn_service::retry_lfm25_warm_pool_autostart();
        return;
    }

    crate::log_info!(
        target: "r";
        "lfm25-warm: role begin role={} slot={} core_kind={} placement=AP2+-pcore\n",
        role.label(),
        actual_slot,
        actual_kind,
    );
    let retry_ms = match role {
        WarmRole::Tokenizer => match crate::r::lfm25_tokenizer::load().await {
            Ok(_) => None,
            Err(error) => Some(log_failure(role, actual_slot, &error)),
        },
        WarmRole::F32Sidecar => {
            match crate::r::lfm25_hybrid_cpu_backend::warm_intel_igc_f32().await {
                Ok(()) => None,
                Err(error) => Some(log_failure(role, actual_slot, &error)),
            }
        }
        WarmRole::PackedModel => {
            match crate::r::lfm25_hybrid_cpu_backend::warm_intel_igc_model().await {
                Ok(()) => None,
                Err(error) => Some(log_failure(role, actual_slot, &error)),
            }
        }
    };
    if let Some(retry_ms) = retry_ms {
        Timer::after(Duration::from_millis(retry_ms)).await;
        LIVE_MASK.fetch_and(!role.ready_bit(), Ordering::AcqRel);
        crate::r::spawn_service::retry_lfm25_warm_pool_autostart();
        return;
    }
    record_ready(role, actual_slot);
}

fn eligible_perf_slots() -> Vec<u32> {
    crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| crate::workers::core_kind_for_slot(*slot) == crate::workers::CORE_KIND_PERF)
        .take(WARM_TASK_CAP)
        .collect()
}

/// Spawn all three sealed warm roles across at most three AP2+ P-core slots.
///
/// Fewer available P-core APs still run every role by sharing an executor;
/// neither the BSP nor the AP1 UI/service executor is a fallback.
pub(crate) fn spawn() -> Result<bool, SpawnError> {
    if !crate::workers::all_topology_spawners_registered() {
        return Ok(false);
    }
    let slots = eligible_perf_slots();
    if slots.is_empty() {
        return Ok(false);
    }
    let roles = [
        WarmRole::Tokenizer,
        WarmRole::F32Sidecar,
        WarmRole::PackedModel,
    ];
    let mut spawned = 0usize;
    let mut active = 0usize;
    for (index, role) in roles.into_iter().enumerate() {
        let role_bit = role.ready_bit();
        if READY_MASK.load(Ordering::Acquire) & role_bit != 0
            || LIVE_MASK.fetch_or(role_bit, Ordering::AcqRel) & role_bit != 0
        {
            active += 1;
            continue;
        }
        let slot = slots[index % slots.len()];
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            LIVE_MASK.fetch_and(!role_bit, Ordering::AcqRel);
            continue;
        };
        let token = match warm_asset_task(role, slot) {
            Ok(token) => token,
            Err(error) if spawned == 0 && active == 0 => {
                LIVE_MASK.fetch_and(!role_bit, Ordering::AcqRel);
                return Err(error);
            }
            Err(error) => {
                LIVE_MASK.fetch_and(!role_bit, Ordering::AcqRel);
                crate::log_warn!(
                    target: "r";
                    "lfm25-warm: spawn failed role={} slot={} error={:?}\n",
                    role.label(),
                    slot,
                    error,
                );
                continue;
            }
        };
        let wake_sent = spawner.spawn_and_wake_remote(token);
        crate::log_info!(
            target: "r";
            "lfm25-warm: role spawned role={} slot={} wake_sent={}\n",
            role.label(),
            slot,
            wake_sent,
        );
        spawned += 1;
        active += 1;
    }
    crate::log_info!(
        target: "r";
        "lfm25-warm: fleet admitted spawned_now={} active_or_ready={} ap_slots={} cap={} slots={:?}\n",
        spawned,
        active,
        slots.len(),
        WARM_TASK_CAP,
        slots,
    );
    // A partial launch remains safe because every asset cache has one builder,
    // but it is not a completed autostart. Let the registry retry; already
    // spawned roles either finish or make the retry a cheap cache hit.
    Ok(active == roles.len())
}
