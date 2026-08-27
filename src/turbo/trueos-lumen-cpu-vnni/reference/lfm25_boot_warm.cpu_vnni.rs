//! Optional post-compositor warmup for the one boot-resident Lumen model.
//!
//! Only immutable, reusable assets are prepared here. Conversation state,
//! prompt prefill, and CPU projection work remain demand-driven.

use trueos_time::{Duration, Instant, Timer};

fn elapsed_ms_since(started: Instant) -> u64 {
    Instant::now()
        .as_millis()
        .saturating_sub(started.as_millis())
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn service_task(expected_worker_slot: u32) {
    let actual_worker_slot = crate::percpu::current_slot() as u32;
    let actual_core_kind = crate::workers::core_kind_for_slot(actual_worker_slot);
    if actual_worker_slot != expected_worker_slot
        || !crate::workers::is_background_worker_slot(actual_worker_slot)
        || actual_core_kind != crate::workers::CORE_KIND_PERF
    {
        crate::log_error!(
            target: "service";
            "lfm25: boot-warm stage=refused expected_background_ap={} actual_cpu_slot={} actual_core_kind={} policy=ap2+-perf-only\n",
            expected_worker_slot,
            actual_worker_slot,
            actual_core_kind,
        );
        return;
    }

    Timer::after(Duration::from_millis(crate::allcaps::lumen::BOOT_RESIDENT_WARM_SETTLE_MS)).await;

    let projector = match trueos_lfm25_cpu::Q8VnniProjector::detect() {
        Ok(projector) => projector,
        Err(error) => {
            crate::log_warn!(
                target: "service";
                "lfm25: boot-warm stage=deferred accepted=0 executor_slot={} backend=cpu-vnni error={:?} action=leave-cold-for-demand-open\n",
                actual_worker_slot,
                error,
            );
            return;
        }
    };
    let capabilities = projector.capabilities();

    let started = Instant::now();
    crate::log_info!(
        target: "service";
        "lfm25: boot-warm stage=start scope=reusable-assets executor_slot={} core_kind={} settle_ms={} tokenizer_artifact_bytes={} model_artifact_bytes={} f32_artifact_bytes={} backend=cpu-vnni ymm_state={} avx2={} avx_vnni={} fma={} warm_contract=no-project conversation_state=deferred prompt_prefill=deferred\n",
        actual_worker_slot,
        actual_core_kind,
        crate::allcaps::lumen::BOOT_RESIDENT_WARM_SETTLE_MS,
        crate::r::lfm25_tokenizer::TOKENIZER_BYTES,
        crate::r::lfm25_model::NATIVE_IMAGE_BYTES,
        trueos_lfm25_cpu::F32_SIDECAR_BYTES,
        capabilities.ymm_state() as u8,
        capabilities.avx2() as u8,
        capabilities.avx_vnni() as u8,
        capabilities.fma() as u8,
    );

    let tokenizer_started = Instant::now();
    let tokenizer_ready = match crate::r::lfm25_tokenizer::load().await {
        Ok(_) => {
            crate::log_info!(
                target: "service";
                "lfm25: boot-warm stage=tokenizer-ready elapsed_ms={} resident=1\n",
                elapsed_ms_since(tokenizer_started),
            );
            true
        }
        Err(error) => {
            crate::log_warn!(
                target: "service";
                "lfm25: boot-warm stage=tokenizer-failed elapsed_ms={} error={:?} action=continue-other-assets\n",
                elapsed_ms_since(tokenizer_started),
                error,
            );
            false
        }
    };

    let model_started = Instant::now();
    let model_ready = match crate::r::lfm25_hybrid_cpu_backend::warm_cpu_vnni_model().await {
        Ok(()) => {
            crate::log_info!(
                target: "service";
                "lfm25: boot-warm stage=model-ready elapsed_ms={} resident=1 layout=q8_0-row34 backend=cpu-vnni model_mutation=none warm_contract=no-project\n",
                elapsed_ms_since(model_started),
            );
            true
        }
        Err(error) => {
            crate::log_warn!(
                target: "service";
                "lfm25: boot-warm stage=model-failed elapsed_ms={} error={:?} action=continue-f32\n",
                elapsed_ms_since(model_started),
                error,
            );
            false
        }
    };

    let f32_started = Instant::now();
    let f32_ready = match crate::r::lfm25_hybrid_cpu_backend::warm_cpu_vnni_f32().await {
        Ok(()) => {
            crate::log_info!(
                target: "service";
                "lfm25: boot-warm stage=f32-ready elapsed_ms={} resident=1\n",
                elapsed_ms_since(f32_started),
            );
            true
        }
        Err(error) => {
            crate::log_warn!(
                target: "service";
                "lfm25: boot-warm stage=f32-failed elapsed_ms={} error={:?} action=leave-missing-asset-for-demand-open\n",
                elapsed_ms_since(f32_started),
                error,
            );
            false
        }
    };

    let assets_ready = crate::r::lfm25_hybrid_cpu_backend::cpu_vnni_resident_assets_ready();
    let accepted = tokenizer_ready && model_ready && f32_ready && assets_ready;
    crate::log_info!(
        target: "service";
        "lfm25: boot-warm stage=done accepted={} elapsed_ms={} tokenizer_ready={} model_ready={} f32_ready={} resident_assets_ready={} executor_slot={} backend=cpu-vnni warm_contract=no-project conversation_state=deferred prompt_prefill=deferred first_lum_work=session-state-allocation+prompt-encode+first-cpu-projection\n",
        accepted as u8,
        elapsed_ms_since(started),
        tokenizer_ready as u8,
        model_ready as u8,
        f32_ready as u8,
        assets_ready as u8,
        actual_worker_slot,
    );
}
