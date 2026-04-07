use core::sync::atomic::{AtomicBool, Ordering};

const GUC_STATUS: usize = 0x0000_C000;
const SOFT_SCRATCH_BASE: usize = 0x0000_C180;
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
const GUC_MODULE_STRING: &[u8] = b"trueos.fw.guc";

const GUC_DISABLE_SRAM_INIT_TO_ZEROES: u32 = 1 << 0;
const GT_DOORBELL_ENABLE: u32 = 1 << 0;
const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;
const GUC_ENABLE_MIA_CACHING: u32 = 1 << 2;
const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
const GUC_ENABLE_DEBUG_REG: u32 = 1 << 11;
const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
const ARAT_EXPIRED_INTRMSK: u32 = 1 << 9;
const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
const UOS_MOVE: u32 = 1 << 4;
const START_DMA: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;
const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;
const GUC_BOOT_DEST_WOPCM_OFFSET: u32 = 0x2000;
const GUC_RSA_IN_MEMORY_THRESHOLD_BYTES: usize = 256;
const GUC_DMA_POLL_ITERS: usize = 20_000;
const GUC_READY_POLL_ITERS: usize = 200_000;
const GUC_RESET_POLL_ITERS: usize = 100_000;
const GUC_ADS_ADDR_SHIFT: u32 = 1;
const GS_AUTH_STATUS_BAD: u32 = 1;
const GRDOM_GUC: u32 = 1 << 3;
const INTEL_GUC_LOAD_STATUS_READY: u32 = 0xF0;
const GUC_MAX_ENGINE_CLASSES: usize = 16;
const GUC_MAX_INSTANCES_PER_CLASS: usize = 32;
const GLOBAL_POLICY_MAX_NUM_WI: u32 = 15;
const GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US: u32 = 500_000;
const DOORBELLS_PER_SQIDI_MASK: u32 = 0x00FF_0000;
const DOORBELLS_PER_SQIDI_SHIFT: u32 = 16;

