use super::hvlogf;
use spin::Mutex;
use crate::phys::HeapArena;

// Guest memory constants
pub const PAGE_SIZE_4K: usize = 4096;
pub const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const GUEST_STACK_VA_BASE: u64 = 0x0000_0000_0040_0000;
pub const GUEST_STACK_MIN_MIB: usize = 64;
pub const GUEST_STACK_DEFAULT_MIB: usize = 64;
pub const GUEST_STACK_MAX_MIB: usize = 512;
pub const GUEST_STACK_MIN_BYTES: usize = GUEST_STACK_MIN_MIB * 1024 * 1024;
pub const GUEST_STACK_DEFAULT_BYTES: usize = GUEST_STACK_DEFAULT_MIB * 1024 * 1024;
pub const GUEST_STACK_MAX_BYTES: usize = GUEST_STACK_MAX_MIB * 1024 * 1024;
pub const GUEST_STACK_PT_CAP: usize = GUEST_STACK_MAX_BYTES.div_ceil(PAGE_SIZE_2M as usize);
pub const GUEST_LOW_PT_COUNT: usize = GUEST_STACK_PT_CAP + 1;
pub const GUEST_COMM_PAGE_VA: u64 = GUEST_STACK_VA_BASE + GUEST_STACK_MAX_BYTES as u64;
pub const GUEST_HIGH_IMAGE_PT_COUNT: usize = 1024;
pub const GUEST_HEAP_PD_COUNT: usize = 8;
pub const GUEST_HIGH_IMAGE_MAX_BYTES: u64 = GUEST_HIGH_IMAGE_PT_COUNT as u64 * PAGE_SIZE_2M;
pub const ELF64_HEADER_LEN: usize = 64;
pub const EPT_PDPT_ENTRIES: usize = 4;
pub const EPT_PD_ENTRIES: usize = 512;

// Page table entry flags
pub const PT_ENTRY_PRESENT: u64 = 1 << 0;
pub const PT_ENTRY_WRITABLE: u64 = 1 << 1;

#[repr(C, align(4096))]
#[derive(Copy, Clone)]
pub struct EptPage(pub [u64; 512]);

#[repr(C, align(4096))]
#[derive(Copy, Clone)]
pub struct GuestPage(pub [u64; 512]);

static mut EPT_PML4: EptPage = EptPage([0u64; 512]);
static mut EPT_PDPT: EptPage = EptPage([0u64; 512]);
static mut EPT_PD: [EptPage; EPT_PDPT_ENTRIES] = [EptPage([0u64; 512]); EPT_PDPT_ENTRIES];

// EPTP list for VMFUNC leaf-0 (EPTP switching): 512 slots × 8 bytes = one 4K page.
// Slot 0 = current identity EPT; remaining slots zero (unused).
#[repr(C, align(4096))]
pub struct EptpList(pub [u64; 512]);

pub static mut EPTP_LIST: EptpList = EptpList([0u64; 512]);

pub fn init_eptp_list(slot0_eptp: u64) -> Result<u64, &'static str> {
    let list = unsafe { core::ptr::addr_of_mut!(EPTP_LIST.0) };
    unsafe {
        (*list)[0] = slot0_eptp;
    }
    kernel_va_to_pa(list as u64).ok_or("eptp list pa")
}

pub static mut GUEST_PML4: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_LOW_PDPT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_LOW_PD: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_LOW_PTS: [GuestPage; GUEST_LOW_PT_COUNT] =
    [GuestPage([0u64; 512]); GUEST_LOW_PT_COUNT];
pub static mut GUEST_HIGH_PDPT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_HIGH_PD: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_HEAP_PDPT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_HEAP_PDS: [GuestPage; GUEST_HEAP_PD_COUNT] =
    [GuestPage([0u64; 512]); GUEST_HEAP_PD_COUNT];
pub static mut GUEST_IMAGE_PTS: [GuestPage; GUEST_HIGH_IMAGE_PT_COUNT] =
    [GuestPage([0u64; 512]); GUEST_HIGH_IMAGE_PT_COUNT];
pub static mut GUEST_CODE_PT: GuestPage = GuestPage([0u64; 512]);

#[derive(Copy, Clone)]
struct GuestStackBacking {
    arena: Option<HeapArena>,
    active_bytes: usize,
}

static GUEST_STACK_BACKING: Mutex<GuestStackBacking> = Mutex::new(GuestStackBacking {
    arena: None,
    active_bytes: GUEST_STACK_DEFAULT_BYTES,
});

#[derive(Copy, Clone)]
pub struct Vm1SnapshotMeta {
    pub guest_cr3: u64,
    pub guest_rip: u64,
    pub guest_rsp: u64,
    pub code_base: u64,
    pub code_len: u64,
    pub exit_reason: u64,
    pub exit_qualification: u64,
    pub exit_guest_rip: u64,
}

