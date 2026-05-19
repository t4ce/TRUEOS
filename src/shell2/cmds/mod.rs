use alloc::string::String as AllocString;

pub(crate) mod acpi;
pub(crate) mod ample;
pub(crate) mod bench;
pub(crate) mod c4;
pub(crate) mod disc;
pub(crate) mod etc;
pub(crate) mod format;
pub(crate) mod install;
pub(crate) mod lsd;
pub(crate) mod net;
pub(crate) mod run;
pub(crate) mod set;
pub(crate) mod shader;
pub(crate) mod smp;
pub(crate) mod tlb;
pub(crate) mod tlb_helper;
pub(crate) mod turbo;
pub(crate) mod update;

pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
