mod display;
mod guc;
mod render;
pub(crate) mod shader;
pub(crate) mod xelp_media_mp4;
pub(crate) mod xelp_media_ngin;

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub(crate) const INTEL_VENDOR_ID: u16 = 0x8086;
pub(crate) const PCI_CLASS_DISPLAY: u8 = 0x03;
pub(crate) const GPU_VA_GUC_FW_BASE: u64 = 0x0085_0000;
pub(crate) const GPU_VA_GUC_ADS_BASE: u64 = 0x0100_0000;
pub(crate) const GPU_VA_DISPLAY_PRIMARY_BASE: u64 = 0x0200_0000;
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
const MEDIA_BOOT_DEMO_DELAY_MS: u64 = 5_000;

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
    if !map_ggtt(dev, fw.phys, fw.len, fw.gpu) || !map_ggtt(dev, ads.phys, ads.len, ads.gpu) {
        crate::log!("intel/guc: ggtt map failed fw_len=0x{:X} ads_len=0x{:X}\n", fw.len, ads.len);
        return;
    }
    ggtt_invalidate(dev);
    forcewake(dev);
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
    let warm = self::render::warm_once(dev);
    self::render::log_cursor_plane_info(warm);
    self::render::log_sprite_plane_info(warm);
    self::display::init_primary_gradient(dev);
    if self::render::forcewake_render_acquire(warm) {
        self::render::forcewake_render_sanity(warm);
    }
    crate::log!("intel/media: scheduled boot demo delay_ms={}\n", MEDIA_BOOT_DEMO_DELAY_MS);
    crate::wait::spawn_local_detached(async move {
        Timer::after(EmbassyDuration::from_millis(MEDIA_BOOT_DEMO_DELAY_MS)).await;
        crate::log!("intel/media: boot demo begin\n");
        self::run_https_media_demo_once_async().await;
    });
}

pub fn guc_ready() -> bool {
    self::guc::ready()
}

pub fn has_claimed_device() -> bool {
    CLAIMED_DEVICE.lock().is_some()
}

pub(crate) fn claimed_device() -> Option<Dev> {
    *CLAIMED_DEVICE.lock()
}

pub fn warm_state() -> Option<self::render::RenderWarmState> {
    self::render::warm_state()
}

pub fn active_scanout_dimensions() -> Option<(u32, u32)> {
    self::display::active_scanout_dimensions()
}

pub fn media_kickoff_once() {
    self::xelp_media_ngin::kickoff_once();
}

pub fn media_kickoff_state() -> Option<self::xelp_media_ngin::MediaKickoffState> {
    self::xelp_media_ngin::kickoff_state()
}

pub fn media_demo_surface_window(name: &str) -> Option<self::xelp_media_ngin::MediaSurfaceWindow> {
    self::xelp_media_ngin::demo_surface_window(name)
}

pub async fn run_https_media_demo_once_async() {
    self::xelp_media_ngin::run_https_media_demo_once_async().await
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

fn map_ggtt(dev: Dev, phys: u64, len: usize, gpu: u64) -> bool {
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
