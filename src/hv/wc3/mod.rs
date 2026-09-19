//! Private Warcraft III x86 bridge.
//!
//! The launcher personality lives in the wc3.bp application. The kernel
//! keeps only the generic x86 carrier and the optional Gate-0 probe.

#![allow(dead_code, reason = "private feature-gated hardware bring-up path")]

mod guest32;
mod trace;
mod x86_cabi;
mod x86_runtime;

pub(crate) use guest32::{guest_mapping, handle_vmcall, prepare_gate0};

use super::VmBootMode;

pub(crate) fn entry_for_mode(mode: VmBootMode, normal: u64) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => guest32::entry_for_mode(mode, normal),
        _ => normal,
    }
}

pub(crate) fn fs_base_for_mode(mode: VmBootMode) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => guest32::fs_base_for_mode(mode),
        _ => 0,
    }
}

/// The only loader hook for the WC3-private x86 bridge.
pub(crate) fn wc3_x86_cabi_resolve(name: &str) -> Option<usize> {
    x86_cabi::resolve(name)
}

pub(crate) fn shared_x86_runtime_state_span(vm_id: u8) -> Option<(u64, usize)> {
    x86_runtime::shared_runtime_state_span(vm_id)
}

pub(crate) fn carrier_pdpt_span(vm_id: u8) -> Result<(u64, usize), &'static str> {
    x86_runtime::carrier_pdpt_span(vm_id)
}

pub(crate) fn purge_one_shot_state(vm_id: u8) {
    x86_runtime::purge_one_shot_state(vm_id);
    if guest32::purge_one_shot_state(vm_id) {
        crate::log!(target: "hv"; "wc3: one-shot purge vm={} probe_backing_released=1\n", vm_id);
    }
}
