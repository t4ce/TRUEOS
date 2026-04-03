use core::sync::atomic::{AtomicBool, Ordering};

const INTEL_VENDOR_ID: u16 = 0x8086;
const PCI_CLASS_DISPLAY: u8 = 0x03;
const GPU_VA_GUC_FW_BASE: u64 = 0x0085_0000;
const GPU_VA_GUC_ADS_BASE: u64 = 0x0100_0000;
const WARM_ALIGN: usize = 4096;
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
const GUC_DISABLE_SRAM_INIT_TO_ZEROES: u32 = 1 << 0;
const GT_DOORBELL_ENABLE: u32 = 1 << 0;
const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;
const GUC_ENABLE_MIA_CACHING: u32 = 1 << 2;
const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
const GUC_ENABLE_DEBUG_REG: u32 = 1 << 11;
const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
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
const GUC_RSA_IN_MEMORY_THRESHOLD_BYTES: usize = 256;
const GUC_DMA_POLL_ITERS: usize = 20_000;
const GUC_READY_POLL_ITERS: usize = 200_000;
const GUC_RESET_POLL_ITERS: usize = 100_000;
const GUC_ADS_ADDR_SHIFT: u32 = 1;
const GEN11_WOPCM_SIZE: u32 = 0x0020_0000;
const WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_STACK_RESERVED_SIZE: u32 = 0x0000_2000;
const WOPCM_HW_CTX_RESERVED_SIZE: u32 = 0x0000_9000;
const GUC_WOPCM_OFFSET_ALIGNMENT: u32 = 1 << GUC_WOPCM_OFFSET_SHIFT;
const GS_BOOTROM_MASK: u32 = 0x7F << 1;
const GS_UKERNEL_MASK: u32 = 0xFF << 8;
const GS_AUTH_STATUS_MASK: u32 = 0x03 << 30;
const GS_AUTH_STATUS_BAD: u32 = 1;
const GRDOM_GUC: u32 = 1 << 3;
const INTEL_GUC_LOAD_STATUS_READY: u32 = 0xF0;
const GUC_MAX_ENGINE_CLASSES: usize = 16;
const GUC_MAX_INSTANCES_PER_CLASS: usize = 32;
const GLOBAL_POLICY_MAX_NUM_WI: u32 = 15;
const GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US: u32 = 500_000;
const DOORBELLS_PER_SQIDI_MASK: u32 = 0x00FF_0000;
const DOORBELLS_PER_SQIDI_SHIFT: u32 = 16;
const GUC_MODULE_STRING: &[u8] = b"trueos.fw.guc";

