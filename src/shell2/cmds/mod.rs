use alloc::string::String as AllocString;

use crate::disc::block::{DeviceHandle, DeviceInfo, DeviceKind};

pub(crate) mod acpi;
pub(crate) mod bench;
pub(crate) mod etc;
pub(crate) mod file;
pub(crate) mod format;
pub(crate) mod hv;
pub(crate) mod install;
pub(crate) mod net;
pub(crate) mod run;
pub(crate) mod set;
pub(crate) mod smp;
pub(crate) mod tetris;
pub(crate) mod tlb;
pub(crate) mod tlb_helper;
pub(crate) mod turbo;
pub(crate) mod update;

fn is_default_disk_candidate(info: &DeviceInfo) -> bool {
    info.writable
        && info.parent.is_none()
        && !matches!(info.kind, DeviceKind::Partition | DeviceKind::Ramdisk)
}

fn is_preferred_default_disk_candidate(info: &DeviceInfo) -> bool {
    is_default_disk_candidate(info) && info.label.as_deref() == Some("usbms")
}

pub(crate) fn select_default_disk_target() -> Option<DeviceHandle> {
    let handles = crate::disc::block::device_handles();
    handles
        .iter()
        .copied()
        .find(|handle| is_preferred_default_disk_candidate(&handle.info()))
        .or_else(|| {
            handles
                .into_iter()
                .find(|handle| is_default_disk_candidate(&handle.info()))
        })
}

pub(crate) fn command_registry_json() -> AllocString {
    super::shell2_cmd_registry::command_registry_json()
}
