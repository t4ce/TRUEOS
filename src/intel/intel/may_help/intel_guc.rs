use core::sync::atomic::{AtomicBool, Ordering};

use super::intel_igpu770::{Igpu770WarmState, forcewake_all_acquire, mmio_read32, mmio_write32};

const GUC_MODULE_STRING: &[u8] = b"trueos.fw.guc";
const ZSTD_MAGIC: u32 = 0xFD2F_B528;
const GPU_VA_GUC_FW_BASE: u64 = 0x0085_0000;

const GUC_STATUS: usize = 0x0000_C000;
const SOFT_SCRATCH_BASE: usize = 0x0000_C180;
const SOFT_SCRATCH_COUNT: usize = 16;
const GDRST: usize = 0x0000_941C;
const GUC_SHIM_CONTROL: usize = 0x0000_C064;
const GUC_SHIM_CONTROL2: usize = 0x0000_C068;
const PMINTRMSK: usize = 0x0000_A168;
const GT_PM_CONFIG: usize = 0x0013_816C;
const DIST_DBS_POPULATED: usize = 0x0000_0D08;
const DMA_ADDR_0_LOW: usize = 0x0000_C300;
const DMA_ADDR_0_HIGH: usize = 0x0000_C304;
const DMA_ADDR_1_LOW: usize = 0x0000_C308;
const DMA_ADDR_1_HIGH: usize = 0x0000_C30C;
const DMA_COPY_SIZE: usize = 0x0000_C310;
const DMA_CTRL: usize = 0x0000_C314;
const DMA_GUC_WOPCM_OFFSET: usize = 0x0000_C340;
const GUC_WOPCM_SIZE: usize = 0x0000_C050;
const UOS_RSA_SCRATCH_BASE: usize = 0x0000_C200;
const UOS_RSA_SCRATCH_COUNT: usize = 64;
const GUC_BOOTSTRAP_REV: &str = "guc-bootstrap-r14";

const GUC_DISABLE_SRAM_INIT_TO_ZEROES: u32 = 1 << 0;
const GT_DOORBELL_ENABLE: u32 = 1 << 0;
const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;
const GUC_ENABLE_MIA_CACHING: u32 = 1 << 2;
const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
const GUC_ENABLE_DEBUG_REG: u32 = 1 << 11;
const GUC_LOG_DISABLED: u32 = 1 << 6;
const ARAT_EXPIRED_INTRMSK: u32 = 1 << 9;
const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
const UOS_MOVE: u32 = 1 << 4;
const START_DMA: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_SHIFT: u32 = 14;
const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;
const GUC_WOPCM_SIZE_MASK: u32 = 0xFFFFF << 12;
const GUC_BOOT_DEST_WOPCM_OFFSET: u32 = 0x2000;
const GUC_USE_LEGACY_SHIM_BITS: bool = true;
const GUC_RSA_IN_MEMORY_THRESHOLD_BYTES: usize = 256;
const GUC_DMA_POLL_ITERS: usize = 20_000;
const GUC_READY_POLL_ITERS: usize = 200_000;
const GUC_RESET_POLL_ITERS: usize = 100_000;
const GUC_CTL_LOG_PARAMS: usize = 0;
const GUC_CTL_WA: usize = 1;
const GUC_CTL_FEATURE: usize = 2;
const GUC_CTL_DEBUG: usize = 3;
const GUC_CTL_ADS: usize = 4;
const GUC_CTL_DEVID: usize = 5;
const GUC_CTL_MAX_DWORDS: usize = SOFT_SCRATCH_COUNT - 2;
const GUC_ADS_ADDR_SHIFT: u32 = 1;
const GUC_MAX_ENGINE_CLASSES: usize = 16;
const GUC_MAX_INSTANCES_PER_CLASS: usize = 32;
const GUC_CAPTURE_LIST_INDEX_MAX: usize = 2;
const GUC_UM_HW_QUEUE_MAX: usize = 3;
const GLOBAL_POLICY_MAX_NUM_WI: u32 = 15;
const GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US: u32 = 500_000;
const DOORBELLS_PER_SQIDI_MASK: u32 = 0x00FF_0000;
const DOORBELLS_PER_SQIDI_SHIFT: u32 = 16;
const GEN11_WOPCM_SIZE: u32 = 0x0020_0000;
const WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_STACK_RESERVED_SIZE: u32 = 0x0000_2000;
const WOPCM_HW_CTX_RESERVED_SIZE: u32 = 0x0000_9000;
const GUC_WOPCM_OFFSET_ALIGNMENT: u32 = 1 << GUC_WOPCM_OFFSET_SHIFT;
const GS_BOOTROM_SHIFT: u32 = 1;
const GS_BOOTROM_MASK: u32 = 0x7F << GS_BOOTROM_SHIFT;
const GS_UKERNEL_SHIFT: u32 = 8;
const GS_UKERNEL_MASK: u32 = 0xFF << GS_UKERNEL_SHIFT;
const GS_AUTH_STATUS_SHIFT: u32 = 30;
const GS_AUTH_STATUS_MASK: u32 = 0x03 << GS_AUTH_STATUS_SHIFT;
const GS_AUTH_STATUS_BAD: u32 = 0x01;
const GS_MIA_IN_RESET: u32 = 1 << 0;
const GRDOM_GUC: u32 = 1 << 3;

const INTEL_GUC_LOAD_STATUS_START: u32 = 0x01;
const INTEL_GUC_LOAD_STATUS_ERROR_DEVID_BUILD_MISMATCH: u32 = 0x02;
const INTEL_GUC_LOAD_STATUS_GUC_PREPROD_BUILD_MISMATCH: u32 = 0x03;
const INTEL_GUC_LOAD_STATUS_ERROR_DEVID_INVALID_GUCTYPE: u32 = 0x04;
const INTEL_GUC_LOAD_STATUS_HWCONFIG_START: u32 = 0x05;
const INTEL_GUC_LOAD_STATUS_HWCONFIG_DONE: u32 = 0x06;
const INTEL_GUC_LOAD_STATUS_HWCONFIG_ERROR: u32 = 0x07;
const INTEL_GUC_LOAD_STATUS_GDT_DONE: u32 = 0x10;
const INTEL_GUC_LOAD_STATUS_IDT_DONE: u32 = 0x20;
const INTEL_GUC_LOAD_STATUS_LAPIC_DONE: u32 = 0x30;
const INTEL_GUC_LOAD_STATUS_GUCINT_DONE: u32 = 0x40;
const INTEL_GUC_LOAD_STATUS_DPC_READY: u32 = 0x50;
const INTEL_GUC_LOAD_STATUS_BOOTROM_VERSION_MISMATCH: u32 = 0x08;
const INTEL_GUC_LOAD_STATUS_DPC_ERROR: u32 = 0x60;
const INTEL_GUC_LOAD_STATUS_EXCEPTION: u32 = 0x70;
const INTEL_GUC_LOAD_STATUS_INIT_DATA_INVALID: u32 = 0x71;
const INTEL_GUC_LOAD_STATUS_PXP_TEARDOWN_CTRL_ENABLED: u32 = 0x72;
const INTEL_GUC_LOAD_STATUS_MPU_DATA_INVALID: u32 = 0x73;
const INTEL_GUC_LOAD_STATUS_INIT_MMIO_SAVE_RESTORE_INVALID: u32 = 0x74;
const INTEL_GUC_LOAD_STATUS_KLV_WORKAROUND_INIT_ERROR: u32 = 0x75;
const INTEL_GUC_LOAD_STATUS_READY: u32 = 0xF0;