pub static VM1_SNAPSHOT_META: Mutex<Option<Vm1SnapshotMeta>> = Mutex::new(None);
pub static VM1_RESTORE_META: Mutex<Option<Vm1SnapshotMeta>> = Mutex::new(None);

unsafe extern "C" {
    static kernel_end: u8;
}

pub fn build_ept_identity_4g() -> Result<u64, &'static str> {
    let pml4 = unsafe { core::ptr::addr_of_mut!(EPT_PML4.0) };
    let pdpt = unsafe { core::ptr::addr_of_mut!(EPT_PDPT.0) };
    unsafe {
        core::ptr::write_bytes(pml4 as *mut u8, 0, PAGE_SIZE_4K);
        core::ptr::write_bytes(pdpt as *mut u8, 0, PAGE_SIZE_4K);
    }
    for i in 0..EPT_PDPT_ENTRIES {
        let pd = unsafe { core::ptr::addr_of_mut!(EPT_PD[i].0) };
        unsafe { core::ptr::write_bytes(pd as *mut u8, 0, PAGE_SIZE_4K) };
    }

    let pml4_pa = kernel_va_to_pa(pml4 as u64).ok_or("ept pml4 pa")?;
    let pdpt_pa = kernel_va_to_pa(pdpt as u64).ok_or("ept pdpt pa")?;
    unsafe {
        (*pml4)[0] = (pdpt_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
    }

    for i in 0..EPT_PDPT_ENTRIES {
        let pd = unsafe { core::ptr::addr_of!(EPT_PD[i].0) };
        let pd_pa = kernel_va_to_pa(pd as u64).ok_or("ept pd pa")?;
        unsafe {
            (*pdpt)[i] = (pd_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
        }
        for j in 0..EPT_PD_ENTRIES {
            let gpa = ((i as u64) << 30) | ((j as u64) << 21);
            let pde = (gpa & 0x000F_FFFF_FFE0_0000) | 0x7 | (1 << 7) | (6 << 3);
            unsafe {
                (*core::ptr::addr_of_mut!(EPT_PD[i].0))[j] = pde;
            }
        }
    }

    let eptp = (pml4_pa & 0x000F_FFFF_FFFF_F000) | 6 | (3 << 3);
    hvlogf(format_args!("hv: vm1 reporting: ept v1 identity map ready eptp=0x{:016X}", eptp));
    Ok(eptp)
}

pub fn guest_launch_rip() -> u64 {
    crate::hv::guest::entry as *const () as usize as u64
}

#[inline]
pub const fn guest_stack_default_mb() -> usize {
    GUEST_STACK_DEFAULT_MIB
}

#[inline]
pub const fn clamp_guest_stack_mb(stack_mb: usize) -> usize {
    if stack_mb < GUEST_STACK_MIN_MIB {
        GUEST_STACK_MIN_MIB
    } else if stack_mb > GUEST_STACK_MAX_MIB {
        GUEST_STACK_MAX_MIB
    } else {
        stack_mb
    }
}

#[inline]
fn mib_to_bytes(mib: usize) -> usize {
    mib.saturating_mul(1024 * 1024)
}

pub fn active_guest_stack_bytes() -> usize {
    GUEST_STACK_BACKING.lock().active_bytes
}

pub fn active_guest_stack_mb() -> usize {
    active_guest_stack_bytes() / (1024 * 1024)
}

fn active_guest_stack_arena() -> Option<HeapArena> {
    GUEST_STACK_BACKING.lock().arena
}

pub fn guest_stack_slice() -> Option<&'static [u8]> {
    let backing = *GUEST_STACK_BACKING.lock();
    let arena = backing.arena?;
    Some(unsafe { core::slice::from_raw_parts(arena.virt_start as *const u8, backing.active_bytes) })
}

pub fn guest_stack_mut_ptr() -> Option<*mut u8> {
    let backing = *GUEST_STACK_BACKING.lock();
    backing.arena.map(|arena| arena.virt_start as *mut u8)
}

pub fn prepare_guest_stack_mb(stack_mb: usize) -> Result<usize, &'static str> {
    prepare_guest_stack_bytes(mib_to_bytes(clamp_guest_stack_mb(stack_mb)))
}

