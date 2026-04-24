use super::hvlogf;
use crate::phys::HeapArena;
use spin::Mutex;

#[inline]
fn primary_vm_id() -> u8 {
    crate::hv::snapshot::VM1_ID
}

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
const EPT_ROOT_PML4_INDEX: usize = 0;
const EPT_DYNAMIC_PD_CAP: usize = 16;
// Sparse EPT still maps only the spans we explicitly request, but those spans
// include up to 1 GiB of host heap plus the guest heap, stack, and kernel image.
// At 4 KiB granularity each PT covers 2 MiB, so we need a few hundred PT pages.
const EPT_DYNAMIC_PT_CAP: usize = 1024;

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
static mut EPT_PDS: [EptPage; EPT_DYNAMIC_PD_CAP] = [EptPage([0u64; 512]); EPT_DYNAMIC_PD_CAP];
static mut EPT_PTS: [EptPage; EPT_DYNAMIC_PT_CAP] = [EptPage([0u64; 512]); EPT_DYNAMIC_PT_CAP];

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

#[repr(C, align(4096))]
struct GuestTables {
    pml4: GuestPage,
    low_pdpt: GuestPage,
    low_pd: GuestPage,
    low_pts: [GuestPage; GUEST_LOW_PT_COUNT],
    high_pdpt: GuestPage,
    high_pd: GuestPage,
    heap_pdpt: GuestPage,
    heap_pds: [GuestPage; GUEST_HEAP_PD_COUNT],
    image_pts: [GuestPage; GUEST_HIGH_IMAGE_PT_COUNT],
    code_pt: GuestPage,
}

static GUEST_TABLES: Mutex<Option<usize>> = Mutex::new(None);
static GUEST_TABLES_ARENA: Mutex<Option<HeapArena>> = Mutex::new(None);

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

fn guest_tables_ptr() -> Result<*mut GuestTables, &'static str> {
    let mut ptr_guard = GUEST_TABLES.lock();
    if let Some(ptr) = *ptr_guard {
        return Ok(ptr as *mut GuestTables);
    }

    let arena = crate::phys::reserve_heap_arena(core::mem::size_of::<GuestTables>(), PAGE_SIZE_4K)
        .ok_or("guest tables alloc")?;
    unsafe {
        core::ptr::write_bytes(arena.virt_start as *mut u8, 0, arena.length);
    }
    let ptr = arena.virt_start as *mut GuestTables;
    *GUEST_TABLES_ARENA.lock() = Some(arena);
    *ptr_guard = Some(ptr as usize);
    Ok(ptr)
}

fn guest_tables_ptr_opt() -> Option<*mut GuestTables> {
    (*GUEST_TABLES.lock()).map(|ptr| ptr as *mut GuestTables)
}

fn guest_tables_arena() -> Option<HeapArena> {
    *GUEST_TABLES_ARENA.lock()
}

fn guest_pml4_page() -> Option<*const [u64; 512]> {
    guest_tables_ptr_opt().map(|tables| unsafe { core::ptr::addr_of!((*tables).pml4.0) })
}

fn guest_low_pdpt_page() -> Option<*const [u64; 512]> {
    guest_tables_ptr_opt().map(|tables| unsafe { core::ptr::addr_of!((*tables).low_pdpt.0) })
}

fn guest_low_pd_page() -> Option<*const [u64; 512]> {
    guest_tables_ptr_opt().map(|tables| unsafe { core::ptr::addr_of!((*tables).low_pd.0) })
}

fn guest_high_pdpt_page() -> Option<*const [u64; 512]> {
    guest_tables_ptr_opt().map(|tables| unsafe { core::ptr::addr_of!((*tables).high_pdpt.0) })
}

fn guest_high_pd_page() -> Option<*const [u64; 512]> {
    guest_tables_ptr_opt().map(|tables| unsafe { core::ptr::addr_of!((*tables).high_pd.0) })
}

unsafe extern "C" {
    static kernel_end: u8;
}