const INTEL_BOOTROM_STATUS_NO_KEY_FOUND: u32 = 0x13;
const INTEL_BOOTROM_STATUS_AES_PROD_KEY_FOUND: u32 = 0x1A;
const INTEL_BOOTROM_STATUS_RSA_FAILED: u32 = 0x50;
const INTEL_BOOTROM_STATUS_PAVPC_FAILED: u32 = 0x73;
const INTEL_BOOTROM_STATUS_WOPCM_FAILED: u32 = 0x74;
const INTEL_BOOTROM_STATUS_LOADLOC_FAILED: u32 = 0x75;
const INTEL_BOOTROM_STATUS_JUMP_PASSED: u32 = 0x76;
const INTEL_BOOTROM_STATUS_JUMP_FAILED: u32 = 0x77;
const INTEL_BOOTROM_STATUS_RC6CTXCONFIG_FAILED: u32 = 0x79;
const INTEL_BOOTROM_STATUS_MPUMAP_INCORRECT: u32 = 0x7A;
const INTEL_BOOTROM_STATUS_EXCEPTION: u32 = 0x7E;
const INTEL_BOOTROM_STATUS_PROD_KEY_CHECK_FAILURE: u32 = 0x2B;

static GUC_FW_BOOTSTRAP_RAN: AtomicBool = AtomicBool::new(false);
static GUC_FW_READY: AtomicBool = AtomicBool::new(false);
static GUC_FW_BLOB_LEN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_DMA_OFFSET: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_RSA_OFFSET: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_RSA_SIZE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_PRIVATE_DATA_SIZE: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

#[derive(Copy, Clone, Debug)]
pub struct GucFirmwareInfo {
    pub phys: u64,
    pub virt: *mut u8,
    pub len: usize,
    pub xfer_len: usize,
    pub gpu_addr: u64,
}

/// CSS header struct matching i915's uc_css_header exactly (128 bytes).
/// Layout from drivers/gpu/drm/i915/gt/uc/intel_uc_fw_abi.h
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct UcCssHeader {
    module_type: u32,
    header_size_dw: u32,
    header_version: u32,
    module_id: u32,
    module_vendor: u32,
    date: u32,
    size_dw: u32,
    key_size_dw: u32,
    modulus_size_dw: u32,
    exponent_size_dw: u32,
    time: u32,
    username: [u8; 8],
    buildnumber: [u8; 12],
    sw_version: u32,
    vf_version: u32,
    reserved0: [u32; 12],
    private_data_size: u32,
    header_info: u32,
}

const _: () = assert!(core::mem::size_of::<UcCssHeader>() == 128);