pub fn prepare_guest_stack_bytes(requested_bytes: usize) -> Result<usize, &'static str> {
    let bytes = requested_bytes
        .max(GUEST_STACK_MIN_BYTES)
        .min(GUEST_STACK_MAX_BYTES);
    let arena = crate::phys::reserve_heap_arena(bytes, PAGE_SIZE_2M as usize)
        .ok_or("guest stack alloc")?;
    unsafe {
        core::ptr::write_bytes(arena.virt_start as *mut u8, 0, bytes);
    }

    let old = {
        let mut backing = GUEST_STACK_BACKING.lock();
        let old = backing.arena;
        backing.arena = Some(arena);
        backing.active_bytes = bytes;
        old
    };

    if let Some(old) = old {
        let _ = crate::phys::free_phys_range(old.phys_start, old.length);
    }

    Ok(bytes)
}

pub fn guest_stack_top() -> u64 {
    (GUEST_STACK_VA_BASE + active_guest_stack_bytes() as u64) & !0xF
}

pub fn guest_kernel_elf_entry(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < ELF64_HEADER_LEN {
        return None;
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return None;
    }
    if bytes.get(4).copied()? != 2 || bytes.get(5).copied()? != 1 {
        return None;
    }
    let raw: [u8; 8] = bytes.get(24..32)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

pub fn build_guest_cr3(guest_rip: u64, guest_rsp: u64) -> Result<u64, &'static str> {
    build_guest_cr3_with_mode(guest_rip, guest_rsp, crate::hv::VmBootMode::Hull)
}

