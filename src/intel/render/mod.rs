// Render proof contract.
//
// The goal of this file is not "triangle or nothing."  Each probe should
// move one named boundary and say what it does not prove:
//
// - `batch-submit-proof`: RCS/execlist accepted enough command stream to run
//   markers.  Current captures still show `final_marker=0`, so full retire is
//   not proven.
// - `mi-scanout-store-proof`: RCS command streamer wrote one DWORD into the
//   live scanout surface via MI_STORE_DATA_IMM.  This proves neither 3D stage
//   progress nor PS/color-backend writes.
// - `memory-proof`: warm render buffers were mapped into their fixed GGTT
//   slots, cache-flushed, and CPU-read back in one source-level proof line.
//   This does not prove each 3D stage actually consumed its buffer.
// - `gpgpu-preflight`: RCS submission writes deterministic vector proof results
//   into the warm result buffer. This proves the buffer/result runway we need
//   for GPGPU bring-up, but not EU thread execution or matmul arithmetic yet.
// - `vertex-upload-proof`: CPU wrote/read back the triangle vertex bytes and
//   flushed them.  This does not prove VF consumed them.
// - `vf-proof`: IA/VF counters advance for three vertices.  Current captures
//   prove this with `vf-proof accepted=1`.
// - `vs-proof`: AOT/uploaded VS bytes match and VS counters advance.  Current
//   captures prove this with `vs-proof accepted=1 vs_delta=3`.
// - `clip-raster-proof`: clipper counters advance.  Current captures prove this
//   on the VF draw path only; VS-to-clipper handoff is still the frontier.
// - `ps-dispatch-proof` and `ps-rt-proof`: not proven in current captures;
//   `ps_delta=0`, `rt_any_change=0`.
//
// Keep these proof lines conservative.  Packet markers are useful context, but
// stage proofs should only accept on counters or memory changes owned by that
// boundary.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

static SUMMARY_ONLY_SUBMIT_DEPTH: AtomicU32 = AtomicU32::new(0);

// Host diagnostic only: keep the public material ABI and its reserved fields
// unchanged. Every view uses the same retained VS and full PBR PS contract.
static PICASSO_MATERIAL_VIEW: AtomicU32 = AtomicU32::new(0);
static PICASSO_DEPTH_TEST_ENABLED: AtomicBool = AtomicBool::new(true);
static PICASSO_CULL_ENABLED: AtomicBool = AtomicBool::new(true);
static PICASSO_SHADER_PIPELINE: AtomicU32 = AtomicU32::new(0);
static PICASSO_VUE_CAPTURE_PENDING: AtomicBool = AtomicBool::new(false);

pub(crate) fn request_picasso_vue_capture() {
    PICASSO_VUE_CAPTURE_PENDING.store(true, Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-vue-capture: armed=1 scope=next-eligible-retained-draw\n",
    );
}

pub(crate) fn take_picasso_vue_capture() -> bool {
    PICASSO_VUE_CAPTURE_PENDING.swap(false, Ordering::AcqRel)
}

pub(crate) fn set_picasso_uv_pipeline_enabled(enabled: bool) {
    PICASSO_SHADER_PIPELINE.store(u32::from(enabled), Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-pipeline-view: pipeline={} applies=next-encoded-pbr-draw\n",
        if enabled { "authored-uv" } else { "pbr" },
    );
}

pub(crate) fn picasso_uv_pipeline_enabled() -> bool {
    PICASSO_SHADER_PIPELINE.load(Ordering::Acquire) != 0
}

pub(crate) fn set_picasso_uv_simd8_pipeline() {
    PICASSO_SHADER_PIPELINE.store(2, Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-pipeline-view: pipeline=authored-uv-simd8 applies=next-encoded-pbr-draw\n",
    );
}

pub(crate) fn picasso_uv_pipeline_simd8() -> bool {
    PICASSO_SHADER_PIPELINE.load(Ordering::Acquire) == 2
}

