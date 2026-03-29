use core::sync::atomic::{AtomicBool, Ordering};

use super::intel_igpu770::{forcewake_gt_acquire, mmio_read32, mmio_write32, Igpu770WarmState};

const GUC_MODULE_STRING: &[u8] = b"trueos.fw.guc";
const ZSTD_MAGIC: u32 = 0xFD2F_B528;
const GPU_VA_GUC_FW_BASE: u64 = 0x0084_0000;

const GUC_STATUS: usize = 0x0000_C000;
const GDRST: usize = 0x0000_941C;
const GUC_SHIM_CONTROL: usize = 0x0000_C064;
const GT_PM_CONFIG: usize = 0x0013_816C;
const DMA_ADDR_0_LOW: usize = 0x0000_C300;
const DMA_ADDR_0_HIGH: usize = 0x0000_C304;
const DMA_ADDR_1_LOW: usize = 0x0000_C308;
const DMA_ADDR_1_HIGH: usize = 0x0000_C30C;
const DMA_COPY_SIZE: usize = 0x0000_C310;
const DMA_CTRL: usize = 0x0000_C314;
const DMA_GUC_WOPCM_OFFSET: usize = 0x0000_C340;
const GUC_SEND_INTERRUPT: usize = 0x0000_C4C8;
const GUC_WOPCM_SIZE: usize = 0x0000_C050;
const UOS_RSA_SCRATCH_BASE: usize = 0x0000_C200;
const UOS_RSA_SCRATCH_COUNT: usize = 64;
const GUC_BOOTSTRAP_REV: &str = "guc-bootstrap-r7";

const GT_DOORBELL_ENABLE: u32 = 1 << 0;
const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;
const GUC_ENABLE_MIA_CACHING: u32 = 1 << 2;
const GUC_MSGCH_ENABLE: u32 = 1 << 4;
const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
const UOS_MOVE: u32 = 1 << 4;
const START_DMA: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_SHIFT: u32 = 14;
const GUC_WOPCM_OFFSET_MASK: u32 = 0x3FFFF << GUC_WOPCM_OFFSET_SHIFT;
const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;
const GUC_WOPCM_SIZE_MASK: u32 = 0xFFFFF << 12;
const GUC_BOOT_WOPCM_BASE: u32 = 0x4000;
const GUC_BOOT_DEST_WOPCM_OFFSET: u32 = 0x2000;
const GUC_DMA_POLL_ITERS: usize = 20_000;
const GUC_READY_POLL_ITERS: usize = 200_000;
const GUC_RESET_POLL_ITERS: usize = 100_000;
const GUC_STATUS_BOOT_DEFAULT: u32 = 0x0000_0001;
const GS_BOOTROM_SHIFT: u32 = 1;
const GS_BOOTROM_MASK: u32 = 0x7F << GS_BOOTROM_SHIFT;
const GS_UKERNEL_SHIFT: u32 = 8;
const GS_UKERNEL_MASK: u32 = 0xFF << GS_UKERNEL_SHIFT;
const GS_AUTH_STATUS_SHIFT: u32 = 30;
const GS_AUTH_STATUS_MASK: u32 = 0x03 << GS_AUTH_STATUS_SHIFT;
const GS_AUTH_STATUS_BAD: u32 = 0x01;
const GS_MIA_IN_RESET: u32 = 1 << 0;
const GRDOM_GUC: u32 = 1 << 3;

