use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const BLUEPRINT_HEADER_LEN: usize = 24;
const ELF64_HEADER_LEN: usize = 64;
const ELF64_SECTION_HEADER_LEN: usize = 64;
const ELF64_SYM_LEN: usize = 24;
const ELF64_RELA_LEN: usize = 24;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const SHF_ALLOC: u64 = 0x2;
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_32S: u32 = 11;

pub(crate) struct BlueprintModule<'a> {
    pub(crate) version: u16,
    pub(crate) flags: u16,
    pub(crate) entry: u64,
    pub(crate) raw_payload_len: usize,
    pub(crate) payload: &'a [u8],
}

pub(crate) struct ElfImport<'a> {
    pub(crate) name: &'a str,
    pub(crate) resolved_addr: Option<usize>,
}

#[derive(Copy, Clone)]
struct ElfSection {
    section_type: u32,
    flags: u64,
    file_offset: usize,
    size: usize,
    link: usize,
    info: usize,
    align: usize,
    entsize: usize,
}

#[derive(Copy, Clone)]
struct ElfSymbol {
    name_offset: usize,
    info: u8,
    section_index: u16,
    value: u64,
}

struct LoadedRelImage {
    base: *mut u8,
    used_len: usize,
    section_bases: Vec<usize>,
    _arena_lease: PortalImageArenaLease,
}

impl Drop for LoadedRelImage {
    fn drop(&mut self) {
        let _ = self.base;
        let _ = self.used_len;
    }
}

const PORTAL_IMAGE_ARENA_BYTES: usize = 4 * 1024 * 1024;

#[repr(align(4096))]
struct PortalImageArena([u8; PORTAL_IMAGE_ARENA_BYTES]);

static PORTAL_IMAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut PORTAL_IMAGE_ARENA: PortalImageArena = PortalImageArena([0; PORTAL_IMAGE_ARENA_BYTES]);
static UNRESOLVED_IMPORT_STUBS: Mutex<Vec<UnresolvedImportStub>> = Mutex::new(Vec::new());

struct PortalImageArenaLease {
    armed: bool,
}

struct UnresolvedImportStub {
    name: String,
    warned: bool,
}

impl PortalImageArenaLease {
    fn acquire() -> Result<Self, String> {
        if PORTAL_IMAGE_IN_USE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(String::from("portal image arena busy"));
        }
        Ok(Self { armed: true })
    }
}

impl Drop for PortalImageArenaLease {
    fn drop(&mut self) {
        if self.armed {
            PORTAL_IMAGE_IN_USE.store(false, Ordering::Release);
        }
    }
}

macro_rules! unresolved_import_stubs {
    ($(($fn_name:ident, $slot:expr)),* $(,)?) => {
        $(
            extern "C" fn $fn_name() -> usize {
                unresolved_import_called($slot)
            }
        )*

        static UNRESOLVED_IMPORT_STUB_FNS: &[extern "C" fn() -> usize] = &[
            $($fn_name),*
        ];
    };
}

unresolved_import_stubs!(
    (unresolved_import_stub_0, 0),
    (unresolved_import_stub_1, 1),
    (unresolved_import_stub_2, 2),
    (unresolved_import_stub_3, 3),
    (unresolved_import_stub_4, 4),
    (unresolved_import_stub_5, 5),
    (unresolved_import_stub_6, 6),
    (unresolved_import_stub_7, 7),
    (unresolved_import_stub_8, 8),
    (unresolved_import_stub_9, 9),
    (unresolved_import_stub_10, 10),
    (unresolved_import_stub_11, 11),
    (unresolved_import_stub_12, 12),
    (unresolved_import_stub_13, 13),
    (unresolved_import_stub_14, 14),
    (unresolved_import_stub_15, 15),
    (unresolved_import_stub_16, 16),
    (unresolved_import_stub_17, 17),
    (unresolved_import_stub_18, 18),
    (unresolved_import_stub_19, 19),
    (unresolved_import_stub_20, 20),
    (unresolved_import_stub_21, 21),
    (unresolved_import_stub_22, 22),
    (unresolved_import_stub_23, 23),
    (unresolved_import_stub_24, 24),
    (unresolved_import_stub_25, 25),
    (unresolved_import_stub_26, 26),
    (unresolved_import_stub_27, 27),
    (unresolved_import_stub_28, 28),
    (unresolved_import_stub_29, 29),
    (unresolved_import_stub_30, 30),
    (unresolved_import_stub_31, 31),
    (unresolved_import_stub_32, 32),
    (unresolved_import_stub_33, 33),
    (unresolved_import_stub_34, 34),
    (unresolved_import_stub_35, 35),
    (unresolved_import_stub_36, 36),
    (unresolved_import_stub_37, 37),
    (unresolved_import_stub_38, 38),
    (unresolved_import_stub_39, 39),
    (unresolved_import_stub_40, 40),
    (unresolved_import_stub_41, 41),
    (unresolved_import_stub_42, 42),
    (unresolved_import_stub_43, 43),
    (unresolved_import_stub_44, 44),
    (unresolved_import_stub_45, 45),
    (unresolved_import_stub_46, 46),
    (unresolved_import_stub_47, 47),
    (unresolved_import_stub_48, 48),
    (unresolved_import_stub_49, 49),
    (unresolved_import_stub_50, 50),
    (unresolved_import_stub_51, 51),
    (unresolved_import_stub_52, 52),
    (unresolved_import_stub_53, 53),
    (unresolved_import_stub_54, 54),
    (unresolved_import_stub_55, 55),
    (unresolved_import_stub_56, 56),
    (unresolved_import_stub_57, 57),
    (unresolved_import_stub_58, 58),
    (unresolved_import_stub_59, 59),
    (unresolved_import_stub_60, 60),
    (unresolved_import_stub_61, 61),
    (unresolved_import_stub_62, 62),
    (unresolved_import_stub_63, 63),
);

