use alloc::string::String as AllocString;

pub(crate) mod acpi;
pub(crate) mod aud;
pub(crate) mod cpp;
pub(crate) mod cry;
pub(crate) mod disc;
pub(crate) mod dobby;
pub(crate) mod format;
pub(crate) mod fslog;
pub(crate) mod grid;
pub(crate) mod helio;
pub(crate) mod hyper;
pub(crate) mod install;
pub(crate) mod lsd;
#[cfg(feature = "trueos_lumen")]
pub(crate) mod lum;
pub(crate) mod mv;
pub(crate) mod net;
pub(crate) mod qjs;
pub(crate) mod ram;
pub(crate) mod rapl;
pub(crate) mod rm;
pub(crate) mod run;
pub(crate) mod set;
pub(crate) mod sevenz;
pub(crate) mod sha;
pub(crate) mod smp;
pub(crate) mod ssh;
pub(crate) mod surf;
pub(crate) mod tlb;
pub(crate) mod tlb_helper;
pub(crate) mod tlb_smbios;
pub(crate) mod ttstt;
pub(crate) mod txt;
pub(crate) mod update;
pub(crate) mod vgpu;
pub(crate) mod vid;
pub(crate) mod xhci;

pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
