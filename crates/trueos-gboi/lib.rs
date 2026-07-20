#![no_std]

extern crate alloc;

// The emulator used to be included directly in the kernel, where its small
// diagnostic messages resolved to the kernel's `log!` macro.  Keep the core
// crate host-agnostic now that a Blueprint owns it; the application reports
// lifecycle errors at its UI boundary.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        let _ = core::format_args!($($arg)*);
    }};
}

#[path = "mod.rs"]
mod emulator;

pub use emulator::*;