fn unresolved_import_stub_slot(name: &str) -> Option<usize> {
    let mut stubs = UNRESOLVED_IMPORT_STUBS.lock();
    if let Some(slot) = stubs.iter().position(|entry| entry.name == name) {
        return Some(slot);
    }
    if stubs.len() >= UNRESOLVED_IMPORT_STUB_FNS.len() {
        return None;
    }
    let slot = stubs.len();
    stubs.push(UnresolvedImportStub {
        name: String::from(name),
        warned: false,
    });
    Some(slot)
}

fn unresolved_import_called(slot: usize) -> usize {
    let mut warn_name = None;
    {
        let mut stubs = UNRESOLVED_IMPORT_STUBS.lock();
        if let Some(entry) = stubs.get_mut(slot)
            && !entry.warned
        {
            entry.warned = true;
            warn_name = Some(entry.name.clone());
        }
    }
    if let Some(name) = warn_name {
        crate::hv::hvlogf(format_args!("portal: joker import invoked name={}", name));
    }
    0
}

fn portal_logf(args: core::fmt::Arguments<'_>) {
    if crate::logflag::PORTAL_LOGS {
        crate::log!("{}\n", args);
    }
}

fn resolve_unresolved_import(name: &str) -> Option<usize> {
    let slot = unresolved_import_stub_slot(name)?;
    Some(UNRESOLVED_IMPORT_STUB_FNS[slot] as *const () as usize)
}

fn portal_alloc_error_handler(layout: Layout) -> ! {
    portal_logf(format_args!(
        "portal: alloc error size={} align={}",
        layout.size(),
        layout.align()
    ));
    let stats = crate::allocators::hv_guest_heap_stats();
    portal_logf(format_args!(
        "portal: hv-guest-heap virt=0x{:X}..0x{:X} phys=0x{:X} src={:?} usable_total={} free_bytes={} largest_free={} free_blocks={} init={}",
        stats.heap_start,
        stats.heap_end,
        stats.phys_start,
        stats.source,
        stats.usable_total,
        stats.free_bytes,
        stats.largest_free_block,
        stats.free_blocks,
        stats.initialized,
    ));
    let trace = crate::allocators::last_alloc_trace();
    portal_logf(format_args!(
        "portal: last-alloc seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X} block_size={} next=0x{:016X} payload=0x{:016X} aligned_used={}",
        trace.seq,
        trace.caller_rip,
        trace.caller_rip_1,
        trace.caller_rip_2,
        trace.layout_size,
        trace.layout_align,
        trace.stage,
        trace.head_ptr,
        trace.block_ptr,
        trace.block_size,
        trace.block_next,
        trace.payload_start,
        trace.aligned_used,
    ));
    loop {
        core::hint::spin_loop();
    }
}

unsafe extern "C" fn portal_rust_alloc_error_handler(size: usize, align: usize) -> ! {
    let layout = unsafe { Layout::from_size_align_unchecked(size, align.max(1)) };
    portal_alloc_error_handler(layout)
}

fn portal_no_alloc_shim_is_unstable_v2() {}

include!(concat!(env!("OUT_DIR"), "/generated_portal_imports.rs"));

pub(crate) fn entry_hint_section(entry: u64) -> u32 {
    (entry >> 32) as u32
}

pub(crate) fn entry_hint_offset(entry: u64) -> u32 {
    entry as u32
}

fn align_up(value: usize, align: usize) -> Result<usize, &'static str> {
    if align == 0 {
        return Ok(value);
    }
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|v| v & !mask)
        .ok_or("alignment overflow")
}