#[inline]
fn read_le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = bytes.get(off..end)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[derive(Copy, Clone, Debug)]
struct GucCssLayout {
    dma_bytes: usize,
    rsa_offset: usize,
    rsa_size: usize,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucMmioRegSet {
    address: u32,
    count: u16,
    reserved: u16,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucPolicies {
    submission_queue_depth: [u32; GUC_MAX_ENGINE_CLASSES],
    dpc_promote_time: u32,
    is_valid: u32,
    max_num_work_items: u32,
    global_flags: u32,
    reserved: [u32; 4],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucGtSystemInfo {
    mapping_table: [[u8; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES],
    engine_enabled_masks: [u32; GUC_MAX_ENGINE_CLASSES],
    generic_gt_sysinfo: [u32; GUC_MAX_ENGINE_CLASSES],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAds {
    reg_state_list: [[GucMmioRegSet; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES],
    reserved0: u32,
    scheduler_policies: u32,
    gt_system_info: u32,
    reserved1: u32,
    control_data: u32,
    golden_context_lrca: [u32; GUC_MAX_ENGINE_CLASSES],
    eng_state_size: [u32; GUC_MAX_ENGINE_CLASSES],
    private_data: u32,
    um_init_data: u32,
    capture_instance: [[u32; GUC_MAX_ENGINE_CLASSES]; GUC_CAPTURE_LIST_INDEX_MAX],
    capture_class: [[u32; GUC_MAX_ENGINE_CLASSES]; GUC_CAPTURE_LIST_INDEX_MAX],
    capture_global: [u32; GUC_CAPTURE_LIST_INDEX_MAX],
    wa_klv_addr_lo: u32,
    wa_klv_addr_hi: u32,
    wa_klv_size: u32,
    reserved: [u32; 11],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucEngineUsageRecord {
    current_context_index: u32,
    last_switch_in_stamp: u32,
    reserved0: u32,
    total_runtime: u32,
    reserved1: [u32; 4],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucEngineUsage {
    engines: [[GucEngineUsageRecord; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucUmQueueParams {
    base_dpa: u64,
    base_ggtt_address: u32,
    size_in_bytes: u32,
    rsvd: [u32; 4],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucUmInitParams {
    page_response_timeout_in_us: u64,
    rsvd: [u32; 6],
    queue_params: [GucUmQueueParams; GUC_UM_HW_QUEUE_MAX],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAdsBlobHeader {
    ads: GucAds,
    policies: GucPolicies,
    system_info: GucGtSystemInfo,
    engine_usage: GucEngineUsage,
    um_init_params: GucUmInitParams,
}

impl GucFirmwareInfo {
    pub const fn empty() -> Self {
        Self {
            phys: 0,
            virt: core::ptr::null_mut(),
            len: 0,
            xfer_len: 0,
            gpu_addr: 0,
        }
    }
}

/// Parse CSS header from firmware blob using exact i915 semantics.
/// Handles both standard (module_type=0x6) and signed (0x5) variants.
fn parse_guc_css_layout_at(blob: &[u8], css_off: usize) -> Option<GucCssLayout> {
    let min_len = css_off.checked_add(core::mem::size_of::<UcCssHeader>())?;
    if blob.len() < min_len {
        return None;
    }

    // Read CSS header via struct repr(C, packed).
    let css_ptr = unsafe { blob.as_ptr().add(css_off) as *const UcCssHeader };
    let css = unsafe { css_ptr.read_unaligned() };

    // i915: check module_type is GuC (0x6) or signed variant (0x5)
    if css.module_type != 0x0000_0006 && css.module_type != 0x0000_0005 {
        return None;
    }

    // i915: Validate CSS header structure
    // The fixed (non-crypto) CSS header size is:
    // header_size_dw - key_size_dw - modulus_size_dw - exponent_size_dw
    // This should equal 32 dwords (128 bytes).
    let fixed_dw = css
        .header_size_dw
        .checked_sub(css.key_size_dw)?
        .checked_sub(css.modulus_size_dw)?
        .checked_sub(css.exponent_size_dw)?;
    if fixed_dw != 32 {
        return None;
    }

    // i915: Calculate ucode and total DMA transfer sizes
    let header_size_bytes = css.header_size_dw.checked_mul(4)? as usize;
    let size_bytes = css.size_dw.checked_mul(4)? as usize;
    let ucode_bytes = size_bytes.checked_sub(header_size_bytes)?;

    // For DMA transfer, we send: CSS header (128) + ucode (all non-header code)
    let dma_bytes = 128usize.checked_add(ucode_bytes)?;

    // RSA signature follows immediately after DMA region in the firmware image
    let rsa_size = css.key_size_dw.checked_mul(4)? as usize;
    let rsa_offset = css_off.checked_add(dma_bytes)?;

    // i915: Bounds checks
    // 1. DMA region must be at least 128 bytes (CSS header)
    // 2. DMA region must not exceed firmware size
    // 3. If RSA present, it must not exceed firmware size
    if dma_bytes < 128 {
        return None;
    }
    if css_off.checked_add(dma_bytes)? > blob.len() {
        return None;
    }
    if rsa_size > 0 {
        let rsa_end = rsa_offset.checked_add(rsa_size)?;
        if rsa_end > blob.len() {
            return None;
        }
    }

    Some(GucCssLayout {
        dma_bytes,
        rsa_offset,
        rsa_size,
    })
}

/// Locate CSS header in firmware blob. Standard i915 search: offset 0 first, then 4-byte aligned.
fn find_guc_css_layout(blob: &[u8]) -> Option<(usize, GucCssLayout)> {
    // Try offset 0 first (common case)
    if let Some(layout) = parse_guc_css_layout_at(blob, 0) {
        return Some((0, layout));
    }

    // Log why offset 0 failed (for debugging CSS layout mismatches)
    let module_type_0 = read_le_u32(blob, 0).unwrap_or(0);
    let header_size_0 = read_le_u32(blob, 4).unwrap_or(0);
    let size_0 = read_le_u32(blob, 16).unwrap_or(0);
    let key_size_0 = read_le_u32(blob, 20).unwrap_or(0);
    let modulus_0 = read_le_u32(blob, 24).unwrap_or(0);
    let exp_0 = read_le_u32(blob, 28).unwrap_or(0);
    let fixed_calc = header_size_0
        .saturating_sub(key_size_0)
        .saturating_sub(modulus_0)
        .saturating_sub(exp_0);
    crate::log_trace!(
        "intel/igpu770: guc-fw css-parse-fail off=0 module_type=0x{:X} header_size_dw=0x{:X} size_dw=0x{:X} key_dw=0x{:X} mod_dw=0x{:X} exp_dw=0x{:X} fixed_calc=0x{:X}\n",
        module_type_0,
        header_size_0,
        size_0,
        key_size_0,
        modulus_0,
        exp_0,
        fixed_calc
    );

    // Search 4-byte aligned offsets
    let end = blob
        .len()
        .saturating_sub(core::mem::size_of::<UcCssHeader>());
    for off in (4..=end).step_by(4) {
        if let Some(layout) = parse_guc_css_layout_at(blob, off) {
            crate::log_trace!(
                "intel/igpu770: guc-fw css-found off=0x{:X} dma_bytes=0x{:X} rsa_off=0x{:X} rsa_size=0x{:X}\n",
                off,
                layout.dma_bytes,
                layout.rsa_offset,
                layout.rsa_size
            );
            return Some((off, layout));
        }
    }

    crate::log_trace!("intel/igpu770: guc-fw css-search-failed no valid header found in blob\n");
    None
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn dma_cache_flush(ptr: *const u8, len: usize) {
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if ptr.is_null() || len == 0 {
            return;
        }

        let line = 64usize;
        let start = (ptr as usize) & !(line - 1);
        let end = (ptr as usize).saturating_add(len);
        let mut cur = start;
        while cur < end {
            _mm_clflush(cur as *const _);
            cur = cur.saturating_add(line);
        }
        _mm_mfence();
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn dma_cache_flush(_ptr: *const u8, _len: usize) {}

#[inline]
fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

#[inline]
fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

#[inline]
fn align_up_u32(value: u32, alignment: u32) -> Option<u32> {
    if alignment == 0 {
        return None;
    }

    let mask = alignment.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

fn align_up_usize(value: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 {
        return None;
    }

    let mask = alignment.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

fn write_u8(buf: &mut [u8], off: usize, value: u8) -> bool {
    if off >= buf.len() {
        return false;
    }
    buf[off] = value;
    true
}

fn write_u32_le(buf: &mut [u8], off: usize, value: u32) -> bool {
    let Some(end) = off.checked_add(4) else {
        return false;
    };
    let Some(dst) = buf.get_mut(off..end) else {
        return false;
    };
    dst.copy_from_slice(&value.to_le_bytes());
    true
}

fn guc_ads_private_data_offset() -> Option<usize> {
    align_up_usize(core::mem::size_of::<GucAdsBlobHeader>(), 4096)
}

pub fn minimal_ads_size() -> usize {
    let private_data_size = GUC_FW_PRIVATE_DATA_SIZE.load(Ordering::Acquire);
    let private_data_size = align_up_usize(private_data_size, 4096).unwrap_or(0);
    guc_ads_private_data_offset()
        .and_then(|off| off.checked_add(private_data_size))
        .unwrap_or(0)
}

fn build_minimal_ads(warm: Igpu770WarmState) -> bool {
    if warm.guc_ads_virt.is_null() || warm.guc_ads_len == 0 || warm.guc_ads_gpu_addr == 0 {
        return false;
    }

    let Some(private_data_offset) = guc_ads_private_data_offset() else {
        return false;
    };
    if private_data_offset > warm.guc_ads_len {
        return false;
    }

    let buf = unsafe { core::slice::from_raw_parts_mut(warm.guc_ads_virt, warm.guc_ads_len) };
    buf.fill(0);

    let policies_off = core::mem::offset_of!(GucAdsBlobHeader, policies);
    let system_info_off = core::mem::offset_of!(GucAdsBlobHeader, system_info);
    let ads_off = core::mem::offset_of!(GucAdsBlobHeader, ads);

    let _ = write_u32_le(
        buf,
        policies_off + core::mem::offset_of!(GucPolicies, dpc_promote_time),
        GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US,
    );
    let _ = write_u32_le(buf, policies_off + core::mem::offset_of!(GucPolicies, is_valid), 1);
    let _ = write_u32_le(
        buf,
        policies_off + core::mem::offset_of!(GucPolicies, max_num_work_items),
        GLOBAL_POLICY_MAX_NUM_WI,
    );

    let mapping_base = system_info_off + core::mem::offset_of!(GucGtSystemInfo, mapping_table);
    for class in 0..GUC_MAX_ENGINE_CLASSES {
        for instance in 0..GUC_MAX_INSTANCES_PER_CLASS {
            let _ = write_u8(
                buf,
                mapping_base + class * GUC_MAX_INSTANCES_PER_CLASS + instance,
                GUC_MAX_INSTANCES_PER_CLASS as u8,
            );
        }
    }

    let doorbells = ((mmio_read32(warm, DIST_DBS_POPULATED) & DOORBELLS_PER_SQIDI_MASK)
        >> DOORBELLS_PER_SQIDI_SHIFT)
        .saturating_add(1);
    let _ = write_u32_le(
        buf,
        system_info_off
            + core::mem::offset_of!(GucGtSystemInfo, generic_gt_sysinfo)
            + 2 * core::mem::size_of::<u32>(),
        doorbells,
    );

    let scheduler_policies = warm.guc_ads_gpu_addr.saturating_add(policies_off as u64) as u32;
    let gt_system_info = warm.guc_ads_gpu_addr.saturating_add(system_info_off as u64) as u32;
    let private_data = warm
        .guc_ads_gpu_addr
        .saturating_add(private_data_offset as u64) as u32;

    let _ = write_u32_le(
        buf,
        ads_off + core::mem::offset_of!(GucAds, scheduler_policies),
        scheduler_policies,
    );
    let _ =
        write_u32_le(buf, ads_off + core::mem::offset_of!(GucAds, gt_system_info), gt_system_info);
    let _ = write_u32_le(buf, ads_off + core::mem::offset_of!(GucAds, private_data), private_data);

    dma_cache_flush(warm.guc_ads_virt as *const u8, warm.guc_ads_len);
    crate::log_trace!(
        "intel/igpu770: guc-fw ads minimal gpu=0x{:X} phys=0x{:X} len=0x{:X} policies=0x{:X} sysinfo=0x{:X} private=0x{:X} doorbells={}\n",
        warm.guc_ads_gpu_addr,
        warm.guc_ads_phys,
        warm.guc_ads_len,
        scheduler_policies,
        gt_system_info,
        private_data,
        doorbells
    );
    true
}

fn guc_ads_param(gpu_addr: u64) -> u32 {
    ((gpu_addr >> 12) as u32) << GUC_ADS_ADDR_SHIFT
}

fn guc_write_init_params(warm: Igpu770WarmState) -> [u32; GUC_CTL_MAX_DWORDS] {
    let mut params = [0u32; GUC_CTL_MAX_DWORDS];
    let devid = ((warm.device_id as u32) << 16) | (warm.revision_id as u32);
    params[GUC_CTL_LOG_PARAMS] = 0;
    params[GUC_CTL_WA] = 0;
    params[GUC_CTL_FEATURE] = 0;
    params[GUC_CTL_DEBUG] = GUC_LOG_DISABLED;
    params[GUC_CTL_ADS] = guc_ads_param(warm.guc_ads_gpu_addr);
    params[GUC_CTL_DEVID] = devid;

    let _ = mmio_write32(warm, SOFT_SCRATCH_BASE, 0);
    for (index, value) in params.iter().enumerate() {
        let _ = mmio_write32(warm, SOFT_SCRATCH_BASE + (index + 1) * 4, *value);
    }

    crate::log_trace!(
        "intel/igpu770: guc-fw params log=0x{:08X} wa=0x{:08X} feature=0x{:08X} debug=0x{:08X} ads=0x{:08X} devid=0x{:08X} scratch1=0x{:08X} scratch5=0x{:08X} scratch6=0x{:08X}\n",
        params[GUC_CTL_LOG_PARAMS],
        params[GUC_CTL_WA],
        params[GUC_CTL_FEATURE],
        params[GUC_CTL_DEBUG],
        params[GUC_CTL_ADS],
        devid,
        mmio_read32(warm, SOFT_SCRATCH_BASE + 4),
        mmio_read32(warm, SOFT_SCRATCH_BASE + (1 + GUC_CTL_ADS) * 4),
        mmio_read32(warm, SOFT_SCRATCH_BASE + (1 + GUC_CTL_DEVID) * 4),
    );

    params
}

fn compute_gen11_guc_wopcm_layout(guc_fw_size: u32, huc_fw_size: u32) -> Option<(u32, u32)> {
    if guc_fw_size == 0 || guc_fw_size >= GEN11_WOPCM_SIZE {
        return None;
    }

    let usable_limit = GEN11_WOPCM_SIZE.checked_sub(WOPCM_HW_CTX_RESERVED_SIZE)?;
    let min_guc_space = guc_fw_size
        .checked_add(GUC_WOPCM_RESERVED_SIZE)?
        .checked_add(GUC_WOPCM_STACK_RESERVED_SIZE)?;
    let huc_floor = huc_fw_size.checked_add(WOPCM_RESERVED_SIZE)?;
    let guc_base = align_up_u32(huc_floor, GUC_WOPCM_OFFSET_ALIGNMENT)?;
    if guc_base >= usable_limit {
        return None;
    }

    let guc_size = (usable_limit - guc_base) & GUC_WOPCM_SIZE_MASK;
    if guc_size < min_guc_space {
        return None;
    }

    Some((guc_base, guc_size))
}

#[inline]
fn guc_bootrom(status: u32) -> u32 {
    (status & GS_BOOTROM_MASK) >> GS_BOOTROM_SHIFT
}

#[inline]
fn guc_ukernel(status: u32) -> u32 {
    (status & GS_UKERNEL_MASK) >> GS_UKERNEL_SHIFT
}

#[inline]
fn guc_auth(status: u32) -> u32 {
    (status & GS_AUTH_STATUS_MASK) >> GS_AUTH_STATUS_SHIFT
}

#[inline]
fn error_name_ukernel(code: u32) -> &'static str {
    match code {
        INTEL_GUC_LOAD_STATUS_START => "START",
        INTEL_GUC_LOAD_STATUS_HWCONFIG_START => "HWCONFIG_START",
        INTEL_GUC_LOAD_STATUS_HWCONFIG_DONE => "HWCONFIG_DONE",
        INTEL_GUC_LOAD_STATUS_GDT_DONE => "GDT_DONE",
        INTEL_GUC_LOAD_STATUS_IDT_DONE => "IDT_DONE",
        INTEL_GUC_LOAD_STATUS_LAPIC_DONE => "LAPIC_DONE",
        INTEL_GUC_LOAD_STATUS_GUCINT_DONE => "GUCINT_DONE",
        INTEL_GUC_LOAD_STATUS_DPC_READY => "DPC_READY",
        INTEL_GUC_LOAD_STATUS_READY => "READY",
        INTEL_GUC_LOAD_STATUS_ERROR_DEVID_BUILD_MISMATCH => "DEVID_MISMATCH",
        INTEL_GUC_LOAD_STATUS_GUC_PREPROD_BUILD_MISMATCH => "PREPROD_MISMATCH",
        INTEL_GUC_LOAD_STATUS_ERROR_DEVID_INVALID_GUCTYPE => "INVALID_GUCTYPE",
        INTEL_GUC_LOAD_STATUS_HWCONFIG_ERROR => "HWCONFIG_ERROR",
        INTEL_GUC_LOAD_STATUS_BOOTROM_VERSION_MISMATCH => "BOOTROM_VERSION_MISMATCH",
        INTEL_GUC_LOAD_STATUS_DPC_ERROR => "DPC_ERROR",
        INTEL_GUC_LOAD_STATUS_EXCEPTION => "EXCEPTION",
        INTEL_GUC_LOAD_STATUS_INIT_DATA_INVALID => "INIT_DATA_INVALID",
        INTEL_GUC_LOAD_STATUS_PXP_TEARDOWN_CTRL_ENABLED => "PXP_TEARDOWN_CTRL_ENABLED",
        INTEL_GUC_LOAD_STATUS_MPU_DATA_INVALID => "MPU_DATA_INVALID",
        INTEL_GUC_LOAD_STATUS_INIT_MMIO_SAVE_RESTORE_INVALID => "MMIO_SR_INVALID",
        INTEL_GUC_LOAD_STATUS_KLV_WORKAROUND_INIT_ERROR => "KLV_INIT_ERROR",
        _ => "UNKNOWN",
    }
}

#[inline]
fn error_name_bootrom(code: u32) -> &'static str {
    match code {
        INTEL_BOOTROM_STATUS_NO_KEY_FOUND => "NO_KEY",
        INTEL_BOOTROM_STATUS_AES_PROD_KEY_FOUND => "AES_PROD_KEY_FOUND",
        INTEL_BOOTROM_STATUS_RSA_FAILED => "RSA_FAILED",
        INTEL_BOOTROM_STATUS_PAVPC_FAILED => "PAVPC_FAILED",
        INTEL_BOOTROM_STATUS_WOPCM_FAILED => "WOPCM_FAILED",
        INTEL_BOOTROM_STATUS_LOADLOC_FAILED => "LOADLOC_FAILED",
        INTEL_BOOTROM_STATUS_JUMP_PASSED => "JUMP_PASSED",
        INTEL_BOOTROM_STATUS_JUMP_FAILED => "JUMP_FAILED",
        INTEL_BOOTROM_STATUS_RC6CTXCONFIG_FAILED => "RC6CTXCONFIG_FAILED",
        INTEL_BOOTROM_STATUS_MPUMAP_INCORRECT => "MPUMAP_INCORRECT",
        INTEL_BOOTROM_STATUS_EXCEPTION => "EXCEPTION",
        INTEL_BOOTROM_STATUS_PROD_KEY_CHECK_FAILURE => "PROD_KEY_CHECK",
        _ => "OK_OR_UNKNOWN",
    }
}

fn guc_status_terminal(status: u32) -> Option<bool> {
    let uk = guc_ukernel(status);
    match uk {
        INTEL_GUC_LOAD_STATUS_READY => return Some(true),
        INTEL_GUC_LOAD_STATUS_ERROR_DEVID_BUILD_MISMATCH
        | INTEL_GUC_LOAD_STATUS_GUC_PREPROD_BUILD_MISMATCH
        | INTEL_GUC_LOAD_STATUS_ERROR_DEVID_INVALID_GUCTYPE
        | INTEL_GUC_LOAD_STATUS_HWCONFIG_ERROR
        | INTEL_GUC_LOAD_STATUS_BOOTROM_VERSION_MISMATCH
        | INTEL_GUC_LOAD_STATUS_DPC_ERROR
        | INTEL_GUC_LOAD_STATUS_EXCEPTION
        | INTEL_GUC_LOAD_STATUS_INIT_DATA_INVALID
        | INTEL_GUC_LOAD_STATUS_MPU_DATA_INVALID
        | INTEL_GUC_LOAD_STATUS_INIT_MMIO_SAVE_RESTORE_INVALID
        | INTEL_GUC_LOAD_STATUS_KLV_WORKAROUND_INIT_ERROR => return Some(false),
        _ => {}
    }

    match guc_bootrom(status) {
        INTEL_BOOTROM_STATUS_NO_KEY_FOUND
        | INTEL_BOOTROM_STATUS_RSA_FAILED
        | INTEL_BOOTROM_STATUS_PAVPC_FAILED
        | INTEL_BOOTROM_STATUS_WOPCM_FAILED
        | INTEL_BOOTROM_STATUS_LOADLOC_FAILED
        | INTEL_BOOTROM_STATUS_JUMP_FAILED
        | INTEL_BOOTROM_STATUS_RC6CTXCONFIG_FAILED
        | INTEL_BOOTROM_STATUS_MPUMAP_INCORRECT
        | INTEL_BOOTROM_STATUS_EXCEPTION
        | INTEL_BOOTROM_STATUS_PROD_KEY_CHECK_FAILURE => Some(false),
        _ => None,
    }
}

pub fn load_firmware_from_module(warm_align: usize) -> GucFirmwareInfo {
    let Some(blob) = crate::limine::module_bytes_by_string(GUC_MODULE_STRING) else {
        crate::log_trace!("intel/igpu770: guc-fw module not found string=trueos.fw.guc\n");
        return GucFirmwareInfo::empty();
    };

    if blob.is_empty() {
        crate::log_trace!("intel/igpu770: guc-fw module present but empty\n");
        return GucFirmwareInfo::empty();
    }

    let blob_magic = read_le_u32(blob, 0).unwrap_or(0);

    // Log first 4 dwords (16 bytes) in both hex bytes and dword LE format
    let d0 = read_le_u32(blob, 0).unwrap_or(0);
    let d1 = read_le_u32(blob, 4).unwrap_or(0);
    let d2 = read_le_u32(blob, 8).unwrap_or(0);
    let d3 = read_le_u32(blob, 12).unwrap_or(0);
    let b0 = blob.get(0).copied().unwrap_or(0);
    let b1 = blob.get(1).copied().unwrap_or(0);
    let b2 = blob.get(2).copied().unwrap_or(0);
    let b3 = blob.get(3).copied().unwrap_or(0);
    let b4 = blob.get(4).copied().unwrap_or(0);
    let b5 = blob.get(5).copied().unwrap_or(0);
    let b6 = blob.get(6).copied().unwrap_or(0);
    let b7 = blob.get(7).copied().unwrap_or(0);
    let b8 = blob.get(8).copied().unwrap_or(0);
    let b9 = blob.get(9).copied().unwrap_or(0);
    let b10 = blob.get(10).copied().unwrap_or(0);
    let b11 = blob.get(11).copied().unwrap_or(0);
    let b12 = blob.get(12).copied().unwrap_or(0);
    let b13 = blob.get(13).copied().unwrap_or(0);
    let b14 = blob.get(14).copied().unwrap_or(0);
    let b15 = blob.get(15).copied().unwrap_or(0);
    crate::log_trace!(
        "intel/igpu770: guc-fw hdr-dwords d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X}\n",
        d0,
        d1,
        d2,
        d3
    );
    crate::log_trace!(
        "intel/igpu770: guc-fw hdr-bytes[0..16]={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}\n",
        b0,
        b1,
        b2,
        b3,
        b4,
        b5,
        b6,
        b7,
        b8,
        b9,
        b10,
        b11,
        b12,
        b13,
        b14,
        b15
    );
    // Check if firmware is zstd-compressed (should have been decompressed, but guard against it)
    if blob_magic == ZSTD_MAGIC {
        crate::log_trace!(
            "intel/igpu770: guc-fw ERROR module is zstd-compressed magic=0x{:08X}; decompression required before load\n",
            blob_magic
        );
        return GucFirmwareInfo::empty();
    }

    let alloc_len = blob.len().div_ceil(warm_align) * warm_align;
    let Some((phys, virt)) = crate::dma::alloc(alloc_len, warm_align) else {
        crate::log_trace!(
            "intel/igpu770: guc-fw module present but alloc failed size=0x{:X}\n",
            alloc_len
        );
        return GucFirmwareInfo::empty();
    };

    unsafe {
        core::ptr::write_bytes(virt, 0, alloc_len);
        core::ptr::copy_nonoverlapping(blob.as_ptr(), virt, blob.len());
    }
    dma_cache_flush(virt as *const u8, alloc_len);

    let (css_off, xfer_len, rsa_off, rsa_size) =
        if let Some((off, layout)) = find_guc_css_layout(blob) {
            (off, layout.dma_bytes, layout.rsa_offset, layout.rsa_size)
        } else {
            (0usize, blob.len(), 0usize, 0usize)
        };
    let private_data_size = if css_off
        .checked_add(core::mem::size_of::<UcCssHeader>())
        .is_some_and(|end| end <= blob.len())
    {
        let css_ptr = unsafe { blob.as_ptr().add(css_off) as *const UcCssHeader };
        let css = unsafe { css_ptr.read_unaligned() };
        css.private_data_size as usize
    } else {
        0
    };
    GUC_FW_BLOB_LEN.store(blob.len(), Ordering::Release);
    GUC_FW_DMA_OFFSET.store(css_off, Ordering::Release);
    GUC_FW_RSA_OFFSET.store(rsa_off, Ordering::Release);
    GUC_FW_RSA_SIZE.store(rsa_size, Ordering::Release);
    GUC_FW_PRIVATE_DATA_SIZE.store(private_data_size, Ordering::Release);
    crate::log_trace!(
        "intel/igpu770: guc-fw module found size=0x{:X} alloc=0x{:X} phys=0x{:X} gpu=0x{:X} css_off=0x{:X} xfer=0x{:X} rsa_off=0x{:X} rsa_size=0x{:X} priv_data=0x{:X} zstd_magic={}\n",
        blob.len(),
        alloc_len,
        phys,
        GPU_VA_GUC_FW_BASE,
        css_off,
        xfer_len,
        rsa_off,
        rsa_size,
        private_data_size,
        (blob_magic == ZSTD_MAGIC) as u8
    );

    GucFirmwareInfo {
        phys,
        virt,
        len: alloc_len,
        xfer_len,
        gpu_addr: GPU_VA_GUC_FW_BASE,
    }
}

pub fn ready() -> bool {
    GUC_FW_READY.load(Ordering::Acquire)
}

pub fn status(warm: Igpu770WarmState) -> u32 {
    mmio_read32(warm, GUC_STATUS)
}

pub fn bootstrap_once(warm: Igpu770WarmState) {
    if GUC_FW_BOOTSTRAP_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    if warm.guc_fw_len == 0 || warm.guc_fw_phys == 0 || warm.guc_fw_gpu_addr == 0 {
        crate::log_trace!("intel/igpu770: guc-fw skipped reason=no-module\n");
        GUC_FW_READY.store(false, Ordering::Release);
        return;
    }
    if warm.guc_ads_len == 0 || warm.guc_ads_phys == 0 || warm.guc_ads_gpu_addr == 0 {
        crate::log_trace!("intel/igpu770: guc-fw skipped reason=no-ads\n");
        GUC_FW_READY.store(false, Ordering::Release);
        return;
    }

    let _ = forcewake_all_acquire(warm);

    // Reset GuC domain and wait for reset completion before programming DMA.
    let _ = mmio_write32(warm, GDRST, GRDOM_GUC);
    let mut gdrst_rb = mmio_read32(warm, GDRST);
    let mut gdrst_iters = 0usize;
    while gdrst_iters < GUC_RESET_POLL_ITERS {
        if (gdrst_rb & GRDOM_GUC) == 0 {
            break;
        }
        core::hint::spin_loop();
        gdrst_iters += 1;
        gdrst_rb = mmio_read32(warm, GDRST);
    }
    let status_after_reset = mmio_read32(warm, GUC_STATUS);
    crate::log_trace!(
        "intel/igpu770: guc-fw reset gdrst_rb=0x{:08X} gdrst_iters={} mia_in_reset={}\n",
        gdrst_rb,
        gdrst_iters,
        ((status_after_reset & GS_MIA_IN_RESET) != 0) as u8
    );

    let status_before = mmio_read32(warm, GUC_STATUS);
    let shim2_before = mmio_read32(warm, GUC_SHIM_CONTROL2);
    let pm_before = mmio_read32(warm, GT_PM_CONFIG);
    let pmintrmsk_before = mmio_read32(warm, PMINTRMSK);
    let wopcm_size = mmio_read32(warm, GUC_WOPCM_SIZE);
    let wopcm_base = mmio_read32(warm, DMA_GUC_WOPCM_OFFSET);
    let mut shim_after = GUC_ENABLE_READ_CACHE_LOGIC
        | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA
        | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA
        | GUC_ENABLE_MIA_CLOCK_GATING;
    if GUC_USE_LEGACY_SHIM_BITS {
        shim_after |= GUC_DISABLE_SRAM_INIT_TO_ZEROES | GUC_ENABLE_MIA_CACHING;
    }
    let _ = mmio_write32(warm, GUC_SHIM_CONTROL, shim_after);
    let _ = mmio_write32(warm, GUC_SHIM_CONTROL2, shim2_before | GUC_ENABLE_DEBUG_REG);
    let _ = mmio_write32(warm, GT_PM_CONFIG, pm_before | GT_DOORBELL_ENABLE);
    let _ = mmio_write32(warm, PMINTRMSK, pmintrmsk_before & !ARAT_EXPIRED_INTRMSK);
    let ads_ready = build_minimal_ads(warm);
    let params = guc_write_init_params(warm);

    let fw_blob_len = GUC_FW_BLOB_LEN.load(Ordering::Acquire);
    let fw_dma_off = GUC_FW_DMA_OFFSET.load(Ordering::Acquire);
    let fw_rsa_off = GUC_FW_RSA_OFFSET.load(Ordering::Acquire);
    let fw_rsa_size = GUC_FW_RSA_SIZE.load(Ordering::Acquire);

    // Parse and log CSS header struct fields for deeper debugging
    let blob =
        unsafe { core::slice::from_raw_parts(warm.guc_fw_virt as *const u8, warm.guc_fw_len) };
    if fw_dma_off
        .checked_add(core::mem::size_of::<UcCssHeader>())
        .map_or(false, |e| e <= warm.guc_fw_len)
    {
        let css_ptr = unsafe { blob.as_ptr().add(fw_dma_off) as *const UcCssHeader };
        let css = unsafe { css_ptr.read_unaligned() };
        // Extract fields from the read struct (not by reference to avoid alignment issues)
        let module_type = css.module_type;
        let header_size_dw = css.header_size_dw;
        let size_dw = css.size_dw;
        let key_size_dw = css.key_size_dw;
        let modulus_size_dw = css.modulus_size_dw;
        let exponent_size_dw = css.exponent_size_dw;
        let sw_version = css.sw_version;
        let vf_version = css.vf_version;
        let private_data_size = css.private_data_size;
        crate::log_trace!(
            "intel/igpu770: guc-fw css-parsed rev=r10 module_type=0x{:X} header_size_dw=0x{:X} size_dw=0x{:X} key_size_dw=0x{:X} modulus_size_dw=0x{:X} exponent_size_dw=0x{:X} sw_version=0x{:X} vf_version=0x{:X} priv_data_sz=0x{:X}\n",
            module_type,
            header_size_dw,
            size_dw,
            key_size_dw,
            modulus_size_dw,
            exponent_size_dw,
            sw_version,
            vf_version,
            private_data_size
        );
    }

    let src = warm.guc_fw_gpu_addr.saturating_add(fw_dma_off as u64);
    if fw_rsa_size != 0 && fw_blob_len != 0 {
        let rsa_end = fw_rsa_off.saturating_add(fw_rsa_size);
        if rsa_end <= fw_blob_len {
            if fw_rsa_size > GUC_RSA_IN_MEMORY_THRESHOLD_BYTES {
                // Match Xe/i915: for large RSA payloads, scratch0 carries GGTT offset.
                let rsa_gpu_addr = warm.guc_fw_gpu_addr.saturating_add(fw_rsa_off as u64);
                let _ = mmio_write32(warm, UOS_RSA_SCRATCH_BASE, rsa_gpu_addr as u32);
            } else {
                // Small RSA payloads are mirrored directly into scratch registers.
                let rsa_dwords = core::cmp::min(fw_rsa_size / 4, UOS_RSA_SCRATCH_COUNT);
                for i in 0..rsa_dwords {
                    let off = fw_rsa_off + i * 4;
                    let v = u32::from_le_bytes([
                        blob[off],
                        blob[off + 1],
                        blob[off + 2],
                        blob[off + 3],
                    ]);
                    let _ = mmio_write32(warm, UOS_RSA_SCRATCH_BASE + i * 4, v);
                }
            }
        }
    }
    let max_dma = warm.guc_fw_len.saturating_sub(fw_dma_off);
    let copy_size = warm.guc_fw_xfer_len.min(max_dma).min(u32::MAX as usize) as u32;
    let computed_wopcm = compute_gen11_guc_wopcm_layout(copy_size, 0);
    // Match i915 behavior: respect pre-locked WOPCM regs (common on depriv platforms).
    // Only program a fallback partition if firmware/BIOS has not already locked a valid one.
    let wopcm_locked =
        (wopcm_size & GUC_WOPCM_SIZE_LOCKED) != 0 && (wopcm_base & GUC_WOPCM_OFFSET_VALID) != 0;
    let wopcm_size_cfg = if wopcm_locked {
        wopcm_size
    } else if let Some((_, size)) = computed_wopcm {
        size | GUC_WOPCM_SIZE_LOCKED
    } else {
        (GEN11_WOPCM_SIZE & GUC_WOPCM_SIZE_MASK) | GUC_WOPCM_SIZE_LOCKED
    };
    let wopcm_base_cfg = if wopcm_locked {
        wopcm_base
    } else if let Some((base, _)) = computed_wopcm {
        base | GUC_WOPCM_OFFSET_VALID
    } else {
        WOPCM_RESERVED_SIZE | GUC_WOPCM_OFFSET_VALID
    };
    if !wopcm_locked {
        let _ = mmio_write32(warm, GUC_WOPCM_SIZE, wopcm_size_cfg);
        let _ = mmio_write32(warm, DMA_GUC_WOPCM_OFFSET, wopcm_base_cfg);
    }
    let _ = mmio_write32(warm, DMA_ADDR_0_LOW, src as u32);
    let _ = mmio_write32(warm, DMA_ADDR_0_HIGH, ((src >> 32) as u32) | DMA_ADDRESS_SPACE_GGTT);
    let _ = mmio_write32(warm, DMA_ADDR_1_LOW, GUC_BOOT_DEST_WOPCM_OFFSET);
    let _ = mmio_write32(warm, DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM);
    let _ = mmio_write32(warm, DMA_COPY_SIZE, copy_size);
    let _ = mmio_write32(warm, DMA_CTRL, masked_bit_enable(UOS_MOVE | START_DMA));

    let mut dma_done = false;
    let mut dma_iters = 0usize;
    while dma_iters < GUC_DMA_POLL_ITERS {
        if (mmio_read32(warm, DMA_CTRL) & START_DMA) == 0 {
            dma_done = true;
            break;
        }
        core::hint::spin_loop();
        dma_iters += 1;
    }
    let _ = mmio_write32(warm, DMA_CTRL, masked_bit_disable(UOS_MOVE));

    let status_after = mmio_read32(warm, GUC_STATUS);
    let mut status_final = status_after;
    let mut status_iters = 0usize;
    let mut terminal = guc_status_terminal(status_after);
    let mut terminal_success = terminal.unwrap_or(false);
    while status_iters < GUC_READY_POLL_ITERS {
        let s = mmio_read32(warm, GUC_STATUS);
        status_final = s;
        terminal = guc_status_terminal(s);
        if let Some(ok) = terminal {
            terminal_success = ok;
            break;
        }
        core::hint::spin_loop();
        status_iters += 1;
    }

    let ready = dma_done && terminal_success && guc_auth(status_final) != GS_AUTH_STATUS_BAD;
    GUC_FW_READY.store(ready, Ordering::Release);
    let shim_rb = mmio_read32(warm, GUC_SHIM_CONTROL);
    let shim2_rb = mmio_read32(warm, GUC_SHIM_CONTROL2);
    let pm_rb = mmio_read32(warm, GT_PM_CONFIG);
    let pmintrmsk_rb = mmio_read32(warm, PMINTRMSK);
    let dma_ctrl_rb = mmio_read32(warm, DMA_CTRL);
    let wopcm_size_rb = mmio_read32(warm, GUC_WOPCM_SIZE);
    let dma_wopcm_off_rb = mmio_read32(warm, DMA_GUC_WOPCM_OFFSET);
    let dma_addr0_low_rb = mmio_read32(warm, DMA_ADDR_0_LOW);
    let dma_addr0_high_rb = mmio_read32(warm, DMA_ADDR_0_HIGH);
    let dma_addr1_low_rb = mmio_read32(warm, DMA_ADDR_1_LOW);
    let dma_addr1_high_rb = mmio_read32(warm, DMA_ADDR_1_HIGH);
    let computed_wopcm_base = computed_wopcm.map(|(base, _)| base).unwrap_or(0);
    let computed_wopcm_size = computed_wopcm.map(|(_, size)| size).unwrap_or(0);

    crate::log_trace!(
        "intel/igpu770: guc-fw bootstrap rev={} src_gpu=0x{:X} fw_phys=0x{:X} fw_len=0x{:X} xfer=0x{:X} ads_gpu=0x{:X} ads_len=0x{:X} ads_ready={} ads_param=0x{:08X} wopcm_size=0x{:08X} wopcm_size_rb=0x{:08X} wopcm_cfg=0x{:08X} wopcm_locked={} wopcm_calc_base=0x{:08X} wopcm_calc_size=0x{:08X} status_before=0x{:08X} status_after=0x{:08X} status_final=0x{:08X} status_iters={} shim=0x{:08X} shim2=0x{:08X} pm=0x{:08X} pmintrmsk=0x{:08X} dma_done={} dma_iters={} dma_ctrl=0x{:08X} a0_lo=0x{:08X} a0_hi=0x{:08X} a1_lo=0x{:08X} a1_hi=0x{:08X} ready={}\n",
        GUC_BOOTSTRAP_REV,
        warm.guc_fw_gpu_addr,
        warm.guc_fw_phys,
        warm.guc_fw_len,
        copy_size,
        warm.guc_ads_gpu_addr,
        warm.guc_ads_len,
        ads_ready as u8,
        params[GUC_CTL_ADS],
        wopcm_size,
        wopcm_size_rb,
        dma_wopcm_off_rb,
        wopcm_locked as u8,
        computed_wopcm_base,
        computed_wopcm_size,
        status_before,
        status_after,
        status_final,
        status_iters,
        shim_rb,
        shim2_rb,
        pm_rb,
        pmintrmsk_rb,
        dma_done as u8,
        dma_iters,
        dma_ctrl_rb,
        dma_addr0_low_rb,
        dma_addr0_high_rb,
        dma_addr1_low_rb,
        dma_addr1_high_rb,
        ready as u8
    );
    let uk_code = guc_ukernel(status_final);
    let br_code = guc_bootrom(status_final);
    crate::log_trace!(
        "intel/igpu770: guc-fw status-decode bootrom=0x{:02X}({}) ukernel=0x{:02X}({}) auth=0x{:X} dma_off=0x{:X} rsa_off=0x{:X} rsa_size=0x{:X}\n",
        br_code,
        error_name_bootrom(br_code),
        uk_code,
        error_name_ukernel(uk_code),
        guc_auth(status_final),
        fw_dma_off,
        fw_rsa_off,
        fw_rsa_size
    );
    if !ready {
        crate::log_trace!("intel/igpu770: guc-fw readiness=not-ready; rcs smoke submit gated\n");
    }
}