pub fn build_ept_identity_4g() -> Result<u64, &'static str> {
    // VMX may need to walk guest paging structures before the guest executes
    // its first instruction, so make sure the backing arena exists before we
    // decide which physical spans sparse EPT must cover.
    let _ = guest_tables_ptr()?;

    let pml4 = unsafe { core::ptr::addr_of_mut!(EPT_PML4.0) };
    let pdpt = unsafe { core::ptr::addr_of_mut!(EPT_PDPT.0) };
    unsafe {
        core::ptr::write_bytes(pml4 as *mut u8, 0, PAGE_SIZE_4K);
        core::ptr::write_bytes(pdpt as *mut u8, 0, PAGE_SIZE_4K);
    }
    for i in 0..EPT_DYNAMIC_PD_CAP {
        let pd = unsafe { core::ptr::addr_of_mut!(EPT_PDS[i].0) };
        unsafe { core::ptr::write_bytes(pd as *mut u8, 0, PAGE_SIZE_4K) };
    }
    for i in 0..EPT_DYNAMIC_PT_CAP {
        let pt = unsafe { core::ptr::addr_of_mut!(EPT_PTS[i].0) };
        unsafe { core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE_4K) };
    }

    let pml4_pa = kernel_va_to_pa(pml4 as u64).ok_or("ept pml4 pa")?;
    let pdpt_pa = kernel_va_to_pa(pdpt as u64).ok_or("ept pdpt pa")?;
    unsafe {
        (*pml4)[EPT_ROOT_PML4_INDEX] = (pdpt_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
    }
    let mut next_pd = 0usize;
    let mut next_pt = 0usize;

    let (kernel_virt_base, kernel_phys_base) =
        crate::limine::executable_address_bases().ok_or("ept kernel image base")?;
    let kernel_phys_end = kernel_phys_base
        .checked_add(kernel_image_end_va().saturating_sub(kernel_virt_base))
        .ok_or("ept kernel image end")?;
    map_ept_identity_span(
        pdpt,
        &mut next_pd,
        &mut next_pt,
        kernel_phys_base,
        kernel_phys_end.saturating_sub(kernel_phys_base),
        "kernel-image",
    )?;

    if let Some(stack) = active_guest_stack_arena() {
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            stack.phys_start,
            stack.length as u64,
            "guest-stack",
        )?;
    }

    if let Some(comm_pa) = crate::hv::vmcall::pa() {
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            comm_pa,
            PAGE_SIZE_4K as u64,
            "comm-page",
        )?;
    }

    if let Some((cpu_slot_table_va, cpu_slot_table_len)) = crate::percpu::cpu_slot_table_span() {
        let cpu_slot_table_pa =
            host_va_to_pa(cpu_slot_table_va as u64).ok_or("ept percpu slot table pa")?;
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            cpu_slot_table_pa,
            cpu_slot_table_len as u64,
            "percpu-slot-table",
        )?;
    }

    if let Some(guest_tables) = guest_tables_arena() {
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            guest_tables.phys_start,
            guest_tables.length as u64,
            "guest-tables",
        )?;
    }

    let host_heap = crate::allocators::heap_stats();
    if host_heap.initialized
        && host_heap.phys_start != 0
        && host_heap.heap_end > host_heap.heap_start
    {
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            host_heap.phys_start as u64,
            host_heap.heap_end.saturating_sub(host_heap.heap_start) as u64,
            "host-heap",
        )?;
    }

    let guest_heap = crate::allocators::hv_guest_heap_stats();
    if guest_heap.initialized
        && guest_heap.phys_start != 0
        && guest_heap.heap_end > guest_heap.heap_start
    {
        map_ept_identity_span(
            pdpt,
            &mut next_pd,
            &mut next_pt,
            guest_heap.phys_start as u64,
            guest_heap.heap_end.saturating_sub(guest_heap.heap_start) as u64,
            "hv-guest-heap",
        )?;
    }

    let eptp = (pml4_pa & 0x000F_FFFF_FFFF_F000) | 6 | (3 << 3);
    hvlogf(format_args!(
        "hv: vm{} reporting: ept v1 sparse map ready eptp=0x{:016X} pd_used={} pt_used={}",
        primary_vm_id(),
        eptp,
        next_pd,
        next_pt,
    ));
    Ok(eptp)
}