static READY: AtomicBool = AtomicBool::new(false);

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct UcCssHeader {
    module_type: u32,
    header_size_dw: u32,
    _v: u32,
    _id: u32,
    _vendor: u32,
    _date: u32,
    size_dw: u32,
    key_size_dw: u32,
    modulus_size_dw: u32,
    exponent_size_dw: u32,
    _time: u32,
    _user: [u8; 8],
    _build: [u8; 12],
    _sw: u32,
    _vf: u32,
    _r: [u32; 12],
    private_data_size: u32,
    _info: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucMmioRegSet {
    _a: u32,
    _b: u16,
    _c: u16,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucPolicies {
    _depth: [u32; GUC_MAX_ENGINE_CLASSES],
    dpc_promote_time: u32,
    is_valid: u32,
    max_num_work_items: u32,
    _flags: u32,
    _r: [u32; 4],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucGtSystemInfo {
    mapping_table: [[u8; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES],
    _masks: [u32; GUC_MAX_ENGINE_CLASSES],
    generic_gt_sysinfo: [u32; GUC_MAX_ENGINE_CLASSES],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAds {
    reg_state_list: [[GucMmioRegSet; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES],
    _r0: u32,
    scheduler_policies: u32,
    gt_system_info: u32,
    _r1: u32,
    _ctrl: u32,
    _golden: [u32; GUC_MAX_ENGINE_CLASSES],
    _eng: [u32; GUC_MAX_ENGINE_CLASSES],
    private_data: u32,
    _um: u32,
    _rest: [u32; 45],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAdsBlobHeader {
    ads: GucAds,
    policies: GucPolicies,
    system_info: GucGtSystemInfo,
    _rest: [u8; 1184],
}

pub(crate) fn ready() -> bool {
    READY.load(Ordering::Acquire)
}

pub(crate) fn load_fw() -> crate::intel::Buf {
    let Some(blob) = crate::limine::module_bytes_by_string(GUC_MODULE_STRING) else {
        return crate::intel::empty();
    };
    let Some((css_offset, xfer_len, rsa_offset, rsa_size, private_data_size)) = parse_css(blob)
    else {
        return crate::intel::empty();
    };
    let len = blob.len().div_ceil(crate::intel::WARM_ALIGN) * crate::intel::WARM_ALIGN;
    let Some((phys, virt)) = crate::dma::alloc(len, crate::intel::WARM_ALIGN) else {
        return crate::intel::empty();
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, len);
        core::ptr::copy_nonoverlapping(blob.as_ptr(), virt, blob.len());
    }
    crate::intel::dma_flush(virt, len);
    crate::intel::Buf {
        phys,
        virt,
        len,
        gpu: crate::intel::GPU_VA_GUC_FW_BASE,
        css_offset,
        xfer_len,
        private_data_size,
        rsa_offset,
        rsa_size,
    }
}

pub(crate) fn alloc_ads(private_data_size: usize) -> crate::intel::Buf {
    let Some(priv_off) = crate::intel::align_up(core::mem::size_of::<GucAdsBlobHeader>(), 4096)
    else {
        return crate::intel::empty();
    };
    let Some(len) =
        priv_off.checked_add(crate::intel::align_up(private_data_size, 4096).unwrap_or(0))
    else {
        return crate::intel::empty();
    };
    let Some((phys, virt)) = crate::dma::alloc(len.max(4096), crate::intel::WARM_ALIGN) else {
        return crate::intel::empty();
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, len.max(4096));
    }
    crate::intel::Buf {
        phys,
        virt,
        len: len.max(4096),
        gpu: crate::intel::GPU_VA_GUC_ADS_BASE,
        css_offset: 0,
        xfer_len: 0,
        private_data_size,
        rsa_offset: 0,
        rsa_size: 0,
    }
}

pub(crate) fn bootstrap(
    dev: crate::intel::Dev,
    fw: crate::intel::Buf,
    ads: crate::intel::Buf,
) -> bool {
    crate::intel::mmio_write(dev, GDRST, GRDOM_GUC);
    for _ in 0..GUC_RESET_POLL_ITERS {
        if (crate::intel::mmio_read(dev, GDRST) & GRDOM_GUC) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    let shim = GUC_DISABLE_SRAM_INIT_TO_ZEROES
        | GUC_ENABLE_READ_CACHE_LOGIC
        | GUC_ENABLE_MIA_CACHING
        | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA
        | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA
        | GUC_ENABLE_MIA_CLOCK_GATING;
    crate::intel::mmio_write(dev, GUC_SHIM_CONTROL, shim);
    crate::intel::mmio_write(
        dev,
        GUC_SHIM_CONTROL2,
        crate::intel::mmio_read(dev, GUC_SHIM_CONTROL2) | GUC_ENABLE_DEBUG_REG,
    );
    crate::intel::mmio_write(
        dev,
        GT_PM_CONFIG,
        crate::intel::mmio_read(dev, GT_PM_CONFIG) | GT_DOORBELL_ENABLE,
    );
    crate::intel::mmio_write(
        dev,
        PMINTRMSK,
        crate::intel::mmio_read(dev, PMINTRMSK) & !ARAT_EXPIRED_INTRMSK,
    );
    build_ads(dev, ads);
    init_params(dev, ads);
    mirror_rsa(dev, fw);
    let size = crate::intel::mmio_read(dev, GUC_WOPCM_SIZE);
    let off = crate::intel::mmio_read(dev, DMA_GUC_WOPCM_OFFSET);
    if (size & GUC_WOPCM_SIZE_LOCKED) == 0 || (off & GUC_WOPCM_OFFSET_VALID) == 0 {
        if let Some((base, sz)) = crate::intel::compute_wopcm(fw.xfer_len as u32) {
            crate::intel::mmio_write(dev, GUC_WOPCM_SIZE, sz | GUC_WOPCM_SIZE_LOCKED);
            crate::intel::mmio_write(dev, DMA_GUC_WOPCM_OFFSET, base | GUC_WOPCM_OFFSET_VALID);
        }
    }
    let src = fw.gpu + fw.css_offset as u64;
    crate::intel::mmio_write(dev, DMA_ADDR_0_LOW, src as u32);
    crate::intel::mmio_write(dev, DMA_ADDR_0_HIGH, ((src >> 32) as u32) | DMA_ADDRESS_SPACE_GGTT);
    crate::intel::mmio_write(dev, DMA_ADDR_1_LOW, GUC_BOOT_DEST_WOPCM_OFFSET);
    crate::intel::mmio_write(dev, DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM);
    crate::intel::mmio_write(dev, DMA_COPY_SIZE, fw.xfer_len as u32);
    crate::intel::mmio_write(dev, DMA_CTRL, crate::intel::mask_en(UOS_MOVE | START_DMA));
    for _ in 0..GUC_DMA_POLL_ITERS {
        if (crate::intel::mmio_read(dev, DMA_CTRL) & START_DMA) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    crate::intel::mmio_write(dev, DMA_CTRL, crate::intel::mask_dis(UOS_MOVE));
    for _ in 0..GUC_READY_POLL_ITERS {
        let status = crate::intel::mmio_read(dev, GUC_STATUS);
        if let Some(ok) = terminal(status) {
            let ready = ok && auth(status) != GS_AUTH_STATUS_BAD;
            READY.store(ready, Ordering::Release);
            return ready;
        }
        core::hint::spin_loop();
    }
    READY.store(false, Ordering::Release);
    false
}

pub(crate) fn status(dev: crate::intel::Dev) -> u32 {
    crate::intel::mmio_read(dev, GUC_STATUS)
}

pub(crate) fn describe_status(s: u32) -> (&'static str, &'static str, u32) {
    let boot = match bootrom(s) {
        0x13 => "NO_KEY",
        0x50 => "RSA_FAILED",
        0x73 => "PAVPC_FAILED",
        0x74 => "WOPCM_FAILED",
        0x75 => "LOADLOC_FAILED",
        0x76 => "JUMP_PASSED",
        0x77 => "JUMP_FAILED",
        0x79 => "RC6CTXCONFIG_FAILED",
        0x7A => "MPUMAP_INCORRECT",
        0x7E => "EXCEPTION",
        0x2B => "PROD_KEY_CHECK",
        _ => "OK_OR_UNKNOWN",
    };
    let uk = match ukernel(s) {
        0x01 => "START",
        0x05 => "HWCONFIG_START",
        0x06 => "HWCONFIG_DONE",
        0x10 => "GDT_DONE",
        0x20 => "IDT_DONE",
        0x30 => "LAPIC_DONE",
        0x40 => "GUCINT_DONE",
        0x50 => "DPC_READY",
        0x60 => "DPC_ERROR",
        0x70 => "EXCEPTION",
        0x71 => "INIT_DATA_INVALID",
        0x73 => "MPU_DATA_INVALID",
        0x74 => "MMIO_SR_INVALID",
        0x75 => "KLV_INIT_ERROR",
        INTEL_GUC_LOAD_STATUS_READY => "READY",
        _ => "UNKNOWN",
    };
    (boot, uk, auth(s))
}

fn build_ads(dev: crate::intel::Dev, ads: crate::intel::Buf) {
    let buf = unsafe { core::slice::from_raw_parts_mut(ads.virt, ads.len) };
    let p = core::mem::offset_of!(GucAdsBlobHeader, policies);
    let s = core::mem::offset_of!(GucAdsBlobHeader, system_info);
    let a = core::mem::offset_of!(GucAdsBlobHeader, ads);
    let priv_off =
        crate::intel::align_up(core::mem::size_of::<GucAdsBlobHeader>(), 4096).unwrap_or(4096);
    crate::intel::wr32(
        buf,
        p + core::mem::offset_of!(GucPolicies, dpc_promote_time),
        GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US,
    );
    crate::intel::wr32(buf, p + core::mem::offset_of!(GucPolicies, is_valid), 1);
    crate::intel::wr32(
        buf,
        p + core::mem::offset_of!(GucPolicies, max_num_work_items),
        GLOBAL_POLICY_MAX_NUM_WI,
    );
    let map = s + core::mem::offset_of!(GucGtSystemInfo, mapping_table);
    for i in 0..(GUC_MAX_ENGINE_CLASSES * GUC_MAX_INSTANCES_PER_CLASS) {
        buf[map + i] = GUC_MAX_INSTANCES_PER_CLASS as u8;
    }
    let doorbells = ((crate::intel::mmio_read(dev, DIST_DBS_POPULATED) & DOORBELLS_PER_SQIDI_MASK)
        >> DOORBELLS_PER_SQIDI_SHIFT)
        .saturating_add(1);
    crate::intel::wr32(
        buf,
        s + core::mem::offset_of!(GucGtSystemInfo, generic_gt_sysinfo) + 8,
        doorbells,
    );
    crate::intel::wr32(
        buf,
        a + core::mem::offset_of!(GucAds, scheduler_policies),
        (ads.gpu + p as u64) as u32,
    );
    crate::intel::wr32(
        buf,
        a + core::mem::offset_of!(GucAds, gt_system_info),
        (ads.gpu + s as u64) as u32,
    );
    crate::intel::wr32(
        buf,
        a + core::mem::offset_of!(GucAds, private_data),
        (ads.gpu + priv_off as u64) as u32,
    );
    crate::intel::dma_flush(ads.virt, ads.len);
}

fn init_params(dev: crate::intel::Dev, ads: crate::intel::Buf) {
    let vals = [
        0,
        0,
        0,
        0,
        ((ads.gpu >> 12) as u32) << GUC_ADS_ADDR_SHIFT,
        ((dev.device_id as u32) << 16) | dev.revision_id as u32,
    ];
    crate::intel::mmio_write(dev, SOFT_SCRATCH_BASE, 0);
    for (i, v) in vals.iter().enumerate() {
        crate::intel::mmio_write(dev, SOFT_SCRATCH_BASE + (i + 1) * 4, *v);
    }
}

fn mirror_rsa(dev: crate::intel::Dev, fw: crate::intel::Buf) {
    if fw.rsa_size == 0 {
        return;
    }
    let blob = unsafe { core::slice::from_raw_parts(fw.virt as *const u8, fw.len) };
    if fw.rsa_size > GUC_RSA_IN_MEMORY_THRESHOLD_BYTES {
        crate::intel::mmio_write(dev, UOS_RSA_SCRATCH_BASE, (fw.gpu + fw.rsa_offset as u64) as u32);
        return;
    }
    for i in 0..(fw.rsa_size / 4).min(64) {
        let o = fw.rsa_offset + i * 4;
        crate::intel::mmio_write(
            dev,
            UOS_RSA_SCRATCH_BASE + i * 4,
            u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]),
        );
    }
}

fn bootrom(s: u32) -> u32 {
    (s & crate::intel::GS_BOOTROM_MASK) >> 1
}

fn ukernel(s: u32) -> u32 {
    (s & crate::intel::GS_UKERNEL_MASK) >> 8
}

fn auth(s: u32) -> u32 {
    (s & crate::intel::GS_AUTH_STATUS_MASK) >> 30
}

fn terminal(s: u32) -> Option<bool> {
    if ukernel(s) == INTEL_GUC_LOAD_STATUS_READY {
        Some(true)
    } else {
        match bootrom(s) {
            0x13 | 0x50 | 0x73 | 0x74 | 0x75 | 0x77 | 0x79 | 0x7A | 0x7E | 0x2B => Some(false),
            _ => None,
        }
    }
}

fn parse_css(blob: &[u8]) -> Option<(usize, usize, usize, usize, usize)> {
    let end = blob
        .len()
        .checked_sub(core::mem::size_of::<UcCssHeader>())?;
    for off in (0..=end).step_by(4) {
        let css = unsafe { (blob.as_ptr().add(off) as *const UcCssHeader).read_unaligned() };
        if css.module_type != 5 && css.module_type != 6 {
            continue;
        }
        let fixed = css
            .header_size_dw
            .checked_sub(css.key_size_dw)?
            .checked_sub(css.modulus_size_dw)?
            .checked_sub(css.exponent_size_dw)?;
        if fixed != 32 {
            continue;
        }
        let header = css.header_size_dw.checked_mul(4)? as usize;
        let total = css.size_dw.checked_mul(4)? as usize;
        let xfer = 128usize.checked_add(total.checked_sub(header)?)?;
        let rsa_size = css.key_size_dw.checked_mul(4)? as usize;
        let rsa_off = off.checked_add(xfer)?;
        if off.checked_add(xfer)? <= blob.len() && rsa_off.checked_add(rsa_size)? <= blob.len() {
            return Some((off, xfer, rsa_off, rsa_size, css.private_data_size as usize));
        }
    }
    None
}
