//! Cached, best-effort vGPU memory accounting.
//!
//! These snapshots report allocations charged by the mediated vGPU broker.
//! They do not probe the GPU and must not be interpreted as physical dedicated
//! VRAM residency or instantaneous hardware-memory occupancy.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use embassy_sync::watch::{Receiver as WatchReceiver, Watch};

use super::vgpu::{self, DeviceHandle, Principal};

const VRAM_WATCH_RECEIVERS: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct HullGuestTag {
    pub(crate) vm_id: u8,
    pub(crate) lifecycle: &'static str,
    pub(crate) app_archive: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct VramDeviceSnapshot {
    pub(crate) handle: DeviceHandle,
    pub(crate) principal: Principal,
    pub(crate) epoch: u64,
    pub(crate) lost: bool,
    pub(crate) vgpu_mapped_bytes: usize,
    pub(crate) memory_quota: usize,
    pub(crate) buffer_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct VramPrincipalSnapshot {
    pub(crate) principal: Principal,
    pub(crate) hull_guest: Option<HullGuestTag>,
    pub(crate) device_count: usize,
    pub(crate) lost_device_count: usize,
    pub(crate) vgpu_mapped_bytes: usize,
    pub(crate) buffer_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct VramSnapshot {
    pub(crate) update_count: u64,
    pub(crate) last_update_ms: u64,
    pub(crate) sample_valid: bool,
    pub(crate) broker_epoch: u64,
    pub(crate) total_vgpu_mapped_bytes: usize,
    pub(crate) total_buffer_count: usize,
    pub(crate) devices: Vec<VramDeviceSnapshot>,
    pub(crate) principals: Vec<VramPrincipalSnapshot>,
}

impl VramSnapshot {
    pub(crate) const fn empty() -> Self {
        Self {
            update_count: 0,
            last_update_ms: 0,
            sample_valid: false,
            broker_epoch: 0,
            total_vgpu_mapped_bytes: 0,
            total_buffer_count: 0,
            devices: Vec::new(),
            principals: Vec::new(),
        }
    }

    pub(crate) const fn has_data(&self) -> bool {
        self.sample_valid
    }
}

static VRAM_WATCH: Watch<crate::wait::EmbassySpinRawMutex, VramSnapshot, VRAM_WATCH_RECEIVERS> =
    Watch::new_with(VramSnapshot::empty());

pub(crate) type VramReceiver<'a> =
    WatchReceiver<'a, crate::wait::EmbassySpinRawMutex, VramSnapshot, VRAM_WATCH_RECEIVERS>;

pub(crate) fn latest_snapshot() -> VramSnapshot {
    VRAM_WATCH.try_get().unwrap_or_else(VramSnapshot::empty)
}

pub(crate) fn latest_snapshot_text() -> String {
    format_snapshot_text(&latest_snapshot())
}

pub(crate) fn subscribe() -> Option<VramReceiver<'static>> {
    VRAM_WATCH.receiver()
}

pub(crate) fn anon_snapshot() -> VramSnapshot {
    let mut receiver = VRAM_WATCH.anon_receiver();
    receiver.try_get().unwrap_or_else(VramSnapshot::empty)
}

/// Refresh the cached snapshot from vGPU broker-owned counters only.
///
/// The accounting read does not validate GPU mappings, query the physical
/// device, or infer an owner from memory addresses.  VM metadata is attached
/// only when the broker principal explicitly carries `HullGuest(vm_id)`.
pub(crate) fn refresh_snapshot_once() -> VramSnapshot {
    let previous = latest_snapshot();
    let accounting = vgpu::broker_memory_accounting();
    let mut devices = Vec::with_capacity(accounting.devices.len());
    let mut principals: Vec<VramPrincipalSnapshot> = Vec::new();

    for device in accounting.devices {
        let principal = device.principal;
        if let Some(summary) = principals
            .iter_mut()
            .find(|summary| summary.principal == principal)
        {
            summary.device_count = summary.device_count.saturating_add(1);
            summary.lost_device_count = summary
                .lost_device_count
                .saturating_add(usize::from(device.lost));
            summary.vgpu_mapped_bytes = summary
                .vgpu_mapped_bytes
                .saturating_add(device.mapped_bytes);
            summary.buffer_count = summary.buffer_count.saturating_add(device.buffer_count);
        } else {
            principals.push(VramPrincipalSnapshot {
                principal,
                hull_guest: hull_guest_tag(principal),
                device_count: 1,
                lost_device_count: usize::from(device.lost),
                vgpu_mapped_bytes: device.mapped_bytes,
                buffer_count: device.buffer_count,
            });
        }

        devices.push(VramDeviceSnapshot {
            handle: device.handle,
            principal,
            epoch: device.epoch,
            lost: device.lost,
            vgpu_mapped_bytes: device.mapped_bytes,
            memory_quota: device.memory_quota,
            buffer_count: device.buffer_count,
        });
    }

    let snapshot = VramSnapshot {
        update_count: previous.update_count.saturating_add(1),
        last_update_ms: service_now_ms(),
        sample_valid: true,
        broker_epoch: accounting.epoch,
        total_vgpu_mapped_bytes: accounting.total_mapped_bytes,
        total_buffer_count: accounting.total_buffer_count,
        devices,
        principals,
    };
    VRAM_WATCH.sender().send(snapshot.clone());
    snapshot
}

fn hull_guest_tag(principal: Principal) -> Option<HullGuestTag> {
    let Principal::HullGuest(raw_vm_id) = principal else {
        return None;
    };
    let Ok(vm_id) = u8::try_from(raw_vm_id) else {
        return None;
    };
    let state = crate::hv::vm_state(vm_id);
    Some(HullGuestTag {
        vm_id,
        lifecycle: vm_lifecycle_name(state),
        app_archive: crate::hv::app_vm_archive(vm_id),
    })
}

fn vm_lifecycle_name(state: crate::hv::HvVmState) -> &'static str {
    if !state.supported {
        "unsupported"
    } else if state.restore_inflight {
        "restore-inflight"
    } else if state.stop_requested {
        "stop-requested"
    } else if state.preserve_requested || state.preserve_exit {
        "preserve-requested"
    } else if state.running {
        "running"
    } else if state.starting {
        "starting"
    } else if state.pause_latched && state.pause_snapshot_ready {
        "paused-snapshot-ready"
    } else if state.pause_latched {
        "paused"
    } else {
        "offline"
    }
}

fn format_snapshot_text(snapshot: &VramSnapshot) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "vram snapshot");
    let _ = writeln!(out, "measurement=best-effort-vgpu-mapped-bytes");
    let _ = writeln!(out, "source=mediated-vgpu-broker-accounting");
    let _ = writeln!(out, "caveat=not-physical-dedicated-vram-residency-or-hardware-occupancy");
    let _ = writeln!(out, "update_count={}", snapshot.update_count);
    let _ = writeln!(out, "last_update_ms={}", snapshot.last_update_ms);
    let _ = writeln!(out, "sample_valid={}", snapshot.sample_valid);
    let _ = writeln!(out, "broker_epoch={}", snapshot.broker_epoch);
    let _ = writeln!(
        out,
        "totals vgpu_mapped_bytes={} buffers={} devices={} principals={}",
        snapshot.total_vgpu_mapped_bytes,
        snapshot.total_buffer_count,
        snapshot.devices.len(),
        snapshot.principals.len()
    );

    let _ = writeln!(out, "principal summaries");
    for summary in &snapshot.principals {
        let _ = write!(
            out,
            "principal={}{} devices={} lost_devices={} vgpu_mapped_bytes={} buffers={}",
            summary.principal.name(),
            principal_instance_suffix(summary.principal),
            summary.device_count,
            summary.lost_device_count,
            summary.vgpu_mapped_bytes,
            summary.buffer_count
        );
        if let Some(guest) = &summary.hull_guest {
            let _ = write!(
                out,
                " vm_id={} lifecycle={} app_archive={:?}",
                guest.vm_id, guest.lifecycle, guest.app_archive
            );
        } else if matches!(summary.principal, Principal::HullGuest(_)) {
            let _ = write!(out, " vm_id=unrepresentable lifecycle=unresolved app_archive=None");
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "device details");
    for device in &snapshot.devices {
        let _ = writeln!(
            out,
            "device=0x{:016X} principal={}{} epoch={} lost={} vgpu_mapped_bytes={} memory_quota={} buffers={}",
            device.handle.raw(),
            device.principal.name(),
            principal_instance_suffix(device.principal),
            device.epoch,
            device.lost,
            device.vgpu_mapped_bytes,
            quota_text(device.memory_quota),
            device.buffer_count
        );
    }

    out
}

fn principal_instance_suffix(principal: Principal) -> String {
    match principal {
        Principal::HullGuest(id) | Principal::RuntimeTest(id) => alloc::format!("({id})"),
        Principal::KernelRender
        | Principal::KernelGpgpuSystem
        | Principal::KernelGpgpuExecution
        | Principal::KernelLfm25
        | Principal::KernelUi4Compositor
        | Principal::KernelUi4Blitter
        | Principal::HostRuntime => String::new(),
    }
}

fn quota_text(bytes: usize) -> String {
    if bytes == usize::MAX {
        String::from("unlimited")
    } else {
        alloc::format!("{bytes}")
    }
}

fn service_now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}
