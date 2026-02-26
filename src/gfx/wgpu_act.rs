#![allow(dead_code)]

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time_driver::{TICK_HZ, now};
use spin::Mutex;

use crate::wait;

use super::virtio_gpu_3d::VirtioGpu3d;

// Serialized virtio-gpu access actor used by higher-level gfx/WebGPU-facing paths.
// This is the core dispatch surface allowing thousands of independent agents or entities
// to run in parallel on a graphics card.
// "keys" here are command ids + resource/scanout handles carried through GpuCmd.
static GPU_ACTOR_GPU: Mutex<Option<VirtioGpu3d>> = Mutex::new(None);
static GPU_ACTOR_QUEUE: Mutex<VecDeque<(u32, GpuCmd)>> = Mutex::new(VecDeque::new());
static GPU_ACTOR_RESP: Mutex<BTreeMap<u32, GpuResp>> = Mutex::new(BTreeMap::new());
static GPU_ACTOR_NEXT_ID: AtomicU32 = AtomicU32::new(1);
static GPU_ACTOR_PROCESSING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
enum GpuCmd {
    GetDisplayInfo,
    ResourceCreate2D {
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    },
    ResourceAttachBacking {
        resource_id: u32,
        backing_phys: u64,
        backing_len: u32,
    },
    SetScanout {
        scanout_id: u32,
        resource_id: u32,
        width: u32,
        height: u32,
    },
    TransferToHost2D {
        resource_id: u32,
        width: u32,
        height: u32,
    },
    ResourceFlush {
        resource_id: u32,
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Copy, Debug)]
enum GpuResp {
    DisplayInfo(Option<(u32, u32, u32)>),
    Bool(bool),
}

fn gpu_submit(cmd: GpuCmd) -> u32 {
    let id = GPU_ACTOR_NEXT_ID
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    GPU_ACTOR_QUEUE.lock().push_back((id, cmd));
    id
}

fn gpu_take_resp(id: u32) -> Option<GpuResp> {
    GPU_ACTOR_RESP.lock().remove(&id)
}

fn gpu_ensure_inited_locked(gpu_slot: &mut Option<VirtioGpu3d>) -> bool {
    if gpu_slot.is_some() {
        return true;
    }
    *gpu_slot = VirtioGpu3d::init_first();
    gpu_slot.is_some()
}

fn gpu_service_step() {
    if GPU_ACTOR_PROCESSING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let maybe_cmd = GPU_ACTOR_QUEUE.lock().pop_front();
    if let Some((id, cmd)) = maybe_cmd {
        let mut gpu_guard = GPU_ACTOR_GPU.lock();
        if !gpu_ensure_inited_locked(&mut gpu_guard) {
            GPU_ACTOR_RESP.lock().insert(
                id,
                match cmd {
                    GpuCmd::GetDisplayInfo => GpuResp::DisplayInfo(None),
                    _ => GpuResp::Bool(false),
                },
            );
        } else {
            let gpu = gpu_guard.as_mut().expect("gpu init");
            let resp = match cmd {
                GpuCmd::GetDisplayInfo => GpuResp::DisplayInfo(gpu.get_display_info()),
                GpuCmd::ResourceCreate2D {
                    resource_id,
                    format,
                    width,
                    height,
                } => GpuResp::Bool(gpu.resource_create_2d(resource_id, format, width, height)),
                GpuCmd::ResourceAttachBacking {
                    resource_id,
                    backing_phys,
                    backing_len,
                } => GpuResp::Bool(gpu.resource_attach_backing(
                    resource_id,
                    backing_phys,
                    backing_len,
                )),
                GpuCmd::SetScanout {
                    scanout_id,
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.set_scanout(scanout_id, resource_id, width, height)),
                GpuCmd::TransferToHost2D {
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.transfer_to_host_2d(resource_id, width, height)),
                GpuCmd::ResourceFlush {
                    resource_id,
                    width,
                    height,
                } => GpuResp::Bool(gpu.resource_flush(resource_id, width, height)),
            };
            GPU_ACTOR_RESP.lock().insert(id, resp);
        }
    }

    GPU_ACTOR_PROCESSING.store(false, Ordering::Release);
}

fn gpu_wait_resp(id: u32, timeout_ms: u64) -> Option<GpuResp> {
    let hz = TICK_HZ;
    let ticks = if hz == 0 {
        0
    } else {
        timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
    };
    let deadline = now().saturating_add(ticks);

    loop {
        if let Some(r) = gpu_take_resp(id) {
            return Some(r);
        }

        gpu_service_step();

        if let Some(r) = gpu_take_resp(id) {
            return Some(r);
        }
        if timeout_ms != 0 && now() >= deadline {
            return None;
        }
        wait::spin_step_no_exec();
    }
}

pub fn gpu_get_display_info(timeout_ms: u64) -> Option<(u32, u32, u32)> {
    let id = gpu_submit(GpuCmd::GetDisplayInfo);
    match gpu_wait_resp(id, timeout_ms)? {
        GpuResp::DisplayInfo(v) => v,
        _ => None,
    }
}

pub fn gpu_resource_create_2d(
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::ResourceCreate2D {
        resource_id,
        format,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_resource_attach_backing(
    resource_id: u32,
    backing_phys: u64,
    backing_len: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::ResourceAttachBacking {
        resource_id,
        backing_phys,
        backing_len,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_set_scanout(
    scanout_id: u32,
    resource_id: u32,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> bool {
    let id = gpu_submit(GpuCmd::SetScanout {
        scanout_id,
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_transfer_to_host_2d(resource_id: u32, width: u32, height: u32, timeout_ms: u64) -> bool {
    let id = gpu_submit(GpuCmd::TransferToHost2D {
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}

pub fn gpu_resource_flush(resource_id: u32, width: u32, height: u32, timeout_ms: u64) -> bool {
    let id = gpu_submit(GpuCmd::ResourceFlush {
        resource_id,
        width,
        height,
    });
    matches!(gpu_wait_resp(id, timeout_ms), Some(GpuResp::Bool(true)))
}
