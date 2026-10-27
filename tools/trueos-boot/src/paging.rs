//! Initial mappings; page-table storage is reserved before firmware shutdown.
pub const HHDM: u64 = 0xffff_8000_0000_0000;
pub const ARENA_PAGES: usize = 512;
const HUGE: u64 = 2 * 1024 * 1024;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;

pub struct Tables { arena: u64, used: usize }

impl Tables {
    pub unsafe fn new(arena: u64) -> Self {
        unsafe { core::ptr::write_bytes(arena as *mut u8, 0, ARENA_PAGES * 4096); }
        Self { arena, used: 1 }
    }
    pub fn root(&self) -> u64 { self.arena }
    unsafe fn child(&mut self, parent: u64, index: usize) -> Result<u64, &'static str> {
        let p = unsafe { (parent as *mut u64).add(index) };
        let entry = unsafe { p.read() };
        if entry & 1 != 0 {
            if entry & 128 != 0 { return Err("conflicting large mapping"); }
            return Ok(entry & ADDRESS);
        }
        if self.used == ARENA_PAGES { return Err("initial page-table arena exhausted"); }
        let child = self.arena + self.used as u64 * 4096;
        self.used += 1;
        unsafe { p.write(child | 3); }
        Ok(child)
    }
    unsafe fn directory(&mut self, virtual_address: u64) -> Result<u64, &'static str> {
        let pdpt = unsafe { self.child(self.arena, ((virtual_address >> 39) & 511) as usize)? };
        unsafe { self.child(pdpt, ((virtual_address >> 30) & 511) as usize) }
    }
    unsafe fn huge(&mut self, v: u64, p: u64) -> Result<(), &'static str> {
        let pd = unsafe { self.directory(v)? };
        let slot = unsafe { (pd as *mut u64).add(((v >> 21) & 511) as usize) };
        let old = unsafe { slot.read() };
        if old & 1 != 0 && old != (p | 0x83) { return Err("overlapping initial mapping"); }
        unsafe { slot.write(p | 0x83); }
        Ok(())
    }
    pub unsafe fn physical_range(&mut self, base: u64, bytes: u64) -> Result<(), &'static str> {
        let limit = base.checked_add(bytes).and_then(|n| n.checked_add(HUGE - 1))
            .ok_or("physical mapping overflow")? & !(HUGE - 1);
        if limit > 1 << 47 { return Err("physical address exceeds initial HHDM"); }
        let mut at = base & !(HUGE - 1);
        while at < limit {
            unsafe { self.huge(at, at)?; self.huge(HHDM + at, at)?; }
            at += HUGE;
        }
        Ok(())
    }
    pub unsafe fn kernel(&mut self, base: u64, phys: u64, bytes: usize) -> Result<(), &'static str> {
        let mut offset = 0u64;
        while offset < bytes as u64 {
            let v = base + offset;
            let pd = unsafe { self.directory(v)? };
            let pt = unsafe { self.child(pd, ((v >> 21) & 511) as usize)? };
            let pte = unsafe { (pt as *mut u64).add(((v >> 12) & 511) as usize) };
            unsafe { pte.write((phys + offset) | 3); }
            offset += 4096;
        }
        Ok(())
    }
    pub unsafe fn uncached_physical_range(&mut self, base: u64, bytes: u64) -> Result<(), &'static str> {
        unsafe { self.physical_range(base, bytes)?; }
        let end = base.checked_add(bytes).ok_or("MMIO address overflow")?;
        let mut at = base & !(HUGE - 1);
        while at < end {
            for virtual_address in [at, HHDM + at] {
                let pd = unsafe { self.directory(virtual_address)? };
                unsafe { (pd as *mut u64).add(((virtual_address >> 21) & 511) as usize).write(at | 0x9b); }
            }
            at += HUGE;
        }
        Ok(())
    }
}
