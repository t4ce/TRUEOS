use alloc::string::String as AllocString;

pub(crate) mod acpi;
pub(crate) mod install;
pub(crate) mod set;
pub(crate) mod update;

pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