fn map_ept_identity_span(
    pdpt: *mut [u64; 512],
    next_pd: &mut usize,
    next_pt: &mut usize,
    phys_start: u64,
    bytes: u64,
    label: &str,
) -> Result<(), &'static str> {
    if bytes == 0 {
        return Ok(());
    }

    let start = page_align_down(phys_start);
    let end = page_align_up_4k(phys_start.checked_add(bytes).ok_or("ept span overflow")?);
    if pml4_index(start) != EPT_ROOT_PML4_INDEX
        || pml4_index(end.saturating_sub(1)) != EPT_ROOT_PML4_INDEX
    {
        return Err("ept pml4 range");
    }

    let mut gpa = start;
    while gpa < end {
        let pdpt_idx = pdpt_index(gpa);
        let pd = ensure_ept_table_entry(
            pdpt,
            pdpt_idx,
            next_pd,
            unsafe { core::ptr::addr_of_mut!(EPT_PDS[0].0) },
            EPT_DYNAMIC_PD_CAP,
            "ept pd pool",
        )?;
        let pt = ensure_ept_table_entry(
            pd,
            pd_index(gpa),
            next_pt,
            unsafe { core::ptr::addr_of_mut!(EPT_PTS[0].0) },
            EPT_DYNAMIC_PT_CAP,
            "ept pt pool",
        )?;
        unsafe {
            (*pt)[pt_index(gpa)] = (gpa & 0x000F_FFFF_FFFF_F000) | 0x7 | (6 << 3);
        }
        gpa = gpa
            .checked_add(PAGE_SIZE_4K as u64)
            .ok_or("ept gpa overflow")?;
    }

    hvlogf(format_args!(
        "hv: vm{} reporting: ept span {} phys=0x{:016X}..0x{:016X}",
        primary_vm_id(),
        label,
        start,
        end
    ));
    Ok(())
}

fn ensure_ept_table_entry(
    table: *mut [u64; 512],
    index: usize,
    next_slot: &mut usize,
    pool_base: *mut [u64; 512],
    pool_cap: usize,
    err: &'static str,
) -> Result<*mut [u64; 512], &'static str> {
    let existing = unsafe { (*table)[index] };
    if existing & 0x7 != 0 {
        let pa = existing & 0x000F_FFFF_FFFF_F000;
        return ept_table_ptr_from_pa(pool_base, pool_cap, pa).ok_or(err);
    }

    if *next_slot >= pool_cap {
        return Err(err);
    }

    let slot_ptr = unsafe { pool_base.add(*next_slot) };
    unsafe {
        core::ptr::write_bytes(slot_ptr as *mut u8, 0, PAGE_SIZE_4K);
    }
    let slot_pa = kernel_va_to_pa(slot_ptr as u64).ok_or(err)?;
    unsafe {
        (*table)[index] = (slot_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
    }
    *next_slot += 1;
    Ok(slot_ptr)
}

