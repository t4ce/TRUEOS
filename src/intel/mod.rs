mod blt;
mod display;
mod dmc;
pub(crate) mod format;
mod fw_probe;
pub(crate) mod gpgpu;
mod guc;
pub(crate) mod guc_ctb;
pub mod hda;
mod huc;
mod hw_cursor;
pub(crate) mod hw_pic;
pub(crate) mod opencl;
pub(crate) mod ppgtt;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod types;
mod uc_fw;
pub(crate) mod xelp_media2_ngin;
pub(crate) mod xelp_media2_ngin_hw_pic;

use core::sync::atomic::{AtomicBool, Ordering};
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
pub(crate) const GPU_VA_DISPLAY_UI2_BASE_BASE: u64 = 0x0400_0000;
pub(crate) const GPU_VA_DISPLAY_UI2_FRAME_BASE: u64 = 0x0500_0000;
pub(crate) const GPU_VA_DISPLAY_CURSOR_BASE: u64 = 0x0600_0000;
pub(crate) const GPU_VA_DISPLAY_UI3_TEXT_BASE: u64 = 0x1000_0000;
pub(crate) const GPU_VA_DISPLAY_UI3_CANVAS_BASE: u64 = 0x1100_0000;
pub(crate) const GPU_VA_DISPLAY_UI3_SCENE_BASE: u64 = 0x1400_0000;
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
const HW_VID_PROBE_H264: &[u8] = include_bytes!("../../tools/vid/x31_head_movie_first_frame.h264");
const PCI_DEVICE_ALDER_LAKE_S_GT1: u16 = 0x4680;
const PCI_DEVICE_ALDER_LAKE_N_N100_UHD: u16 = 0x46D1;
const PCI_DEVICE_RAPTOR_LAKE_S_GT1_UHD770: u16 = 0xA780;
static INIT: AtomicBool = AtomicBool::new(false);
static CLAIMED_DEVICE: Mutex<Option<Dev>> = Mutex::new(None);