fn parse_sections(bytes: &[u8]) -> Result<Vec<ElfSection>, &'static str> {
    if bytes.len() < ELF64_HEADER_LEN {
        return Err("ELF header truncated");
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return Err("payload is not ELF");
    }
    if bytes.get(4).copied() != Some(2) || bytes.get(5).copied() != Some(1) {
        return Err("unsupported ELF class/data");
    }

    let shoff = le_u64(bytes, 40).ok_or("ELF header truncated")? as usize;
    let shentsize = le_u16(bytes, 58).ok_or("ELF header truncated")? as usize;
    let shnum = le_u16(bytes, 60).ok_or("ELF header truncated")? as usize;
    if shentsize != ELF64_SECTION_HEADER_LEN {
        return Err("unsupported ELF section header size");
    }

    let mut out = Vec::with_capacity(shnum);
    for section_index in 0..shnum {
        let shdr_off = shoff
            .checked_add(
                section_index
                    .checked_mul(shentsize)
                    .ok_or("ELF section header overflow")?,
            )
            .ok_or("ELF section header overflow")?;
        let shdr = bytes
            .get(shdr_off..shdr_off + ELF64_SECTION_HEADER_LEN)
            .ok_or("ELF section header truncated")?;
        out.push(ElfSection {
            section_type: le_u32(shdr, 4).ok_or("ELF section header truncated")?,
            flags: le_u64(shdr, 8).ok_or("ELF section header truncated")?,
            file_offset: le_u64(shdr, 24).ok_or("ELF section header truncated")? as usize,
            size: le_u64(shdr, 32).ok_or("ELF section header truncated")? as usize,
            link: le_u32(shdr, 40).ok_or("ELF section header truncated")? as usize,
            info: le_u32(shdr, 44).ok_or("ELF section header truncated")? as usize,
            align: le_u64(shdr, 48).ok_or("ELF section header truncated")? as usize,
            entsize: le_u64(shdr, 56).ok_or("ELF section header truncated")? as usize,
        });
    }
    Ok(out)
}

fn read_symbol(symtab: &[u8], index: usize) -> Result<ElfSymbol, &'static str> {
    let sym_off = index
        .checked_mul(ELF64_SYM_LEN)
        .ok_or("ELF symbol table overflow")?;
    let sym = symtab
        .get(sym_off..sym_off + ELF64_SYM_LEN)
        .ok_or("ELF symbol truncated")?;
    Ok(ElfSymbol {
        name_offset: le_u32(sym, 0).ok_or("ELF symbol truncated")? as usize,
        info: *sym.get(4).ok_or("ELF symbol truncated")?,
        section_index: le_u16(sym, 6).ok_or("ELF symbol truncated")?,
        value: le_u64(sym, 8).ok_or("ELF symbol truncated")?,
    })
}

fn sym_name<'a>(strtab: &'a [u8], sym: &ElfSymbol) -> Result<&'a str, &'static str> {
    let name_bytes = strtab
        .get(sym.name_offset..)
        .ok_or("ELF symbol name truncated")?;
    let name_len = name_bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or("ELF symbol name unterminated")?;
    core::str::from_utf8(&name_bytes[..name_len]).map_err(|_| "ELF symbol name is not UTF-8")
}

fn find_symtab(sections: &[ElfSection]) -> Result<usize, &'static str> {
    sections
        .iter()
        .position(|section| section.section_type == SHT_SYMTAB)
        .ok_or("ELF symbol table missing")
}

fn rel_symbol_value(
    bytes: &[u8],
    sections: &[ElfSection],
    loaded: &[usize],
    symtab_index: usize,
    sym_index: usize,
) -> Result<usize, String> {
    let symtab_section = sections
        .get(symtab_index)
        .ok_or("ELF symbol table missing")?;
    let symtab = bytes
        .get(symtab_section.file_offset..symtab_section.file_offset + symtab_section.size)
        .ok_or("ELF symbol table truncated")?;
    let strtab_section = sections
        .get(symtab_section.link)
        .ok_or("ELF string table missing")?;
    let strtab = bytes
        .get(strtab_section.file_offset..strtab_section.file_offset + strtab_section.size)
        .ok_or("ELF string table truncated")?;
    let sym = read_symbol(symtab, sym_index)?;
    let bind = sym.info >> 4;

    match sym.section_index {
        SHN_UNDEF => {
            let name = sym_name(strtab, &sym)?;
            if name.is_empty() {
                return Ok(0);
            }
            if let Some(addr) = resolve_import(name) {
                return Ok(addr);
            }
            if bind == STB_WEAK {
                return Ok(0);
            }
            Err(alloc::format!("unresolved import: {} (sym={} bind={})", name, sym_index, bind))
        }
        SHN_ABS => Ok(sym.value as usize),
        section_index => {
            let section_index = usize::from(section_index);
            let Some(&base) = loaded.get(section_index) else {
                return Err(String::from("ELF symbol section out of range"));
            };
            if base == 0 {
                return Err(String::from("ELF symbol section not loaded"));
            }
            base.checked_add(sym.value as usize)
                .ok_or_else(|| String::from("ELF symbol address overflow"))
        }
    }
}