static INIT: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct UcCssHeader { module_type: u32, header_size_dw: u32, _v: u32, _id: u32, _vendor: u32, _date: u32, size_dw: u32, key_size_dw: u32, modulus_size_dw: u32, exponent_size_dw: u32, _time: u32, _user: [u8; 8], _build: [u8; 12], _sw: u32, _vf: u32, _r: [u32; 12], private_data_size: u32, _info: u32 }
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucMmioRegSet { _a: u32, _b: u16, _c: u16 }
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucPolicies { _depth: [u32; GUC_MAX_ENGINE_CLASSES], dpc_promote_time: u32, is_valid: u32, max_num_work_items: u32, _flags: u32, _r: [u32; 4] }
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucGtSystemInfo { mapping_table: [[u8; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES], _masks: [u32; GUC_MAX_ENGINE_CLASSES], generic_gt_sysinfo: [u32; GUC_MAX_ENGINE_CLASSES] }
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAds { reg_state_list: [[GucMmioRegSet; GUC_MAX_INSTANCES_PER_CLASS]; GUC_MAX_ENGINE_CLASSES], _r0: u32, scheduler_policies: u32, gt_system_info: u32, _r1: u32, _ctrl: u32, _golden: [u32; GUC_MAX_ENGINE_CLASSES], _eng: [u32; GUC_MAX_ENGINE_CLASSES], private_data: u32, _um: u32, _rest: [u32; 45] }
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GucAdsBlobHeader { ads: GucAds, policies: GucPolicies, system_info: GucGtSystemInfo, _rest: [u8; 1184] }

#[derive(Copy, Clone)]
struct Dev { bus: u8, slot: u8, function: u8, device_id: u16, revision_id: u8, mmio: *mut u8, mmio_len: usize }
#[derive(Copy, Clone)]
struct Buf { phys: u64, virt: *mut u8, len: usize, gpu: u64, css_offset: usize, xfer_len: usize, private_data_size: usize, rsa_offset: usize, rsa_size: usize }

pub fn init_once() {
    if INIT.swap(true, Ordering::AcqRel) { return; }
    let Some(dev) = find_dev() else { return; };
    let fw = load_fw();
    if fw.len == 0 { return; }
    let ads = alloc_ads(fw.private_data_size);
    if ads.len == 0 { return; }
    if !map_ggtt(dev, fw.phys, fw.len, fw.gpu) || !map_ggtt(dev, ads.phys, ads.len, ads.gpu) { return; }
    ggtt_invalidate(dev);
    forcewake(dev);
    if bootstrap(dev, fw, ads) { READY.store(true, Ordering::Release); }
}

pub fn guc_ready() -> bool { READY.load(Ordering::Acquire) }

fn find_dev() -> Option<Dev> {
    let mut out = None;
    crate::pci::with_devices(|list| for d in list {
        if d.vendor == INTEL_VENDOR_ID && d.class == PCI_CLASS_DISPLAY && out.is_none() {
            let Some(size) = crate::pci::bar0_size_bytes(d.bus, d.slot, d.function) else { continue; };
            let (lo, hi) = crate::pci::read_bar0_raw(d.bus, d.slot, d.function);
            if lo == 0 || lo == 0xFFFF_FFFF || (lo & 1) != 0 { continue; }
            let phys = if let Some(hi) = hi { (((hi as u64) << 32) | lo as u64) & !0xF } else { (lo as u64) & !0xF };
            crate::pci::enable_mem_and_bus_master(d.bus, d.slot, d.function);
            let Some(mmio) = crate::pci::mmio::map_mmio_region_exact(phys, size as usize).ok().map(|p| p.as_ptr()) else { continue; };
            out = Some(Dev { bus: d.bus, slot: d.slot, function: d.function, device_id: d.device, revision_id: crate::pci::config_read_u8(d.bus, d.slot, d.function, 0x08), mmio, mmio_len: size as usize });
        }
    });
    out
}

fn load_fw() -> Buf {
    let Some(blob) = crate::limine::module_bytes_by_string(GUC_MODULE_STRING) else { return empty(); };
    let Some((css_offset, xfer_len, rsa_offset, rsa_size, private_data_size)) = parse_css(blob) else { return empty(); };
    let len = blob.len().div_ceil(WARM_ALIGN) * WARM_ALIGN;
    let Some((phys, virt)) = crate::dma::alloc(len, WARM_ALIGN) else { return empty(); };
    unsafe { core::ptr::write_bytes(virt, 0, len); core::ptr::copy_nonoverlapping(blob.as_ptr(), virt, blob.len()); }
    dma_flush(virt, len);
    Buf { phys, virt, len, gpu: GPU_VA_GUC_FW_BASE, css_offset, xfer_len, private_data_size, rsa_offset, rsa_size }
}

fn alloc_ads(private_data_size: usize) -> Buf {
    let Some(priv_off) = align_up(core::mem::size_of::<GucAdsBlobHeader>(), 4096) else { return empty(); };
    let Some(len) = priv_off.checked_add(align_up(private_data_size, 4096).unwrap_or(0)) else { return empty(); };
    let Some((phys, virt)) = crate::dma::alloc(len.max(4096), WARM_ALIGN) else { return empty(); };
    unsafe { core::ptr::write_bytes(virt, 0, len.max(4096)); }
    Buf { phys, virt, len: len.max(4096), gpu: GPU_VA_GUC_ADS_BASE, css_offset: 0, xfer_len: 0, private_data_size, rsa_offset: 0, rsa_size: 0 }
}

fn bootstrap(dev: Dev, fw: Buf, ads: Buf) -> bool {
    mmio_write(dev, GDRST, GRDOM_GUC);
    for _ in 0..GUC_RESET_POLL_ITERS { if (mmio_read(dev, GDRST) & GRDOM_GUC) == 0 { break; } core::hint::spin_loop(); }
    let shim = GUC_DISABLE_SRAM_INIT_TO_ZEROES | GUC_ENABLE_READ_CACHE_LOGIC | GUC_ENABLE_MIA_CACHING | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA | GUC_ENABLE_MIA_CLOCK_GATING;
    mmio_write(dev, GUC_SHIM_CONTROL, shim);
    mmio_write(dev, GUC_SHIM_CONTROL2, mmio_read(dev, GUC_SHIM_CONTROL2) | GUC_ENABLE_DEBUG_REG);
    mmio_write(dev, GT_PM_CONFIG, mmio_read(dev, GT_PM_CONFIG) | GT_DOORBELL_ENABLE);
    mmio_write(dev, PMINTRMSK, mmio_read(dev, PMINTRMSK) & !ARAT_EXPIRED_INTRMSK);
    build_ads(dev, ads);
    init_params(dev, ads);
    mirror_rsa(dev, fw);
    let size = mmio_read(dev, GUC_WOPCM_SIZE);
    let off = mmio_read(dev, DMA_GUC_WOPCM_OFFSET);
    if (size & GUC_WOPCM_SIZE_LOCKED) == 0 || (off & GUC_WOPCM_OFFSET_VALID) == 0 {
        if let Some((base, sz)) = compute_wopcm(fw.xfer_len as u32) {
            mmio_write(dev, GUC_WOPCM_SIZE, sz | GUC_WOPCM_SIZE_LOCKED);
            mmio_write(dev, DMA_GUC_WOPCM_OFFSET, base | GUC_WOPCM_OFFSET_VALID);
        }
    }
    let src = fw.gpu + fw.css_offset as u64;
    mmio_write(dev, DMA_ADDR_0_LOW, src as u32);
    mmio_write(dev, DMA_ADDR_0_HIGH, ((src >> 32) as u32) | DMA_ADDRESS_SPACE_GGTT);
    mmio_write(dev, DMA_ADDR_1_LOW, GUC_BOOT_DEST_WOPCM_OFFSET);
    mmio_write(dev, DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM);
    mmio_write(dev, DMA_COPY_SIZE, fw.xfer_len as u32);
    mmio_write(dev, DMA_CTRL, mask_en(UOS_MOVE | START_DMA));
    for _ in 0..GUC_DMA_POLL_ITERS { if (mmio_read(dev, DMA_CTRL) & START_DMA) == 0 { break; } core::hint::spin_loop(); }
    mmio_write(dev, DMA_CTRL, mask_dis(UOS_MOVE));
    for _ in 0..GUC_READY_POLL_ITERS {
        let s = mmio_read(dev, GUC_STATUS);
        if let Some(ok) = terminal(s) { return ok && auth(s) != GS_AUTH_STATUS_BAD; }
        core::hint::spin_loop();
    }
    false
}

fn build_ads(dev: Dev, ads: Buf) {
    let buf = unsafe { core::slice::from_raw_parts_mut(ads.virt, ads.len) };
    let p = core::mem::offset_of!(GucAdsBlobHeader, policies);
    let s = core::mem::offset_of!(GucAdsBlobHeader, system_info);
    let a = core::mem::offset_of!(GucAdsBlobHeader, ads);
    let priv_off = align_up(core::mem::size_of::<GucAdsBlobHeader>(), 4096).unwrap_or(4096);
    wr32(buf, p + core::mem::offset_of!(GucPolicies, dpc_promote_time), GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US);
    wr32(buf, p + core::mem::offset_of!(GucPolicies, is_valid), 1);
    wr32(buf, p + core::mem::offset_of!(GucPolicies, max_num_work_items), GLOBAL_POLICY_MAX_NUM_WI);
    let map = s + core::mem::offset_of!(GucGtSystemInfo, mapping_table);
    for i in 0..(GUC_MAX_ENGINE_CLASSES * GUC_MAX_INSTANCES_PER_CLASS) { buf[map + i] = GUC_MAX_INSTANCES_PER_CLASS as u8; }
    let doorbells = ((mmio_read(dev, DIST_DBS_POPULATED) & DOORBELLS_PER_SQIDI_MASK) >> DOORBELLS_PER_SQIDI_SHIFT).saturating_add(1);
    wr32(buf, s + core::mem::offset_of!(GucGtSystemInfo, generic_gt_sysinfo) + 8, doorbells);
    wr32(buf, a + core::mem::offset_of!(GucAds, scheduler_policies), (ads.gpu + p as u64) as u32);
    wr32(buf, a + core::mem::offset_of!(GucAds, gt_system_info), (ads.gpu + s as u64) as u32);
    wr32(buf, a + core::mem::offset_of!(GucAds, private_data), (ads.gpu + priv_off as u64) as u32);
    dma_flush(ads.virt, ads.len);
}

fn init_params(dev: Dev, ads: Buf) {
    let vals = [0, 0, 0, GUC_LOG_DISABLED, ((ads.gpu >> 12) as u32) << GUC_ADS_ADDR_SHIFT, ((dev.device_id as u32) << 16) | dev.revision_id as u32];
    mmio_write(dev, SOFT_SCRATCH_BASE, 0);
    for (i, v) in vals.iter().enumerate() { mmio_write(dev, SOFT_SCRATCH_BASE + (i + 1) * 4, *v); }
}

fn mirror_rsa(dev: Dev, fw: Buf) {
    if fw.rsa_size == 0 { return; }
    let blob = unsafe { core::slice::from_raw_parts(fw.virt as *const u8, fw.len) };
    if fw.rsa_size > GUC_RSA_IN_MEMORY_THRESHOLD_BYTES { mmio_write(dev, UOS_RSA_SCRATCH_BASE, (fw.gpu + fw.rsa_offset as u64) as u32); return; }
    for i in 0..(fw.rsa_size / 4).min(64) { let o = fw.rsa_offset + i * 4; mmio_write(dev, UOS_RSA_SCRATCH_BASE + i * 4, u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]])); }
}