pub fn build_guest_cr3_with_mode(
    guest_rip: u64,
    guest_rsp: u64,
    boot_mode: crate::hv::VmBootMode,
) -> Result<u64, &'static str> {
    unsafe {
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_PML4.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PD.0));
        for i in 0..GUEST_LOW_PT_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PTS[i].0));
        }
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HEAP_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_CODE_PT.0));
        for i in 0..GUEST_HEAP_PD_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!(GUEST_HEAP_PDS[i].0));
        }
        for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[i].0));
        }

        let pml4_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_PML4.0) as u64).ok_or("guest pml4 pa")?;
        let low_pdpt_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PDPT.0) as u64)
            .ok_or("guest low pdpt pa")?;
        let low_pd_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PD.0) as u64).ok_or("guest low pd pa")?;
        let high_pdpt_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_HIGH_PDPT.0) as u64)
            .ok_or("guest high pdpt pa")?;
        let high_pd_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_HIGH_PD.0) as u64)
            .ok_or("guest high pd pa")?;
        let code_pt_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_CODE_PT.0) as u64)
            .ok_or("guest code pt pa")?;

        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_PML4.0),
            pml4_index(GUEST_STACK_VA_BASE),
            low_pdpt_pa,
        );
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0),
            pdpt_index(GUEST_STACK_VA_BASE),
            low_pd_pa,
        );
        let stack = active_guest_stack_arena().ok_or("guest stack backing")?;
        let stack_bytes = active_guest_stack_bytes();
        let stack_pt_count = stack_bytes.div_ceil(PAGE_SIZE_2M as usize);
        let stack_pa = stack.phys_start;
        let mut stack_va = page_align_down(GUEST_STACK_VA_BASE);
        let mut stack_pa_cur = stack_pa;
        let mut stack_left = stack_bytes;
        for i in 0..stack_pt_count {
            let low_pt = core::ptr::addr_of_mut!(GUEST_LOW_PTS[i].0);
            let low_pt_pa = kernel_va_to_pa(low_pt as u64).ok_or("guest low pt pa")?;
            map_table_entry(core::ptr::addr_of_mut!(GUEST_LOW_PD.0), pd_index(stack_va), low_pt_pa);
            let chunk_bytes = core::cmp::min(stack_left, PAGE_SIZE_2M as usize);
            map_region_4k(
                low_pt,
                stack_va,
                stack_pa_cur,
                chunk_bytes,
                PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
            )?;
            stack_va = stack_va
                .checked_add(PAGE_SIZE_2M)
                .ok_or("guest stack span overflow")?;
            stack_pa_cur = stack_pa_cur
                .checked_add(chunk_bytes as u64)
                .ok_or("guest stack pa overflow")?;
            stack_left -= chunk_bytes;
        }

        // Map comm page at a fixed VA above the maximum supported stack span so
        // guest-side helpers can keep using a stable address.
        let comm_pa = crate::hv::vmcall::pa().ok_or("comm page pa")?;
        let comm_pt = core::ptr::addr_of_mut!(GUEST_LOW_PTS[GUEST_STACK_PT_CAP].0);
        let comm_pt_pa = kernel_va_to_pa(comm_pt as u64).ok_or("comm page pt pa")?;
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_LOW_PD.0),
            pd_index(crate::hv::vmcall::comm_page_guest_va()),
            comm_pt_pa,
        );
        (*comm_pt)[pt_index(crate::hv::vmcall::comm_page_guest_va())] =
            (comm_pa & 0x000F_FFFF_FFFF_F000) | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;

        let code_base = page_align_down(guest_rip);
        let code_pt_base = page_align_down_2m(guest_rip);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_PML4.0), pml4_index(code_base), high_pdpt_pa);
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0),
            pdpt_index(code_base),
            high_pd_pa,
        );
        map_table_entry(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0), pd_index(code_base), code_pt_pa);
        map_region_4k(
            core::ptr::addr_of_mut!(GUEST_CODE_PT.0),
            code_pt_base,
            kernel_va_to_pa(code_pt_base).ok_or("guest code pa")?,
            PAGE_SIZE_2M as usize,
            PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
        )?;
        let mut pt_slot = 0usize;
        let (mapped_code_base, mapped_code_len) = match boot_mode {
            crate::hv::VmBootMode::Hull => {
                let layout = crate::hv::guest::hull_image_layout();
                hvlogf(format_args!(
                    "hv: vm1 reporting: hull sections text=[0x{:016X}..0x{:016X}) rodata=[0x{:016X}..0x{:016X}) bss=[0x{:016X}..0x{:016X})",
                    layout.text_start,
                    layout.text_end,
                    layout.rodata_start,
                    layout.rodata_end,
                    layout.bss_start,
                    layout.bss_end
                ));
                hvlogf(format_args!(
                    "hv: vm1 reporting: hull bss anchors vmcall=[0x{:016X}..0x{:016X}) vpanic=[0x{:016X}..0x{:016X}) demo=[0x{:016X}..0x{:016X})",
                    layout.vmcall_bss_start,
                    layout.vmcall_bss_end,
                    layout.vpanic_bss_start,
                    layout.vpanic_bss_end,
                    layout.demo_bss_start,
                    layout.demo_bss_end
                ));
                let (_, end) = crate::hv::guest::hull_image_bounds();
                let start = kernel_image_start_va().ok_or("guest kernel image base")?;
                map_guest_image_span(
                    core::ptr::addr_of_mut!(GUEST_HIGH_PD.0),
                    code_pt_base,
                    start,
                    end,
                    "hull",
                    &mut pt_slot,
                )?;
                let actual_len = end.saturating_sub(start);
                (start, actual_len)
            }
            crate::hv::VmBootMode::Full => {
                map_guest_kernel_image(
                    core::ptr::addr_of_mut!(GUEST_HIGH_PD.0),
                    code_pt_base,
                    &mut pt_slot,
                )?;
                let start = kernel_image_start_va().ok_or("guest kernel image base")?;
                let end = kernel_image_end_va();
                let actual_len = end.saturating_sub(start);
                (start, actual_len)
            }
        };

        hvlogf(format_args!(
            "hv: vm1 reporting: image map done pt_used={}",
            pt_slot
        ));
        map_guest_heap_span(
            core::ptr::addr_of_mut!(GUEST_PML4.0),
            &mut pt_slot,
        )?;
        hvlogf(format_args!(
            "hv: vm1 reporting: heap map done pt_used={}",
            pt_slot
        ));

        hvlogf(format_args!(
            "hv: vm1 reporting: guest-cr3=0x{:016X} code=0x{:016X} stack=0x{:016X} stack_mib={}",
            pml4_pa,
            guest_rip,
            guest_rsp,
            stack_bytes / (1024 * 1024)
        ));
        log_guest_mapping("stack-base", GUEST_STACK_VA_BASE);
        log_guest_mapping("stack-top-8", guest_rsp.saturating_sub(8));
        log_guest_mapping("stack-top-0x40", guest_rsp.saturating_sub(0x40));
        log_guest_mapping("comm-page", crate::hv::vmcall::comm_page_guest_va());
        log_guest_mapping("guest-rip", guest_rip);
        let heap = crate::allocators::heap_stats();
        log_guest_mapping("heap-start", heap.heap_start as u64);
        log_guest_mapping("heap-end-8", (heap.heap_end as u64).saturating_sub(8));
        verify_guest_mapping_chain("guest-rip", guest_rip)?;
        verify_guest_mapping_chain("image-start", mapped_code_base)?;
        verify_guest_mapping_chain(
            "image-late-75pct",
            mapped_code_base.saturating_add((mapped_code_len / 4) * 3),
        )?;
        verify_guest_mapping_chain(
            "image-end-8",
            mapped_code_base.saturating_add(mapped_code_len).saturating_sub(8),
        )?;
        *VM1_SNAPSHOT_META.lock() = Some(Vm1SnapshotMeta {
            guest_cr3: pml4_pa,
            guest_rip,
            guest_rsp,
            code_base: mapped_code_base,
            code_len: mapped_code_len,
            exit_reason: 0,
            exit_qualification: 0,
            exit_guest_rip: guest_rip,
        });
        Ok(pml4_pa)
    }
}