const INTEL_GUC_LOAD_STATUS_ERROR_DEVID_BUILD_MISMATCH: u32 = 0x02;
const INTEL_GUC_LOAD_STATUS_GUC_PREPROD_BUILD_MISMATCH: u32 = 0x03;
const INTEL_GUC_LOAD_STATUS_ERROR_DEVID_INVALID_GUCTYPE: u32 = 0x04;
const INTEL_GUC_LOAD_STATUS_HWCONFIG_ERROR: u32 = 0x07;
const INTEL_GUC_LOAD_STATUS_BOOTROM_VERSION_MISMATCH: u32 = 0x08;
const INTEL_GUC_LOAD_STATUS_DPC_ERROR: u32 = 0x60;
const INTEL_GUC_LOAD_STATUS_EXCEPTION: u32 = 0x70;
const INTEL_GUC_LOAD_STATUS_INIT_DATA_INVALID: u32 = 0x71;
const INTEL_GUC_LOAD_STATUS_MPU_DATA_INVALID: u32 = 0x73;
const INTEL_GUC_LOAD_STATUS_INIT_MMIO_SAVE_RESTORE_INVALID: u32 = 0x74;
const INTEL_GUC_LOAD_STATUS_KLV_WORKAROUND_INIT_ERROR: u32 = 0x75;
const INTEL_GUC_LOAD_STATUS_READY: u32 = 0xF0;

const INTEL_BOOTROM_STATUS_NO_KEY_FOUND: u32 = 0x13;
const INTEL_BOOTROM_STATUS_RSA_FAILED: u32 = 0x50;
const INTEL_BOOTROM_STATUS_PAVPC_FAILED: u32 = 0x73;
const INTEL_BOOTROM_STATUS_WOPCM_FAILED: u32 = 0x74;
const INTEL_BOOTROM_STATUS_LOADLOC_FAILED: u32 = 0x75;
const INTEL_BOOTROM_STATUS_JUMP_FAILED: u32 = 0x76;
const INTEL_BOOTROM_STATUS_RC6CTXCONFIG_FAILED: u32 = 0x77;
const INTEL_BOOTROM_STATUS_MPUMAP_INCORRECT: u32 = 0x79;
const INTEL_BOOTROM_STATUS_EXCEPTION: u32 = 0x7A;
const INTEL_BOOTROM_STATUS_PROD_KEY_CHECK_FAILURE: u32 = 0x2B;

static GUC_FW_BOOTSTRAP_RAN: AtomicBool = AtomicBool::new(false);
static GUC_FW_READY: AtomicBool = AtomicBool::new(false);
static GUC_FW_BLOB_LEN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_DMA_OFFSET: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_RSA_OFFSET: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static GUC_FW_RSA_SIZE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[derive(Copy, Clone, Debug)]
pub struct GucFirmwareInfo {
    pub phys: u64,
    pub virt: *mut u8,
    pub len: usize,
    pub xfer_len: usize,
    pub gpu_addr: u64,
}

