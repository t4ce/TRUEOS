extern crate alloc;

use alloc::vec::Vec;

const PAGE_BYTES: usize = 4096;
const ENTRIES: usize = 512;
const ENTRY_ADDR_MASK: u64 = !0xFFF;
const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_RW: u64 = 1 << 1;
const PAGE_PWT: u64 = 1 << 3;
const PAGE_PCD: u64 = 1 << 4;
const GEN12_PPGTT_PTE_PAT0: u64 = 1 << 3;
const GEN12_PPGTT_PTE_PAT1: u64 = 1 << 4;
const GEN12_PPGTT_PTE_PAT2: u64 = 1 << 7;
const GEN12_SYSTEM_MEMORY_WB_PAT_INDEX: u8 = 0;
const GEN12_SCANOUT_SYSTEM_MEMORY_UC_PAT_INDEX: u8 = 3;
const PDE_PRESENT_RW_UC: u64 = PAGE_PRESENT | PAGE_RW | PAGE_PWT | PAGE_PCD;

const fn gen12_ppgtt_leaf_flags(pat_index: u8) -> u64 {
    PAGE_PRESENT
        | PAGE_RW
        | if pat_index & 1 != 0 {
            GEN12_PPGTT_PTE_PAT0
        } else {
            0
        }
        | if pat_index & 2 != 0 {
            GEN12_PPGTT_PTE_PAT1
        } else {
            0
        }
        | if pat_index & 4 != 0 {
            GEN12_PPGTT_PTE_PAT2
        } else {
            0
        }
}

const PTE_PRESENT_RW_PAT0_WB: u64 = gen12_ppgtt_leaf_flags(GEN12_SYSTEM_MEMORY_WB_PAT_INDEX);
const _: () = assert!(PTE_PRESENT_RW_PAT0_WB == PAGE_PRESENT | PAGE_RW);
const PTE_PRESENT_RW_PAT3_UC: u64 =
    gen12_ppgtt_leaf_flags(GEN12_SCANOUT_SYSTEM_MEMORY_UC_PAT_INDEX);
const _: () = assert!(
    PTE_PRESENT_RW_PAT3_UC == PAGE_PRESENT | PAGE_RW | GEN12_PPGTT_PTE_PAT0 | GEN12_PPGTT_PTE_PAT1
);

#[derive(Copy, Clone, Debug)]
pub(crate) struct PpgttRange {
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
}

#[derive(Copy, Clone, Debug)]
struct TablePage {
    phys: u64,
    virt: *mut u64,
}

#[derive(Debug)]
pub(crate) struct SparsePpgtt {
    pml4: TablePage,
    pages: Vec<TablePage>,
}

unsafe impl Send for SparsePpgtt {}
unsafe impl Sync for SparsePpgtt {}

impl SparsePpgtt {
    pub(crate) fn new() -> Option<Self> {
        // Do not create a Gen12 PPGTT whose PAT meanings are inherited from
        // firmware. Ordinary resources use PAT0/WB while direct-scanout
        // render targets deliberately use PAT3/UC.
        if !crate::intel::gen12_integrated_pat_ready() {
            return None;
        }
        let pml4 = alloc_table_page()?;
        Some(Self {
            pml4,
            pages: alloc::vec![pml4],
        })
    }

    pub(crate) fn pml4_phys(&self) -> u64 {
        self.pml4.phys
    }

    pub(crate) fn table_page_count(&self) -> usize {
        self.pages.len()
    }

    pub(crate) fn flush(&self) {
        for page in &self.pages {
            crate::intel::dma_flush(page.virt as *mut u8, PAGE_BYTES);
        }
    }

    pub(crate) fn map_range(&mut self, range: PpgttRange) -> Option<()> {
        self.map_range_with_pat(range, GEN12_SYSTEM_MEMORY_WB_PAT_INDEX)
    }

    /// Map a system-memory render target that will be handed directly to the
    /// display engine. Unlike ordinary render resources, this mapping uses
    /// PAT3/UC so completion cannot leave the newest pixels resident only in
    /// a cache that the display fetch path does not observe.
    pub(crate) fn map_scanout_range(&mut self, range: PpgttRange) -> Option<()> {
        self.map_range_with_pat(range, GEN12_SCANOUT_SYSTEM_MEMORY_UC_PAT_INDEX)
    }

    fn map_range_with_pat(&mut self, range: PpgttRange, pat_index: u8) -> Option<()> {
        map_range(self, range, pat_index)?;
        self.flush();
        verify_range(self, range, pat_index)?;
        Some(())
    }

    pub(crate) fn unmap_range(&mut self, gpu: u64, bytes: usize) -> Option<()> {
        unmap_range(self, gpu, bytes)?;
        self.flush();
        Some(())
    }
}

impl Drop for SparsePpgtt {
    fn drop(&mut self) {
        for page in self.pages.drain(..) {
            crate::dma::dealloc(page.virt as *mut u8, PAGE_BYTES);
        }
    }
}

pub(crate) fn build_sparse_ppgtt_for_ranges(ranges: &[PpgttRange]) -> Option<SparsePpgtt> {
    let mut ppgtt = SparsePpgtt::new()?;

    for range in ranges {
        map_range(
            &mut ppgtt,
            *range,
            GEN12_SYSTEM_MEMORY_WB_PAT_INDEX,
        )?;
    }

    ppgtt.flush();
    Some(ppgtt)
}