pub fn active_restore_meta() -> Option<Vm1SnapshotMeta> {
    *VM1_RESTORE_META.lock()
}

pub fn current_guest_cr3_pa() -> Result<u64, &'static str> {
    kernel_va_to_pa(unsafe { core::ptr::addr_of!(GUEST_PML4.0) as u64 }).ok_or("guest pml4 pa")
}

unsafe fn read_guest_page_entry(page: *const [u64; 512], index: usize) -> u64 {
    if index >= 512 {
        return 0;
    }

    let base = page.cast::<u64>();
    unsafe { core::ptr::read_volatile(base.add(index)) }
}

pub fn log_guest_mapping(label: &str, guest_va: u64) {
    let low_half = pml4_index(guest_va) == pml4_index(GUEST_STACK_VA_BASE);
    let pml4e =
        unsafe { read_guest_page_entry(core::ptr::addr_of!(GUEST_PML4.0), pml4_index(guest_va)) };
    let pdpte = unsafe {
        if pml4e & PT_ENTRY_PRESENT == 0 {
            0
        } else if low_half {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_LOW_PDPT.0), pdpt_index(guest_va))
        } else {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_HIGH_PDPT.0), pdpt_index(guest_va))
        }
    };

    let (pde, pte) = unsafe {
        if low_half {
            let pde = read_guest_page_entry(core::ptr::addr_of!(GUEST_LOW_PD.0), pd_index(guest_va));
            let pte = if pde & PT_ENTRY_PRESENT != 0 {
                read_guest_low_pt_entry(guest_va, pde)
            } else {
                0
            };
            (pde, pte)
        } else {
            let pde =
                read_guest_page_entry(core::ptr::addr_of!(GUEST_HIGH_PD.0), pd_index(guest_va));
            let pte = if pde & PT_ENTRY_PRESENT != 0 {
                read_guest_high_pt_entry(guest_va, pde)
            } else {
                0
            };
            (pde, pte)
        }
    };

    hvlogf(format_args!(
        "hv: vm1 reporting: guest-map {} va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
        label,
        guest_va,
        classify_guest_va(guest_va),
        pml4_index(guest_va),
        pdpt_index(guest_va),
        pd_index(guest_va),
        pt_index(guest_va),
        pml4e,
        pdpte,
        pde,
        pte
    ));
}

fn verify_guest_mapping_chain(label: &str, guest_va: u64) -> Result<(), &'static str> {
    let low_half = pml4_index(guest_va) == pml4_index(GUEST_STACK_VA_BASE);
    let pml4e =
        unsafe { read_guest_page_entry(core::ptr::addr_of!(GUEST_PML4.0), pml4_index(guest_va)) };
    if pml4e & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: guest-verify {} broken=pml4 va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X}",
            label,
            guest_va,
            classify_guest_va(guest_va),
            pml4_index(guest_va),
            pdpt_index(guest_va),
            pd_index(guest_va),
            pt_index(guest_va),
            pml4e
        ));
        return Err("guest verify pml4");
    }

    let pdpte = unsafe {
        if low_half {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_LOW_PDPT.0), pdpt_index(guest_va))
        } else {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_HIGH_PDPT.0), pdpt_index(guest_va))
        }
    };
    if pdpte & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: guest-verify {} broken=pdpt va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X}",
            label,
            guest_va,
            classify_guest_va(guest_va),
            pml4_index(guest_va),
            pdpt_index(guest_va),
            pd_index(guest_va),
            pt_index(guest_va),
            pml4e,
            pdpte
        ));
        return Err("guest verify pdpt");
    }

    let pde = unsafe {
        if low_half {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_LOW_PD.0), pd_index(guest_va))
        } else {
            read_guest_page_entry(core::ptr::addr_of!(GUEST_HIGH_PD.0), pd_index(guest_va))
        }
    };
    if pde & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: guest-verify {} broken=pd va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X}",
            label,
            guest_va,
            classify_guest_va(guest_va),
            pml4_index(guest_va),
            pdpt_index(guest_va),
            pd_index(guest_va),
            pt_index(guest_va),
            pml4e,
            pdpte,
            pde
        ));
        return Err("guest verify pd");
    }

    let pte = unsafe {
        if low_half {
            read_guest_low_pt_entry(guest_va, pde)
        } else {
            read_guest_high_pt_entry(guest_va, pde)
        }
    };
    if pte & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: guest-verify {} broken=pt va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
            label,
            guest_va,
            classify_guest_va(guest_va),
            pml4_index(guest_va),
            pdpt_index(guest_va),
            pd_index(guest_va),
            pt_index(guest_va),
            pml4e,
            pdpte,
            pde,
            pte
        ));
        return Err("guest verify pt");
    }

    hvlogf(format_args!(
        "hv: vm1 reporting: guest-verify {} ok va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
        label,
        guest_va,
        classify_guest_va(guest_va),
        pml4_index(guest_va),
        pdpt_index(guest_va),
        pd_index(guest_va),
        pt_index(guest_va),
        pml4e,
        pdpte,
        pde,
        pte
    ));
    Ok(())
}