fn find_main_addr(
    bytes: &[u8],
    sections: &[ElfSection],
    loaded: &[usize],
    entry_hint: u64,
) -> Result<usize, String> {
    let hinted_section = entry_hint_section(entry_hint) as usize;
    let hinted_offset = entry_hint_offset(entry_hint) as usize;
    if hinted_section > 0
        && let Some(&base) = loaded.get(hinted_section)
        && base != 0
    {
        return base
            .checked_add(hinted_offset)
            .ok_or_else(|| String::from("entry hint overflow"));
    }

    let symtab_index = find_symtab(sections)?;
    let symtab_section = sections
        .get(symtab_index)
        .ok_or("ELF symbol table missing")?;
    let symtab = bytes
        .get(symtab_section.file_offset..symtab_section.file_offset + symtab_section.size)
        .ok_or("ELF symbol table truncated")?;
    let strtab_section = sections
        .get(symtab_section.link)
        .ok_or("ELF string table missing")?;
    let strtab = bytes
        .get(strtab_section.file_offset..strtab_section.file_offset + strtab_section.size)
        .ok_or("ELF string table truncated")?;

    for index in 0..(symtab.len() / ELF64_SYM_LEN) {
        let sym = read_symbol(symtab, index)?;
        if sym.section_index == SHN_UNDEF {
            continue;
        }
        if sym.info & 0x0f != 2 {
            continue;
        }
        if sym_name(strtab, &sym)? != "main" {
            continue;
        }
        let section_index = usize::from(sym.section_index);
        let base = *loaded
            .get(section_index)
            .ok_or_else(|| String::from("ELF main section out of range"))?;
        if base == 0 {
            return Err(String::from("ELF main section not loaded"));
        }
        return base
            .checked_add(sym.value as usize)
            .ok_or_else(|| String::from("ELF main address overflow"));
    }

    Err(String::from("ELF main symbol missing"))
}

