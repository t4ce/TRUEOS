use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use spin::Mutex;

// These fragments intentionally share this module namespace. Keeping the
// implementation in one namespace preserves all existing crate-visible paths,
// private helper access, static initialization, and kernel ABI types while
// allowing each concern to live in a focused source file.
include!("artifacts/contract.rs");
include!("kernel_catalog.rs");
include!("rcs/constants.rs");
include!("runtime_state.rs");
include!("types/kernel.rs");
include!("types/worklists.rs");
include!("types/surfaces.rs");
include!("artifacts/metadata.rs");
include!("artifacts/uploads.rs");
include!("operations/primitives.rs");
include!("operations/svg_outline.rs");
include!("operations/fill_rect_worklist.rs");
include!("operations/sprite_quad_worklist.rs");
include!("operations/ui4.rs");
include!("operations/surfaces.rs");
include!("operations/lab256.rs");
include!("operations/spirit_vfx.rs");
include!("operations/probes.rs");
include!("operations/submission_2d.rs");
include!("operations/effects.rs");
include!("operations/cpp_demo.rs");
include!("operations/cpp_audio_visualizer.rs");
include!("operations/particle_craft.rs");
include!("operations/lfm25_q8.rs");
include!("operations/kokoro_qgemm.rs");
include!("operations/kokoro_conv1d.rs");
include!("operations/helio_retained_transform.rs");
include!("operations/worklists.rs");
include!("artifacts/runtime.rs");
include!("rcs/runtime.rs");
include!("rcs/worklists.rs");
include!("rcs/two_d.rs");
include!("rcs/effects.rs");
include!("rcs/cpp_demo.rs");
include!("rcs/cpp_audio_visualizer.rs");
include!("rcs/particle_craft.rs");
include!("rcs/lfm25_q8.rs");
include!("rcs/kokoro_qgemm.rs");
include!("rcs/kokoro_conv1d.rs");
include!("rcs/payloads.rs");
include!("rcs/descriptors.rs");
include!("rcs/commands.rs");
include!("rcs/context.rs");
include!("rcs/helio_retained_transform.rs");
include!("rcs/lab256.rs");
include!("rcs/spirit_vfx.rs");