fn ept_table_ptr_from_pa(
    pool_base: *mut [u64; 512],
    pool_cap: usize,
    target_pa: u64,
) -> Option<*mut [u64; 512]> {
    let mut i = 0usize;
    while i < pool_cap {
        let slot_ptr = unsafe { pool_base.add(i) };
        let slot_pa = kernel_va_to_pa(slot_ptr as u64)?;
        if slot_pa == target_pa {
            return Some(slot_ptr);
        }
        i += 1;
    }
    None
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
    Some(unsafe {
        core::slice::from_raw_parts(arena.virt_start as *const u8, backing.active_bytes)
    })
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
    let arena =
        crate::phys::reserve_heap_arena(bytes, PAGE_SIZE_2M as usize).ok_or("guest stack alloc")?;
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
        let tables = guest_tables_ptr()?;
        let guest_pml4 = core::ptr::addr_of_mut!((*tables).pml4.0);
        let guest_low_pdpt = core::ptr::addr_of_mut!((*tables).low_pdpt.0);
        let guest_low_pd = core::ptr::addr_of_mut!((*tables).low_pd.0);
        let guest_high_pdpt = core::ptr::addr_of_mut!((*tables).high_pdpt.0);
        let guest_high_pd = core::ptr::addr_of_mut!((*tables).high_pd.0);
        let guest_heap_pdpt = core::ptr::addr_of_mut!((*tables).heap_pdpt.0);
        let guest_code_pt = core::ptr::addr_of_mut!((*tables).code_pt.0);

        zero_guest_page(guest_pml4);
        zero_guest_page(guest_low_pdpt);
        zero_guest_page(guest_low_pd);
        for i in 0..GUEST_LOW_PT_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!((*tables).low_pts[i].0));
        }
        zero_guest_page(guest_high_pdpt);
        zero_guest_page(guest_high_pd);
        zero_guest_page(guest_heap_pdpt);
        zero_guest_page(guest_code_pt);
        for i in 0..GUEST_HEAP_PD_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!((*tables).heap_pds[i].0));
        }
        for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
            zero_guest_page(core::ptr::addr_of_mut!((*tables).image_pts[i].0));
        }

        let pml4_pa = host_va_to_pa(guest_pml4 as u64).ok_or("guest pml4 pa")?;
        let low_pdpt_pa = host_va_to_pa(guest_low_pdpt as u64).ok_or("guest low pdpt pa")?;
        let low_pd_pa = host_va_to_pa(guest_low_pd as u64).ok_or("guest low pd pa")?;
        let high_pdpt_pa = host_va_to_pa(guest_high_pdpt as u64).ok_or("guest high pdpt pa")?;
        let high_pd_pa = host_va_to_pa(guest_high_pd as u64).ok_or("guest high pd pa")?;
        let code_pt_pa = host_va_to_pa(guest_code_pt as u64).ok_or("guest code pt pa")?;

        map_table_entry(guest_pml4, pml4_index(GUEST_STACK_VA_BASE), low_pdpt_pa);
        map_table_entry(guest_low_pdpt, pdpt_index(GUEST_STACK_VA_BASE), low_pd_pa);
        let stack = active_guest_stack_arena().ok_or("guest stack backing")?;
        let stack_bytes = active_guest_stack_bytes();
        let stack_pt_count = stack_bytes.div_ceil(PAGE_SIZE_2M as usize);
        let stack_pa = stack.phys_start;
        let mut stack_va = page_align_down(GUEST_STACK_VA_BASE);
        let mut stack_pa_cur = stack_pa;
        let mut stack_left = stack_bytes;
        for i in 0..stack_pt_count {
            let low_pt = core::ptr::addr_of_mut!((*tables).low_pts[i].0);
            let low_pt_pa = host_va_to_pa(low_pt as u64).ok_or("guest low pt pa")?;
            map_table_entry(guest_low_pd, pd_index(stack_va), low_pt_pa);
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
        let comm_pt = core::ptr::addr_of_mut!((*tables).low_pts[GUEST_STACK_PT_CAP].0);
        let comm_pt_pa = host_va_to_pa(comm_pt as u64).ok_or("comm page pt pa")?;
        map_table_entry(
            guest_low_pd,
            pd_index(crate::hv::vmcall::comm_page_guest_va()),
            comm_pt_pa,
        );
        (*comm_pt)[pt_index(crate::hv::vmcall::comm_page_guest_va())] =
            (comm_pa & 0x000F_FFFF_FFFF_F000) | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;

        let code_base = page_align_down(guest_rip);
        let code_pt_base = page_align_down_2m(guest_rip);
        map_table_entry(guest_pml4, pml4_index(code_base), high_pdpt_pa);
        map_table_entry(guest_high_pdpt, pdpt_index(code_base), high_pd_pa);
        map_table_entry(guest_high_pd, pd_index(code_base), code_pt_pa);
        map_region_4k(
            guest_code_pt,
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
                    "hv: vm{} reporting: hull sections text=[0x{:016X}..0x{:016X}) rodata=[0x{:016X}..0x{:016X}) data=[0x{:016X}..0x{:016X}) bss=[0x{:016X}..0x{:016X})",
                    primary_vm_id(),
                    layout.text_start,
                    layout.text_end,
                    layout.rodata_start,
                    layout.rodata_end,
                    layout.data_start,
                    layout.data_end,
                    layout.bss_start,
                    layout.bss_end
                ));
                hvlogf(format_args!(
                    "hv: vm{} reporting: hull bss anchors vmcall=[0x{:016X}..0x{:016X}) vpanic=[0x{:016X}..0x{:016X}) demo=[0x{:016X}..0x{:016X})",
                    primary_vm_id(),
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
                    guest_high_pd,
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
                map_guest_kernel_image(guest_high_pd, code_pt_base, &mut pt_slot)?;
                let start = kernel_image_start_va().ok_or("guest kernel image base")?;
                let end = kernel_image_end_va();
                let actual_len = end.saturating_sub(start);
                (start, actual_len)
            }
        };

        hvlogf(format_args!(
            "hv: vm{} reporting: image map done pt_used={}",
            primary_vm_id(),
            pt_slot
        ));
        map_guest_heap_span(guest_pml4, &mut pt_slot)?;
        hvlogf(format_args!(
            "hv: vm{} reporting: heap map done pt_used={}",
            primary_vm_id(),
            pt_slot
        ));

        hvlogf(format_args!(
            "hv: vm{} reporting: guest-cr3=0x{:016X} code=0x{:016X} stack=0x{:016X} stack_mib={}",
            primary_vm_id(),
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
            mapped_code_base
                .saturating_add(mapped_code_len)
                .saturating_sub(8),
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
    let Some(pml4) = guest_pml4_page() else {
        return Err("guest pml4 pa");
    };
    host_va_to_pa(pml4 as u64).ok_or("guest pml4 pa")
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
    let Some(pml4) = guest_pml4_page() else {
        return;
    };
    let Some(low_pdpt) = guest_low_pdpt_page() else {
        return;
    };
    let Some(low_pd) = guest_low_pd_page() else {
        return;
    };
    let Some(high_pdpt) = guest_high_pdpt_page() else {
        return;
    };
    let Some(high_pd) = guest_high_pd_page() else {
        return;
    };
    let pml4e = unsafe { read_guest_page_entry(pml4, pml4_index(guest_va)) };
    let _pdpte = unsafe {
        if pml4e & PT_ENTRY_PRESENT == 0 {
            0
        } else if low_half {
            read_guest_page_entry(low_pdpt, pdpt_index(guest_va))
        } else {
            read_guest_page_entry(high_pdpt, pdpt_index(guest_va))
        }
    };

    let (pde, pte) = unsafe {
        if low_half {
            let pde = read_guest_page_entry(low_pd, pd_index(guest_va));
            let pte = if pde & PT_ENTRY_PRESENT != 0 {
                read_guest_low_pt_entry(guest_va, pde)
            } else {
                0
            };
            (pde, pte)
        } else {
            let pde = read_guest_page_entry(high_pd, pd_index(guest_va));
            let pte = if pde & PT_ENTRY_PRESENT != 0 {
                read_guest_high_pt_entry(guest_va, pde)
            } else {
                0
            };
            (pde, pte)
        }
    };

    hvlogf(format_args!(
        "hv: vm{} reporting: guest-map {} va=0x{:016X} region={} idx[pd={},pt={}] pde=0x{:016X} pte=0x{:016X}",
        primary_vm_id(),
        label,
        guest_va,
        classify_guest_va(guest_va),
        pd_index(guest_va),
        pt_index(guest_va),
        pde,
        pte
    ));
}

pub fn log_guest_pt_context(label: &str, guest_va: u64) {
    let low_half = pml4_index(guest_va) == pml4_index(GUEST_STACK_VA_BASE);
    let Some(low_pd) = guest_low_pd_page() else {
        return;
    };
    let Some(high_pd) = guest_high_pd_page() else {
        return;
    };

    let pde = unsafe {
        if low_half {
            read_guest_page_entry(low_pd, pd_index(guest_va))
        } else {
            read_guest_page_entry(high_pd, pd_index(guest_va))
        }
    };
    if pde & PT_ENTRY_PRESENT == 0 {
        return;
    }

    let Some(pt_page) = guest_pt_page_from_pde(low_half, pde) else {
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-pt {} va=0x{:016X} pt_pa=0x{:016X} page=unresolved",
            primary_vm_id(),
            label,
            guest_va,
            pde_addr(pde)
        ));
        return;
    };

    let center = pt_index(guest_va);
    let start = center.saturating_sub(2);
    let end = core::cmp::min(center.saturating_add(2), 511);
    let mut idx = start;
    while idx <= end {
        let entry = unsafe { read_guest_page_entry(pt_page, idx) };
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-pt {} pt_pa=0x{:016X} idx={} entry=0x{:016X}",
            primary_vm_id(),
            label,
            pde_addr(pde),
            idx,
            entry
        ));
        if idx == usize::MAX {
            break;
        }
        idx += 1;
    }
}

