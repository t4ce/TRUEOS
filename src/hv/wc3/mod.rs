//! Private Warcraft III launcher bring-up.
//!
//! Gate 0 remains a standalone compatibility-mode probe. Gate 1 owns its
//! launcher preparation, PE mapping, and Win32 trap dispatch separately.

#![allow(dead_code, reason = "private feature-gated hardware bring-up path")]

mod guest32;
mod launcher;
mod trace;

pub(crate) use guest32::{
    entry_for_mode, fs_base_for_mode, guest_mapping, handle_vmcall, prepare_gate0,
};
pub(crate) use launcher::{handle_vmcall as handle_launcher_vmcall, prepare as prepare_launcher};
