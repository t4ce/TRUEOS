extern crate alloc;

#[path = "copy/blt.rs"]
mod blt;
mod display;
pub(crate) mod format;
#[path = "gpgpu/gpgpu.rs"]
#[allow(dead_code)]
pub(crate) mod gpgpu;
mod gpu_device;
pub(crate) mod gpu_font;
mod gt_state;
mod guc;
pub(crate) mod guc_ctb;
pub(crate) mod guc_submission;
#[path = "sound/hda.rs"]
pub mod hda;
pub(crate) mod media;
pub(crate) mod opencl;
pub(crate) mod ppgtt;
pub(crate) mod render;
pub(crate) mod shader;
pub(crate) mod stats;
pub(crate) mod types;
mod uc_fw;

pub(crate) use self::blt::{
    GucBcs0CopyCompletion, GucBcs0CopySubmission, GucBcs0CopySubmitError, GucBcs0RgbaCopy,
    GucBcs0RgbaSurface, poll_guc_bcs0_rgba_copies, queue_guc_bcs0_rgba_copies,
    submit_guc_bcs0_fast_copy_probe_now,
};
pub(crate) use self::media::h264_cmd as xelp_media_avc_decode_recipe;
pub(crate) use self::media::hw_pic;
pub(crate) use self::media::xelp_media2_ngin;
pub(crate) use self::media::xelp_media2_ngin_hw_pic;

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Once;

pub(crate) const INTEL_VENDOR_ID: u16 = 0x8086;
pub(crate) const PCI_CLASS_DISPLAY: u8 = 0x03;
const PCI_CLAIM_OWNER: &str = "intel-gen12-gpu";
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const GPU_VA_DISPLAY_OVERLAY_BASE: u64 = 0x0300_0000;
pub(crate) const GPU_VA_DISPLAY_CURSOR_BASE: u64 = 0x0600_0000;
pub(crate) const SPIRIT_CURSOR_DBUF_S1_START: u16 = 1008;
pub(crate) const WARM_ALIGN: usize = 4096;
const GGTT_ALIAS_BASE_OFF: usize = 0x0080_0000;
const GGTT_ALIAS_BYTES: usize = 0x0080_0000;
const GGTT_PAGE_BYTES: u64 = 4096;
const GEN8_PAGE_PRESENT: u64 = 1;
const GEN12_GGTT_PTE_ADDR_MASK: u64 = ((1u64 << 46) - 1) & !0xFFF;
const GEN12_PAT_INDEX_BASE: usize = 0x4800;
const GEN12_PAT_INDEX_COUNT: usize = 8;
const GEN12_PAT_INDEX_STRIDE: usize = 4;
const GEN12_PAT_VALUE_MASK: u32 = 0x3;
const GEN12_PAT_WB: u32 = 0x3;
const GEN12_PAT_WC: u32 = 0x1;
const GEN12_PAT_WT: u32 = 0x2;
const GEN12_PAT_UC: u32 = 0x0;
// Match the Intel Xe-LP PRM required table and the Gen12 table used by i915:
// PPGTT PAT0 is WB, PAT1 is WC, PAT2 is WT, PAT3 is UC, and the
// otherwise-unused entries remain WB.
// Pre-Meteor-Lake GGTT PTEs do not carry a PAT selector, so scanout relies on
// the producer's explicit PPGTT policy plus its render-cache release packet.
const GEN12_INTEGRATED_PAT: [u32; GEN12_PAT_INDEX_COUNT] = [
    GEN12_PAT_WB,
    GEN12_PAT_WC,
    GEN12_PAT_WT,
    GEN12_PAT_UC,
    GEN12_PAT_WB,
    GEN12_PAT_WB,
    GEN12_PAT_WB,
    GEN12_PAT_WB,
];
const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_MEDIA: usize = 0x0A184;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
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
const PCI_DEVICE_WHISKEY_LAKE_UHD_620_GT2: [u16; 2] = [0x3EA0, 0x3EA1];
const GEN11_GT_VEBOX_VDBOX_DISABLE: usize = 0x9140;
const GEN12_S_GT_PLATFORM_VDBOX_MASK: u8 = (1 << 0) | (1 << 2);
static INIT: AtomicBool = AtomicBool::new(false);
static GEN12_INTEGRATED_PAT_READY: AtomicBool = AtomicBool::new(false);
static GEN12_LUMEN_MOCS_READY: AtomicBool = AtomicBool::new(false);
static DISPLAY_GGTT_POLICY_LOGGED: AtomicBool = AtomicBool::new(false);
static GEN12_LUMEN_MOCS_BOOT_REPORT: Once<self::gt_state::Gen12LumenMocsInitReport> = Once::new();
static PHYSICAL_GT_BOOT_REPORT: Once<PhysicalGtBootReport> = Once::new();
// The display device is selected exactly once during boot and never mutates.
// Keep readers lock-free so interrupt-adjacent display/media paths cannot
// deadlock an executor by re-entering a spin mutex held by the interrupted CPU.
static CLAIMED_DEVICE: Once<Dev> = Once::new();

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
struct PhysicalGtBootReport {
    device_id: u16,
    revision_id: u8,
    mmio_base: usize,
    mmio_len: usize,
    render_forcewake_ready: bool,
    media_forcewake_ready: bool,
    gt_forcewake_ready: bool,
    rcs_workarounds_ready: bool,
}