fn pick_media_boot_demo_spawner() -> Option<(u32, crate::workers::WorkerSpawner)> {
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
        "intel: claimed {:02X}:{:02X}.{} device=0x{:04X} name={} rev=0x{:02X} mmio_len=0x{:X} ui3_boot={} media_decode={}\n",
        dev.bus,
        dev.slot,
        dev.function,
        dev.device_id,
        display_device_name(dev.device_id),
        dev.revision_id,
        dev.mmio_len,
        full_ui3_boot_enabled_for_device(dev.device_id) as u8,
        media_decode_enabled_for_device(dev.device_id) as u8
    );
    *CLAIMED_DEVICE.lock() = Some(dev);
    let full_ui3_boot = full_ui3_boot_enabled_for_device(dev.device_id);
    if full_ui3_boot {
        let _ = self::gpgpu::upload_fill_rect_worklist_rgba8_kernel();
        let _ = self::gpgpu::upload_gradient_rect_worklist_rgba8_kernel();
        let _ = self::gpgpu::upload_alpha_blend_worklist_rgba8_kernel();
        let _ = self::gpgpu::upload_glyph_mask_rgba8_kernel();
        let _ = self::gpgpu::upload_present_rgba8_to_primary_xrgb_rect_kernel();
        let _ = self::gpgpu::upload_sprite64_worklist_rgba8_kernel();
        let _ = self::gpgpu::upload_mandel64_worklist_rgba8_kernel();
        let _ = self::gpgpu::upload_canvas3d_project_rgba8_kernel();
        let _ = self::gpgpu::upload_canvas3d_transform_q16_kernel();
        let _ = self::gpgpu::upload_canvas3d_clip_box_q16_kernel();
        let _ = self::gpgpu::upload_canvas3d_plane_sample_rgba8_kernel();
        let _ = self::gpgpu::upload_canvas3d_plane_fill_rgba8_kernel();
        let _ = self::gpgpu::upload_canvas3d_plane_patch_fill_cut_rgba8_kernel();
        let _ = self::gpgpu::upload_canvas3d_plane_patch_worklist_rgba8_kernel();
        let opencl_smoke = self::opencl::trueos_cl_source_build_smoke();
        crate::log!(
            "intel/opencl: source-build-smoke source_compile={} build_err={} registry_kernels={} registry_ok={} queue_completed={} fill_rect_uploaded={} queue_err={} note=source-build-currently-scaffold-aot-path-active\n",
            opencl_smoke.source_compile_cap as u8,
            opencl_smoke
                .source_build_error
                .map(|err| err.code())
                .unwrap_or(0),
            opencl_smoke.registry_kernels,
            opencl_smoke.registry_passed as u8,
            opencl_smoke.queue_completed_commands,
            opencl_smoke.fill_rect_uploaded as u8,
            opencl_smoke.queue_error.map(|err| err.code()).unwrap_or(0),
        );
        if crate::allcaps::probes::INTEL_GPGPU_ARTIFACT_BOOT_SMOKETESTS {
            let _ = self::gpgpu::submit_direct_rcs_smoke_once();
            let _ = self::gpgpu::submit_fill_rect_worklist_rgba8_probe_once();
            let _ = self::gpgpu::submit_gradient_rect_worklist_rgba8_probe_once();
            let _ = self::gpgpu::submit_alpha_blend_worklist_rgba8_probe_once();
            crate::log!(
                "intel/gpgpu: rect-worklist-probes fill_ran={} fill_ok={} gradient_ran={} gradient_ok={} alpha_ran={} alpha_ok={} ready={}\n",
                self::gpgpu::fill_rect_worklist_probe_ran() as u8,
                self::gpgpu::fill_rect_worklist_probe_ok() as u8,
                self::gpgpu::gradient_rect_worklist_probe_ran() as u8,
                self::gpgpu::gradient_rect_worklist_probe_ok() as u8,
                self::gpgpu::alpha_blend_worklist_probe_ran() as u8,
                self::gpgpu::alpha_blend_worklist_probe_ok() as u8,
                self::gpgpu::rect_worklist_probe_ready() as u8
            );
            let _ = self::gpgpu::submit_canvas3d_project_once();
            let _ = self::gpgpu::submit_canvas3d_transform_smoke_once();
            let _ = self::gpgpu::submit_canvas3d_clip_box_q16_once();
            let _ = self::gpgpu::submit_canvas3d_plane_sample_rgba8_once();
            let _ = self::gpgpu::submit_canvas3d_plane_fill_rgba8_once();
            let _ = self::gpgpu::submit_canvas3d_plane_patch_fill_cut_rgba8_once();
            let _ = self::gpgpu::submit_canvas3d_plane_patch_worklist_rgba8_once();
        } else {
            crate::log!("intel/gpgpu: artifact boot smoketests skipped allcaps=0\n");
        }
    } else {
        crate::log!(
            "intel/gpgpu: upload and boot probes skipped device=0x{:04X} name={} reason=logo-only-bringup\n",
            dev.device_id,
            display_device_name(dev.device_id)
        );
    }
    if full_ui3_boot {
        let _ = self::blt::submit_bcs0_mi_smoke_once();
        self::fw_probe::log_probe_modules(dev.device_id);
        self::dmc::wire_load_path(dev);
        let huc_fw = self::huc::load_fw();
        let fw = self::guc::load_fw();
        if fw.len == 0 {
            crate::log!(
                "intel/guc: firmware module missing or invalid; continuing to display bring-up\n"
            );
        } else {
            crate::log!(
                "intel/guc: firmware found phys=0x{:X} gpu=0x{:X} len=0x{:X} xfer=0x{:X}\n",
                fw.phys,
                fw.gpu,
                fw.len,
                fw.xfer_len
            );
            let ads = self::guc::alloc_ads(fw.private_data_size);
            if ads.len == 0 {
                crate::log!(
                    "intel/guc: ads alloc failed private_data=0x{:X}; continuing to display bring-up\n",
                    fw.private_data_size
                );
            } else {
                let huc_mapped = huc_fw.len != 0
                    && map_ggtt(dev, huc_fw.phys, huc_fw.len, huc_fw.gpu)
                    && self::huc::map_rsa(dev);
                if !map_ggtt(dev, fw.phys, fw.len, fw.gpu)
                    || !map_ggtt(dev, ads.phys, ads.len, ads.gpu)
                {
                    crate::log!(
                        "intel/guc: ggtt map failed fw_len=0x{:X} ads_len=0x{:X}; continuing to display bring-up\n",
                        fw.len,
                        ads.len
                    );
                } else {
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
                }
            }
        }
    } else {
        crate::log!(
            "intel/guc: firmware/engine bring-up skipped device=0x{:04X} name={} reason=logo-only-bringup\n",
            dev.device_id,
            display_device_name(dev.device_id)
        );
    }
    if DISPLAY_PLANE1_BOOT_DEMO_ENABLED {
        self::display::init_primary_boot_surface(dev);
    } else {
        crate::log!("intel/display: plane1 boot demo disabled\n");
    }
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

pub(crate) fn full_ui3_boot_enabled() -> bool {
    claimed_device()
        .map(|dev| full_ui3_boot_enabled_for_device(dev.device_id))
        .unwrap_or(false)
}

