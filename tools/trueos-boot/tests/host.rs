//! Run from the repository root with tools/trueos-boot/test.sh, which supplies
//! host-built r-efi protocol definitions and the bundled Rust linker.
//! Set TRUEOS_TEST_KERNEL to exercise a different compiled kernel ELF.

extern crate alloc;

#[path = "../src/elf.rs"]
mod elf;
#[path = "../src/handoff.rs"]
mod handoff;
#[path = "../src/graphics.rs"]
mod graphics;
#[path = "../src/paging.rs"]
mod paging;
#[path = "../src/time.rs"]
mod time;

use std::alloc::{Layout, alloc_zeroed, dealloc};

struct Pages {
    pointer: *mut u8,
    layout: Layout,
}

impl Pages {
    fn new(length: usize) -> Self {
        let layout = Layout::from_size_align(length.next_multiple_of(4096), 4096).unwrap();
        let pointer = unsafe { alloc_zeroed(layout) };
        assert!(!pointer.is_null());
        Self { pointer, layout }
    }
    fn address(&self) -> u64 { self.pointer as u64 }
}

impl Drop for Pages {
    fn drop(&mut self) { unsafe { dealloc(self.pointer, self.layout); } }
}

fn kernel_bytes() -> Vec<u8> {
    let path = std::env::var("TRUEOS_TEST_KERNEL")
        .unwrap_or_else(|_| "tgt/x86_64-unknown-trueos/release/TRUEOS".into());
    std::fs::read(path).expect("build the kernel first or set TRUEOS_TEST_KERNEL")
}

/// Walk the generated hardware table entries independently of the mapper.
unsafe fn translate(root: u64, address: u64) -> Option<u64> {
    let mut table = root;
    for (level, shift) in [39, 30, 21, 12].iter().enumerate() {
        let entry = unsafe { (table as *const u64).add(((address >> shift) & 511) as usize).read() };
        if entry & 1 == 0 { return None; }
        if entry & 128 != 0 && (level == 1 || level == 2) {
            let size = 1u64 << shift;
            return Some((entry & 0x000f_ffff_ffff_f000 & !(size - 1)) | (address & (size - 1)));
        }
        table = entry & 0x000f_ffff_ffff_f000;
    }
    Some(table | (address & 4095))
}

#[test]
fn actual_kernel_loads_and_its_request_section_accepts_the_handoff() {
    let bytes = kernel_bytes();
    let image = elf::parse(&bytes).expect("current kernel ELF must satisfy loader contract");
    let loaded = Pages::new(image.size);
    for segment in &image.segments {
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr().add(segment.file_offset),
                loaded.pointer.add((segment.virtual_address - image.base) as usize),
                segment.file_size,
            );
        }
    }
    let arena = Pages::new(2 * 1024 * 1024);
    let mut handoff = unsafe {
        handoff::Handoff::new(arena.address(), arena.layout.size(), 0, 4096)
    }.unwrap();
    handoff.prepare(&handoff::BootFacts {
        kernel: handoff::Blob { phys: bytes.as_ptr() as u64, len: bytes.len(), path: "/TRUEOS.elf", cmdline: "keyboard=de" },
        modules: &[], tftp_server: None, kernel_physical_base: loaded.address(), kernel_virtual_base: image.base,
        framebuffer: None, framebuffer_edid: &[], rsdp: 0, efi_system_table: 0, smbios32: 0, smbios64: 0,
        timestamp: None, stack_size: 16 * 1024 * 1024, mp_response: 0,
    }).unwrap();
    let report = unsafe {
        handoff.patch_requests(
            loaded.pointer.add((image.requests - image.base) as usize), image.requests_size,
        )
    }.expect("compiled kernel requests must be accepted");
    assert!(report.answered >= 7, "kernel's required response set was not populated");
    println!("ELF base={:#x} entry={:#x} memory={} requests={} answered={}",
        image.base, image.entry, image.size, image.requests_size, report.answered);
}

#[test]
fn actual_kernel_pages_and_physical_aliases_translate_correctly() {
    let image = elf::parse(&kernel_bytes()).unwrap();
    let arena = Pages::new(paging::ARENA_PAGES * 4096);
    let mut tables = unsafe { paging::Tables::new(arena.address()) };
    // Deliberately not aligned to 2 MiB: AllocatePages guarantees only 4 KiB.
    let kernel_phys = 0x0200_1000;
    unsafe {
        tables.physical_range(0, 0x100000).unwrap();
        tables.physical_range(kernel_phys, image.size as u64).unwrap();
        tables.uncached_physical_range(0xfee00000, 4096).unwrap();
        tables.kernel(image.base, kernel_phys, image.size).unwrap();
    }
    for segment in &image.segments {
        for offset in [0, segment.memory_size as u64 - 1] {
            let virtual_address = segment.virtual_address + offset;
            let physical = kernel_phys + virtual_address - image.base;
            unsafe {
                assert_eq!(translate(tables.root(), virtual_address), Some(physical));
                assert_eq!(translate(tables.root(), physical), Some(physical));
                assert_eq!(translate(tables.root(), paging::HHDM + physical), Some(physical));
            }
        }
    }
    unsafe {
        assert_eq!(translate(tables.root(), 0xfee00020), Some(0xfee00020));
        assert_eq!(translate(tables.root(), paging::HHDM + 0xfee00020), Some(0xfee00020));
        assert_eq!(translate(tables.root(), 0x4000000000), None);
        for virtual_address in [0xfee00000, paging::HHDM + 0xfee00000] {
            let pml4e = (tables.root() as *const u64).add(((virtual_address >> 39) & 511) as usize).read();
            let pdpte = ((pml4e & 0x000f_ffff_ffff_f000) as *const u64)
                .add(((virtual_address >> 30) & 511) as usize).read();
            let pde = ((pdpte & 0x000f_ffff_ffff_f000) as *const u64)
                .add(((virtual_address >> 21) & 511) as usize).read();
            assert_eq!(pde & 0x18, 0x18, "both LAPIC aliases must set PWT and PCD");
        }
    }
}

#[test]
fn elf_parser_rejects_truncation_overflow_and_invalid_entry() {
    let bytes = kernel_bytes();
    assert!(elf::parse(&bytes[..63]).is_err());
    let mut overflow = bytes.clone();
    overflow[32..40].copy_from_slice(&(u64::MAX - 8).to_le_bytes());
    assert!(elf::parse(&overflow).is_err());
    let mut bad_entry = bytes;
    bad_entry[24..32].copy_from_slice(&0x100000u64.to_le_bytes());
    assert!(elf::parse(&bad_entry).is_err());
}

#[test]
fn physical_mapper_rejects_overflow_and_unrepresentable_hhdm() {
    let arena = Pages::new(paging::ARENA_PAGES * 4096);
    let mut tables = unsafe { paging::Tables::new(arena.address()) };
    unsafe {
        assert!(tables.physical_range(u64::MAX - 4095, 8192).is_err());
        assert!(tables.physical_range(1 << 47, 4096).is_err());
    }
}