unsafe fn read_phys_page_entry(page_pa: u64, index: usize) -> Option<u64> {
    if index >= 512 {
        return None;
    }
    let page = crate::phys::phys_to_virt(page_pa as usize) as *const u64;
    Some(unsafe { core::ptr::read_volatile(page.add(index)) })
}

pub fn log_guest_mapping_from_cr3(label: &str, guest_cr3: u64, guest_va: u64) {
    let pml4_pa = pde_addr(guest_cr3);
    if pml4_pa == 0 {
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-walk {} va=0x{:016X} cr3=0x{:016X} pml4=missing",
            primary_vm_id(),
            label,
            guest_va,
            guest_cr3
        ));
        return;
    }

    let pml4e = unsafe { read_phys_page_entry(pml4_pa, pml4_index(guest_va)) }.unwrap_or(0);
    let pdpte = if pml4e & PT_ENTRY_PRESENT != 0 {
        unsafe { read_phys_page_entry(pde_addr(pml4e), pdpt_index(guest_va)) }.unwrap_or(0)
    } else {
        0
    };
    let pde = if pdpte & PT_ENTRY_PRESENT != 0 {
        unsafe { read_phys_page_entry(pde_addr(pdpte), pd_index(guest_va)) }.unwrap_or(0)
    } else {
        0
    };
    let pte = if pde & PT_ENTRY_PRESENT != 0 {
        unsafe { read_phys_page_entry(pde_addr(pde), pt_index(guest_va)) }.unwrap_or(0)
    } else {
        0
    };

    hvlogf(format_args!(
        "hv: vm{} reporting: guest-walk {} va=0x{:016X} cr3=0x{:016X} idx[pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
        primary_vm_id(),
        label,
        guest_va,
        guest_cr3,
        pd_index(guest_va),
        pt_index(guest_va),
        pml4e,
        pdpte,
        pde,
        pte
    ));
}

