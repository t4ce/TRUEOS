mod display;
mod dmc;
pub(crate) mod format;
mod fw_probe;
mod gpgpu;
mod guc;
pub(crate) mod guc_ctb;
pub mod hda;
mod huc;
mod hw_cursor;
pub(crate) mod hw_pic;
pub(crate) mod hw_vid;
pub(crate) mod medbak;
pub(crate) mod state;
pub(crate) mod stats;
mod uc_fw;
pub(crate) mod xelp_media2_ngin;
pub(crate) mod xelp_media2_ngin_hw_pic;

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::SendSpawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub(crate) const INTEL_VENDOR_ID: u16 = 0x8086;
pub(crate) const PCI_CLASS_DISPLAY: u8 = 0x03;
pub(crate) const GPU_VA_HUC_FW_BASE: u64 = 0x0020_0000;
pub(crate) const GPU_VA_HUC_RSA_BASE: u64 = 0x0030_0000;
pub(crate) const GPU_VA_GUC_CTB_BASE: u64 = 0x0700_0000;
pub(crate) const GPU_VA_GUC_FW_BASE: u64 = 0x0085_0000;
pub(crate) const GPU_VA_GUC_ADS_BASE: u64 = 0x0100_0000;
pub(crate) const GPU_VA_DISPLAY_PRIMARY_BASE: u64 = 0x0200_0000;
pub(crate) const GPU_VA_DISPLAY_OVERLAY_BASE: u64 = 0x0300_0000;
pub(crate) const GPU_VA_DISPLAY_CURSOR_BASE: u64 = 0x0600_0000;
pub(crate) const WARM_ALIGN: usize = 4096;
const GGTT_ALIAS_BASE_OFF: usize = 0x0080_0000;
const GGTT_ALIAS_BYTES: usize = 0x0080_0000;
const GGTT_PAGE_BYTES: u64 = 4096;
const GEN8_PAGE_PRESENT: u64 = 1;
const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_MEDIA: usize = 0x0A184;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const GFX_FLSH_CNTL_GEN6: usize = 0x101008;
const GFX_FLSH_CNTL_EN: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_SHIFT: u32 = 14;
const GUC_WOPCM_SIZE_MASK: u32 = 0xFFFFF << 12;
const GEN11_WOPCM_SIZE: u32 = 0x0020_0000;
const WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_STACK_RESERVED_SIZE: u32 = 0x0000_2000;
const WOPCM_HW_CTX_RESERVED_SIZE: u32 = 0x0000_9000;
const GUC_WOPCM_OFFSET_ALIGNMENT: u32 = 1 << GUC_WOPCM_OFFSET_SHIFT;
pub(crate) const GS_BOOTROM_MASK: u32 = 0x7F << 1;
pub(crate) const GS_UKERNEL_MASK: u32 = 0xFF << 8;
pub(crate) const GS_AUTH_STATUS_MASK: u32 = 0x03 << 30;
const DISPLAY_PLANE1_BOOT_DEMO_ENABLED: bool = true;
const MEDIA_BOOT_DEMO_ENABLED: bool = false;
const MEDIA_BOOT_DEMO_DELAY_MS: u64 = 5_000;
const MEDIA_BOOT_DEMO_PREFERRED_AP_SLOT: u32 = 3;
static INIT: AtomicBool = AtomicBool::new(false);
static CLAIMED_DEVICE: Mutex<Option<Dev>> = Mutex::new(None);

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuPreflightStatus {
    pub(crate) submitted: bool,
    pub(crate) accepted: bool,
    pub(crate) completed: bool,
    pub(crate) guc_ready: bool,
    pub(crate) marker: u32,
    pub(crate) dot: u32,
    pub(crate) sum_a: u32,
    pub(crate) sum_b: u32,
    pub(crate) lanes: u32,
    pub(crate) min_burn_rows: usize,
    pub(crate) min_burn_k_dim: usize,
    pub(crate) arena_gpu_base: u64,
    pub(crate) arena_bytes: usize,
    pub(crate) tile_rows: usize,
    pub(crate) max_tiles: usize,
    pub(crate) enough_for_shape: bool,
    pub(crate) eu_kernel_uploaded: bool,
    pub(crate) eu_walker_encoded: bool,
    pub(crate) eu_walker_submitted: bool,
    pub(crate) eu_walker_retired: bool,
    pub(crate) eu_dispatch_delta: u32,
    pub(crate) eu_c_store_value: u32,
    pub(crate) eu_expected_store_value: u32,
    pub(crate) eu_program_name: &'static str,
    pub(crate) result_c_changed_by_eu: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuOneTileStageProof {
    pub(crate) staged: bool,
    pub(crate) reason: &'static str,
    pub(crate) readback_ok: bool,
    pub(crate) output_zeroed: bool,
    pub(crate) arena_mapped: bool,
    pub(crate) arena_gpu_base: u64,
    pub(crate) x_gpu: u64,
    pub(crate) row_gpu: u64,
    pub(crate) output_gpu: u64,
    pub(crate) x_bytes: usize,
    pub(crate) row_bytes: usize,
    pub(crate) output_bytes: usize,
    pub(crate) tile_rows: usize,
    pub(crate) k_dim: usize,
    pub(crate) output_first_bits: u32,
    pub(crate) output_nonzero_dwords: usize,
    pub(crate) output_expected_hits_lo64: u64,
    pub(crate) output_checksum: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuTileRowsStageProof {
    pub(crate) staged: bool,
    pub(crate) reason: &'static str,
    pub(crate) readback_ok: bool,
    pub(crate) output_zeroed: bool,
    pub(crate) output_gpu: u64,
    pub(crate) row_count: usize,
    pub(crate) row_bytes: usize,
    pub(crate) rows_checksum: u64,
    pub(crate) staged_rows_checksum: u64,
    pub(crate) output_nonzero_dwords: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuOneTileSentinelProof {
    pub(crate) submitted: bool,
    pub(crate) finished: bool,
    pub(crate) readback_ok: bool,
    pub(crate) reason: &'static str,
    pub(crate) program_name: &'static str,
    pub(crate) output_gpu: u64,
    pub(crate) sentinel: u32,
    pub(crate) output_first_before: u32,
    pub(crate) output_first_after: u32,
    pub(crate) output_nonzero_before: usize,
    pub(crate) output_nonzero_after: usize,
    pub(crate) output_hits_lo64: u64,
    pub(crate) dispatch_delta: u64,
    pub(crate) finish_marker: u32,
    pub(crate) expected_finish_marker: u32,
    pub(crate) batch_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuOneTileCompareProof {
    pub(crate) submitted: bool,
    pub(crate) finished: bool,
    pub(crate) readback_ok: bool,
    pub(crate) compare_ok: bool,
    pub(crate) reason: &'static str,
    pub(crate) program_name: &'static str,
    pub(crate) output_gpu: u64,
    pub(crate) gpu_value: u32,
    pub(crate) cpu_expected_bits: u32,
    pub(crate) output_first_before: u32,
    pub(crate) output_first_after: u32,
    pub(crate) output_hits_lo64: u64,
    pub(crate) dispatch_delta: u64,
    pub(crate) finish_marker: u32,
    pub(crate) expected_finish_marker: u32,
    pub(crate) batch_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuT5OneRowMatvecProof {
    pub(crate) submitted: bool,
    pub(crate) finished: bool,
    pub(crate) readback_ok: bool,
    pub(crate) compare_ok: bool,
    pub(crate) reason: &'static str,
    pub(crate) program_name: &'static str,
    pub(crate) output_gpu: u64,
    pub(crate) gpu_value: u32,
    pub(crate) cpu_expected_bits: u32,
    pub(crate) output_first_before: u32,
    pub(crate) output_first_after: u32,
    pub(crate) output_hits_lo64: u64,
    pub(crate) dispatch_delta: u64,
    pub(crate) finish_marker: u32,
    pub(crate) expected_finish_marker: u32,
    pub(crate) batch_bytes: usize,
    pub(crate) live_k_dim: usize,
    pub(crate) requires_live_gpu_load: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuT62PartialMatvecProof {
    pub(crate) submitted: bool,
    pub(crate) finished: bool,
    pub(crate) readback_ok: bool,
    pub(crate) compare_ok: bool,
    pub(crate) reason: &'static str,
    pub(crate) program_name: &'static str,
    pub(crate) output_gpu: u64,
    pub(crate) output_words: [u32; 8],
    pub(crate) expected_words: [u32; 8],
    pub(crate) output_words16: [u32; 16],
    pub(crate) expected_words16: [u32; 16],
    pub(crate) output_words32: [u32; 32],
    pub(crate) expected_words32: [u32; 32],
    pub(crate) compare_mask: u32,
    pub(crate) expected_mask: u32,
    pub(crate) dispatch_delta: u64,
    pub(crate) finish_marker: u32,
    pub(crate) expected_finish_marker: u32,
    pub(crate) batch_bytes: usize,
    pub(crate) row_count: usize,
    pub(crate) live_k_dim: usize,
}

fn pick_media_boot_demo_spawner() -> Option<(u32, SendSpawner)> {
    let background_slots = crate::workers::background_worker_slots();

    let pick_slot = |predicate: fn(u32) -> bool| {
        background_slots.iter().copied().find(|slot| {
            predicate(*slot)
                && crate::cpu::CpuProfile::for_slot(*slot)
                    .map(|profile| profile.is_perf())
                    .unwrap_or(false)
        })
    };

    let selected_slot = pick_slot(|slot| slot >= MEDIA_BOOT_DEMO_PREFERRED_AP_SLOT)
        .or_else(|| {
            background_slots
                .iter()
                .copied()
                .find(|slot| *slot >= MEDIA_BOOT_DEMO_PREFERRED_AP_SLOT)
        })
        .or_else(|| pick_slot(|slot| slot > 2))
        .or_else(|| background_slots.iter().copied().find(|slot| *slot > 2))?;

    crate::workers::spawner_for_slot(selected_slot).map(|spawner| (selected_slot, spawner))
}

fn log_media_demo_task_profile(origin: &str, requested_slot: u32, queued_at_ms: u64) {
    let queued_ms = embassy_time::Instant::now()
        .as_millis()
        .saturating_sub(queued_at_ms);
    let cpu = crate::percpu::this_cpu();
    let executor_ptr = cpu.executor_ptr() as usize;
    if let Some(profile) = crate::cpu::CpuProfile::current() {
        crate::log!(
            "intel/media: task profile origin={} slot={} lapic={} core={} requested_slot={} queued_ms={} executor=0x{:016X}\n",
            origin,
            profile.slot(),
            profile.lapic_id(),
            profile.core_kind_name(),
            requested_slot,
            queued_ms,
            executor_ptr,
        );
    } else {
        crate::log!(
            "intel/media: task profile origin={} slot=? lapic=? core=? requested_slot={} queued_ms={} executor=0x{:016X}\n",
            origin,
            requested_slot,
            queued_ms,
            executor_ptr,
        );
    }
}

#[embassy_executor::task]
async fn media_boot_demo_task(requested_slot: u32, queued_at_ms: u64) {
    log_media_demo_task_profile("worker", requested_slot, queued_at_ms);
    crate::log!("intel/media: boot demo begin\n");
    let first_frame = run_media2_first_frame_async().await;
    crate::log!(
        "intel/media: boot demo first-frame origin=worker returned={}\n",
        first_frame.is_some() as u8,
    );
}

#[derive(Copy, Clone)]
pub(crate) struct Dev {
    pub(crate) bus: u8,
    pub(crate) slot: u8,
    pub(crate) function: u8,
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) mmio: *mut u8,
    pub(crate) mmio_len: usize,
}
unsafe impl Send for Dev {}
unsafe impl Sync for Dev {}
#[derive(Copy, Clone)]
pub(crate) struct Buf {
    pub(crate) phys: u64,
    pub(crate) virt: *mut u8,
    pub(crate) len: usize,
    pub(crate) gpu: u64,
    pub(crate) css_offset: usize,
    pub(crate) xfer_len: usize,
    pub(crate) private_data_size: usize,
    pub(crate) rsa_offset: usize,
    pub(crate) rsa_size: usize,
}

pub fn init_once() {
    if INIT.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(dev) = find_dev() else {
        crate::log!("intel: no Intel display-class PCI device claimed\n");
        return;
    };
    crate::log!(
        "intel: claimed {:02X}:{:02X}.{} device=0x{:04X} rev=0x{:02X} mmio_len=0x{:X}\n",
        dev.bus,
        dev.slot,
        dev.function,
        dev.device_id,
        dev.revision_id,
        dev.mmio_len
    );
    *CLAIMED_DEVICE.lock() = Some(dev);
    self::fw_probe::log_probe_modules(dev.device_id);
    self::dmc::wire_load_path(dev);
    let huc_fw = self::huc::load_fw();
    let fw = self::guc::load_fw();
    if fw.len == 0 {
        crate::log!("intel/guc: firmware module missing or invalid\n");
        return;
    }
    crate::log!(
        "intel/guc: firmware found phys=0x{:X} gpu=0x{:X} len=0x{:X} xfer=0x{:X}\n",
        fw.phys,
        fw.gpu,
        fw.len,
        fw.xfer_len
    );
    let ads = self::guc::alloc_ads(fw.private_data_size);
    if ads.len == 0 {
        crate::log!("intel/guc: ads alloc failed private_data=0x{:X}\n", fw.private_data_size);
        return;
    }
    let huc_mapped = huc_fw.len != 0
        && map_ggtt(dev, huc_fw.phys, huc_fw.len, huc_fw.gpu)
        && self::huc::map_rsa(dev);
    if !map_ggtt(dev, fw.phys, fw.len, fw.gpu) || !map_ggtt(dev, ads.phys, ads.len, ads.gpu) {
        crate::log!("intel/guc: ggtt map failed fw_len=0x{:X} ads_len=0x{:X}\n", fw.len, ads.len);
        return;
    }
    ggtt_invalidate(dev);
    forcewake(dev);
    let huc_uploaded = if huc_fw.len != 0 {
        if huc_mapped {
            self::huc::upload_via_dma(dev, huc_fw)
        } else {
            crate::log!(
                "intel/huc: dma-upload skipped reason=ggtt-map-failed fw_len=0x{:X}\n",
                huc_fw.len
            );
            false
        }
    } else {
        false
    };
    let ready = self::guc::bootstrap(dev, fw, ads);
    let status = self::guc::status(dev);
    let (bootrom, ukernel, auth) = self::guc::describe_status(status);
    crate::log!(
        "intel/guc: bootstrap ready={} status=0x{:08X} bootrom={} ukernel={} auth=0x{:X}\n",
        ready as u8,
        status,
        bootrom,
        ukernel,
        auth
    );
    if ready {
        let ctb_ready = self::guc_ctb::init_and_enable(dev);
        if !ctb_ready {
            self::guc::prove_h2g_mmio_once(dev, "boot-control-ctb-disable");
        }
        if huc_uploaded {
            self::huc::authenticate_via_guc(dev, huc_fw);
        } else if huc_fw.len != 0 {
            crate::log!("intel/huc: auth skipped reason=dma-upload-not-complete\n");
        }
    }
    if DISPLAY_PLANE1_BOOT_DEMO_ENABLED {
        self::display::init_primary_boot_surface(dev);
    } else {
        crate::log!("intel/display: plane1 boot demo disabled\n");
    }
    crate::log!(
        "intel/render: disabled reason=primary-render-module-removed gpgpu_probe=enabled\n"
    );
    self::gpgpu::submit_gpgpu_preflight_once();
    crate::log!("intel/media: source warmup disabled trigger=trueosfs-root-mounted\n",);
    if MEDIA_BOOT_DEMO_ENABLED {
        crate::log!("intel/media: scheduled boot demo delay_ms={}\n", MEDIA_BOOT_DEMO_DELAY_MS);
        crate::wait::spawn_local_detached(async move {
            Timer::after(EmbassyDuration::from_millis(MEDIA_BOOT_DEMO_DELAY_MS)).await;
            let queued_at_ms = embassy_time::Instant::now().as_millis() as u64;
            if let Some((slot, worker_spawner)) = pick_media_boot_demo_spawner() {
                match media_boot_demo_task(slot, queued_at_ms) {
                    Ok(token) => {
                        crate::log!(
                            "intel/media: boot demo handoff target_slot={} mode=worker\n",
                            slot,
                        );
                        worker_spawner.spawn(token);
                        return;
                    }
                    Err(err) => {
                        crate::log!(
                            "intel/media: boot demo handoff failed target_slot={} err={:?} fallback=local\n",
                            slot,
                            err,
                        );
                    }
                }
            } else {
                crate::log!(
                    "intel/media: boot demo handoff skipped reason=no-worker-ap fallback=local\n"
                );
            }

            log_media_demo_task_profile("local", 0, queued_at_ms);
            crate::log!("intel/media: boot demo begin\n");
            let first_frame = self::run_media2_first_frame_async().await;
            crate::log!(
                "intel/media: boot demo first-frame origin=local returned={}\n",
                first_frame.is_some() as u8,
            );
        });
    } else {
        crate::log!("intel/media: boot demo disabled\n");
    }
}

pub fn guc_ready() -> bool {
    self::guc::ready()
}

pub(crate) fn guc_h2g_mmio_accepted() -> bool {
    self::guc::h2g_mmio_accepted()
}

pub(crate) fn guc_ctb_enabled() -> bool {
    self::guc_ctb::enabled()
}

pub fn huc_ready() -> bool {
    self::huc::authenticated()
}

pub fn has_claimed_device() -> bool {
    CLAIMED_DEVICE.lock().is_some()
}

pub(crate) fn claimed_device() -> Option<Dev> {
    *CLAIMED_DEVICE.lock()
}

pub(crate) fn gpgpu_preflight_status() -> GpgpuPreflightStatus {
    let status = self::gpgpu::gpgpu_preflight_status();
    GpgpuPreflightStatus {
        submitted: status.submitted,
        accepted: status.accepted,
        completed: status.completed,
        guc_ready: status.guc_ready,
        marker: status.marker,
        dot: status.dot,
        sum_a: status.sum_a,
        sum_b: status.sum_b,
        lanes: status.lanes,
        min_burn_rows: status.min_burn_rows,
        min_burn_k_dim: status.min_burn_k_dim,
        arena_gpu_base: status.arena_gpu_base,
        arena_bytes: status.arena_bytes,
        tile_rows: status.tile_rows,
        max_tiles: status.max_tiles,
        enough_for_shape: status.enough_for_shape,
        eu_kernel_uploaded: status.eu_kernel_uploaded,
        eu_walker_encoded: status.eu_walker_encoded,
        eu_walker_submitted: status.eu_walker_submitted,
        eu_walker_retired: status.eu_walker_retired,
        eu_dispatch_delta: status.eu_dispatch_delta,
        eu_c_store_value: status.eu_c_store_value,
        eu_expected_store_value: status.eu_expected_store_value,
        eu_program_name: status.eu_program_name,
        result_c_changed_by_eu: status.result_c_changed_by_eu,
    }
}

pub(crate) fn stage_gpgpu_one_tile_record_probe(
    x: &[f32],
    row_bf16: &[u8],
    k_dim: usize,
    row_index: usize,
    x_checksum: u64,
    row_checksum: u64,
    cpu_expected_bits: u32,
) -> GpgpuOneTileStageProof {
    self::gpgpu::stage_gpgpu_one_tile_record_probe(
        x,
        row_bf16,
        k_dim,
        row_index,
        x_checksum,
        row_checksum,
        cpu_expected_bits,
    )
}

pub(crate) fn stage_gpgpu_tile_record_rows_probe(
    output_gpu: u64,
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
    rows_checksum: u64,
) -> GpgpuTileRowsStageProof {
    self::gpgpu::stage_gpgpu_tile_record_rows_probe(
        output_gpu,
        rows_bf16,
        row_count,
        k_dim,
        rows_checksum,
    )
}

pub(crate) fn stage_gpgpu_tile_record_rows_trusted(
    output_gpu: u64,
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
) -> GpgpuTileRowsStageProof {
    self::gpgpu::stage_gpgpu_tile_record_rows_trusted(output_gpu, rows_bf16, row_count, k_dim)
}

pub(crate) fn stage_gpgpu_tile_record_accum16_window_probe(
    output_gpu: u64,
    x: &[f32],
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
    source_start: usize,
) -> GpgpuTileRowsStageProof {
    self::gpgpu::stage_gpgpu_tile_record_accum16_window_probe(
        output_gpu,
        x,
        rows_bf16,
        row_count,
        k_dim,
        source_start,
    )
}

pub(crate) fn stage_gpgpu_tile_record_accum16_window_trusted(
    output_gpu: u64,
    x: &[f32],
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
    source_start: usize,
) -> GpgpuTileRowsStageProof {
    self::gpgpu::stage_gpgpu_tile_record_accum16_window_trusted(
        output_gpu,
        x,
        rows_bf16,
        row_count,
        k_dim,
        source_start,
    )
}

pub(crate) fn submit_gpgpu_one_tile_output_sentinel_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_one_tile_output_sentinel_probe(
        output_gpu,
        output_bytes,
        cpu_expected_bits,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_line1280_groupid_rows_fullwidth_color_burst(
    color_seed: u32,
    first_row_group: u32,
    row_group_count: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_primary_scanout_line1280_groupid_rows_fullwidth_color_burst(
        color_seed,
        first_row_group,
        row_group_count,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
    )
}

pub(crate) fn notify_gpgpu_primary_scanout_external_write(
    reason: &str,
    flush_offset: usize,
    flush_bytes: usize,
) -> bool {
    self::gpgpu::notify_gpgpu_primary_scanout_external_write(reason, flush_offset, flush_bytes)
}

pub(crate) fn submit_gpgpu_primary_scanout_groupid_line320_probe(
    mode: u32,
    row_index: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_primary_scanout_groupid_line320_probe(mode, row_index)
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
    mode: u32,
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
        mode, row_index, x_base, lhs, rhs,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet(
    mode: u32,
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet(
        mode, row_index, x_base, lhs, rhs,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot_preview(
    cursor: usize,
    target_phase: usize,
    pixel_budget: usize,
) -> (GpgpuOneTileSentinelProof, usize) {
    self::gpgpu::submit_gpgpu_primary_scanout_mandelbrot_preview(cursor, target_phase, pixel_budget)
}

pub(crate) fn submit_gpgpu_primary_scanout_row2560_simd8_probe(
    mode: u32,
    row_index: u32,
) -> GpgpuOneTileSentinelProof {
    self::gpgpu::submit_gpgpu_primary_scanout_row2560_simd8_probe(mode, row_index)
}

pub(crate) fn submit_gpgpu_one_tile_output_compare_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
) -> GpgpuOneTileCompareProof {
    self::gpgpu::submit_gpgpu_one_tile_output_compare_probe(
        output_gpu,
        output_bytes,
        cpu_expected_bits,
    )
}

pub(crate) fn submit_gpgpu_t5_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> GpgpuT5OneRowMatvecProof {
    self::gpgpu::submit_gpgpu_t5_one_row_matvec_probe(
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t6_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> GpgpuT5OneRowMatvecProof {
    self::gpgpu::submit_gpgpu_t6_one_row_matvec_probe(
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t61_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> GpgpuT5OneRowMatvecProof {
    self::gpgpu::submit_gpgpu_t61_one_row_matvec_probe(
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t62_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t62_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t62_partial_matvec_trusted(
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t62_partial_matvec_trusted(
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t62_partial_matvec_trusted_no_readback(
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t62_partial_matvec_trusted_no_readback(
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t8_groupid_live16_trusted_no_readback(
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t8_groupid_live16_trusted_no_readback(
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn stage_gpgpu_tile_record_output_rows_trusted(
    output_gpu: u64,
    src_row: usize,
    dst_row: usize,
    row_count: usize,
) -> GpgpuTileRowsStageProof {
    self::gpgpu::stage_gpgpu_tile_record_output_rows_trusted(
        output_gpu, src_row, dst_row, row_count,
    )
}

pub(crate) fn submit_gpgpu_t63_accum16_hi_live32_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted(
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted(
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted_no_readback(
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t63_accum16_hi_live32_partial_matvec_trusted_no_readback(
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t8_batch2_rowblock_retire_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t8_batch2_rowblock_retire_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t8_groupid_live16_distinct_row_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t8_groupid_live16_distinct_row_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t8_groupid_live16_distinct_row16_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words16: [u32; 16],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t8_groupid_live16_distinct_row16_probe(
        output_gpu,
        output_bytes,
        expected_words16,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t8_groupid_live16_distinct_row32_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words32: [u32; 32],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t8_groupid_live16_distinct_row32_probe(
        output_gpu,
        output_bytes,
        expected_words32,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t9_existing_t63_groupid_live32_negative_control_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words32: [u32; 32],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t9_existing_t63_groupid_live32_negative_control_probe(
        output_gpu,
        output_bytes,
        expected_words32,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t64_windowed_accum16_live48_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t64_windowed_accum16_live48_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t65_windowed_accum16_live64_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t65_windowed_accum16_live64_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t66_windowed_accum16_live80_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t66_windowed_accum16_live80_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t67_windowed_accum16_live96_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t67_windowed_accum16_live96_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t68_windowed_accum16_live112_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t68_windowed_accum16_live112_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t69_windowed_accum16_live128_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t69_windowed_accum16_live128_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t610_windowed_accum16_live144_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t610_windowed_accum16_live144_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t611_windowed_accum16_live160_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t611_windowed_accum16_live160_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_windowed_accum16_partial_matvec_probe(
    program_name: &'static str,
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_windowed_accum16_partial_matvec_probe(
        program_name,
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_windowed_accum16_partial_matvec_trusted(
    program_name: &'static str,
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_windowed_accum16_partial_matvec_trusted(
        program_name,
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_windowed_accum16_partial_matvec_trusted_no_readback(
    program_name: &'static str,
    output_gpu: u64,
    output_bytes: usize,
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_windowed_accum16_partial_matvec_trusted_no_readback(
        program_name,
        output_gpu,
        output_bytes,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t66_accum32_hi_live96_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> GpgpuT62PartialMatvecProof {
    self::gpgpu::submit_gpgpu_t66_accum32_hi_live96_partial_matvec_probe(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn log_gpgpu_t63_first_tile_output_detail_once(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) {
    self::gpgpu::log_gpgpu_t63_first_tile_output_detail_once(
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    self::display::active_scanout_dimensions()
}

pub fn primary_surface_gpu_addr() -> Option<u64> {
    self::display::primary_surface_gpu_addr()
}

pub fn present_rgba_overlay_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    self::display::present_rgba_overlay_top_right(src, src_width, src_height, src_pitch_bytes)
}

pub fn present_rgba_primary_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    self::display::present_rgba_primary_top_right(src, src_width, src_height, src_pitch_bytes)
}

pub fn update_kernel_hw_cursor() -> Option<u32> {
    self::hw_cursor::update_kernel_hw_cursor()
}

pub fn kernel_hw_cursor_slot() -> Option<u32> {
    self::hw_cursor::kernel_hw_cursor_slot()
}

pub async fn run_media2_first_frame_async() -> Option<self::xelp_media2_ngin::Media2FirstFrameState>
{
    self::xelp_media2_ngin::run_media2_first_frame_async().await
}

pub(crate) fn has_media_decode_engine() -> bool {
    has_claimed_device()
}

pub(crate) fn hw_pic_service()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::hw_pic::hw_pic_service()
}

pub(crate) fn hw_pic_submit_jpeg(encoded: &[u8]) -> Result<u32, i32> {
    self::hw_pic::submit_jpeg(encoded)
}

pub(crate) async fn hw_pic_wait_output_for_id(
    id: u32,
    timeout_ms: u64,
) -> Option<self::hw_pic::HwPicOutput> {
    self::hw_pic::wait_output_for_id(id, timeout_ms).await
}

pub(crate) fn hw_pic_snapshot() -> self::hw_pic::HwPicQueueSnapshot {
    self::hw_pic::snapshot()
}

pub(crate) fn hw_vid_probe_task()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::hw_vid::hw_vid_probe_task()
}

pub(crate) fn hw_logo_present_task()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::display::hw_logo_present_task()
}

pub async fn run_media_source_warmup_async() {
    crate::log!("intel/media: source warmup skipped reason=media-decode-disabled\n");
}

fn find_dev() -> Option<Dev> {
    let mut out = None;
    crate::pci::with_devices(|list| {
        for d in list {
            if d.vendor == INTEL_VENDOR_ID && d.class == PCI_CLASS_DISPLAY && out.is_none() {
                let Some(size) = crate::pci::bar0_size_bytes(d.bus, d.slot, d.function) else {
                    continue;
                };
                let (lo, hi) = crate::pci::read_bar0_raw(d.bus, d.slot, d.function);
                if lo == 0 || lo == 0xFFFF_FFFF || (lo & 1) != 0 {
                    continue;
                }
                let phys = if let Some(hi) = hi {
                    (((hi as u64) << 32) | lo as u64) & !0xF
                } else {
                    (lo as u64) & !0xF
                };
                crate::pci::enable_mem_and_bus_master(d.bus, d.slot, d.function);
                let Some(mmio) = crate::pci::mmio::map_mmio_region_exact(phys, size as usize)
                    .ok()
                    .map(|p| p.as_ptr())
                else {
                    continue;
                };
                out = Some(Dev {
                    bus: d.bus,
                    slot: d.slot,
                    function: d.function,
                    device_id: d.device,
                    revision_id: crate::pci::config_read_u8(d.bus, d.slot, d.function, 0x08),
                    mmio,
                    mmio_len: size as usize,
                });
            }
        }
    });
    out
}

fn forcewake(dev: Dev) {
    mmio_write(dev, FORCEWAKE_RENDER, mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK));
    wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );
    mmio_write(dev, FORCEWAKE_RENDER, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_RENDER, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_MEDIA, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_GT, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
}

fn map_ggtt_pages(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
    for page in 0..len.div_ceil(WARM_ALIGN) {
        let g = gpu + (page as u64) * GGTT_PAGE_BYTES;
        let p = (phys + (page as u64) * GGTT_PAGE_BYTES) & !0xFFF;
        let idx = match usize::try_from(g / GGTT_PAGE_BYTES)
            .ok()
            .and_then(|v| v.checked_mul(8))
        {
            Some(v) if v + 8 <= GGTT_ALIAS_BYTES => v,
            _ => return false,
        };
        unsafe {
            core::ptr::write_volatile(
                dev.mmio.add(GGTT_ALIAS_BASE_OFF + idx) as *mut u64,
                p | GEN8_PAGE_PRESENT,
            );
        }
    }
    true
}

fn ggtt_offset_index(gpu: u64) -> Option<usize> {
    usize::try_from(gpu / GGTT_PAGE_BYTES)
        .ok()
        .and_then(|v| v.checked_mul(8))
        .filter(|v| *v + 8 <= GGTT_ALIAS_BYTES)
}

pub(crate) fn map_ggtt(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
    map_ggtt_pages(dev, phys, len, gpu)
}

pub(crate) fn map_display_scanout_ggtt(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
    map_ggtt_pages(dev, phys, len, gpu)
}

pub(crate) fn read_ggtt_pte(dev: Dev, gpu: u64) -> Option<u64> {
    let idx = ggtt_offset_index(gpu)?;
    Some(unsafe { core::ptr::read_volatile(dev.mmio.add(GGTT_ALIAS_BASE_OFF + idx) as *const u64) })
}

fn ggtt_invalidate(dev: Dev) {
    mmio_write(dev, GFX_FLSH_CNTL_GEN6, GFX_FLSH_CNTL_EN);
}
pub(crate) fn mmio_read(dev: Dev, off: usize) -> u32 {
    if off + 4 > dev.mmio_len {
        0
    } else {
        unsafe { core::ptr::read_volatile(dev.mmio.add(off) as *const u32) }
    }
}
pub(crate) fn mmio_write(dev: Dev, off: usize, v: u32) {
    if off + 4 <= dev.mmio_len {
        unsafe { core::ptr::write_volatile(dev.mmio.add(off) as *mut u32, v) }
    }
}
fn wait_eq(dev: Dev, reg: usize, mask: u32, want: u32, n: usize) {
    for _ in 0..n {
        if (mmio_read(dev, reg) & mask) == want {
            break;
        }
        core::hint::spin_loop();
    }
}
pub(crate) fn mask_en(v: u32) -> u32 {
    v | (v << 16)
}
pub(crate) fn mask_dis(v: u32) -> u32 {
    v << 16
}
pub(crate) fn compute_wopcm(fw: u32) -> Option<(u32, u32)> {
    let usable = GEN11_WOPCM_SIZE.checked_sub(WOPCM_HW_CTX_RESERVED_SIZE)?;
    let min = fw
        .checked_add(GUC_WOPCM_RESERVED_SIZE)?
        .checked_add(GUC_WOPCM_STACK_RESERVED_SIZE)?;
    let base = align_up_u32(WOPCM_RESERVED_SIZE, GUC_WOPCM_OFFSET_ALIGNMENT)?;
    if base >= usable {
        return None;
    }
    let size = (usable - base) & GUC_WOPCM_SIZE_MASK;
    if size < min { None } else { Some((base, size)) }
}
pub(crate) fn align_up(v: usize, a: usize) -> Option<usize> {
    let m = a.checked_sub(1)?;
    v.checked_add(m).map(|x| x & !m)
}
fn align_up_u32(v: u32, a: u32) -> Option<u32> {
    let m = a.checked_sub(1)?;
    v.checked_add(m).map(|x| x & !m)
}
pub(crate) fn wr32(buf: &mut [u8], off: usize, v: u32) {
    if let Some(dst) = buf.get_mut(off..off + 4) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}
pub(crate) fn empty() -> Buf {
    Buf {
        phys: 0,
        virt: core::ptr::null_mut(),
        len: 0,
        gpu: 0,
        css_offset: 0,
        xfer_len: 0,
        private_data_size: 0,
        rsa_offset: 0,
        rsa_size: 0,
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn dma_flush(ptr: *mut u8, len: usize) {
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};
        let mut p = (ptr as usize) & !63usize;
        let end = (ptr as usize).saturating_add(len);
        while p < end {
            _mm_clflush(p as *const _);
            p += 64;
        }
        _mm_mfence();
    }
}
#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn dma_flush(_ptr: *mut u8, _len: usize) {}