fn classify_guest_va(guest_va: u64) -> &'static str {
    if guest_va >= GUEST_STACK_VA_BASE
        && guest_va < GUEST_STACK_VA_BASE.saturating_add(active_guest_stack_bytes() as u64)
    {
        return "stack";
    }

    if guest_va >= crate::hv::vmcall::comm_page_guest_va()
        && guest_va < crate::hv::vmcall::comm_page_guest_va().saturating_add(PAGE_SIZE_4K as u64)
    {
        return "comm-page";
    }

    let heap = crate::allocators::heap_stats();
    if heap.initialized && guest_va >= heap.heap_start as u64 && guest_va < heap.heap_end as u64 {
        return "heap";
    }

    if let Some(region) = classify_hull_guest_va(guest_va) {
        return region;
    }

    if let Some(meta) = *VM1_SNAPSHOT_META.lock() {
        let code_end = meta.code_base.saturating_add(meta.code_len);
        if guest_va >= meta.code_base && guest_va < code_end {
            return "image-window";
        }
    }

    "unclassified"
}

fn classify_hull_guest_va(guest_va: u64) -> Option<&'static str> {
    let layout = crate::hv::guest::hull_image_layout();
    if guest_va >= layout.text_start && guest_va < layout.text_end {
        return Some("hull-text");
    }
    if guest_va >= layout.rodata_start && guest_va < layout.rodata_end {
        return Some("hull-rodata");
    }
    if guest_va >= layout.vmcall_bss_start && guest_va < layout.vmcall_bss_end {
        return Some("hull-bss-vmcall");
    }
    if guest_va >= layout.vpanic_bss_start && guest_va < layout.vpanic_bss_end {
        return Some("hull-bss-vpanic");
    }
    if guest_va >= layout.demo_bss_start && guest_va < layout.demo_bss_end {
        return Some("hull-bss-demo");
    }
    if guest_va >= layout.bss_start && guest_va < layout.bss_end {
        return Some("hull-bss");
    }
    None
}

pub fn kernel_va_to_pa(va: u64) -> Option<u64> {
    let (virt_base, phys_base) = crate::limine::executable_address_bases()?;
    let offset = va.checked_sub(virt_base)?;
    phys_base.checked_add(offset)
}

fn map_table_entry(table: *mut [u64; 512], index: usize, next_pa: u64) {
    unsafe {
        (*table)[index] = (next_pa & 0x000F_FFFF_FFFF_F000) | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;
    }
}

fn map_region_4k(
    pt: *mut [u64; 512],
    virt_base: u64,
    phys_base: u64,
    bytes: usize,
    flags: u64,
) -> Result<(), &'static str> {
    let pages = bytes.div_ceil(PAGE_SIZE_4K);
    let first_pt = pt_index(virt_base);
    if first_pt + pages > 512 {
        return Err("guest pt range");
    }
    for page in 0..pages {
        let phys = phys_base
            .checked_add((page * PAGE_SIZE_4K) as u64)
            .ok_or("guest phys overflow")?;
        unsafe {
            (*pt)[first_pt + page] = (phys & 0x000F_FFFF_FFFF_F000) | flags;
        }
    }
    Ok(())
}

fn page_align_down(addr: u64) -> u64 {
    addr & !((PAGE_SIZE_4K as u64) - 1)
}

fn page_align_down_2m(addr: u64) -> u64 {
    addr & !(PAGE_SIZE_2M - 1)
}

fn page_align_up_2m(addr: u64) -> u64 {
    if addr & (PAGE_SIZE_2M - 1) == 0 {
        addr
    } else {
        (addr + PAGE_SIZE_2M) & !(PAGE_SIZE_2M - 1)
    }
}

fn kernel_image_start_va() -> Option<u64> {
    let (virt_base, _) = crate::limine::executable_address_bases()?;
    Some(virt_base)
}

fn kernel_image_end_va() -> u64 {
    unsafe { core::ptr::addr_of!(kernel_end) as u64 }
}

pub fn map_guest_kernel_image(
    pd: *mut [u64; 512],
    code_pt_base: u64,
    pt_slot: &mut usize,
) -> Result<(), &'static str> {
    let start = kernel_image_start_va().ok_or("guest kernel image base")?;
    let end = kernel_image_end_va();
    map_guest_image_span(pd, code_pt_base, start, end, "full", pt_slot)
}

