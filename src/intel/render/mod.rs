// Xe-LP render-engine triangle proof path.
//
// This module is imported from the previous Intel bring-up tree and scoped to
// the 3D triangle path. TRUEOS now has a newer standalone GPGPU module, so the
// old render-local GPGPU include intentionally stays out of this revival pass.

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