pub(crate) fn picasso_pipeline_name() -> &'static str {
    match PICASSO_SHADER_PIPELINE.load(Ordering::Acquire) {
        1 => "authored-uv",
        2 => "authored-uv-simd8",
        _ => "pbr",
    }
}

pub(crate) fn set_picasso_cull_enabled(enabled: bool) {
    PICASSO_CULL_ENABLED.store(enabled, Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-cull-view: enabled={} applies=next-encoded-pbr-draw\n", enabled,
    );
}

pub(crate) fn picasso_cull_enabled() -> bool {
    PICASSO_CULL_ENABLED.load(Ordering::Acquire)
}

pub(crate) fn set_picasso_depth_test_enabled(enabled: bool) {
    PICASSO_DEPTH_TEST_ENABLED.store(enabled, Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-depth-view: enabled={} applies=next-encoded-pbr-draw\n", enabled,
    );
}

pub(crate) fn picasso_depth_test_enabled() -> bool {
    PICASSO_DEPTH_TEST_ENABLED.load(Ordering::Acquire)
}

pub(crate) fn set_picasso_material_view(view: &str) -> bool {
    let mode = match view {
        "pbr" => 0,
        "base" => 1,
        "normal" => 2,
        "uv" => 3,
        "solid" => 4,
        _ => return false,
    };
    PICASSO_MATERIAL_VIEW.store(mode, Ordering::Release);
    crate::log_important!(target: "render";
        "picasso-material-view: mode={} name={} applies=next-encoded-pbr-draw\n",
        mode, view,
    );
    true
}

pub(crate) fn picasso_material_view() -> u32 {
    PICASSO_MATERIAL_VIEW.load(Ordering::Acquire)
}

struct RenderSummaryOnlyGuard;

impl RenderSummaryOnlyGuard {
    fn enter() -> Self {
        SUMMARY_ONLY_SUBMIT_DEPTH.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for RenderSummaryOnlyGuard {
    fn drop(&mut self) {
        SUMMARY_ONLY_SUBMIT_DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
}

fn render_detail_logs_enabled() -> bool {
    SUMMARY_ONLY_SUBMIT_DEPTH.load(Ordering::Acquire) == 0
}

macro_rules! intel_render_focus_log {
    ($($arg:tt)*) => {
        if render_detail_logs_enabled()
            && (crate::log_os::flags::INTEL_STAGE1_LOGS
                || crate::log_os::flags::INTEL_RENDER_NGIN_LOGS)
        {
            crate::log_info!(target: "render"; $($arg)*);
        }
    };
}

macro_rules! intel_render_verbose_log {
    ($($arg:tt)*) => {
        if render_detail_logs_enabled()
            && crate::log_os::flags::INTEL_RENDER_NGIN_LOGS
            && !crate::log_os::flags::INTEL_STAGE1_LOGS
        {
            crate::log_trace!(target: "render"; $($arg)*);
        }
    };
}

macro_rules! intel_render_batch_log {
    ($($arg:tt)*) => {
        if render_detail_logs_enabled()
            && crate::log_os::flags::INTEL_RENDER_NGIN_BATCH_LOGS
            && !crate::log_os::flags::INTEL_STAGE1_LOGS
        {
            crate::log_trace!(target: "render"; $($arg)*);
        }
    };
}

mod joker_config;
pub(crate) use joker_config::render_joker_variant_names;
use joker_config::{
    RenderJokerSpec, RenderJokerTarget, parse_render_joker_spec,
    render_joker_real_vs_front_end_contract, render_joker_streamout_kind,
    render_joker_vf_experiment, retired_render_joker_variant_reason,
};

include!("constants.rs");
include!("picasso_carrier.rs");
include!("state.rs");
include!("warmup.rs");
include!("primary.rs");
include!("picasso_vue_compare.rs");
include!("pipeline.rs");
include!("resources.rs");
include!("submit.rs");
include!("lrc.rs");