fn load_rel_image(bytes: &[u8]) -> Result<LoadedRelImage, String> {
    let sections = parse_sections(bytes)?;
    let mut section_offsets = vec![0usize; sections.len()];
    let mut section_bases = vec![0usize; sections.len()];
    let mut total_size = 0usize;
    let mut max_align = 1usize;

    for (index, section) in sections.iter().enumerate() {
        if section.flags & SHF_ALLOC == 0 {
            continue;
        }
        let align = section.align.max(1);
        max_align = max_align.max(align);
        total_size = align_up(total_size, align)?;
        section_offsets[index] = total_size;
        if section.size != 0 {
            total_size = total_size
                .checked_add(section.size)
                .ok_or_else(|| String::from("ELF image too large"))?;
        }
    }

    if total_size == 0 {
        return Err(String::from("ELF image has no allocatable sections"));
    }

    let layout = Layout::from_size_align(total_size, max_align)
        .map_err(|_| String::from("bad ELF layout"))?;
    let arena_align = 4096usize;
    let slop = layout.align().saturating_sub(arena_align);
    let needed = total_size
        .checked_add(slop)
        .ok_or_else(|| String::from("ELF image too large"))?;
    if needed > PORTAL_IMAGE_ARENA_BYTES {
        return Err(String::from("ELF image exceeds portal arena"));
    }
    let arena_lease = PortalImageArenaLease::acquire()?;
    let arena_ptr = core::ptr::addr_of_mut!(PORTAL_IMAGE_ARENA) as *mut u8;
    let base_addr = align_up(arena_ptr as usize, layout.align())?;
    let base = base_addr as *mut u8;
    unsafe {
        core::ptr::write_bytes(base, 0, total_size);
    }

    for (index, section) in sections.iter().enumerate() {
        if section.flags & SHF_ALLOC == 0 {
            continue;
        }
        let section_base = unsafe { base.add(section_offsets[index]) };
        section_bases[index] = section_base as usize;
        if section.size == 0 {
            continue;
        }
        match section.section_type {
            SHT_PROGBITS => {
                let src = bytes
                    .get(section.file_offset..section.file_offset + section.size)
                    .ok_or_else(|| String::from("ELF section truncated"))?;
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), section_base, section.size);
                }
            }
            SHT_NOBITS => {}
            _ => {}
        }
    }

    let symtab_index = find_symtab(sections.as_slice())?;
    for section in sections.iter() {
        if section.section_type != SHT_RELA {
            continue;
        }
        let Some(target) = sections.get(section.info) else {
            return Err(String::from("ELF relocation target out of range"));
        };
        if target.flags & SHF_ALLOC == 0 {
            continue;
        }
        let target_base = *section_bases
            .get(section.info)
            .ok_or_else(|| String::from("ELF relocation target out of range"))?;
        if target_base == 0 {
            return Err(String::from("ELF relocation target not loaded"));
        }
        if section.entsize != ELF64_RELA_LEN {
            return Err(String::from("unsupported ELF relocation size"));
        }
        let rela = bytes
            .get(section.file_offset..section.file_offset + section.size)
            .ok_or_else(|| String::from("ELF relocation section truncated"))?;
        for chunk in rela.chunks_exact(ELF64_RELA_LEN) {
            let r_offset =
                le_u64(chunk, 0).ok_or_else(|| String::from("ELF relocation truncated"))? as usize;
            let r_info =
                le_u64(chunk, 8).ok_or_else(|| String::from("ELF relocation truncated"))?;
            let r_addend =
                le_u64(chunk, 16).ok_or_else(|| String::from("ELF relocation truncated"))? as i64;
            let r_sym = (r_info >> 32) as usize;
            let r_type = r_info as u32;
            let place = target_base
                .checked_add(r_offset)
                .ok_or_else(|| String::from("ELF relocation place overflow"))?;
            let sym = rel_symbol_value(
                bytes,
                sections.as_slice(),
                section_bases.as_slice(),
                symtab_index,
                r_sym,
            )? as i64;
            let place_i64 = place as i64;
            unsafe {
                match r_type {
                    R_X86_64_NONE => {}
                    R_X86_64_64 => {
                        let value = sym
                            .checked_add(r_addend)
                            .ok_or_else(|| String::from("R_X86_64_64 overflow"))?;
                        (place as *mut u64).write_unaligned(value as u64);
                    }
                    R_X86_64_32S => {
                        let value = sym
                            .checked_add(r_addend)
                            .ok_or_else(|| String::from("R_X86_64_32S overflow"))?;
                        let value_i32 = i32::try_from(value)
                            .map_err(|_| String::from("R_X86_64_32S out of range"))?;
                        (place as *mut i32).write_unaligned(value_i32);
                    }
                    R_X86_64_PC32 | R_X86_64_PLT32 => {
                        let value = sym
                            .checked_add(r_addend)
                            .and_then(|v| v.checked_sub(place_i64))
                            .ok_or_else(|| String::from("R_X86_64_PC32 overflow"))?;
                        let value_i32 = i32::try_from(value)
                            .map_err(|_| String::from("R_X86_64_PC32 out of range"))?;
                        (place as *mut i32).write_unaligned(value_i32);
                    }
                    _ => return Err(alloc::format!("unsupported ELF relocation type: {}", r_type)),
                }
            }
        }
    }

    Ok(LoadedRelImage {
        base,
        used_len: total_size,
        section_bases,
        _arena_lease: arena_lease,
    })
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(raw))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let raw: [u8; 8] = bytes.get(offset..offset + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

pub(crate) fn elf_type_name(bytes: &[u8]) -> Option<&'static str> {
    match le_u16(bytes, 16)? {
        1 => Some("REL"),
        2 => Some("EXEC"),
        3 => Some("DYN"),
        4 => Some("CORE"),
        _ => Some("UNKNOWN"),
    }
}