fn map_guest_image_span(
    pd: *mut [u64; 512],
    code_pt_base: u64,
    start: u64,
    end: u64,
    label: &str,
    pt_slot: &mut usize,
) -> Result<(), &'static str> {
    let start_chunk_base = page_align_down_2m(start);
    let end_aligned = page_align_up_2m(end);
    let span_bytes = end_aligned.saturating_sub(start_chunk_base);
    let total_chunks = (span_bytes / PAGE_SIZE_2M) as usize;
    let extra_chunks = total_chunks.saturating_sub(1);
    hvlogf(format_args!(
        "hv: vm1 reporting: {} image map start=0x{:016X} end=0x{:016X} span_mib={} extra_pts={} cap={} max_mib={}",
        label,
        start,
        end,
        span_bytes / (1024 * 1024),
        extra_chunks,
        GUEST_HIGH_IMAGE_PT_COUNT,
        GUEST_HIGH_IMAGE_MAX_BYTES / (1024 * 1024)
    ));
    if pml4_index(start) != pml4_index(code_pt_base)
        || pdpt_index(start) != pdpt_index(code_pt_base)
        || pml4_index(end.saturating_sub(1)) != pml4_index(code_pt_base)
        || pdpt_index(end.saturating_sub(1)) != pdpt_index(code_pt_base)
    {
        return Err("guest kernel image range");
    }

    let mut va = start_chunk_base;
    let mut mapped_chunks = 0usize;
    while va < end_aligned {
        if va != code_pt_base {
            if *pt_slot >= GUEST_HIGH_IMAGE_PT_COUNT {
                return Err("guest image pt pool");
            }

            let chunk_start = if va < start { start } else { va };
            let chunk_end = core::cmp::min(va.saturating_add(PAGE_SIZE_2M), end);
            if chunk_start < chunk_end {
                let image_pt = unsafe { core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[*pt_slot].0) };
                let image_pt_pa = kernel_va_to_pa(image_pt as u64).ok_or("guest image pt pa")?;
                map_table_entry(pd, pd_index(va), image_pt_pa);
                let phys = kernel_va_to_pa(chunk_start).ok_or("guest kernel image pa")?;
                map_region_4k(
                    image_pt,
                    chunk_start,
                    phys,
                    chunk_end.saturating_sub(chunk_start) as usize,
                    PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
                )?;
                *pt_slot += 1;
                mapped_chunks += 1;
            }
        }
        va = va
            .checked_add(PAGE_SIZE_2M)
            .ok_or("guest exec span overflow")?;
    }
    Ok(())
}

fn map_guest_heap_span(
    pml4: *mut [u64; 512],
    pt_slot: &mut usize,
) -> Result<(), &'static str> {
    let heap = crate::allocators::heap_stats();
    if !heap.initialized || heap.heap_start == 0 || heap.heap_end <= heap.heap_start {
        return Ok(());
    }

    let start = heap.heap_start as u64;
    let end = heap.heap_end as u64;
    if pml4_index(start) != pml4_index(end.saturating_sub(1)) {
        return Err("guest heap pml4 range");
    }

    let heap_pdpt = unsafe { core::ptr::addr_of_mut!(GUEST_HEAP_PDPT.0) };
    let heap_pdpt_pa = kernel_va_to_pa(heap_pdpt as u64).ok_or("guest heap pdpt pa")?;
    map_table_entry(pml4, pml4_index(start), heap_pdpt_pa);

    let start_chunk_base = page_align_down_2m(start);
    let end_aligned = page_align_up_2m(end);
    let mut heap_pd_slots = [usize::MAX; 512];
    let mut heap_pd_count = 0usize;
    let total_chunks = ((end_aligned.saturating_sub(start_chunk_base)) / PAGE_SIZE_2M) as usize;
    let mut mapped_chunks = 0usize;

    let mut va = start_chunk_base;
    while va < end_aligned {
        if *pt_slot >= GUEST_HIGH_IMAGE_PT_COUNT {
            return Err("guest image pt pool");
        }
        let pdpt_idx = pdpt_index(va);
        let pd_slot = if heap_pd_slots[pdpt_idx] != usize::MAX {
            heap_pd_slots[pdpt_idx]
        } else {
            if heap_pd_count >= GUEST_HEAP_PD_COUNT {
                return Err("guest heap pd pool");
            }
            let slot = heap_pd_count;
            let heap_pd = unsafe { core::ptr::addr_of_mut!(GUEST_HEAP_PDS[slot].0) };
            let heap_pd_pa = kernel_va_to_pa(heap_pd as u64).ok_or("guest heap pd pa")?;
            map_table_entry(heap_pdpt, pdpt_idx, heap_pd_pa);
            heap_pd_slots[pdpt_idx] = slot;
            heap_pd_count += 1;
            slot
        };

        let chunk_start = if va < start { start } else { va };
        let chunk_end = core::cmp::min(va.saturating_add(PAGE_SIZE_2M), end);
        if chunk_start < chunk_end {
            let heap_pd = unsafe { core::ptr::addr_of_mut!(GUEST_HEAP_PDS[pd_slot].0) };
            let heap_pt = unsafe { core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[*pt_slot].0) };
            let heap_pt_pa = kernel_va_to_pa(heap_pt as u64).ok_or("guest heap pt pa")?;
            map_table_entry(heap_pd, pd_index(va), heap_pt_pa);
            let phys = host_va_to_pa(chunk_start).ok_or("guest heap pa")?;
            map_region_4k(
                heap_pt,
                chunk_start,
                phys,
                chunk_end.saturating_sub(chunk_start) as usize,
                PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
            )?;
            *pt_slot += 1;
            mapped_chunks += 1;
        }

        va = va
            .checked_add(PAGE_SIZE_2M)
            .ok_or("guest heap span overflow")?;
    }

    hvlogf(format_args!(
        "hv: vm1 reporting: heap map start=0x{:016X} end=0x{:016X} span_mib={} pt_cap={} pt_used={}",
        start,
        end,
        end.saturating_sub(start) / (1024 * 1024),
        GUEST_HIGH_IMAGE_PT_COUNT,
        *pt_slot
    ));
    Ok(())
}