pub(crate) fn display_device_name(device_id: u16) -> &'static str {
    match device_id {
        PCI_DEVICE_ALDER_LAKE_S_GT1 => "alder-lake-s-gt1",
        PCI_DEVICE_ALDER_LAKE_N_N100_UHD => "alder-lake-n-n100-uhd",
        PCI_DEVICE_RAPTOR_LAKE_S_GT1_UHD770 => "raptor-lake-s-gt1-uhd770",
        _ => "intel-display-unknown",
    }
}

fn full_ui3_boot_enabled_for_device(device_id: u16) -> bool {
    !matches!(device_id, PCI_DEVICE_ALDER_LAKE_N_N100_UHD)
}

fn media_decode_enabled_for_device(device_id: u16) -> bool {
    !matches!(device_id, PCI_DEVICE_ALDER_LAKE_N_N100_UHD)
}

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    self::display::active_scanout_dimensions()
}

pub(crate) use self::display::{LiveOverlayRect, PrimaryPlaneSource, PrimaryPlaneSourceFormat};

pub(crate) fn set_primary_plane_source(source: PrimaryPlaneSource, reason: &str) -> bool {
    self::display::set_primary_plane_source(source, reason)
}

pub(crate) fn set_primary_plane_source_mapped(source: PrimaryPlaneSource, reason: &str) -> bool {
    self::display::set_primary_plane_source_mapped(source, reason)
}

pub(crate) fn present_ui_surface_to_primary_plane(
    surface: types::UiSurface,
    phys: u64,
    byte_len: usize,
    src: types::UiRect,
    dst: types::UiRect,
    reason: &str,
) -> bool {
    self::display::present_ui_surface_to_primary_plane(surface, phys, byte_len, src, dst, reason)
}

pub fn primary_surface_gpu_addr() -> Option<u64> {
    self::display::primary_surface_gpu_addr()
}

pub fn primary_present_surface_gpu_addr() -> Option<u64> {
    primary_surface_gpu_addr()
}

pub fn primary_present_shadow_surface_gpu_addr() -> Option<u64> {
    primary_surface_gpu_addr()
}

pub fn dma_cache_flush_range(ptr: *const u8, len: usize) {
    dma_flush(ptr as *mut u8, len)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureStoreSampleKind {
    Mask,
    Rgba,
}

pub fn ggtt_map_screen_rgba_surface(
    _rgba: &[u8],
    _width: u32,
    _height: u32,
    _surface_gpu_addr: u64,
) -> bool {
    false
}

pub fn plane_rebind_present_surface(
    _surface_gpu_addr: u64,
    _width: u32,
    _height: u32,
    _pitch_bytes: u32,
) -> bool {
    false
}

pub fn rcs_present_rgba_frame(rgba: &[u8], width: usize, height: usize) -> bool {
    let Ok(width) = u32::try_from(width) else {
        return false;
    };
    let Ok(height) = u32::try_from(height) else {
        return false;
    };
    self::gpgpu::present_rgba_frame_to_primary(rgba, width, height)
}

pub fn rcs_clear_rgba_surface(
    _rgba: &[u8],
    _width: u32,
    _height: u32,
    _gpu_addr: u64,
    _rgb: u32,
) -> bool {
    false
}

pub fn rcs_draw_rgba_rgb_triangles(
    _target_rgba: &[u8],
    _vertices: &[u8],
    _width: u32,
    _height: u32,
    _target_gpu_addr: u64,
    _scissor: Option<types::ScissorRect>,
    _blend: types::BlendDesc,
) -> bool {
    false
}

pub fn rcs_draw_screen_tex_triangles(
    _target_rgba: &[u8],
    _source_rgba: &[u8],
    _source_width: u32,
    _source_height: u32,
    _vertices: &[u8],
    _target_width: u32,
    _target_height: u32,
    _target_gpu_addr: u64,
    _scissor: Option<types::ScissorRect>,
    _blend: types::BlendDesc,
    _sampler: types::SamplerDesc,
    _sample_kind: TextureStoreSampleKind,
) -> bool {
    false
}

pub fn warm_state() -> Option<()> {
    None
}

pub fn guc_status(_state: ()) -> u32 {
    0
}

pub(crate) fn clear_primary_surface_color(color: u32, reason: &str) -> bool {
    self::display::clear_primary_surface_color(color, reason)
}

pub(crate) async fn wait_hw_logo_sequence_done() {
    self::display::wait_hw_logo_sequence_done().await
}

pub(crate) fn present_i226_diagnostic_screen(
    snapshot: crate::net::i226::I226Snapshot,
    reason: &str,
) -> bool {
    self::display::present_i226_diagnostic_screen(snapshot, reason)
}

pub(crate) fn capture_primary_surface_bgra8() -> Option<self::display::PrimarySurfaceBgra8Snapshot>
{
    self::display::capture_primary_surface_bgra8()
}

pub fn present_rgba_overlay_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    self::display::present_rgba_overlay_top_right(src, src_width, src_height, src_pitch_bytes)
}