fn forcewake(dev: Dev) {
    mmio_write(dev, FORCEWAKE_RENDER, mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK));
    wait_eq(dev, FORCEWAKE_ACK_RENDER, FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK, 0, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_RENDER, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_RENDER, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_MEDIA, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_MEDIA, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
    mmio_write(dev, FORCEWAKE_GT, mask_en(FORCEWAKE_KERNEL));
    wait_eq(dev, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);
}

fn map_ggtt(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
    for page in 0..len.div_ceil(WARM_ALIGN) {
        let g = gpu + (page as u64) * GGTT_PAGE_BYTES;
        let p = (phys + (page as u64) * GGTT_PAGE_BYTES) & !0xFFF;
        let idx = match usize::try_from(g / GGTT_PAGE_BYTES).ok().and_then(|v| v.checked_mul(8)) { Some(v) if v + 8 <= GGTT_ALIAS_BYTES => v, _ => return false };
        unsafe { core::ptr::write_volatile(dev.mmio.add(GGTT_ALIAS_BASE_OFF + idx) as *mut u64, p | GEN8_PAGE_PRESENT); }
    }
    true
}

fn ggtt_invalidate(dev: Dev) { mmio_write(dev, GFX_FLSH_CNTL_GEN6, GFX_FLSH_CNTL_EN); }
fn mmio_read(dev: Dev, off: usize) -> u32 { if off + 4 > dev.mmio_len { 0 } else { unsafe { core::ptr::read_volatile(dev.mmio.add(off) as *const u32) } } }
fn mmio_write(dev: Dev, off: usize, v: u32) { if off + 4 <= dev.mmio_len { unsafe { core::ptr::write_volatile(dev.mmio.add(off) as *mut u32, v) } } }
fn wait_eq(dev: Dev, reg: usize, mask: u32, want: u32, n: usize) { for _ in 0..n { if (mmio_read(dev, reg) & mask) == want { break; } core::hint::spin_loop(); } }
fn mask_en(v: u32) -> u32 { v | (v << 16) }
fn mask_dis(v: u32) -> u32 { v << 16 }
fn bootrom(s: u32) -> u32 { (s & GS_BOOTROM_MASK) >> 1 }
fn ukernel(s: u32) -> u32 { (s & GS_UKERNEL_MASK) >> 8 }
fn auth(s: u32) -> u32 { (s & GS_AUTH_STATUS_MASK) >> 30 }
fn terminal(s: u32) -> Option<bool> { if ukernel(s) == INTEL_GUC_LOAD_STATUS_READY { Some(true) } else { match bootrom(s) { 0x13 | 0x50 | 0x73 | 0x74 | 0x75 | 0x77 | 0x79 | 0x7A | 0x7E | 0x2B => Some(false), _ => None } } }
fn compute_wopcm(fw: u32) -> Option<(u32, u32)> { let usable = GEN11_WOPCM_SIZE.checked_sub(WOPCM_HW_CTX_RESERVED_SIZE)?; let min = fw.checked_add(GUC_WOPCM_RESERVED_SIZE)?.checked_add(GUC_WOPCM_STACK_RESERVED_SIZE)?; let base = align_up_u32(WOPCM_RESERVED_SIZE, GUC_WOPCM_OFFSET_ALIGNMENT)?; if base >= usable { return None; } let size = (usable - base) & GUC_WOPCM_SIZE_MASK; if size < min { None } else { Some((base, size)) } }
fn align_up(v: usize, a: usize) -> Option<usize> { let m = a.checked_sub(1)?; v.checked_add(m).map(|x| x & !m) }
fn align_up_u32(v: u32, a: u32) -> Option<u32> { let m = a.checked_sub(1)?; v.checked_add(m).map(|x| x & !m) }
fn wr32(buf: &mut [u8], off: usize, v: u32) { if let Some(dst) = buf.get_mut(off..off + 4) { dst.copy_from_slice(&v.to_le_bytes()); } }
fn empty() -> Buf { Buf { phys: 0, virt: core::ptr::null_mut(), len: 0, gpu: 0, css_offset: 0, xfer_len: 0, private_data_size: 0, rsa_offset: 0, rsa_size: 0 } }
fn parse_css(blob: &[u8]) -> Option<(usize, usize, usize, usize, usize)> {
    let end = blob.len().checked_sub(core::mem::size_of::<UcCssHeader>())?;
    for off in (0..=end).step_by(4) {
        let css = unsafe { (blob.as_ptr().add(off) as *const UcCssHeader).read_unaligned() };
        if css.module_type != 5 && css.module_type != 6 { continue; }
        let fixed = css.header_size_dw.checked_sub(css.key_size_dw)?.checked_sub(css.modulus_size_dw)?.checked_sub(css.exponent_size_dw)?;
        if fixed != 32 { continue; }
        let header = css.header_size_dw.checked_mul(4)? as usize;
        let total = css.size_dw.checked_mul(4)? as usize;
        let xfer = 128usize.checked_add(total.checked_sub(header)?)?;
        let rsa_size = css.key_size_dw.checked_mul(4)? as usize;
        let rsa_off = off.checked_add(xfer)?;
        if off.checked_add(xfer)? <= blob.len() && rsa_off.checked_add(rsa_size)? <= blob.len() { return Some((off, xfer, rsa_off, rsa_size, css.private_data_size as usize)); }
    }
    None
}

#[cfg(target_arch = "x86_64")]
fn dma_flush(ptr: *mut u8, len: usize) { unsafe { use core::arch::x86_64::{_mm_clflush, _mm_mfence}; let mut p = (ptr as usize) & !63usize; let end = (ptr as usize).saturating_add(len); while p < end { _mm_clflush(p as *const _); p += 64; } _mm_mfence(); } }
#[cfg(not(target_arch = "x86_64"))]
fn dma_flush(_ptr: *mut u8, _len: usize) {}
