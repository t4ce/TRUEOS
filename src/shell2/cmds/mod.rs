use alloc::string::String as AllocString;

pub(crate) mod acpi;
pub(crate) mod c4;
pub(crate) mod diashow;
pub(crate) mod disc;
pub(crate) mod fnt;
pub(crate) mod format;
pub(crate) mod fslog;
pub(crate) mod gboy;
pub(crate) mod gpgpu;
pub(crate) mod hyper;
pub(crate) mod install;
pub(crate) mod lsd;
pub(crate) mod mv;
pub(crate) mod net;
pub(crate) mod rm;
pub(crate) mod run;
pub(crate) mod set;
pub(crate) mod sevenz;
pub(crate) mod sha;
pub(crate) mod smp;
pub(crate) mod tlb;
pub(crate) mod tlb_helper;
pub(crate) mod ttstt;
pub(crate) mod txt;
pub(crate) mod update;
pub(crate) mod vgpu;
pub(crate) mod vid;

pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