pub fn log_guest_phys_pt_context(label: &str, guest_cr3: u64, guest_va: u64) {
    let pml4_pa = pde_addr(guest_cr3);
    if pml4_pa == 0 {
        return;
    }

    let pml4e = unsafe { read_phys_page_entry(pml4_pa, pml4_index(guest_va)) }.unwrap_or(0);
    if pml4e & PT_ENTRY_PRESENT == 0 {
        return;
    }
    let pdpte = unsafe { read_phys_page_entry(pde_addr(pml4e), pdpt_index(guest_va)) }.unwrap_or(0);
    if pdpte & PT_ENTRY_PRESENT == 0 {
        return;
    }
    let pde = unsafe { read_phys_page_entry(pde_addr(pdpte), pd_index(guest_va)) }.unwrap_or(0);
    if pde & PT_ENTRY_PRESENT == 0 {
        return;
    }

    let pt_pa = pde_addr(pde);
    let center = pt_index(guest_va);
    let start = center.saturating_sub(2);
    let end = core::cmp::min(center.saturating_add(2), 511);
    let mut idx = start;
    while idx <= end {
        let entry = unsafe { read_phys_page_entry(pt_pa, idx) }.unwrap_or(0);
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-phys-pt {} pt_pa=0x{:016X} idx={} entry=0x{:016X}",
            primary_vm_id(),
            label,
            pt_pa,
            idx,
            entry
        ));
        if idx == usize::MAX {
            break;
        }
        idx += 1;
    }
}