impl PhysicalGtBootReport {
    const fn forcewake_ready(self) -> bool {
        self.render_forcewake_ready && self.gt_forcewake_ready
    }

    const fn accepted(self) -> bool {
        self.forcewake_ready() && self.rcs_workarounds_ready
    }

    const fn bcs_ready(self) -> bool {
        self.gt_forcewake_ready
    }

    fn owns(self, dev: Dev) -> bool {
        self.device_id == dev.device_id
            && self.revision_id == dev.revision_id
            && self.mmio_base == dev.mmio as usize
            && self.mmio_len == dev.mmio_len
    }
}

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

/// Claim and initialize the boot Intel device before any gated GPU, display,
/// or media service is admitted. Claiming alone is not a usable readiness
/// boundary: the same boot owner must establish GT cache policy, GuC
/// transport, physical-device registration, and the display pipeline state
/// consumed by the service gates.
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
        "intel: claimed {:02X}:{:02X}.{} owner={} device=0x{:04X} name={} rev=0x{:02X} mmio_len=0x{:X} guc_boot={} media_decode={}\n",
        dev.bus,
        dev.slot,
        dev.function,
        PCI_CLAIM_OWNER,
        dev.device_id,
        display_device_name(dev.device_id),
        dev.revision_id,
        dev.mmio_len,
        guc_boot as u8,
        media_decode_enabled_for_device(dev.device_id) as u8
    );
    CLAIMED_DEVICE.call_once(|| dev);
    let physical_gt = init_physical_gt_once(dev);
    let forcewake_ready =
        device_uses_gen12_integrated_pat(dev.device_id) && physical_gt.forcewake_ready();
    let lumen_mocs = if forcewake_ready {
        self::gt_state::init_lumen_mocs(dev)
    } else {
        self::gt_state::Gen12LumenMocsInitReport::default()
    };
    GEN12_LUMEN_MOCS_BOOT_REPORT.call_once(|| lumen_mocs);
    let pat_ready = forcewake_ready && init_gen12_integrated_pat(dev);
    GEN12_INTEGRATED_PAT_READY.store(pat_ready, Ordering::Release);
    GEN12_LUMEN_MOCS_READY.store(pat_ready && lumen_mocs.accepted, Ordering::Release);
    log_gen12_lumen_mocs_init(dev, lumen_mocs);
    let media_fuse = media_vdbox_fuse_raw(dev);
    let media_platform_mask = media_platform_vdbox_mask(dev.device_id);
    let media_enabled_mask = media_vdbox_mask(dev);
    crate::log!(
        "intel/media-topology: device=0x{:04X} fuse_reg=0x{:X} fuse_raw=0x{:08X} semantics=disable platform_vdbox_mask=0x{:X} enabled_vdbox_mask=0x{:X} physical_instances=vcs0+vcs2 logical_map=0->0,1->2\n",
        dev.device_id,
        GEN11_GT_VEBOX_VDBOX_DISABLE,
        media_fuse,
        media_platform_mask,
        media_enabled_mask,
    );
    crate::log!(
        "intel/cache-policy: accepted={} platform={} device=0x{:04X} forcewake={} pat={} lumen_mocs={} lumen_mocs_index={} lumen_mocs_command_value={} lumen_mocs_ownership=upper-half ppgtt_default=pat0-wb ppgtt_scanout=pat3-uc ggtt=system-memory-address-only pat_table=[wb,wc,wt,uc,wb,wb,wb,wb] readiness_scope=pat-global+mocs-lumen-only\n",
        pat_ready as u8,
        display_device_name(dev.device_id),
        dev.device_id,
        forcewake_ready as u8,
        pat_ready as u8,
        lumen_mocs.accepted as u8,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX << 1,
    );
    let guc_ready = if guc_boot {
        let ready = init_required_guc_transport(dev);
        if forcewake_ready {
            let _ = validate_gen12_lumen_mocs_for_dev(dev, "post-guc");
        }
        ready
    } else {
        crate::log!(
            "intel/uc-fw: firmware bring-up skipped device=0x{:04X} name={} reason=unsupported-device-policy\n",
            dev.device_id,
            display_device_name(dev.device_id)
        );
        false
    };
    let fixed_render_ggtt_ready =
        guc_ready && physical_gt.accepted() && self::render::init_fixed_render_ggtt_for_boot(dev);
    crate::log_info!(
        target: "render";
        "intel/gt-global-init: fixed_render_ggtt={} ownership=boot-only guc_ready={} client_remap=forbidden\n",
        fixed_render_ggtt_ready as u8,
        guc_ready as u8,
    );
    self::display::log_bsp_display_metrics_probe(dev);
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
    // Install every immutable GuC control window while GT bring-up still owns
    // the global GGTT boundary. Consumer control setup then performs only
    // private PPGTT work; it cannot introduce a launch-time global control-PTE
    // rewrite beside an already resident client.
    let rcs_controls = registered
        .then(|| self::gpgpu::prewarm_direct_rcs_controls_ggtt(dev))
        .unwrap_or_default();
    let bcs0_control_ready = registered && self::blt::prewarm_guc_bcs0_control_ggtt(dev);
    crate::log!(
        "intel/guc: admission accepted={} firmware_ready=1 ctb_ready={} physical_gpu_registered={} rcs_controls={} system_rcs_control={} font_rcs_control={} execution_rcs_control={} lfm25_rcs_control={} ui4_rcs_control={} bcs0_control={} control_mapping=boot-exact-once submission_owner=guc fallback=none next=context-register-on-first-submit\n",
        ctb_ready as u8,
        ctb_ready as u8,
        registered as u8,
        rcs_controls.accepted() as u8,
        rcs_controls.system_service as u8,
        rcs_controls.font as u8,
        rcs_controls.execution as u8,
        rcs_controls.lfm25 as u8,
        rcs_controls.ui4_compositor as u8,
        bcs0_control_ready as u8,
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
    CLAIMED_DEVICE.get().is_some()
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
    CLAIMED_DEVICE.get().copied()
}