pub(crate) fn elf_imports<'a>(bytes: &'a [u8]) -> Result<Vec<ElfImport<'a>>, &'static str> {
    if bytes.len() < ELF64_HEADER_LEN {
        return Err("ELF header truncated");
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return Err("payload is not ELF");
    }
    if bytes.get(4).copied() != Some(2) || bytes.get(5).copied() != Some(1) {
        return Err("unsupported ELF class/data");
    }

    let shoff = le_u64(bytes, 40).ok_or("ELF header truncated")? as usize;
    let shentsize = le_u16(bytes, 58).ok_or("ELF header truncated")? as usize;
    let shnum = le_u16(bytes, 60).ok_or("ELF header truncated")? as usize;
    if shentsize != ELF64_SECTION_HEADER_LEN {
        return Err("unsupported ELF section header size");
    }

    let mut imports = Vec::new();
    for section_index in 0..shnum {
        let shdr_off = shoff
            .checked_add(
                section_index
                    .checked_mul(shentsize)
                    .ok_or("ELF section header overflow")?,
            )
            .ok_or("ELF section header overflow")?;
        let shdr = bytes
            .get(shdr_off..shdr_off + ELF64_SECTION_HEADER_LEN)
            .ok_or("ELF section header truncated")?;

        let section_type = le_u32(shdr, 4).ok_or("ELF section header truncated")?;
        if section_type != SHT_SYMTAB {
            continue;
        }

        let sym_off = le_u64(shdr, 24).ok_or("ELF section header truncated")? as usize;
        let sym_size = le_u64(shdr, 32).ok_or("ELF section header truncated")? as usize;
        let link = le_u32(shdr, 40).ok_or("ELF section header truncated")? as usize;
        let entsize = le_u64(shdr, 56).ok_or("ELF section header truncated")? as usize;
        if entsize != ELF64_SYM_LEN {
            return Err("unsupported ELF symbol size");
        }

        let str_shdr_off = shoff
            .checked_add(
                link.checked_mul(shentsize)
                    .ok_or("ELF string table overflow")?,
            )
            .ok_or("ELF string table overflow")?;
        let str_shdr = bytes
            .get(str_shdr_off..str_shdr_off + ELF64_SECTION_HEADER_LEN)
            .ok_or("ELF string table truncated")?;
        let str_off = le_u64(str_shdr, 24).ok_or("ELF string table truncated")? as usize;
        let str_size = le_u64(str_shdr, 32).ok_or("ELF string table truncated")? as usize;
        let strtab = bytes
            .get(str_off..str_off + str_size)
            .ok_or("ELF string table truncated")?;

        let symtab = bytes
            .get(sym_off..sym_off + sym_size)
            .ok_or("ELF symbol table truncated")?;

        for sym in symtab.chunks_exact(ELF64_SYM_LEN) {
            let name_off = le_u32(sym, 0).ok_or("ELF symbol truncated")? as usize;
            let info = *sym.get(4).ok_or("ELF symbol truncated")?;
            let shndx = le_u16(sym, 6).ok_or("ELF symbol truncated")?;
            let bind = info >> 4;
            if shndx != SHN_UNDEF || !(bind == STB_GLOBAL || bind == STB_WEAK) {
                continue;
            }

            let name_bytes = strtab.get(name_off..).ok_or("ELF symbol name truncated")?;
            let name_len = name_bytes
                .iter()
                .position(|&b| b == 0)
                .ok_or("ELF symbol name unterminated")?;
            if name_len == 0 {
                continue;
            }
            let name = core::str::from_utf8(&name_bytes[..name_len])
                .map_err(|_| "ELF symbol name is not UTF-8")?;
            imports.push(ElfImport {
                name,
                resolved_addr: resolve_known_import(name),
            });
        }
    }

    imports.sort_by(|a, b| a.name.cmp(b.name));
    imports.dedup_by(|a, b| a.name == b.name);
    Ok(imports)
}

fn resolve_known_import(name: &str) -> Option<usize> {
    match name {
        "_RNvCs75cmLyI1ip2_7___rustc26___rust_alloc_error_handler"
        | "_RNvCs2csqI13tepL_7___rustc26___rust_alloc_error_handler" => {
            Some(portal_rust_alloc_error_handler as *const () as usize)
        }
        "_RNvCs75cmLyI1ip2_7___rustc35___rust_no_alloc_shim_is_unstable_v2"
        | "_RNvCs2csqI13tepL_7___rustc35___rust_no_alloc_shim_is_unstable_v2" => {
            Some(portal_no_alloc_shim_is_unstable_v2 as *const () as usize)
        }
        "memcpy" => Some(trueos_qjs::trueos_shims::memcpy as *const () as usize),
        "memmove" => Some(trueos_qjs::trueos_shims::memmove as *const () as usize),
        "memset" => Some(trueos_qjs::trueos_shims::memset as *const () as usize),
        "memcmp" => Some(trueos_qjs::trueos_shims::memcmp as *const () as usize),
        "strlen" => Some(trueos_qjs::trueos_shims::strlen as *const () as usize),
        "__rust_alloc"
        | "_RNvCs75cmLyI1ip2_7___rustc12___rust_alloc"
        | "_RNvCs2csqI13tepL_7___rustc12___rust_alloc" => {
            Some(portal_rust_alloc as *const () as usize)
        }
        "__rust_dealloc"
        | "_RNvCs75cmLyI1ip2_7___rustc14___rust_dealloc"
        | "_RNvCs2csqI13tepL_7___rustc14___rust_dealloc" => {
            Some(portal_rust_dealloc as *const () as usize)
        }
        "__rust_realloc"
        | "_RNvCs75cmLyI1ip2_7___rustc14___rust_realloc"
        | "_RNvCs2csqI13tepL_7___rustc14___rust_realloc" => {
            Some(portal_rust_realloc as *const () as usize)
        }
        "__rust_alloc_zeroed"
        | "_RNvCs75cmLyI1ip2_7___rustc19___rust_alloc_zeroed"
        | "_RNvCs2csqI13tepL_7___rustc19___rust_alloc_zeroed" => {
            Some(portal_rust_alloc_zeroed as *const () as usize)
        }
        _ => resolve_std_abi_import(name).or_else(|| resolve_cabi_import(name)),
    }
}

