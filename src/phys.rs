use core::sync::atomic::{AtomicU64, Ordering};

static HEAP_VIRT_BASE: AtomicU64 = AtomicU64::new(0);
static HEAP_PHYS_BASE: AtomicU64 = AtomicU64::new(0);
static HEAP_LEN: AtomicU64 = AtomicU64::new(0);
static HHDM_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_VIRT_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_PHYS_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_LEN: AtomicU64 = AtomicU64::new(0);

pub fn register_heap(virt_base: usize, phys_base: usize, length: usize) {
    HEAP_VIRT_BASE.store(virt_base as u64, Ordering::SeqCst);
    HEAP_PHYS_BASE.store(phys_base as u64, Ordering::SeqCst);
    HEAP_LEN.store(length as u64, Ordering::SeqCst);
}

pub fn register_hhdm_base(base: usize) {
    HHDM_BASE.store(base as u64, Ordering::SeqCst);
}

pub fn register_kernel_image(virt_base: usize, phys_base: usize, length: usize) {
    KERNEL_VIRT_BASE.store(virt_base as u64, Ordering::SeqCst);
    KERNEL_PHYS_BASE.store(phys_base as u64, Ordering::SeqCst);
    KERNEL_LEN.store(length as u64, Ordering::SeqCst);
}

/// Translate a physical address into a higher-half direct map (if present).
#[inline(always)]
pub fn phys_to_virt(phys: usize) -> usize {
    let hhdm = HHDM_BASE.load(Ordering::Relaxed);
    if hhdm != 0 {
        phys.checked_add(hhdm as usize).unwrap_or_else(|| {
            crate::debugconf!(
                "phys_to_virt: overflow translating phys=0x{:X} with hhdm=0x{:X}\n",
                phys,
                hhdm
            );
            phys
        })
    } else {
        phys
    }
}

/// Translate a kernel virtual address into a guest-physical address for MMIO/DMA.
#[inline(always)]
pub fn virt_to_phys<T>(ptr: *const T) -> u64 {
    let addr = ptr as usize as u64;

    // Heap window registered from the allocator selection.
    let virt_base = HEAP_VIRT_BASE.load(Ordering::Relaxed);
    let len = HEAP_LEN.load(Ordering::Relaxed);
    if len != 0 {
        let virt_end = virt_base + len;
        if addr >= virt_base && addr < virt_end {
            let phys_base = HEAP_PHYS_BASE.load(Ordering::Relaxed);
            return phys_base + (addr - virt_base);
        }
    }

    // Kernel image mapping (higher-half offset).
    let kern_base = KERNEL_VIRT_BASE.load(Ordering::Relaxed);
    let kern_len = KERNEL_LEN.load(Ordering::Relaxed);
    if kern_len != 0 && addr >= kern_base && addr < kern_base + kern_len {
        let phys_base = KERNEL_PHYS_BASE.load(Ordering::Relaxed);
        return phys_base + (addr - kern_base);
    }

    // Higher-half direct map (HHDM) covers raw physical memory.
    let hhdm = HHDM_BASE.load(Ordering::Relaxed);
    if hhdm != 0 && addr >= hhdm {
        return addr - hhdm;
    }

    addr
}