pub(crate) fn gen12_actual_gt_ratio(dev: Dev) -> u32 {
    self::gt_state::actual_ratio(dev)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn gen12_gt_state_snapshot() -> Option<self::gt_state::Gen12GtStateSnapshot> {
    claimed_device()
        .filter(|dev| device_uses_gen12_integrated_pat(dev.device_id))
        .map(self::gt_state::read)
}

pub(crate) fn begin_lumen_gt_boost() -> Option<self::gt_state::Gen12LumenGtBoost> {
    let dev = claimed_device()?;
    if !device_uses_gen12_integrated_pat(dev.device_id)
        || !gen12_lumen_mocs_ready()
        || !physical_gt_ready(dev)
    {
        return None;
    }
    self::gt_state::begin_lumen_gt_boost(dev)
}

fn log_gen12_lumen_mocs_init(dev: Dev, report: self::gt_state::Gen12LumenMocsInitReport) {
    crate::log_info!(
        target: "gpgpu";
        "intel/lumen-mocs: checkpoint=boot-init available={} accepted={} device=0x{:04X} ownership=upper-half owned_indices=32..63 owned_l3cc_registers=16..31 resident_indices=0..31 residents_preserved={} resident_fingerprint_before=global:0x{:016X},l3cc:0x{:016X} resident_fingerprint_after=global:0x{:016X},l3cc:0x{:016X} compositor_index4_before=global:0x{:08X},l3cc_pair:0x{:08X} compositor_index4_after=global:0x{:08X},l3cc_pair:0x{:08X} lumen_index={} lumen_command_value={} before=global:0x{:08X},l3cc_pair:0x{:08X} after=global:0x{:08X},l3cc_pair:0x{:08X} expected=global:0x00000005,l3cc:0x0030 owned_mismatches=global:{},l3cc:{} writes=global:32,l3cc:16 readiness_scope=lumen-only shared_rcs_gate=0\n",
        report.available as u8,
        report.accepted as u8,
        dev.device_id,
        report.residents_preserved as u8,
        report.before.resident_global_fingerprint,
        report.before.resident_l3cc_fingerprint,
        report.after.resident_global_fingerprint,
        report.after.resident_l3cc_fingerprint,
        report.before.global_index4,
        report.before.l3cc_pair2,
        report.after.global_index4,
        report.after.l3cc_pair2,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX << 1,
        report.before.global_lumen_index,
        report.before.l3cc_lumen_pair,
        report.after.global_lumen_index,
        report.after.l3cc_lumen_pair,
        report.after.lumen_global_mismatches,
        report.after.lumen_l3cc_mismatches,
    );
}

fn validate_gen12_lumen_mocs_for_dev(dev: Dev, checkpoint: &str) -> bool {
    if !device_uses_gen12_integrated_pat(dev.device_id) {
        GEN12_LUMEN_MOCS_READY.store(false, Ordering::Release);
        return false;
    }
    let readback = self::gt_state::read_mocs(dev);
    let residents_preserved = GEN12_LUMEN_MOCS_BOOT_REPORT.get().is_some_and(|boot| {
        boot.accepted
            && readback.resident_global_fingerprint == boot.after.resident_global_fingerprint
            && readback.resident_l3cc_fingerprint == boot.after.resident_l3cc_fingerprint
    });
    let accepted = GEN12_INTEGRATED_PAT_READY.load(Ordering::Acquire)
        && readback.available
        && readback.lumen_half_accepted
        && residents_preserved;
    if !accepted {
        // Cache-policy loss quarantines Lumen's admission only. Existing UI4,
        // display, and general RCS users retain their independent readiness.
        GEN12_LUMEN_MOCS_READY.store(false, Ordering::Release);
    }
    crate::log_info!(
        target: "gpgpu";
        "intel/lumen-mocs: checkpoint={} available={} accepted={} device=0x{:04X} ownership=upper-half residents_preserved={} resident_fingerprint=global:0x{:016X},l3cc:0x{:016X} lumen_index={} lumen_command_value={} observed=global:0x{:08X},l3cc_pair:0x{:08X} expected=global:0x00000005,l3cc:0x0030 owned_mismatches=global:{},l3cc:{} readiness_scope=lumen-only shared_rcs_gate=0\n",
        checkpoint,
        readback.available as u8,
        accepted as u8,
        dev.device_id,
        residents_preserved as u8,
        readback.resident_global_fingerprint,
        readback.resident_l3cc_fingerprint,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX,
        self::gt_state::GEN12_LUMEN_MOCS_INDEX << 1,
        readback.global_lumen_index,
        readback.l3cc_lumen_pair,
        readback.lumen_global_mismatches,
        readback.lumen_l3cc_mismatches,
    );
    accepted
}

pub(crate) const GEN12_LUMEN_MOCS_INDEX: u32 = self::gt_state::GEN12_LUMEN_MOCS_INDEX;

pub(crate) fn gen12_lumen_mocs_ready() -> bool {
    GEN12_LUMEN_MOCS_READY.load(Ordering::Acquire)
}

pub(crate) fn validate_gen12_lumen_mocs(checkpoint: &str) -> bool {
    claimed_device().is_some_and(|dev| validate_gen12_lumen_mocs_for_dev(dev, checkpoint))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

const fn media_platform_vdbox_mask(device_id: u16) -> u8 {
    match device_id {
        PCI_DEVICE_ALDER_LAKE_S_GT1 | PCI_DEVICE_RAPTOR_LAKE_S_GT1_UHD770 => {
            GEN12_S_GT_PLATFORM_VDBOX_MASK
        }
        PCI_DEVICE_ALDER_LAKE_N_N100_UHD => 0,
        _ => 1,
    }
}

pub(crate) fn media_vdbox_fuse_raw(dev: Dev) -> u32 {
    mmio_read(dev, GEN11_GT_VEBOX_VDBOX_DISABLE)
}

const fn media_vdbox_mask_from_fuse(device_id: u16, fuse_raw: u32) -> u8 {
    (!fuse_raw as u8) & media_platform_vdbox_mask(device_id)
}

/// Physical VDBOX instances enabled by both the platform table and fuses.
///
/// Alder/Raptor Lake use the pre-12.55 disable semantics: a set fuse bit
/// removes the corresponding physical engine. ADL-S exposes VCS0 and VCS2;
/// VCS1 is intentionally not a platform engine.
pub(crate) fn media_vdbox_mask(dev: Dev) -> u8 {
    media_vdbox_mask_from_fuse(dev.device_id, media_vdbox_fuse_raw(dev))
}

const fn media_vdbox_logical_instance_in_mask(mask: u8, physical_instance: u8) -> Option<u8> {
    if physical_instance >= 8 || mask & (1 << physical_instance) == 0 {
        return None;
    }
    Some((mask & ((1 << physical_instance) - 1)).count_ones() as u8)
}

pub(crate) fn media_vdbox_logical_instance(dev: Dev, physical_instance: u8) -> Option<u8> {
    media_vdbox_logical_instance_in_mask(media_vdbox_mask(dev), physical_instance)
}

const _: () = {
    assert!(media_vdbox_mask_from_fuse(PCI_DEVICE_ALDER_LAKE_S_GT1, 0) == 0b0101);
    assert!(media_vdbox_mask_from_fuse(PCI_DEVICE_ALDER_LAKE_S_GT1, 1 << 2) == 0b0001);
    assert!(matches!(media_vdbox_logical_instance_in_mask(0b0101, 0), Some(0)));
    assert!(matches!(media_vdbox_logical_instance_in_mask(0b0101, 2), Some(1)));
    assert!(media_vdbox_logical_instance_in_mask(0b0001, 2).is_none());
};

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    self::display::active_scanout_dimensions()
}

pub(crate) fn complete_scanout_pipeline_slot() -> Option<usize> {
    self::display::complete_scanout_pipeline_slot()
}

pub(crate) fn complete_scanout_pipeline_dimensions(slot: usize) -> Option<(u32, u32)> {
    self::display::complete_scanout_pipeline_dimensions(slot)
}

pub(crate) fn ui4_rgba8_plane_stack_is_ready() -> bool {
    self::display::ui4_rgba8_plane_stack_is_ready()
}

pub(crate) fn ui4_direct_scanout_ready_for_frame(producer_frame: u64) -> Option<u64> {
    self::display::ui4_direct_scanout_ready_for_frame(producer_frame)
}

pub(crate) fn physical_extent_pixels(width_mm: u32, height_mm: u32) -> Option<(u32, u32)> {
    self::display::physical_extent_pixels(width_mm, height_mm)
}

pub(crate) fn display_diagnostic_snapshot_text() -> alloc::string::String {
    self::display::diagnostic_snapshot_text()
}

pub(crate) fn copy_engine_activity_snapshot() -> self::blt::CopyEngineActivitySnapshot {
    self::blt::activity_snapshot()
}

pub(crate) use self::display::{
    CompositionDamageRect, CompositionDamageRegion, LiveOverlayRect, MirrorMapMode,
    RgbaOverlayTile, Ui4AsyncComposition, Ui4AsyncCompositionError, Ui4AsyncCompositionPoll,
    Ui4DirectRgbaFrame, Ui4LiveOverlayFlip, Ui4LiveOverlayFlipPoll, Ui4PlaneSurfaceFlipPoll,
    WdCaptureError, WdCapturePoll, WdCaptureStatus, WdXyuv8888Frame, begin_ui4_wd_xyuv8888_capture,
    poll_ui4_wd_xyuv8888_capture, set_mirror_map_mode, start_ui4_wd_xyuv8888_capture,
    stop_ui4_wd_xyuv8888_capture, ui4_wd_xyuv8888_capture_status,
};

pub(crate) fn set_pipe_a_bottom_color_rgb8(red: u8, green: u8, blue: u8) -> bool {
    self::display::set_pipe_a_bottom_color_rgb8(red, green, blue)
}

pub(crate) fn begin_ui4_plane_surface_flip_batch() -> bool {
    self::display::begin_ui4_plane_surface_flip_batch()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn finish_ui4_plane_surface_flip_batch() -> bool {
    self::display::finish_ui4_plane_surface_flip_batch()
}

pub(crate) fn submit_ui4_plane_surface_flip_batch() -> bool {
    self::display::submit_ui4_plane_surface_flip_batch()
}

pub(crate) fn poll_ui4_plane_surface_flip_batch() -> Ui4PlaneSurfaceFlipPoll {
    self::display::poll_ui4_plane_surface_flip_batch()
}

pub(crate) fn activate_ui4_application_rgba_planes() -> bool {
    self::display::activate_ui4_application_rgba_planes()
}

pub(crate) fn cancel_ui4_plane_surface_flip_batch() {
    self::display::cancel_ui4_plane_surface_flip_batch()
}

pub(crate) fn queue_ui4_primary_composition(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    sparse_static_painter: bool,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_primary_composition(tiles, damage, sparse_static_painter, reason)
}

pub(crate) fn queue_ui4_static_primary_composition_bcs0(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_static_primary_composition_bcs0(tiles, damage, reason)
}

pub(crate) fn queue_ui4_primary_transparent_park(
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_primary_transparent_park(damage, reason)
}

pub(crate) fn queue_ui4_overlay_composition(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    sparse_static_painter: bool,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_overlay_composition(
        plane_slot,
        tiles,
        damage,
        sparse_static_painter,
        reason,
    )
}

pub(crate) fn queue_ui4_static_overlay_composition_cpu(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_static_overlay_composition_cpu(plane_slot, tiles, damage, reason)
}

pub(crate) fn queue_ui4_static_overlay_composition_bcs0(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_static_overlay_composition_bcs0(plane_slot, tiles, damage, reason)
}

pub(crate) fn queue_ui4_direct_overlay_frame(
    plane_slot: usize,
    source: Ui4DirectRgbaFrame,
    pos_x: u32,
    pos_y: u32,
    dest_width: u32,
    dest_height: u32,
    opacity: u8,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, self::display::Ui4AsyncCompositionError> {
    self::display::queue_ui4_direct_overlay_frame(
        plane_slot,
        source,
        pos_x,
        pos_y,
        dest_width,
        dest_height,
        opacity,
        reason,
    )
}

pub(crate) fn poll_ui4_composition(composition: Ui4AsyncComposition) -> Ui4AsyncCompositionPoll {
    self::display::poll_ui4_composition(composition)
}

pub(crate) fn stage_ui4_composition_flip(composition: Ui4AsyncComposition) -> bool {
    self::display::stage_ui4_composition_flip(composition)
}

pub(crate) fn commit_ui4_composition_flip(composition: Ui4AsyncComposition) {
    self::display::commit_ui4_composition_flip(composition)
}

pub(crate) fn ui4_direct_composition_plane_slot(composition: Ui4AsyncComposition) -> Option<usize> {
    self::display::ui4_direct_composition_plane_slot(composition)
}

pub(crate) fn ui4_composition_has_guc_work(composition: Ui4AsyncComposition) -> bool {
    self::display::ui4_composition_has_guc_work(composition)
}

pub(crate) fn ui4_composition_flip_is_live(composition: Ui4AsyncComposition) -> bool {
    self::display::ui4_composition_flip_is_live(composition)
}

pub fn dma_cache_flush_range(ptr: *const u8, len: usize) {
    dma_flush(ptr as *mut u8, len)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum TextureStoreSampleKind {
    Mask,
    Rgba,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn ggtt_map_screen_rgba_surface(
    _rgba: &[u8],
    _width: u32,
    _height: u32,
    _surface_gpu_addr: u64,
) -> bool {
    false
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn plane_rebind_present_surface(
    _surface_gpu_addr: u64,
    _width: u32,
    _height: u32,
    _pitch_bytes: u32,
) -> bool {
    false
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn rcs_clear_rgba_surface(
    _rgba: &[u8],
    _width: u32,
    _height: u32,
    _gpu_addr: u64,
    _rgb: u32,
) -> bool {
    false
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn warm_state() -> Option<()> {
    None
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn capture_primary_surface_bgra8() -> Option<self::display::PrimarySurfaceBgra8Snapshot>
{
    self::display::capture_primary_surface_bgra8()
}

pub(crate) fn queue_ui4_live_overlay_scene_on_slot_damage_region(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Option<Ui4LiveOverlayFlip> {
    self::display::queue_ui4_live_overlay_scene_on_slot_damage_region(
        plane_slot, tiles, rects, damage, reason,
    )
}

pub(crate) fn poll_ui4_live_overlay_flip(flip: Ui4LiveOverlayFlip) -> Ui4LiveOverlayFlipPoll {
    self::display::poll_ui4_live_overlay_flip(flip)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn log_display_plane_ladder_probe(label: &str) {
    self::display::log_display_plane_ladder_probe(label)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub async fn run_media2_first_frame_async() -> Option<self::xelp_media2_ngin::Media2FirstFrameState>
{
    self::xelp_media2_ngin::run_media2_first_frame_async().await
}

pub(crate) fn has_media_decode_engine() -> bool {
    claimed_device()
        .map(|dev| media_decode_enabled_for_device(dev.device_id))
        .unwrap_or(false)
}

pub(crate) fn media_encode_readiness() -> self::xelp_media2_ngin::MediaEncodeReadiness {
    self::xelp_media2_ngin::encode_readiness()
}

pub(crate) fn run_media_guc_vcs0_probe_once() -> self::media::guc_probe::GucVcs0ProbeReport {
    self::media::guc_probe::run_once()
}

#[cfg(feature = "trueos_h264_encode_stream")]
pub(crate) async fn run_media_avc_encode_probe_once()
-> self::media::avc_encode_probe::AvcEncodeProbeReport {
    self::media::avc_encode_probe::run_once().await
}

pub(crate) fn hw_pic_service()
-> Result<embassy_executor::SpawnToken<impl Send>, embassy_executor::SpawnError> {
    self::hw_pic::hw_pic_service()
}

pub(crate) async fn hw_jpeg_decode_rgba(
    encoded: &[u8],
    timeout_ms: u64,
) -> Result<self::hw_pic::HwJpegImage, i32> {
    self::hw_pic::decode_jpeg_rgba(encoded, timeout_ms).await
}

pub(crate) fn hw_pic_submit_h264_in_media_session(
    encoded: &[u8],
    media_session_generation: u64,
) -> Result<u32, i32> {
    self::hw_pic::submit_h264_in_media_session(encoded, media_session_generation)
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub async fn run_media_source_warmup_async() {
    crate::log!("intel/media: source warmup skipped reason=media-decode-disabled\n");
}

fn find_dev() -> Option<Dev> {
    let mut out = None;
    crate::pci::with_devices(|list| {
        for d in list {
            if d.vendor == INTEL_VENDOR_ID && d.class == PCI_CLASS_DISPLAY && out.is_none() {
                if !device_uses_gen12_integrated_pat(d.device) {
                    crate::log!(
                        "intel: refusing unsupported display {:02X}:{:02X}.{} device=0x{:04X} reason=not-gen12-xelp\n",
                        d.bus,
                        d.slot,
                        d.function,
                        d.device,
                    );
                    continue;
                }
                match crate::pci::claim_device(d, PCI_CLAIM_OWNER) {
                    Ok(()) => {}
                    Err(crate::pci::PciClaimError::AlreadyClaimed(owner)) => {
                        crate::log!(
                            "intel: cannot claim display {:02X}:{:02X}.{} device=0x{:04X} owner={}\n",
                            d.bus,
                            d.slot,
                            d.function,
                            d.device,
                            owner,
                        );
                        continue;
                    }
                    Err(crate::pci::PciClaimError::RegistryFull) => {
                        crate::log!(
                            "intel: cannot claim display {:02X}:{:02X}.{} device=0x{:04X} reason=pci-claim-registry-full\n",
                            d.bus,
                            d.slot,
                            d.function,
                            d.device,
                        );
                        continue;
                    }
                }
                let Some(size) = crate::pci::bar0_size_bytes(d.bus, d.slot, d.function) else {
                    let _ = crate::pci::release_device_claim(
                        d.bus,
                        d.slot,
                        d.function,
                        PCI_CLAIM_OWNER,
                    );
                    continue;
                };
                let (lo, hi) = crate::pci::read_bar0_raw(d.bus, d.slot, d.function);
                if lo == 0 || lo == 0xFFFF_FFFF || (lo & 1) != 0 {
                    let _ = crate::pci::release_device_claim(
                        d.bus,
                        d.slot,
                        d.function,
                        PCI_CLAIM_OWNER,
                    );
                    continue;
                }
                let phys = if let Some(hi) = hi {
                    (((hi as u64) << 32) | lo as u64) & !0xF
                } else {
                    (lo as u64) & !0xF
                };
                let Some(mmio) = crate::pci::mmio::map_mmio_region_exact(phys, size as usize)
                    .ok()
                    .map(|p| p.as_ptr())
                else {
                    let _ = crate::pci::release_device_claim(
                        d.bus,
                        d.slot,
                        d.function,
                        PCI_CLAIM_OWNER,
                    );
                    continue;
                };
                crate::pci::enable_mem_and_bus_master(d.bus, d.slot, d.function);
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

fn init_physical_gt_once(dev: Dev) -> PhysicalGtBootReport {
    *PHYSICAL_GT_BOOT_REPORT.call_once(|| {
        let (render_forcewake_ready, media_forcewake_ready, gt_forcewake_ready) =
            retain_forcewake_for_boot(dev);
        let rcs_workarounds_ready = render_forcewake_ready
            && gt_forcewake_ready
            && self::render::init_global_rcs_workarounds_for_boot(dev);
        let report = PhysicalGtBootReport {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            render_forcewake_ready,
            media_forcewake_ready,
            gt_forcewake_ready,
            rcs_workarounds_ready,
        };
        crate::log_info!(
            target: "render";
            "intel/gt-global-init: accepted={} ownership=boot-only device=0x{:04X} render_forcewake={} media_forcewake={} gt_forcewake={} rcs_workarounds={} runtime_repair=forbidden\n",
            report.accepted() as u8,
            dev.device_id,
            report.render_forcewake_ready as u8,
            report.media_forcewake_ready as u8,
            report.gt_forcewake_ready as u8,
            report.rcs_workarounds_ready as u8,
        );
        report
    })
}

/// Read-only runtime admission for every RCS GuC client.
///
/// Physical forcewake and global workarounds are established once during
/// `init_once`. The immutable boot report is the workaround authority; a
/// client verifies only the retained physical forcewake acknowledgements and
/// never reaches through Render warm state or repairs shared registers.
pub(crate) fn physical_gt_ready(dev: Dev) -> bool {
    PHYSICAL_GT_BOOT_REPORT.get().is_some_and(|report| {
        report.owns(dev)
            && report.accepted()
            && mmio_read(dev, FORCEWAKE_ACK_RENDER) & FORCEWAKE_KERNEL != 0
            && mmio_read(dev, FORCEWAKE_ACK_GT) & FORCEWAKE_KERNEL != 0
    })
}

/// Read-only runtime admission for BCS GuC clients.
///
/// Copy consumes the boot-retained GT forcewake domain, but it does not depend
/// on Render forcewake acknowledgement or RCS-specific workaround state.
pub(crate) fn physical_bcs_ready(dev: Dev) -> bool {
    PHYSICAL_GT_BOOT_REPORT.get().is_some_and(|report| {
        report.owns(dev)
            && report.bcs_ready()
            && mmio_read(dev, FORCEWAKE_ACK_GT) & FORCEWAKE_KERNEL != 0
    })
}

fn retain_forcewake_for_boot(dev: Dev) -> (bool, bool, bool) {
    // Establish TRUEOS's retained requests without first clearing firmware or
    // another boot owner's request.  These writes are confined to boot; all
    // GuC clients use `physical_gt_ready` as a read-only admission check.
    mmio_write(dev, FORCEWAKE_RENDER, mask_en(FORCEWAKE_KERNEL));
    let render_ready = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    mmio_write(dev, FORCEWAKE_MEDIA, mask_en(FORCEWAKE_KERNEL));
    let media_ready =
        wait_eq(dev, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_GT, mask_en(FORCEWAKE_KERNEL));
    let gt_ready =
        wait_eq(dev, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    (render_ready, media_ready, gt_ready)
}

const fn device_uses_gen12_integrated_pat(device_id: u16) -> bool {
    matches!(
        device_id,
        PCI_DEVICE_ALDER_LAKE_S_GT1
            | 0x4682
            | 0x4688
            | 0x468A
            | 0x468B
            | 0x4690
            | 0x4692
            | 0x4693
            | PCI_DEVICE_ALDER_LAKE_N_N100_UHD
            | PCI_DEVICE_RAPTOR_LAKE_S_GT1_UHD770
    )
}

const _: () = {
    assert!(!device_uses_gen12_integrated_pat(PCI_DEVICE_WHISKEY_LAKE_UHD_620_GT2[0]));
    assert!(!device_uses_gen12_integrated_pat(PCI_DEVICE_WHISKEY_LAKE_UHD_620_GT2[1]));
};

fn init_gen12_integrated_pat(dev: Dev) -> bool {
    if !device_uses_gen12_integrated_pat(dev.device_id) {
        crate::log!(
            "intel/cache-policy: accepted=0 device=0x{:04X} reason=unsupported-pat-register-layout\n",
            dev.device_id,
        );
        return false;
    }
    if GEN12_PAT_INDEX_BASE
        .checked_add(GEN12_PAT_INDEX_COUNT * GEN12_PAT_INDEX_STRIDE)
        .is_none_or(|end| end > dev.mmio_len)
    {
        crate::log!(
            "intel/cache-policy: accepted=0 device=0x{:04X} reason=pat-registers-outside-mmio mmio_len=0x{:X}\n",
            dev.device_id,
            dev.mmio_len,
        );
        return false;
    }

    for (index, value) in GEN12_INTEGRATED_PAT.iter().copied().enumerate() {
        mmio_write(dev, GEN12_PAT_INDEX_BASE + index * GEN12_PAT_INDEX_STRIDE, value);
    }
    // No TRUEOS-owned GGTT or PPGTT mapping is live yet. Invalidate the GGTT
    // translation cache here so subsequent imports cannot inherit firmware's
    // view of the old global policy.
    ggtt_invalidate(dev);

    let mut observed = [0u32; GEN12_PAT_INDEX_COUNT];
    let mut accepted = true;
    for (index, expected) in GEN12_INTEGRATED_PAT.iter().copied().enumerate() {
        let value = mmio_read(dev, GEN12_PAT_INDEX_BASE + index * GEN12_PAT_INDEX_STRIDE);
        observed[index] = value;
        accepted &= value & GEN12_PAT_VALUE_MASK == expected;
    }
    if !accepted {
        crate::log!(
            "intel/cache-policy: accepted=0 device=0x{:04X} reason=pat-readback-mismatch observed=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            dev.device_id,
            observed[0],
            observed[1],
            observed[2],
            observed[3],
            observed[4],
            observed[5],
            observed[6],
            observed[7],
        );
    }
    accepted
}

fn map_ggtt_pages(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
    if phys & (GGTT_PAGE_BYTES - 1) != 0
        || gpu & (GGTT_PAGE_BYTES - 1) != 0
        || phys & !GEN12_GGTT_PTE_ADDR_MASK != 0
    {
        return false;
    }
    let page_count = len.div_ceil(WARM_ALIGN);
    if page_count != 0 {
        let Some(last_offset) = (page_count as u64 - 1).checked_mul(GGTT_PAGE_BYTES) else {
            return false;
        };
        let Some(last_phys) = phys.checked_add(last_offset) else {
            return false;
        };
        if last_phys & !GEN12_GGTT_PTE_ADDR_MASK != 0 || gpu.checked_add(last_offset).is_none() {
            return false;
        }
    }
    for page in 0..page_count {
        let offset = page as u64 * GGTT_PAGE_BYTES;
        let g = gpu + offset;
        let p = phys + offset;
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
                gen12_integrated_ggtt_pte(p),
            );
        }
    }
    true
}

fn gen12_integrated_ggtt_pte(phys: u64) -> u64 {
    // Alder/Raptor Lake GGTT PTEs have no PAT selector for system memory.
    // Bits 3/4/7 are not the PPGTT PAT bits in this address space.
    (phys & GEN12_GGTT_PTE_ADDR_MASK) | GEN8_PAGE_PRESENT
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
    if !gen12_integrated_pat_ready()
        || !device_uses_gen12_integrated_pat(dev.device_id)
        || len == 0
        || !map_ggtt_pages(dev, phys, len, gpu)
    {
        return false;
    }

    let last_page = (len - 1) / GGTT_PAGE_BYTES as usize;
    let last_gpu = match gpu.checked_add(last_page as u64 * GGTT_PAGE_BYTES) {
        Some(address) => address,
        None => return false,
    };
    let last_phys = match phys.checked_add(last_page as u64 * GGTT_PAGE_BYTES) {
        Some(address) => address,
        None => return false,
    };
    let first_ok = read_ggtt_pte(dev, gpu) == Some(gen12_integrated_ggtt_pte(phys));
    let last_ok = read_ggtt_pte(dev, last_gpu) == Some(gen12_integrated_ggtt_pte(last_phys));
    if !first_ok || !last_ok {
        crate::log!(
            "intel/display-cache-contract: accepted=0 reason=ggtt-pte-readback first_ok={} last_ok={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X}\n",
            first_ok as u8,
            last_ok as u8,
            gpu,
            phys,
            len,
        );
        return false;
    }
    if !DISPLAY_GGTT_POLICY_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!(
            "intel/display-cache-contract: accepted=1 device=0x{:04X} render_ppgtt=pat3-uc-for-direct-scanout display_ggtt=address-present-only system_memory=1 cpu_surface_flush=not-part-of-mapping render_release=required\n",
            dev.device_id,
        );
    }
    true
}

pub(crate) fn gen12_integrated_pat_ready() -> bool {
    GEN12_INTEGRATED_PAT_READY.load(Ordering::Acquire)
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
fn wait_eq(dev: Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
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

fn dma_flush_cache_lines(ptr: *mut u8, len: usize) {
    unsafe {
        use core::arch::x86_64::_mm_clflush;
        let mut p = (ptr as usize) & !63usize;
        let end = (ptr as usize).saturating_add(len);
        while p < end {
            _mm_clflush(p as *const _);
            let Some(next) = p.checked_add(64) else {
                break;
            };
            p = next;
        }
    }
}

fn dma_flush_fence() {
    unsafe {
        use core::arch::x86_64::_mm_mfence;
        _mm_mfence();
    }
}

pub(crate) fn dma_flush(ptr: *mut u8, len: usize) {
    dma_flush_cache_lines(ptr, len);
    dma_flush_fence();
}

#[derive(Clone, Copy)]
pub(crate) struct DmaFlushRows {
    ptr: *mut u8,
    row_bytes: usize,
    row_stride: usize,
    rows: usize,
}

impl DmaFlushRows {
    pub(crate) const EMPTY: Self = Self::new(core::ptr::null_mut(), 0, 0, 0);

    pub(crate) const fn new(
        ptr: *mut u8,
        row_bytes: usize,
        row_stride: usize,
        rows: usize,
    ) -> Self {
        Self {
            ptr,
            row_bytes,
            row_stride,
            rows,
        }
    }
}

fn dma_flush_rows_span_len(span: DmaFlushRows) -> Option<usize> {
    if span.row_bytes == 0 || span.rows == 0 {
        return Some(0);
    }
    if span.ptr.is_null() || (span.rows > 1 && span.row_stride < span.row_bytes) {
        return None;
    }
    let last_row_offset = span.row_stride.checked_mul(span.rows - 1)?;
    let total_span = last_row_offset.checked_add(span.row_bytes)?;
    (span.ptr as usize).checked_add(total_span)?;
    Some(total_span)
}

/// Validate every strided set before touching a cache line, then flush the
/// complete collection with one final visibility fence.
pub(crate) fn dma_flush_strided_row_spans(spans: &[DmaFlushRows]) -> bool {
    if spans
        .iter()
        .copied()
        .any(|span| dma_flush_rows_span_len(span).is_none())
    {
        return false;
    }

    let mut flushed = false;
    for span in spans.iter().copied() {
        let Some(total_span) = dma_flush_rows_span_len(span) else {
            unreachable!("DMA flush spans were prevalidated");
        };
        if total_span == 0 {
            continue;
        }
        if span.rows == 1 || span.row_stride == span.row_bytes {
            dma_flush_cache_lines(span.ptr, total_span);
        } else {
            for row in 0..span.rows {
                // Validation of the final row proves every earlier offset.
                let offset = span.row_stride * row;
                dma_flush_cache_lines(unsafe { span.ptr.add(offset) }, span.row_bytes);
            }
        }
        flushed = true;
    }
    if flushed {
        dma_flush_fence();
    }
    true
}

/// Flush `rows` cacheable DMA spans separated by `row_stride`, then establish
/// one visibility point for the complete set. This preserves `dma_flush`'s
/// CLFLUSH + MFENCE contract without paying one MFENCE per scanout row.
pub(crate) fn dma_flush_strided_rows(
    ptr: *mut u8,
    row_bytes: usize,
    row_stride: usize,
    rows: usize,
) -> bool {
    dma_flush_strided_row_spans(&[DmaFlushRows::new(ptr, row_bytes, row_stride, rows)])
}
