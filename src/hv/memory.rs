use core::mem::size_of;

use super::hvlogf;
use spin::Mutex;

// Guest memory constants
pub const PAGE_SIZE_4K: usize = 4096;
pub const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const GUEST_STACK_VA_BASE: u64 = 0x0000_0000_0040_0000;
pub const GUEST_STACK_BYTES: usize = 64 * 1024;
pub const GUEST_CODE_WINDOW_BYTES: usize = 64 * 1024;
pub const GUEST_HIGH_IMAGE_PT_COUNT: usize = 16;
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

#[repr(align(16))]
pub struct GuestStack(pub [u8; GUEST_STACK_BYTES]);

static mut EPT_PML4: EptPage = EptPage([0u64; 512]);
static mut EPT_PDPT: EptPage = EptPage([0u64; 512]);
static mut EPT_PD: [EptPage; EPT_PDPT_ENTRIES] = [EptPage([0u64; 512]); EPT_PDPT_ENTRIES];

pub static mut GUEST_PML4: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_LOW_PDPT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_LOW_PD: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_STACK_PT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_HIGH_PDPT: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_HIGH_PD: GuestPage = GuestPage([0u64; 512]);
pub static mut GUEST_IMAGE_PTS: [GuestPage; GUEST_HIGH_IMAGE_PT_COUNT] =
    [GuestPage([0u64; 512]); GUEST_HIGH_IMAGE_PT_COUNT];
pub static mut GUEST_CODE_PT: GuestPage = GuestPage([0u64; 512]);
pub static mut VM1_GUEST_STACK: GuestStack = GuestStack([0u8; GUEST_STACK_BYTES]);

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
    hvlogf(format_args!(
        "hv: vm1 reporting: ept v1 identity map ready eptp=0x{:016X}",
        eptp
    ));
    Ok(eptp)
}

pub fn guest_launch_rip() -> u64 {
    crate::hv::guest::entry as *const () as usize as u64
}

pub fn guest_stack_top() -> u64 {
    (GUEST_STACK_VA_BASE + GUEST_STACK_BYTES as u64) & !0xF
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
    unsafe {
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_PML4.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PD.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_STACK_PT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_CODE_PT.0));
        for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[i].0));
        }

        let pml4_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_PML4.0) as u64).ok_or("guest pml4 pa")?;
        let low_pdpt_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PDPT.0) as u64)
            .ok_or("guest low pdpt pa")?;
        let low_pd_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PD.0) as u64).ok_or("guest low pd pa")?;
        let stack_pt_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_STACK_PT.0) as u64)
            .ok_or("guest stack pt pa")?;
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
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_LOW_PD.0),
            pd_index(GUEST_STACK_VA_BASE),
            stack_pt_pa,
        );
        map_region_4k(
            core::ptr::addr_of_mut!(GUEST_STACK_PT.0),
            page_align_down(GUEST_STACK_VA_BASE),
            kernel_va_to_pa(core::ptr::addr_of!(VM1_GUEST_STACK.0) as u64)
                .ok_or("guest stack pa")?,
            GUEST_STACK_BYTES,
            PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
        )?;

        // Map comm page immediately after stack top (guest VA 0x410000).
        let comm_pa = crate::hv::vmcall::pa().ok_or("comm page pa")?;
        (*core::ptr::addr_of_mut!(GUEST_STACK_PT.0))
            [pt_index(crate::hv::vmcall::COMM_PAGE_GUEST_VA)] =
            (comm_pa & 0x000F_FFFF_FFFF_F000) | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;

        let code_base = page_align_down(guest_rip);
        let code_pt_base = page_align_down_2m(guest_rip);
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_PML4.0),
            pml4_index(code_base),
            high_pdpt_pa,
        );
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0),
            pdpt_index(code_base),
            high_pd_pa,
        );
        map_table_entry(
            core::ptr::addr_of_mut!(GUEST_HIGH_PD.0),
            pd_index(code_base),
            code_pt_pa,
        );
        map_region_4k(
            core::ptr::addr_of_mut!(GUEST_CODE_PT.0),
            code_pt_base,
            kernel_va_to_pa(code_pt_base).ok_or("guest code pa")?,
            PAGE_SIZE_2M as usize,
            PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
        )?;
        map_guest_kernel_image(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0), code_pt_base)?;

        hvlogf(format_args!(
            "hv: vm1 reporting: guest-cr3=0x{:016X} code=0x{:016X} stack=0x{:016X}",
            pml4_pa, guest_rip, guest_rsp
        ));
        *VM1_SNAPSHOT_META.lock() = Some(Vm1SnapshotMeta {
            guest_cr3: pml4_pa,
            guest_rip,
            guest_rsp,
            code_base,
            code_len: GUEST_CODE_WINDOW_BYTES as u64,
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

pub fn map_guest_kernel_image(pd: *mut [u64; 512], code_pt_base: u64) -> Result<(), &'static str> {
    let start = kernel_image_start_va().ok_or("guest kernel image base")?;
    let end = kernel_image_end_va();
    let start_chunk_base = page_align_down_2m(start);
    if pml4_index(start) != pml4_index(code_pt_base)
        || pdpt_index(start) != pdpt_index(code_pt_base)
        || pml4_index(end.saturating_sub(1)) != pml4_index(code_pt_base)
        || pdpt_index(end.saturating_sub(1)) != pdpt_index(code_pt_base)
    {
        return Err("guest kernel image range");
    }

    let mut pt_slot = 0usize;
    let mut va = start_chunk_base;
    let end_aligned = page_align_up_2m(end);
    while va < end_aligned {
        if va != code_pt_base {
            if pt_slot >= GUEST_HIGH_IMAGE_PT_COUNT {
                return Err("guest image pt pool");
            }

            let chunk_start = if va < start { start } else { va };
            let chunk_end = core::cmp::min(va.saturating_add(PAGE_SIZE_2M), end);
            if chunk_start < chunk_end {
                let image_pt = unsafe { core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[pt_slot].0) };
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
                pt_slot += 1;
            }
        }
        va = va
            .checked_add(PAGE_SIZE_2M)
            .ok_or("guest exec span overflow")?;
    }
    Ok(())
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
