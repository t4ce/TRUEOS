extern crate alloc;

use alloc::vec::Vec;

const PAGE_BYTES: usize = 4096;
const ENTRIES: usize = 512;
const ENTRY_ADDR_MASK: u64 = !0xFFF;
const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_RW: u64 = 1 << 1;
const PAGE_PWT: u64 = 1 << 3;
const PAGE_PCD: u64 = 1 << 4;
const PTE_PRESENT_RW: u64 = PAGE_PRESENT | PAGE_RW;
const PDE_PRESENT_RW_UC: u64 = PAGE_PRESENT | PAGE_RW | PAGE_PWT | PAGE_PCD;

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

impl SparsePpgtt {
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
}

pub(crate) fn build_sparse_ppgtt_for_ranges(ranges: &[PpgttRange]) -> Option<SparsePpgtt> {
    let pml4 = alloc_table_page()?;
    let mut ppgtt = SparsePpgtt {
        pml4,
        pages: Vec::new(),
    };
    ppgtt.pages.push(pml4);

    for range in ranges {
        map_range(&mut ppgtt, *range)?;
    }

    ppgtt.flush();
    Some(ppgtt)
}

fn map_range(ppgtt: &mut SparsePpgtt, range: PpgttRange) -> Option<()> {
    if range.bytes == 0 {
        return Some(());
    }
    let page_count = range.bytes.checked_add(PAGE_BYTES - 1)? / PAGE_BYTES;
    for page in 0..page_count {
        let byte_off = page.checked_mul(PAGE_BYTES)?;
        let gpu = range.gpu.checked_add(byte_off as u64)?;
        let phys = range.phys.checked_add(byte_off as u64)?;
        map_page(ppgtt, gpu, phys)?;
    }
    Some(())
}

fn map_page(ppgtt: &mut SparsePpgtt, gpu: u64, phys: u64) -> Option<()> {
    let pml4_index = ((gpu >> 39) & 0x1FF) as usize;
    let pdp_index = ((gpu >> 30) & 0x1FF) as usize;
    let pd_index = ((gpu >> 21) & 0x1FF) as usize;
    let pt_index = ((gpu >> 12) & 0x1FF) as usize;

    let pdp = ensure_child_table(ppgtt, ppgtt.pml4, pml4_index)?;
    let pd = ensure_child_table(ppgtt, pdp, pdp_index)?;
    let pt = ensure_child_table(ppgtt, pd, pd_index)?;
    unsafe {
        core::ptr::write_volatile(pt.virt.add(pt_index), (phys & ENTRY_ADDR_MASK) | PTE_PRESENT_RW);
    }
    Some(())
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
