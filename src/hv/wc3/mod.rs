//! Private Warcraft III launcher bring-up.
//!
//! Gate 0 remains a standalone compatibility-mode probe. Gate 1 owns its
//! launcher preparation, PE mapping, and Win32 trap dispatch separately.

#![allow(dead_code, reason = "private feature-gated hardware bring-up path")]

mod guest32;
mod imports;
mod launcher;
mod pe32;
mod thunk32;
mod trace;

pub(crate) use guest32::{guest_mapping, handle_vmcall, prepare_gate0};
pub(crate) use launcher::{
    HEAP_VA, PROCESS_DATA_VA, guest_mapping as launcher_guest_mapping,
    handle_vmcall as handle_launcher_vmcall, prepare as prepare_launcher,
    release as release_launcher, schedule_autostart as schedule_launcher_autostart,
};

use super::VmBootMode;

pub(crate) fn entry_for_mode(mode: VmBootMode, normal: u64) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => guest32::entry_for_mode(mode, normal),
        VmBootMode::Wc3Launcher => launcher::ENTRY_VA as u64,
        _ => normal,
    }
}

pub(crate) fn fs_base_for_mode(mode: VmBootMode) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => guest32::fs_base_for_mode(mode),
        VmBootMode::Wc3Launcher => launcher::TEB_VA as u64,
        _ => 0,
    }
}

pub(crate) fn log_launcher_armed(vm_id: u8) {
    trace::info(format_args!(
        "gate-1a armed vm={} entry=0x00402144 fs_base=0x00201000 thunk_base=0x00300000",
        vm_id
    ));
}
