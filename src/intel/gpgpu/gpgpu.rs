use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use spin::Mutex;

#[path = "artifacts/shadertoy_package.rs"]
pub(crate) mod shadertoy_package;
use shadertoy_package::*;

// These fragments intentionally share this module namespace. Keeping the
// implementation in one namespace preserves all existing crate-visible paths,
// private helper access, static initialization, and kernel ABI types while
// allowing each concern to live in a focused source file.
include!("artifacts/contract.rs");
#[path = "artifacts/embedded.rs"]
mod embedded_shader;

// A named allocation prevents const byte arrays from being copied into each
// codegen unit. SPIR-V is a view into the same immutable Zebin.
macro_rules! embedded_compute_artifact {
    ($visibility:vis $bin:ident, $spv:ident, $storage:ident, $path:literal) => {
        static $storage: [u8; include_bytes!($path).len()] = *include_bytes!($path);
        $visibility const $bin: &[u8] = &$storage;
        $visibility const $spv: &[u8] = embedded_shader::spirv($bin);
    };
}

include!("kernel_catalog.rs");
include!("rcs/constants.rs");
include!("runtime_state.rs");
include!("types/kernel.rs");
include!("types/worklists.rs");
include!("types/surfaces.rs");
include!("types/volumes.rs");
include!("artifacts/metadata.rs");
include!("artifacts/uploads.rs");
include!("operations/primitives.rs");
include!("operations/native_scalar.rs");
include!("operations/native_kernel.rs");
include!("operations/generated_pixel.rs");
include!("operations/svg_outline.rs");
include!("operations/solid_rects.rs");
include!("operations/ui4.rs");
include!("operations/surfaces.rs");
include!("operations/spirit_vfx.rs");
include!("operations/probes.rs");
include!("operations/submission_2d.rs");
include!("operations/effects.rs");
include!("operations/cpp_demo.rs");
include!("operations/shadertoy_focus.rs");
include!("operations/shadertoy.rs");
include!("operations/cloud_brush.rs");
include!("operations/shadertoy_catalog.rs");
include!("operations/shadertoy_environment.rs");
include!("operations/cpp_audio_visualizer.rs");
include!("operations/particle_craft.rs");
include!("operations/helio_retained_transform.rs");
include!("operations/worklists.rs");
include!("artifacts/runtime.rs");
include!("rcs/runtime.rs");
include!("rcs/worklists.rs");
include!("rcs/two_d.rs");
include!("rcs/effects.rs");
include!("rcs/cpp_demo.rs");
include!("rcs/shadertoy.rs");
include!("rcs/cpp_audio_visualizer.rs");
include!("rcs/particle_craft.rs");
include!("rcs/payloads.rs");
include!("rcs/descriptors.rs");
include!("rcs/commands.rs");
include!("rcs/context.rs");
include!("rcs/helio_retained_transform.rs");
include!("rcs/spirit_vfx.rs");

include!("operations/lz4_blocks.rs");
include!("rcs/lz4_blocks.rs");