fn map_range(ppgtt: &mut SparsePpgtt, range: PpgttRange, pat_index: u8) -> Option<()> {
    if range.bytes == 0 {
        return Some(());
    }
    if !range.gpu.is_multiple_of(PAGE_BYTES as u64)
        || !range.phys.is_multiple_of(PAGE_BYTES as u64)
        || pat_index > 7
    {
        return None;
    }
    let page_count = range.bytes.checked_add(PAGE_BYTES - 1)? / PAGE_BYTES;
    for page in 0..page_count {
        let byte_off = page.checked_mul(PAGE_BYTES)?;
        let gpu = range.gpu.checked_add(byte_off as u64)?;
        let phys = range.phys.checked_add(byte_off as u64)?;
        map_page(ppgtt, gpu, phys, pat_index)?;
    }
    Some(())
}

fn map_page(ppgtt: &mut SparsePpgtt, gpu: u64, phys: u64, pat_index: u8) -> Option<()> {
    let pml4_index = ((gpu >> 39) & 0x1FF) as usize;
    let pdp_index = ((gpu >> 30) & 0x1FF) as usize;
    let pd_index = ((gpu >> 21) & 0x1FF) as usize;
    let pt_index = ((gpu >> 12) & 0x1FF) as usize;

    let pdp = ensure_child_table(ppgtt, ppgtt.pml4, pml4_index)?;
    let pd = ensure_child_table(ppgtt, pdp, pdp_index)?;
    let pt = ensure_child_table(ppgtt, pd, pd_index)?;
    unsafe {
        core::ptr::write_volatile(
            pt.virt.add(pt_index),
            (phys & ENTRY_ADDR_MASK) | gen12_ppgtt_leaf_flags(pat_index),
        );
    }
    Some(())
}

fn verify_range(ppgtt: &SparsePpgtt, range: PpgttRange, pat_index: u8) -> Option<()> {
    if range.bytes == 0 {
        return Some(());
    }
    let page_count = range.bytes.checked_add(PAGE_BYTES - 1)? / PAGE_BYTES;
    let flags = gen12_ppgtt_leaf_flags(pat_index);
    for page in 0..page_count {
        let byte_off = page.checked_mul(PAGE_BYTES)?;
        let gpu = range.gpu.checked_add(byte_off as u64)?;
        let phys = range.phys.checked_add(byte_off as u64)?;
        let observed = read_leaf_entry(ppgtt, gpu)?;
        let expected = (phys & ENTRY_ADDR_MASK) | flags;
        if observed != expected {
            return None;
        }
    }
    Some(())
}

fn read_leaf_entry(ppgtt: &SparsePpgtt, gpu: u64) -> Option<u64> {
    let pml4_index = ((gpu >> 39) & 0x1FF) as usize;
    let pdp_index = ((gpu >> 30) & 0x1FF) as usize;
    let pd_index = ((gpu >> 21) & 0x1FF) as usize;
    let pt_index = ((gpu >> 12) & 0x1FF) as usize;

    let pdp = existing_child_table(ppgtt, ppgtt.pml4, pml4_index)?;
    let pd = existing_child_table(ppgtt, pdp, pdp_index)?;
    let pt = existing_child_table(ppgtt, pd, pd_index)?;
    Some(unsafe { core::ptr::read_volatile(pt.virt.add(pt_index)) })
}

fn unmap_range(ppgtt: &mut SparsePpgtt, gpu: u64, bytes: usize) -> Option<()> {
    if bytes == 0 {
        return Some(());
    }
    let page_count = bytes.checked_add(PAGE_BYTES - 1)? / PAGE_BYTES;
    for page in 0..page_count {
        let byte_off = page.checked_mul(PAGE_BYTES)?;
        unmap_page(ppgtt, gpu.checked_add(byte_off as u64)?)?;
    }
    Some(())
}

fn unmap_page(ppgtt: &SparsePpgtt, gpu: u64) -> Option<()> {
    let pml4_index = ((gpu >> 39) & 0x1FF) as usize;
    let pdp_index = ((gpu >> 30) & 0x1FF) as usize;
    let pd_index = ((gpu >> 21) & 0x1FF) as usize;
    let pt_index = ((gpu >> 12) & 0x1FF) as usize;

    let pdp = existing_child_table(ppgtt, ppgtt.pml4, pml4_index)?;
    let pd = existing_child_table(ppgtt, pdp, pdp_index)?;
    let pt = existing_child_table(ppgtt, pd, pd_index)?;
    unsafe {
        core::ptr::write_volatile(pt.virt.add(pt_index), 0);
    }
    Some(())
}

fn existing_child_table(ppgtt: &SparsePpgtt, parent: TablePage, index: usize) -> Option<TablePage> {
    let entry = unsafe { core::ptr::read_volatile(parent.virt.add(index)) };
    if entry & PAGE_PRESENT == 0 {
        return None;
    }
    find_table_page(ppgtt, entry & ENTRY_ADDR_MASK)
}

fn ensure_child_table(
    ppgtt: &mut SparsePpgtt,
    parent: TablePage,
    index: usize,
) -> Option<TablePage> {
    let entry = unsafe { core::ptr::read_volatile(parent.virt.add(index)) };
    if entry & PAGE_PRESENT != 0 {
        return find_table_page(ppgtt, entry & ENTRY_ADDR_MASK);
    }

    let child = alloc_table_page()?;
    unsafe {
        core::ptr::write_volatile(parent.virt.add(index), child.phys | PDE_PRESENT_RW_UC);
    }
    ppgtt.pages.push(child);
    Some(child)
}

fn find_table_page(ppgtt: &SparsePpgtt, phys: u64) -> Option<TablePage> {
    ppgtt.pages.iter().copied().find(|page| page.phys == phys)
}

fn alloc_table_page() -> Option<TablePage> {
    let (phys, virt) = crate::dma::alloc(PAGE_BYTES, PAGE_BYTES)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, PAGE_BYTES);
    }
    Some(TablePage {
        phys,
        virt: virt as *mut u64,
    })
}
