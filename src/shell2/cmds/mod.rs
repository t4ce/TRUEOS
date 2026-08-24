use alloc::string::String as AllocString;

pub(crate) mod acpi;
pub(crate) mod aud;
pub(crate) mod cpp;
pub(crate) mod cry;
pub(crate) mod disc;
pub(crate) mod edit;
pub(crate) mod format;
#[cfg(test)]
pub(crate) mod fslog;
pub(crate) mod grid;
pub(crate) mod gridp;
pub(crate) mod helio;
pub(crate) mod hyper;
pub(crate) mod img;
pub(crate) mod install;
#[cfg(feature = "trueos_lumen")]
pub(crate) mod lum;
pub(crate) mod net;
pub(crate) mod os;
pub(crate) mod qjs;
pub(crate) mod ram;
pub(crate) mod rapl;
pub(crate) mod run;
pub(crate) mod set;
pub(crate) mod shell;
pub(crate) mod smp;
pub(crate) mod ssh;
pub(crate) mod surf;
pub(crate) mod tde;
#[path = "tlb_router.rs"]
pub(crate) mod tlb;
#[path = "tlb.rs"]
pub(crate) mod tlb_core;
pub(crate) mod tlb_helper;
pub(crate) mod tlb_nct_probe;
pub(crate) mod tlb_platform;
#[path = "tlb_smbios_wrapper.rs"]
pub(crate) mod tlb_smbios;
pub(crate) mod ttstt;
pub(crate) mod update;
pub(crate) mod vgpu;
pub(crate) mod vid;
pub(crate) mod xhci;

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