#[derive(Copy, Clone, Debug)]
struct GucCssLayout {
    dma_bytes: usize,
    rsa_offset: usize,
    rsa_size: usize,
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

#[inline]
fn read_le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = bytes.get(off..end)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn parse_guc_css_transfer_bytes(blob: &[u8]) -> Option<usize> {
    find_guc_css_layout(blob).map(|(_, l)| l.dma_bytes)
}

fn parse_guc_css_layout_at(blob: &[u8], css_off: usize) -> Option<GucCssLayout> {
    let min_len = css_off.checked_add(128)?;
    if blob.len() < min_len {
        return None;
    }

    let module_type = read_le_u32(blob, css_off)?;
    // Accept 0x6 (GuC) and 0x5 (signed module variant seen on some platforms)
    if module_type != 0x0000_0006 && module_type != 0x0000_0005 {
        return None;
    }

    let header_size_dw = read_le_u32(blob, css_off + 4)?;
    let size_dw = read_le_u32(blob, css_off + 24)?;
    let key_size_dw = read_le_u32(blob, css_off + 28)?;
    let modulus_size_dw = read_le_u32(blob, css_off + 32)?;
    let exponent_size_dw = read_le_u32(blob, css_off + 36)?;

    // fixed_dw is the non-crypto portion of the CSS header (must be >= 32 dwords = 128 bytes)
    let fixed_dw = header_size_dw
        .checked_sub(key_size_dw)?
        .checked_sub(modulus_size_dw)?
        .checked_sub(exponent_size_dw)?;
    // Relaxed: require >= 32 dwords (128 bytes) and dword-aligned; 128 exactly is legacy check
    if fixed_dw < 32 {
        return None;
    }
    let css_header_bytes = (fixed_dw as usize) * 4;

    let header_size_bytes = header_size_dw.checked_mul(4)? as usize;
    let size_bytes = size_dw.checked_mul(4)? as usize;
    let ucode_bytes = size_bytes.checked_sub(header_size_bytes)?;
    let dma_bytes = css_header_bytes.checked_add(ucode_bytes)?;
    let rsa_size = key_size_dw.checked_mul(4)? as usize;
    let rsa_offset = css_off.checked_add(dma_bytes)?;

    if dma_bytes < 128 || css_off.checked_add(dma_bytes)? > blob.len() {
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

fn find_guc_css_layout(blob: &[u8]) -> Option<(usize, GucCssLayout)> {
    if let Some(layout) = parse_guc_css_layout_at(blob, 0) {
        return Some((0, layout));
    }

    let end = blob.len().saturating_sub(128);
    for off in (4..=end).step_by(4) {
        if let Some(layout) = parse_guc_css_layout_at(blob, off) {
            return Some((off, layout));
        }
    }
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
        INTEL_GUC_LOAD_STATUS_READY => "READY",
        INTEL_GUC_LOAD_STATUS_ERROR_DEVID_BUILD_MISMATCH => "DEVID_MISMATCH",
        INTEL_GUC_LOAD_STATUS_GUC_PREPROD_BUILD_MISMATCH => "PREPROD_MISMATCH",
        INTEL_GUC_LOAD_STATUS_ERROR_DEVID_INVALID_GUCTYPE => "INVALID_GUCTYPE",
        INTEL_GUC_LOAD_STATUS_HWCONFIG_ERROR => "HWCONFIG_ERROR",
        INTEL_GUC_LOAD_STATUS_BOOTROM_VERSION_MISMATCH => "BOOTROM_VERSION_MISMATCH",
        INTEL_GUC_LOAD_STATUS_DPC_ERROR => "DPC_ERROR",
        INTEL_GUC_LOAD_STATUS_EXCEPTION => "EXCEPTION",
        INTEL_GUC_LOAD_STATUS_INIT_DATA_INVALID => "INIT_DATA_INVALID",
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
        INTEL_BOOTROM_STATUS_RSA_FAILED => "RSA_FAILED",
        INTEL_BOOTROM_STATUS_PAVPC_FAILED => "PAVPC_FAILED",
        INTEL_BOOTROM_STATUS_WOPCM_FAILED => "WOPCM_FAILED",
        INTEL_BOOTROM_STATUS_LOADLOC_FAILED => "LOADLOC_FAILED",
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
        crate::log!("intel/igpu770: guc-fw module not found string=trueos.fw.guc\n");
        return GucFirmwareInfo::empty();
    };

    if blob.is_empty() {
        crate::log!("intel/igpu770: guc-fw module present but empty\n");
        return GucFirmwareInfo::empty();
    }

    let blob_magic = read_le_u32(blob, 0).unwrap_or(0);
    // Log first 16 bytes so we can directly verify CSS layout at the bytes level
    let b0  = blob.get(0).copied().unwrap_or(0);
    let b1  = blob.get(1).copied().unwrap_or(0);
    let b2  = blob.get(2).copied().unwrap_or(0);
    let b3  = blob.get(3).copied().unwrap_or(0);
    let b4  = blob.get(4).copied().unwrap_or(0);
    let b5  = blob.get(5).copied().unwrap_or(0);
    let b6  = blob.get(6).copied().unwrap_or(0);
    let b7  = blob.get(7).copied().unwrap_or(0);
    let b8  = blob.get(8).copied().unwrap_or(0);
    let b9  = blob.get(9).copied().unwrap_or(0);
    let b10 = blob.get(10).copied().unwrap_or(0);
    let b11 = blob.get(11).copied().unwrap_or(0);
    let b12 = blob.get(12).copied().unwrap_or(0);
    let b13 = blob.get(13).copied().unwrap_or(0);
    let b14 = blob.get(14).copied().unwrap_or(0);
    let b15 = blob.get(15).copied().unwrap_or(0);
    crate::log!(
        "intel/igpu770: guc-fw hdr[0..16]={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}\n",
        b0,b1,b2,b3,b4,b5,b6,b7,b8,b9,b10,b11,b12,b13,b14,b15
    );
    let alloc_len = blob.len().div_ceil(warm_align) * warm_align;
    let Some((phys, virt)) = crate::dma::alloc(alloc_len, warm_align) else {
        crate::log!(
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

    let (css_off, xfer_len, rsa_off, rsa_size) = if let Some((off, layout)) = find_guc_css_layout(blob)
    {
        (off, layout.dma_bytes, layout.rsa_offset, layout.rsa_size)
    } else {
        (0usize, blob.len(), 0usize, 0usize)
    };
    GUC_FW_BLOB_LEN.store(blob.len(), Ordering::Release);
    GUC_FW_DMA_OFFSET.store(css_off, Ordering::Release);
    GUC_FW_RSA_OFFSET.store(rsa_off, Ordering::Release);
    GUC_FW_RSA_SIZE.store(rsa_size, Ordering::Release);
    crate::log!(
        "intel/igpu770: guc-fw module found size=0x{:X} alloc=0x{:X} phys=0x{:X} gpu=0x{:X} css_off=0x{:X} xfer=0x{:X} rsa_off=0x{:X} rsa_size=0x{:X} zstd_magic={}\n",
        blob.len(),
        alloc_len,
        phys,
        GPU_VA_GUC_FW_BASE,
        css_off,
        xfer_len,
        rsa_off,
        rsa_size,
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
        crate::log!("intel/igpu770: guc-fw skipped reason=no-module\n");
        GUC_FW_READY.store(false, Ordering::Release);
        return;
    }

    let _ = forcewake_gt_acquire(warm);

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
    crate::log!(
        "intel/igpu770: guc-fw reset gdrst_rb=0x{:08X} gdrst_iters={} mia_in_reset={}\n",
        gdrst_rb,
        gdrst_iters,
        ((status_after_reset & GS_MIA_IN_RESET) != 0) as u8
    );

    let status_before = mmio_read32(warm, GUC_STATUS);
    let shim_before = mmio_read32(warm, GUC_SHIM_CONTROL);
    let pm_before = mmio_read32(warm, GT_PM_CONFIG);
    let wopcm_size = mmio_read32(warm, GUC_WOPCM_SIZE);
    let shim_after = shim_before
        | GUC_ENABLE_READ_CACHE_LOGIC
        | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA
        | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA
        | GUC_ENABLE_MIA_CLOCK_GATING
        | GUC_ENABLE_MIA_CACHING
        | GUC_MSGCH_ENABLE;
    let _ = mmio_write32(warm, GUC_SHIM_CONTROL, shim_after);
    let _ = mmio_write32(warm, GT_PM_CONFIG, pm_before | GT_DOORBELL_ENABLE);

    let fw_blob_len = GUC_FW_BLOB_LEN.load(Ordering::Acquire);
    let fw_dma_off = GUC_FW_DMA_OFFSET.load(Ordering::Acquire);
    let fw_rsa_off = GUC_FW_RSA_OFFSET.load(Ordering::Acquire);
    let fw_rsa_size = GUC_FW_RSA_SIZE.load(Ordering::Acquire);

    let src = warm.guc_fw_gpu_addr.saturating_add(fw_dma_off as u64);
    let blob = unsafe { core::slice::from_raw_parts(warm.guc_fw_virt as *const u8, warm.guc_fw_len) };
    if fw_rsa_size != 0 && fw_blob_len != 0 {
        let rsa_end = fw_rsa_off.saturating_add(fw_rsa_size);
        if rsa_end <= fw_blob_len {
            let rsa_dwords = core::cmp::min(fw_rsa_size / 4, UOS_RSA_SCRATCH_COUNT);
            for i in 0..rsa_dwords {
                let off = fw_rsa_off + i * 4;
                let v = u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]]);
                let _ = mmio_write32(warm, UOS_RSA_SCRATCH_BASE + i * 4, v);
            }
        }
    }
    let max_dma = warm.guc_fw_len.saturating_sub(fw_dma_off);
    let copy_size = warm.guc_fw_xfer_len.min(max_dma).min(u32::MAX as usize) as u32;
    // Program and lock uC WOPCM partition registers before GuC bootstrap DMA.
    // DMA_GUC_WOPCM_OFFSET is the GuC WOPCM base/config register, not DMA dst.
    let wopcm_size_cfg = (wopcm_size & GUC_WOPCM_SIZE_MASK) | GUC_WOPCM_SIZE_LOCKED;
    let wopcm_base_only = GUC_BOOT_WOPCM_BASE & GUC_WOPCM_OFFSET_MASK;
    let wopcm_base_cfg = wopcm_base_only | GUC_WOPCM_OFFSET_VALID;
    let _ = mmio_write32(warm, GUC_WOPCM_SIZE, wopcm_size_cfg);
    // Tiny register ping-pong: toggle VALID to ensure the base config write is latched.
    let _ = mmio_write32(warm, DMA_GUC_WOPCM_OFFSET, wopcm_base_only);
    let _ = mmio_write32(warm, DMA_GUC_WOPCM_OFFSET, wopcm_base_cfg);
    let _ = mmio_write32(warm, DMA_ADDR_0_LOW, src as u32);
    let _ = mmio_write32(warm, DMA_ADDR_0_HIGH, (src >> 32) as u32);
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

    let _ = mmio_write32(warm, GUC_SEND_INTERRUPT, 1);
    let mut status_after = mmio_read32(warm, GUC_STATUS);
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
    let pm_rb = mmio_read32(warm, GT_PM_CONFIG);
    let dma_ctrl_rb = mmio_read32(warm, DMA_CTRL);
    let wopcm_size_rb = mmio_read32(warm, GUC_WOPCM_SIZE);
    let dma_wopcm_off_rb = mmio_read32(warm, DMA_GUC_WOPCM_OFFSET);
    let dma_addr0_low_rb = mmio_read32(warm, DMA_ADDR_0_LOW);
    let dma_addr0_high_rb = mmio_read32(warm, DMA_ADDR_0_HIGH);
    let dma_addr1_low_rb = mmio_read32(warm, DMA_ADDR_1_LOW);
    let dma_addr1_high_rb = mmio_read32(warm, DMA_ADDR_1_HIGH);

    crate::log!(
        "intel/igpu770: guc-fw bootstrap rev={} src_gpu=0x{:X} fw_phys=0x{:X} fw_len=0x{:X} xfer=0x{:X} wopcm_size=0x{:08X} wopcm_size_rb=0x{:08X} wopcm_cfg=0x{:08X} status_before=0x{:08X} status_after=0x{:08X} status_final=0x{:08X} status_iters={} shim=0x{:08X} pm=0x{:08X} dma_done={} dma_iters={} dma_ctrl=0x{:08X} a0_lo=0x{:08X} a0_hi=0x{:08X} a1_lo=0x{:08X} a1_hi=0x{:08X} ready={}\n",
        GUC_BOOTSTRAP_REV,
        warm.guc_fw_gpu_addr,
        warm.guc_fw_phys,
        warm.guc_fw_len,
        copy_size,
        wopcm_size,
        wopcm_size_rb,
        dma_wopcm_off_rb,
        status_before,
        status_after,
        status_final,
        status_iters,
        shim_rb,
        pm_rb,
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
    crate::log!(
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
        crate::log!("intel/igpu770: guc-fw readiness=not-ready; rcs smoke submit gated\n");
    }
}
