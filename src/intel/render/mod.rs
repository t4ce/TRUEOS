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

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

macro_rules! intel_render_focus_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_STAGE1_LOGS || crate::logflag::INTEL_RENDER_NGIN_LOGS {
            crate::log!($($arg)*);
        }
    };
}

macro_rules! intel_render_verbose_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_RENDER_NGIN_LOGS && !crate::logflag::INTEL_STAGE1_LOGS {
            crate::log!($($arg)*);
        }
    };
}

macro_rules! intel_render_batch_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_RENDER_NGIN_BATCH_LOGS && !crate::logflag::INTEL_STAGE1_LOGS {
            crate::log!($($arg)*);
        }
    };
}

include!("constants.rs");
include!("state.rs");
include!("warmup.rs");
include!("primary.rs");
include!("pipeline.rs");
include!("resources.rs");
include!("submit.rs");
include!("lrc.rs");