pub fn present_rgba_overlay_at(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    x: u32,
    y: u32,
    preserve_alpha: bool,
    reason: &str,
) -> bool {
    self::display::present_rgba_overlay_at(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        x,
        y,
        preserve_alpha,
        reason,
    )
}

pub(crate) fn present_live_overlay_rects(rects: &[LiveOverlayRect], reason: &str) -> bool {
    self::display::present_live_overlay_rects(rects, reason)
}

pub(crate) fn present_live_overlay_rects_preserving(
    rects: &[LiveOverlayRect],
    preserve: Option<LiveOverlayRect>,
    reason: &str,
) -> bool {
    self::display::present_live_overlay_rects_preserving(rects, preserve, reason)
}

pub(crate) fn present_ui3_canvas_rgba(
    rect: LiveOverlayRect,
    src: *mut u8,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    self::display::present_ui3_canvas_rgba(rect, src, src_pitch_bytes, reason)
}

pub fn log_display_plane_ladder_probe(label: &str) {
    self::display::log_display_plane_ladder_probe(label)
}

pub fn present_rgba_primary(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary(src, src_width, src_height, src_pitch_bytes, reason)
}

pub fn blend_rgba_primary_rect(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    dst_x: i32,
    dst_y: i32,
    width: u32,
    height: u32,
    reason: &str,
) -> bool {
    self::display::blend_rgba_primary_rect(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        src_x,
        src_y,
        dst_x,
        dst_y,
        width,
        height,
        reason,
    )
}

pub fn blend_rgba_primary_rect_scaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    dst_x: i32,
    dst_y: i32,
    dst_w: u32,
    dst_h: u32,
    reason: &str,
) -> bool {
    self::display::blend_rgba_primary_rect_scaled(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        src_x,
        src_y,
        src_w,
        src_h,
        dst_x,
        dst_y,
        dst_w,
        dst_h,
        reason,
    )
}

pub fn present_rgba_primary_rot180(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary_rot180(src, src_width, src_height, src_pitch_bytes, reason)
}

pub fn present_rgba_primary_flip_y(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary_flip_y(src, src_width, src_height, src_pitch_bytes, reason)
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
    claimed_device()
        .map(|dev| media_decode_enabled_for_device(dev.device_id))
        .unwrap_or(false)
}

pub(crate) fn hw_pic_service()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::hw_pic::hw_pic_service()
}

pub(crate) fn hw_pic_submit_jpeg(encoded: &[u8]) -> Result<u32, i32> {
    self::hw_pic::submit_jpeg(encoded)
}

pub(crate) fn hw_pic_submit_h264(encoded: &[u8]) -> Result<u32, i32> {
    self::hw_pic::submit_h264(encoded)
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

pub(crate) fn hw_logo_present_task()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::display::hw_logo_present_task()
}

#[embassy_executor::task]
pub(crate) async fn hw_vid_probe_task() {
    crate::log!(
        "intel/hw_vid: probe begin source=embedded-x31-first-frame codec=h264 bytes=0x{:X}\n",
        HW_VID_PROBE_H264.len()
    );
    let id = match self::hw_pic_submit_h264(HW_VID_PROBE_H264) {
        Ok(id) => id,
        Err(code) => {
            let snap = self::hw_pic_snapshot();
            crate::log!(
                "intel/hw_vid: submit failed code={} bytes=0x{:X} pending={} outputs={} service={}\n",
                code,
                HW_VID_PROBE_H264.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            return;
        }
    };
    crate::log!(
        "intel/hw_vid: submit ok id={} bytes=0x{:X} source=embedded-x31-first-frame\n",
        id,
        HW_VID_PROBE_H264.len()
    );
    let Some(output) = self::hw_pic_wait_output_for_id(id, 5000).await else {
        crate::log!("intel/hw_vid: output timeout id={}\n", id);
        return;
    };
    crate::log!(
        "intel/hw_vid: output id={} status={:?} fmt={:?} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} err={} note=stream-probe-only\n",
        output.id,
        output.status,
        output.format,
        output.byte_len,
        output.gpu_addr,
        output.phys_addr,
        output.error_code
    );
}

pub(crate) fn hw_vid_probe_task_spawn()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::hw_vid_probe_task()
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

pub(crate) fn ggtt_invalidate(dev: Dev) {
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