fn resolve_import(name: &str) -> Option<usize> {
    resolve_known_import(name).or_else(|| resolve_unresolved_import(name))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn resolve_std_abi_import(name: &str) -> Option<usize> {
    match name {
        "sys_alloc_words" => Some(crate::std_abi_shim::sys_alloc_words as *const () as usize),
        "sys_alloc_aligned" => Some(crate::std_abi_shim::sys_alloc_aligned as *const () as usize),
        "sys_rand" => Some(crate::std_abi_shim::sys_rand as *const () as usize),
        "sys_write" => Some(crate::std_abi_shim::sys_write as *const () as usize),
        "trueos_internal_log_write" => {
            Some(crate::std_abi_shim::trueos_internal_log_write as *const () as usize)
        }
        "sys_read" => Some(crate::std_abi_shim::sys_read as *const () as usize),
        "sys_getenv" => Some(crate::std_abi_shim::sys_getenv as *const () as usize),
        "sys_argc" => Some(crate::std_abi_shim::sys_argc as *const () as usize),
        "sys_argv" => Some(crate::std_abi_shim::sys_argv as *const () as usize),
        "sys_output" => Some(crate::std_abi_shim::sys_output as *const () as usize),
        "sys_sha_compress" => Some(crate::std_abi_shim::sys_sha_compress as *const () as usize),
        "sys_sha_buffer" => Some(crate::std_abi_shim::sys_sha_buffer as *const () as usize),
        "sys_log" => Some(crate::std_abi_shim::sys_log as *const () as usize),
        "sys_cycle_count" => Some(crate::std_abi_shim::sys_cycle_count as *const () as usize),
        "sys_panic" => Some(crate::std_abi_shim::sys_panic as *const () as usize),
        "sys_halt" => Some(crate::std_abi_shim::sys_halt as *const () as usize),
        "trueos_tokio_time_now_nanos" => {
            Some(crate::std_abi_shim::trueos_tokio_time_now_nanos as *const () as usize)
        }
        "trueos_octocrab_unix_time_seconds" => {
            Some(crate::std_abi_shim::trueos_octocrab_unix_time_seconds as *const () as usize)
        }
        "trueos_mio_tcp_listener_bind" => {
            Some(crate::mio_compat::trueos_mio_tcp_listener_bind as *const () as usize)
        }
        "trueos_mio_tcp_stream_connect" => {
            Some(crate::mio_compat::trueos_mio_tcp_stream_connect as *const () as usize)
        }
        "trueos_mio_udp_socket_bind" => {
            Some(crate::mio_compat::trueos_mio_udp_socket_bind as *const () as usize)
        }
        "trueos_mio_socket_close" => {
            Some(crate::mio_compat::trueos_mio_socket_close as *const () as usize)
        }
        "trueos_mio_socket_local_addr" => {
            Some(crate::mio_compat::trueos_mio_socket_local_addr as *const () as usize)
        }
        "trueos_mio_socket_peer_addr" => {
            Some(crate::mio_compat::trueos_mio_socket_peer_addr as *const () as usize)
        }
        "trueos_mio_socket_take_error" => {
            Some(crate::mio_compat::trueos_mio_socket_take_error as *const () as usize)
        }
        "trueos_mio_tcp_stream_read" => {
            Some(crate::mio_compat::trueos_mio_tcp_stream_read as *const () as usize)
        }
        "trueos_mio_tcp_stream_write" => {
            Some(crate::mio_compat::trueos_mio_tcp_stream_write as *const () as usize)
        }
        "trueos_mio_udp_socket_connect" => {
            Some(crate::mio_compat::trueos_mio_udp_socket_connect as *const () as usize)
        }
        "trueos_mio_udp_socket_send_to" => {
            Some(crate::mio_compat::trueos_mio_udp_socket_send_to as *const () as usize)
        }
        "trueos_mio_udp_socket_recv_from" => {
            Some(crate::mio_compat::trueos_mio_udp_socket_recv_from as *const () as usize)
        }
        "trueos_mio_tcp_listener_accept" => {
            Some(crate::mio_compat::trueos_mio_tcp_listener_accept as *const () as usize)
        }
        "trueos_mio_selector_register_socket" => {
            Some(crate::mio_compat::trueos_mio_selector_register_socket as *const () as usize)
        }
        "trueos_mio_selector_deregister_socket" => {
            Some(crate::mio_compat::trueos_mio_selector_deregister_socket as *const () as usize)
        }
        "trueos_mio_selector_poll" => {
            Some(crate::mio_compat::trueos_mio_selector_poll as *const () as usize)
        }
        _ => None,
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn resolve_std_abi_import(_name: &str) -> Option<usize> {
    None
}

unsafe extern "C" fn portal_rust_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let Ok(layout) = Layout::from_size_align(size, align.max(1)) else {
        return core::ptr::null_mut();
    };

    unsafe { crate::allocators::alloc_raw(layout) }
}

unsafe extern "C" fn portal_rust_dealloc(ptr: *mut u8, _size: usize, _align: usize) {
    unsafe { crate::allocators::dealloc_raw(ptr) }
}

unsafe extern "C" fn portal_rust_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return unsafe { portal_rust_alloc(new_size, align) };
    }

    if new_size == 0 {
        unsafe { crate::allocators::dealloc_raw(ptr) };
        return core::ptr::null_mut();
    }

    let new_ptr = unsafe { portal_rust_alloc(new_size, align) };
    if new_ptr.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(old_size, new_size));
        crate::allocators::dealloc_raw(ptr);
    }
    new_ptr
}