fn verify_guest_mapping_chain(label: &str, guest_va: u64) -> Result<(), &'static str> {
    let low_half = pml4_index(guest_va) == pml4_index(GUEST_STACK_VA_BASE);
    let Some(pml4) = guest_pml4_page() else {
        return Err("guest verify pml4");
    };
    let Some(low_pdpt) = guest_low_pdpt_page() else {
        return Err("guest verify pdpt");
    };
    let Some(low_pd) = guest_low_pd_page() else {
        return Err("guest verify pd");
    };
    let Some(high_pdpt) = guest_high_pdpt_page() else {
        return Err("guest verify pdpt");
    };
    let Some(high_pd) = guest_high_pd_page() else {
        return Err("guest verify pd");
    };
    let pml4e = unsafe { read_guest_page_entry(pml4, pml4_index(guest_va)) };
    if pml4e & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-verify {} broken=pml4 va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X}",
            primary_vm_id(),
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
            read_guest_page_entry(low_pdpt, pdpt_index(guest_va))
        } else {
            read_guest_page_entry(high_pdpt, pdpt_index(guest_va))
        }
    };
    if pdpte & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-verify {} broken=pdpt va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X}",
            primary_vm_id(),
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
            read_guest_page_entry(low_pd, pd_index(guest_va))
        } else {
            read_guest_page_entry(high_pd, pd_index(guest_va))
        }
    };
    if pde & PT_ENTRY_PRESENT == 0 {
        hvlogf(format_args!(
            "hv: vm{} reporting: guest-verify {} broken=pd va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X}",
            primary_vm_id(),
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
            "hv: vm{} reporting: guest-verify {} broken=pt va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
            primary_vm_id(),
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
        "hv: vm{} reporting: guest-verify {} ok va=0x{:016X} region={} idx[pml4={},pdpt={},pd={},pt={}] pml4e=0x{:016X} pdpte=0x{:016X} pde=0x{:016X} pte=0x{:016X}",
        primary_vm_id(),
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
    if guest_va >= layout.data_start && guest_va < layout.data_end {
        return Some("hull-data");
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

fn page_align_up_4k(addr: u64) -> u64 {
    if addr & ((PAGE_SIZE_4K as u64) - 1) == 0 {
        addr
    } else {
        (addr + PAGE_SIZE_4K as u64) & !((PAGE_SIZE_4K as u64) - 1)
    }
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
        "hv: vm{} reporting: {} image map start=0x{:016X} end=0x{:016X} span_mib={} extra_pts={} cap={} max_mib={}",
        primary_vm_id(),
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
                let tables = guest_tables_ptr()?;
                let image_pt = unsafe { core::ptr::addr_of_mut!((*tables).image_pts[*pt_slot].0) };
                let image_pt_pa = host_va_to_pa(image_pt as u64).ok_or("guest image pt pa")?;
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

fn map_guest_heap_span(pml4: *mut [u64; 512], pt_slot: &mut usize) -> Result<(), &'static str> {
    let heap = crate::allocators::heap_stats();
    let hv_guest_heap = crate::allocators::hv_guest_heap_stats();
    let ranges = [
        ("heap", heap.initialized, heap.heap_start as u64, heap.heap_end as u64),
        (
            "hv-guest-heap",
            hv_guest_heap.initialized,
            hv_guest_heap.heap_start as u64,
            hv_guest_heap.heap_end as u64,
        ),
    ];
    if !ranges
        .iter()
        .any(|(_, initialized, start, end)| *initialized && *start != 0 && *end > *start)
    {
        return Ok(());
    }

    let tables = guest_tables_ptr()?;
    let heap_pdpt = unsafe { core::ptr::addr_of_mut!((*tables).heap_pdpt.0) };
    let heap_pdpt_pa = host_va_to_pa(heap_pdpt as u64).ok_or("guest heap pdpt pa")?;
    let mut heap_pd_slots = [usize::MAX; 512];
    let mut heap_pd_count = 0usize;

    for (_, initialized, start, end) in ranges {
        if !initialized || start == 0 || end <= start {
            continue;
        }
        if pml4_index(start) != pml4_index(end.saturating_sub(1)) {
            return Err("guest heap pml4 range");
        }
        map_table_entry(pml4, pml4_index(start), heap_pdpt_pa);
    }

    for (label, initialized, start, end) in ranges {
        if !initialized || start == 0 || end <= start {
            continue;
        }
        let start_chunk_base = page_align_down_2m(start);
        let end_aligned = page_align_up_2m(end);

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
                let heap_pd = unsafe { core::ptr::addr_of_mut!((*tables).heap_pds[slot].0) };
                let heap_pd_pa = host_va_to_pa(heap_pd as u64).ok_or("guest heap pd pa")?;
                map_table_entry(heap_pdpt, pdpt_idx, heap_pd_pa);
                heap_pd_slots[pdpt_idx] = slot;
                heap_pd_count += 1;
                slot
            };

            let chunk_start = if va < start { start } else { va };
            let chunk_end = core::cmp::min(va.saturating_add(PAGE_SIZE_2M), end);
            if chunk_start < chunk_end {
                let heap_pd = unsafe { core::ptr::addr_of_mut!((*tables).heap_pds[pd_slot].0) };
                let heap_pt = unsafe { core::ptr::addr_of_mut!((*tables).image_pts[*pt_slot].0) };
                let heap_pt_pa = host_va_to_pa(heap_pt as u64).ok_or("guest heap pt pa")?;
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
            }

            va = va
                .checked_add(PAGE_SIZE_2M)
                .ok_or("guest heap span overflow")?;
        }

        hvlogf(format_args!(
            "hv: vm{} reporting: {} map start=0x{:016X} end=0x{:016X} span_mib={} pt_cap={} pt_used={}",
            primary_vm_id(),
            label,
            start,
            end,
            end.saturating_sub(start) / (1024 * 1024),
            GUEST_HIGH_IMAGE_PT_COUNT,
            *pt_slot
        ));
    }
    Ok(())
}

fn read_guest_high_pt_entry(guest_va: u64, pde: u64) -> u64 {
    let Some(pt_page) = guest_pt_page_from_pde(false, pde) else {
        return 0;
    };
    unsafe { read_guest_page_entry(pt_page, pt_index(guest_va)) }
}

fn guest_pt_page_from_pde(low_half: bool, pde: u64) -> Option<*const [u64; 512]> {
    let tables = guest_tables_ptr_opt()?;
    if low_half {
        for i in 0..GUEST_LOW_PT_COUNT {
            let low_pt = unsafe { core::ptr::addr_of!((*tables).low_pts[i].0) as u64 };
            let Ok(low_pt_pa) = current_high_pt_pa(low_pt) else {
                continue;
            };
            if pde_addr(pde) == low_pt_pa {
                return Some(unsafe { core::ptr::addr_of!((*tables).low_pts[i].0) });
            }
        }
        return None;
    }

    let Ok(code_pt_pa) =
        current_high_pt_pa(unsafe { core::ptr::addr_of!((*tables).code_pt.0) as u64 })
    else {
        return None;
    };
    if pde_addr(pde) == code_pt_pa {
        return Some(unsafe { core::ptr::addr_of!((*tables).code_pt.0) });
    }

    for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
        let image_pt = unsafe { core::ptr::addr_of!((*tables).image_pts[i].0) as u64 };
        let Ok(image_pt_pa) = current_high_pt_pa(image_pt) else {
            continue;
        };
        if pde_addr(pde) == image_pt_pa {
            return Some(unsafe { core::ptr::addr_of!((*tables).image_pts[i].0) });
        }
    }

    None
}

fn read_guest_low_pt_entry(guest_va: u64, pde: u64) -> u64 {
    let Some(pt_page) = guest_pt_page_from_pde(true, pde) else {
        return 0;
    };
    unsafe { read_guest_page_entry(pt_page, pt_index(guest_va)) }
}

fn current_high_pt_pa(va: u64) -> Result<u64, &'static str> {
    host_va_to_pa(va).ok_or("guest high pt pa")
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
