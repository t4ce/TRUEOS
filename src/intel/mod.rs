#![allow(dead_code)]

extern crate alloc;

#[path = "copy/blt.rs"]
mod blt;
mod display;
pub(crate) mod format;
#[path = "gpgpu/gpgpu.rs"]
pub(crate) mod gpgpu;
mod gpu_device;
pub(crate) mod gpu_font;
mod guc;
pub(crate) mod guc_ctb;
pub(crate) mod guc_submission;
#[path = "sound/hda.rs"]
pub mod hda;
mod hw_cursor;
pub(crate) mod media;
pub(crate) mod opencl;
pub(crate) mod ppgtt;
pub(crate) mod render;
pub(crate) mod shader;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod types;
mod uc_fw;

pub(crate) use self::media::h264_cmd as xelp_media_avc_decode_recipe;
pub(crate) use self::media::hw_pic;
pub(crate) use self::media::xelp_media2_ngin;
pub(crate) use self::media::xelp_media2_ngin_hw_pic;

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub(crate) const INTEL_VENDOR_ID: u16 = 0x8086;
pub(crate) const PCI_CLASS_DISPLAY: u8 = 0x03;
// Permanent GuC GGTT reservations. Firmware stays below the legacy RCS arena.
// ADS and CTB use the otherwise empty 0x0700_0000..0x0800_0000 window between
// the display cursor and media arenas; GuC 70.49 requests about 8 MiB of
// private ADS data, so it cannot share the small low-address firmware window.
pub(crate) const GPU_VA_GUC_FW_BASE: u64 = 0x0010_0000;
const GPU_VA_GUC_FW_LIMIT: u64 = 0x0080_0000;
pub(crate) const GPU_VA_GUC_ADS_BASE: u64 = 0x0700_0000;
pub(crate) const GPU_VA_GUC_CTB_BASE: u64 = 0x07F0_0000;
pub(crate) const GPU_VA_GUC_RUNTIME_LIMIT: u64 = 0x0800_0000;
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
const PCI_DEVICE_ALDER_LAKE_S_GT1: u16 = 0x4680;
const PCI_DEVICE_ALDER_LAKE_N_N100_UHD: u16 = 0x46D1;
const PCI_DEVICE_RAPTOR_LAKE_S_GT1_UHD770: u16 = 0xA780;
static INIT: AtomicBool = AtomicBool::new(false);
static CLAIMED_DEVICE: Mutex<Option<Dev>> = Mutex::new(None);

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
    let guc_boot = guc_boot_enabled_for_device(dev.device_id);
    crate::log!(
        "intel: claimed {:02X}:{:02X}.{} device=0x{:04X} name={} rev=0x{:02X} mmio_len=0x{:X} guc_boot={} media_decode={}\n",
        dev.bus,
        dev.slot,
        dev.function,
        dev.device_id,
        display_device_name(dev.device_id),
        dev.revision_id,
        dev.mmio_len,
        guc_boot as u8,
        media_decode_enabled_for_device(dev.device_id) as u8
    );
    *CLAIMED_DEVICE.lock() = Some(dev);
    if guc_boot {
        let _ = init_required_guc_transport(dev);
    } else {
        crate::log!(
            "intel/uc-fw: firmware bring-up skipped device=0x{:04X} name={} reason=unsupported-device-policy\n",
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
}

fn init_required_guc_transport(dev: Dev) -> bool {
    let fw = self::guc::load_fw();
    if fw.len == 0 {
        crate::log!(
            "intel/guc: admission accepted=0 reason=firmware-module-missing-or-invalid submission_fallback=none display_continues=1\n"
        );
        return false;
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
        crate::log!(
            "intel/guc: admission accepted=0 reason=ads-alloc-failed private_data=0x{:X} submission_fallback=none display_continues=1\n",
            fw.private_data_size
        );
        return false;
    }
    let fw_end = fw.gpu.checked_add(fw.len as u64);
    let ads_end = ads.gpu.checked_add(ads.len as u64);
    if fw_end.is_none_or(|end| end > GPU_VA_GUC_FW_LIMIT)
        || ads_end.is_none_or(|end| end > GPU_VA_GUC_CTB_BASE)
    {
        crate::log!(
            "intel/guc: admission accepted=0 reason=reserved-va-overflow fw_end=0x{:X} fw_limit=0x{:X} ads_end=0x{:X} ads_limit=0x{:X}\n",
            fw_end.unwrap_or(u64::MAX),
            GPU_VA_GUC_FW_LIMIT,
            ads_end.unwrap_or(u64::MAX),
            GPU_VA_GUC_CTB_BASE
        );
        return false;
    }
    if !map_ggtt(dev, fw.phys, fw.len, fw.gpu) || !map_ggtt(dev, ads.phys, ads.len, ads.gpu) {
        crate::log!(
            "intel/guc: admission accepted=0 reason=ggtt-map-failed fw_len=0x{:X} ads_len=0x{:X} submission_fallback=none display_continues=1\n",
            fw.len,
            ads.len
        );
        return false;
    }

    ggtt_invalidate(dev);
    forcewake(dev);
    let ready = self::guc::bootstrap(dev, fw, ads, false);
    let status = self::guc::status(dev);
    let (bootrom, ukernel, auth) = self::guc::describe_status(status);
    crate::log!(
        "intel/guc: bootstrap ready={} status=0x{:08X} bootrom={} ukernel={} auth=0x{:X} scheduler=enabled\n",
        ready as u8,
        status,
        bootrom,
        ukernel,
        auth
    );
    if !ready {
        crate::log!(
            "intel/guc: admission accepted=0 reason=firmware-not-ready submission_fallback=none display_continues=1\n"
        );
        return false;
    }

    let ctb_ready = self::guc_ctb::init_and_enable(dev);
    let registered =
        ctb_ready && crate::gpu::register_physical_device(&self::gpu_device::INTEL_PHYSICAL_GPU);
    crate::log!(
        "intel/guc: admission accepted={} firmware_ready=1 ctb_ready={} physical_gpu_registered={} submission_owner=guc fallback=none next=context-register-on-first-submit\n",
        ctb_ready as u8,
        ctb_ready as u8,
        registered as u8
    );
    ctb_ready
}

pub fn guc_ready() -> bool {
    self::guc::ready()
}

pub(crate) fn guc_submission_ready() -> bool {
    self::guc_submission::ready()
}

pub fn has_claimed_device() -> bool {
    CLAIMED_DEVICE.lock().is_some()
}

/// Keep emulator policy aligned with the existing Intel display hardware split.
/// A VM with the real display device passed through intentionally follows the
/// hardware path, because it needs the same display/GPGPU handling.
pub(crate) fn is_emulator_environment() -> bool {
    !crate::pci::with_devices(|devices| {
        devices
            .iter()
            .any(|device| device.vendor == INTEL_VENDOR_ID && device.class == PCI_CLASS_DISPLAY)
    })
}

pub(crate) fn claimed_device() -> Option<Dev> {
    *CLAIMED_DEVICE.lock()
}

pub(crate) fn guc_boot_enabled() -> bool {
    claimed_device()
        .map(|dev| guc_boot_enabled_for_device(dev.device_id))
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

fn guc_boot_enabled_for_device(device_id: u16) -> bool {
    !matches!(device_id, PCI_DEVICE_ALDER_LAKE_N_N100_UHD)
}

fn media_decode_enabled_for_device(device_id: u16) -> bool {
    !matches!(device_id, PCI_DEVICE_ALDER_LAKE_N_N100_UHD)
}

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    self::display::active_scanout_dimensions()
}

pub(crate) use self::display::{
    CompositionDamageRect, LiveOverlayRect, PrimaryPlaneSource, PrimaryPlaneSourceFormat,
    RgbaOverlayTile,
};

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

pub(crate) fn present_ui_surface_to_primary_backing(
    surface: types::UiSurface,
    virt: *const u8,
    byte_len: usize,
    src: types::UiRect,
    dst: types::UiRect,
    reason: &str,
) -> bool {
    self::display::present_ui_surface_to_primary_backing(surface, virt, byte_len, src, dst, reason)
}

pub(crate) fn present_rgba8_surface_to_primary_swap_xrgb(
    src: self::gpgpu::GpgpuRgba8Surface,
    src_rect: self::gpgpu::GpgpuRect,
    dst_xy: self::gpgpu::GpgpuPoint,
    reason: &str,
) -> bool {
    self::display::present_rgba8_surface_to_primary_swap_xrgb(src, src_rect, dst_xy, reason)
}

pub(crate) fn present_premultiplied_rgba_primary_tiles(
    tiles: &[RgbaOverlayTile<'_>],
    reason: &str,
) -> bool {
    self::display::present_premultiplied_rgba_primary_tiles(tiles, reason)
}

pub(crate) fn present_premultiplied_rgba_primary_tiles_damage(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRect,
    reason: &str,
) -> bool {
    self::display::present_premultiplied_rgba_primary_tiles_damage(tiles, damage, reason)
}

pub(crate) fn hide_decoded_nv12_overlay_plane(reason: &str) -> bool {
    self::display::hide_decoded_nv12_overlay_plane(reason)
}

pub(crate) fn set_decoded_nv12_overlay_plane_alpha(alpha: u8, reason: &str) -> bool {
    self::display::set_decoded_nv12_overlay_plane_alpha(alpha, reason)
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

pub fn rcs_draw_rgba_solid_batch(
    _target_rgba: &[u8],
    _records: &[u8],
    _width: u32,
    _height: u32,
    _target_gpu_addr: u64,
    _scissor: Option<types::ScissorRect>,
    _blend: types::BlendDesc,
) -> bool {
    false
}

pub fn rcs_draw_screen_sprite_batch(
    _target_rgba: &[u8],
    _source_rgba: &[u8],
    _source_width: u32,
    _source_height: u32,
    _records: &[u8],
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

pub(crate) fn present_live_overlay_rects_damage(
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRect,
    reason: &str,
) -> bool {
    self::display::present_live_overlay_rects_damage(rects, damage, reason)
}

pub(crate) fn present_live_overlay_rects_preserving(
    rects: &[LiveOverlayRect],
    preserve: Option<LiveOverlayRect>,
    reason: &str,
) -> bool {
    self::display::present_live_overlay_rects_preserving(rects, preserve, reason)
}

pub(crate) fn present_rgba_overlay_tiles(tiles: &[RgbaOverlayTile<'_>], reason: &str) -> bool {
    self::display::present_rgba_overlay_tiles(tiles, reason)
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

pub fn present_rgba_primary_center_unscaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary_center_unscaled(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        reason,
    )
}

pub fn present_rgba_primary_center_unscaled_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary_center_unscaled_bg(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        bg_xrgb,
        reason,
    )
}

pub fn present_rgba_primary_center_plane_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    self::display::present_rgba_primary_center_plane_bg(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        bg_xrgb,
        reason,
    )
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

pub(crate) fn hw_vid_probe_enabled() -> bool {
    self::media::hw_vid::probe_enabled()
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

pub(crate) fn hw_vid_probe_task_spawn()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::media::hw_vid::hw_vid_probe_task()
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

/// Remove a display-owned GGTT range after its plane has been proven idle.
/// Stable display GPU slots can then be safely reused by a later frame owner.
pub(crate) fn unmap_display_scanout_ggtt(dev: Dev, len: usize, gpu: u64) -> bool {
    if len == 0 {
        return true;
    }
    let page_bytes = GGTT_PAGE_BYTES as usize;
    let page_count = match len.checked_add(page_bytes - 1) {
        Some(bytes) => bytes / page_bytes,
        None => return false,
    };
    for page in 0..page_count {
        let byte_offset = match page.checked_mul(page_bytes) {
            Some(offset) => offset,
            None => return false,
        };
        let page_gpu = match gpu.checked_add(byte_offset as u64) {
            Some(address) => address,
            None => return false,
        };
        let Some(index) = ggtt_offset_index(page_gpu) else {
            return false;
        };
        unsafe {
            core::ptr::write_volatile(dev.mmio.add(GGTT_ALIAS_BASE_OFF + index) as *mut u64, 0);
        }
    }
    ggtt_invalidate(dev);
    true
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
    let minimum = fw
        .checked_add(GUC_WOPCM_RESERVED_SIZE)?
        .checked_add(GUC_WOPCM_STACK_RESERVED_SIZE)?;
    let base = align_up_u32(WOPCM_RESERVED_SIZE, GUC_WOPCM_OFFSET_ALIGNMENT)?;
    if base >= usable {
        return None;
    }
    let size = (usable - base) & GUC_WOPCM_SIZE_MASK;
    if size < minimum {
        None
    } else {
        Some((base, size))
    }
}
pub(crate) fn align_up(v: usize, a: usize) -> Option<usize> {
    let m = a.checked_sub(1)?;
    v.checked_add(m).map(|x| x & !m)
}
fn align_up_u32(v: u32, a: u32) -> Option<u32> {
    let mask = a.checked_sub(1)?;
    v.checked_add(mask).map(|value| value & !mask)
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