fn read_guest_high_pt_entry(guest_va: u64, pde: u64) -> u64 {
    let Ok(code_pt_pa) = current_high_pt_pa(unsafe { core::ptr::addr_of!(GUEST_CODE_PT.0) as u64 }) else {
        return 0;
    };
    if pde_addr(pde) == code_pt_pa {
        return unsafe { read_guest_page_entry(core::ptr::addr_of!(GUEST_CODE_PT.0), pt_index(guest_va)) };
    }

    for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
        let image_pt = unsafe { core::ptr::addr_of!(GUEST_IMAGE_PTS[i].0) as u64 };
        let Ok(image_pt_pa) = current_high_pt_pa(image_pt) else {
            continue;
        };
        if pde_addr(pde) == image_pt_pa {
            return unsafe {
                read_guest_page_entry(core::ptr::addr_of!(GUEST_IMAGE_PTS[i].0), pt_index(guest_va))
            };
        }
    }

    0
}

fn read_guest_low_pt_entry(guest_va: u64, pde: u64) -> u64 {
    for i in 0..GUEST_LOW_PT_COUNT {
        let low_pt = unsafe { core::ptr::addr_of!(GUEST_LOW_PTS[i].0) as u64 };
        let Ok(low_pt_pa) = current_high_pt_pa(low_pt) else {
            continue;
        };
        if pde_addr(pde) == low_pt_pa {
            return unsafe {
                read_guest_page_entry(core::ptr::addr_of!(GUEST_LOW_PTS[i].0), pt_index(guest_va))
            };
        }
    }

    0
}

fn current_high_pt_pa(va: u64) -> Result<u64, &'static str> {
    kernel_va_to_pa(va).ok_or("guest high pt pa")
}

fn host_va_to_pa(va: u64) -> Option<u64> {
    crate::phys::virt_to_phys_checked(va as *const u8).or_else(|| kernel_va_to_pa(va))
}

fn pde_addr(pde: u64) -> u64 {
    pde & 0x000F_FFFF_FFFF_F000
}

fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1FF) as usize
}

fn pdpt_index(addr: u64) -> usize {
    ((addr >> 30) & 0x1FF) as usize
}

fn pd_index(addr: u64) -> usize {
    ((addr >> 21) & 0x1FF) as usize
}

fn pt_index(addr: u64) -> usize {
    ((addr >> 12) & 0x1FF) as usize
}

unsafe fn zero_guest_page(page: *mut [u64; 512]) {
    core::ptr::write_bytes(page as *mut u8, 0, PAGE_SIZE_4K);
}

pub unsafe fn copy_into_guest_page(dst: *mut [u64; 512], src: &[u8]) {
    core::ptr::copy_nonoverlapping(src.as_ptr(), dst.cast::<u8>(), PAGE_SIZE_4K);
}

pub unsafe fn push_guest_page(out: &mut alloc::vec::Vec<u8>, page: *const [u64; 512]) {
    out.extend_from_slice(core::slice::from_raw_parts(page.cast::<u8>(), PAGE_SIZE_4K));
}