unsafe extern "C" fn portal_rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    let ptr = unsafe { portal_rust_alloc(size, align) };
    if !ptr.is_null() {
        unsafe { core::ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

fn build_argv(args: &[String]) -> (Vec<Vec<u8>>, Vec<*const c_char>) {
    let mut arg_storage = Vec::with_capacity(args.len());
    let mut argv = Vec::with_capacity(args.len());
    for arg in args {
        let mut bytes = arg.as_bytes().to_vec();
        bytes.retain(|&b| b != 0);
        bytes.push(0);
        argv.push(bytes.as_ptr() as *const c_char);
        arg_storage.push(bytes);
    }
    (arg_storage, argv)
}

pub(crate) fn build_process_args(archive: &str, app_args: &[String]) -> Vec<String> {
    let mut args = Vec::with_capacity(app_args.len().saturating_add(1));
    args.push(String::from(archive));
    args.extend(app_args.iter().cloned());
    args
}

pub(crate) fn build_process_env(archive: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert(String::from("PWD"), String::from("/"));
    vars.insert(String::from("TRUEOS_APP_ARCHIVE"), String::from(archive));
    vars
}

pub(crate) fn parse_blueprint(bytes: &[u8]) -> Result<BlueprintModule<'_>, &'static str> {
    if bytes.len() < BLUEPRINT_HEADER_LEN {
        return Err("module truncated");
    }
    if bytes.get(0..4) != Some(b"TRBP") {
        return Err("bad blueprint magic");
    }

    let version = le_u16(bytes, 4).ok_or("module truncated")?;
    let flags = le_u16(bytes, 6).ok_or("module truncated")?;
    let entry = le_u64(bytes, 8).ok_or("module truncated")?;
    let payload_len = le_u32(bytes, 16).ok_or("module truncated")? as usize;
    let raw_payload_len = le_u32(bytes, 20).ok_or("module truncated")? as usize;
    let payload_end = BLUEPRINT_HEADER_LEN
        .checked_add(payload_len)
        .ok_or("module too large")?;
    let payload = bytes
        .get(BLUEPRINT_HEADER_LEN..payload_end)
        .ok_or("payload truncated")?;

    Ok(BlueprintModule {
        version,
        flags,
        entry,
        raw_payload_len,
        payload,
    })
}

pub(crate) fn unpack_blueprint(module: &BlueprintModule<'_>) -> Result<Vec<u8>, &'static str> {
    match module.flags {
        1 => Ok(module.payload.to_vec()),
        2 => crate::z7::extract_single_file_to_vec(module.payload)
            .map_err(|_| "7z payload decode failed"),
        _ => Err("unsupported blueprint payload flags"),
    }
}

pub(crate) fn invoke_host_rel(
    unpacked: &[u8],
    entry_hint: u64,
    process_args: Vec<String>,
    process_env: BTreeMap<String, String>,
    console_target: Option<crate::shell2::MatrixTarget>,
) -> Result<(), String> {
    let image = load_rel_image(unpacked)?;
    let sections = parse_sections(unpacked)?;
    let main_addr =
        find_main_addr(unpacked, sections.as_slice(), image.section_bases.as_slice(), entry_hint)?;
    let (_arg_storage, argv) = build_argv(process_args.as_slice());
    let main_fn: extern "C" fn(usize, *const *const c_char) =
        unsafe { core::mem::transmute(main_addr) };
    crate::r::io::env::with_launch_context_console(
        process_args,
        process_env,
        console_target,
        || {
            main_fn(
                argv.len(),
                if argv.is_empty() {
                    core::ptr::null()
                } else {
                    argv.as_ptr()
                },
            );
        },
    );
    drop(image);
    Ok(())
}
