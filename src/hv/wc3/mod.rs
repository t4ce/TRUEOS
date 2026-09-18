//! Private Warcraft III launcher bring-up.
//!
//! Gate 0 is the only enabled stage.  Do not add PE mapping or Win32 thunks
//! until this compatibility-mode probe has passed on the VMX test rig.

#![allow(dead_code, reason = "private feature-gated hardware bring-up path")]

mod guest32;
mod trace;

pub(crate) use guest32::{
    entry_for_mode, fs_base_for_mode, guest_mapping, handle_vmcall, prepare_gate0,
};
