use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicUsize, Ordering};
use object::read::elf::{ElfFile64, SectionHeader as _};
use object::{
    Architecture, Object, ObjectKind, ObjectSection, ObjectSymbol, RelocationFlags,
    RelocationTarget, SectionFlags, SectionIndex, SymbolFlags, SymbolIndex, SymbolSection,
};
use sha2::{Digest, Sha256};
use spin::Mutex;

const BLUEPRINT_HEADER_LEN: usize = 24;
const BLUEPRINT_PAYLOAD_MASK: u16 = 0x00FF;
const BLUEPRINT_PAYLOAD_RAW: u16 = 1;
const BLUEPRINT_PAYLOAD_7Z: u16 = 2;
pub(crate) const BLUEPRINT_CAP_REPLICATABLE: u16 = 1 << 8;
const ELF64_HEADER_LEN: usize = 64;
const ELF64_RELA_LEN: usize = 24;
const SHT_PROGBITS: u32 = 1;
const SHT_NOBITS: u32 = 8;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const SHF_ALLOC: u64 = 0x2;
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_GOTPCREL: u32 = 9;
const R_X86_64_32S: u32 = 11;
const R_X86_64_GOTPCRELX: u32 = 41;
const R_X86_64_REX_GOTPCRELX: u32 = 42;
const IMPORT_THUNK_ALIGN: usize = 16;
const IMPORT_THUNK_SIZE: usize = 16;
const EMBEDDED_ASSET_SECTION: &str = ".trueos.assets";
const ASSET_HEADER_LEN: usize = 12;
const ASSET_ENTRY_HEADER_LEN: usize = 4 + 8 + 32;

pub(crate) struct BlueprintModule<'a> {
    pub(crate) version: u16,
    pub(crate) flags: u16,
    pub(crate) entry: u64,
    pub(crate) raw_payload_len: usize,
    pub(crate) payload: &'a [u8],
}

impl BlueprintModule<'_> {
    pub(crate) const fn is_replicatable(&self) -> bool {
        self.flags & BLUEPRINT_CAP_REPLICATABLE != 0
    }
}

pub(crate) struct ElfImport<'a> {
    pub(crate) name: &'a str,
    pub(crate) resolved_addr: Option<usize>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedAssetStats {
    pub(crate) entries: usize,
    pub(crate) bytes: usize,
}

#[derive(Copy, Clone)]
struct EmbeddedAssetEntry<'a> {
    path: &'a str,
    data: &'a [u8],
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ElfAllocStats {
    pub(crate) sections: usize,
    pub(crate) alloc_sections: usize,
    pub(crate) alloc_bytes: usize,
}

type ParsedElf<'data> = ElfFile64<'data, object::Endianness>;

#[derive(Copy, Clone)]
struct ParsedElfSymbol<'data> {
    name: &'data str,
    info: u8,
    section: SymbolSection,
    value: u64,
}

struct LoadedRelImage {
    base: *mut u8,
    used_len: usize,
    backing: PortalImageBacking,
    section_bases: Vec<usize>,
}

impl Drop for LoadedRelImage {
    fn drop(&mut self) {
        let _ = self.base;
        let _ = &self.backing;
        let _ = self.used_len;
    }
}

enum PortalImageBacking {
    Dynamic { base: *mut u8, layout: Layout },
}

impl Drop for PortalImageBacking {
    fn drop(&mut self) {
        match self {
            Self::Dynamic { base, layout } => {
                unsafe {
                    crate::allocators::dealloc_raw(*base);
                }
                let _ = layout;
            }
        }
    }
}

struct PortalImageAllocationGuard {
    backing: Option<PortalImageBacking>,
}

impl PortalImageAllocationGuard {
    fn disarm(mut self) -> PortalImageBacking {
        self.backing.take().expect("portal image backing missing")
    }
}

impl Drop for PortalImageAllocationGuard {
    fn drop(&mut self) {
        let _ = self.backing.take();
    }
}

struct PortalImageAllocation {
    base: *mut u8,
    guard: PortalImageAllocationGuard,
}

fn portal_guest_alloc_vm_id() -> Option<u8> {
    crate::hv::current_hull_guest_context_vm_id()
        .or_else(crate::hv::current_vm_id)
        .or_else(|| {
            let domain = crate::r::kernel_task_domain::current();
            if matches!(
                domain.domain,
                crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier
                    | crate::r::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc
            ) {
                domain.vm_id
            } else {
                None
            }
        })
}

impl PortalImageAllocation {
    fn allocate(layout: Layout) -> Result<Self, String> {
        let alloc = || unsafe { crate::allocators::alloc_raw(layout) };
        let vm_id = portal_guest_alloc_vm_id();
        let base = if let Some(vm_id) = vm_id {
            unsafe { crate::allocators::alloc_raw_hv_guest(vm_id, layout) }
        } else {
            alloc()
        };
        if base.is_null() {
            return Err(alloc::format!(
                "portal image allocation failed size={} align={}",
                layout.size(),
                layout.align()
            ));
        }
        if PORTAL_IMAGE_ALLOC_TRACE_COUNT.fetch_add(1, Ordering::Relaxed) < 8 {
            let vm_for_stats = vm_id.unwrap_or(0);
            let stats = crate::allocators::hv_guest_heap_stats(vm_for_stats);
            crate::hv::hvlogf(format_args!(
                "hv: rel image alloc vm={:?} size={} align={} ptr=0x{:016X} free_bytes={} largest_free={} free_blocks={} guest_heap=[0x{:016X}..0x{:016X})",
                vm_id,
                layout.size(),
                layout.align(),
                base as usize,
                stats.free_bytes,
                stats.largest_free_block,
                stats.free_blocks,
                stats.heap_start,
                stats.heap_end,
            ));
        }
        Ok(Self {
            base,
            guard: PortalImageAllocationGuard {
                backing: Some(PortalImageBacking::Dynamic { base, layout }),
            },
        })
    }

    fn disarm(self) -> PortalImageBacking {
        self.guard.disarm()
    }
}

const PORTAL_IMAGE_CAP_BYTES: usize = crate::allcaps::blueprint::PORTAL_IMAGE_CAP_BYTES;
static UNRESOLVED_IMPORT_STUBS: Mutex<Vec<UnresolvedImportStub>> = Mutex::new(Vec::new());
static PORTAL_IMAGE_ALLOC_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static PORTAL_RUST_ALLOC_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);

struct UnresolvedImportStub {
    name: String,
    warned: bool,
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
        if is_rustc_runtime_import(name.as_str()) {
            crate::hv::hvlogf(format_args!(
                "portal: WARNING unresolved rust runtime import invoked name={} action=return-zero likely=missed-nightly-symbol-hash update=dynamic-rustc-import-resolver",
                name
            ));
        } else {
            crate::hv::hvlogf(format_args!("portal: joker import invoked name={}", name));
        }
    }
    0
}

fn is_rustc_runtime_import(name: &str) -> bool {
    if matches!(
        name,
        "__rust_alloc"
            | "__rust_dealloc"
            | "__rust_realloc"
            | "__rust_alloc_zeroed"
            | "__rust_alloc_error_handler"
            | "__rust_no_alloc_shim_is_unstable_v2"
    ) {
        return true;
    }

    let Some((_, rustc_tail)) = name.rsplit_once("___rustc") else {
        return false;
    };

    rustc_tail.contains("___rust_alloc")
        || rustc_tail.contains("___rust_dealloc")
        || rustc_tail.contains("___rust_realloc")
        || rustc_tail.contains("___rust_alloc_zeroed")
        || rustc_tail.contains("___rust_alloc_error_handler")
        || rustc_tail.contains("___rust_no_alloc_shim_is_unstable")
}

pub(crate) fn is_joker_import(name: &str) -> bool {
    is_rustc_runtime_import(name)
}

pub(crate) fn rustc_runtime_import_note(name: &str) -> Option<&'static str> {
    if name == "__rust_no_alloc_shim_is_unstable_v2"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_no_alloc_shim_is_unstable"))
    {
        return Some(
            "class=rustc-no-alloc-shim need=noop-provider reason=allocator-presence-marker",
        );
    }
    if name == "__rust_alloc"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_alloc"))
    {
        return Some("class=rustc-alloc need=portal-rust-alloc");
    }
    if name == "__rust_dealloc"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_dealloc"))
    {
        return Some("class=rustc-dealloc need=portal-rust-dealloc");
    }
    if name == "__rust_realloc"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_realloc"))
    {
        return Some("class=rustc-realloc need=portal-rust-realloc");
    }
    if name == "__rust_alloc_zeroed"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_alloc_zeroed"))
    {
        return Some("class=rustc-alloc-zeroed need=portal-rust-alloc-zeroed");
    }
    if name == "__rust_alloc_error_handler"
        || name
            .rsplit_once("___rustc")
            .is_some_and(|(_, tail)| tail.contains("___rust_alloc_error_handler"))
    {
        return Some("class=rustc-alloc-error-handler need=portal-rust-alloc-error-handler");
    }
    None
}

fn portal_logf(args: core::fmt::Arguments<'_>) {
    if crate::log_os::flags::PORTAL_LOGS {
        crate::log!("{}\n", args);
    }
}

fn resolve_unresolved_import(name: &str) -> Option<usize> {
    if is_rustc_runtime_import(name) {
        crate::hv::hvwarnf(format_args!(
            "portal: WARNING unresolved rust runtime import registered name={} class=rustc-runtime note=missed-nightly-symbol-hash action=joker-stub-installed",
            name
        ));
    }
    let slot = unresolved_import_stub_slot(name)?;
    Some(UNRESOLVED_IMPORT_STUB_FNS[slot] as *const () as usize)
}

fn portal_alloc_error_handler(layout: Layout) -> ! {
    crate::hv::hverrorf(format_args!(
        "portal: alloc error size={} align={}",
        layout.size(),
        layout.align()
    ));
    let stats = crate::allocators::hv_guest_heap_stats(crate::hv::current_vm_id().unwrap_or(0));
    crate::hv::hvlogf(format_args!(
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
    crate::hv::hvlogf(format_args!(
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

type PortalUnwindReasonCode = u32;
type PortalUnwindTraceFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> PortalUnwindReasonCode;

const PORTAL_URC_END_OF_STACK: PortalUnwindReasonCode = 5;

unsafe extern "C" fn portal_unwind_backtrace(
    _trace: Option<PortalUnwindTraceFn>,
    _trace_argument: *mut c_void,
) -> PortalUnwindReasonCode {
    PORTAL_URC_END_OF_STACK
}

unsafe extern "C" fn portal_unwind_get_ip(_context: *mut c_void) -> usize {
    0
}

unsafe extern "C" fn portal_unwind_get_cfa(_context: *mut c_void) -> usize {
    0
}

unsafe extern "C" fn portal_unwind_find_enclosing_function(_pc: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

unsafe extern "C" fn portal_unwind_raise_exception(
    _exception: *mut c_void,
) -> PortalUnwindReasonCode {
    PORTAL_URC_END_OF_STACK
}

include!(concat!(env!("OUT_DIR"), "/generated_portal_imports.rs"));
include!(concat!(env!("OUT_DIR"), "/generated_ring_runtime_imports.rs"));

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

fn parse_elf(bytes: &[u8]) -> Result<ParsedElf<'_>, &'static str> {
    if bytes.len() < ELF64_HEADER_LEN {
        return Err("ELF header truncated");
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return Err("payload is not ELF");
    }
    if bytes.get(4).copied() != Some(2) || bytes.get(5).copied() != Some(1) {
        return Err("unsupported ELF class/data");
    }
    ParsedElf::parse(bytes).map_err(|_| "malformed ELF")
}

fn section_is_alloc(flags: SectionFlags) -> bool {
    matches!(flags, SectionFlags::Elf { sh_flags } if sh_flags & SHF_ALLOC != 0)
}

fn section_type(file: &ParsedElf<'_>, index: SectionIndex) -> Result<u32, &'static str> {
    file.elf_section_table()
        .section(index)
        .map(|section| section.sh_type(file.endian()))
        .map_err(|_| "ELF section out of range")
}

fn require_symtab(file: &ParsedElf<'_>) -> Result<(), &'static str> {
    if file.elf_symbol_table().is_empty() {
        Err("ELF symbol table missing")
    } else {
        Ok(())
    }
}

fn read_symbol<'data>(
    file: &ParsedElf<'data>,
    sym_index: usize,
) -> Result<ParsedElfSymbol<'data>, String> {
    require_symtab(file)?;
    if sym_index == 0 {
        return Ok(ParsedElfSymbol {
            name: "",
            info: 0,
            section: SymbolSection::Undefined,
            value: 0,
        });
    }
    let symbol = file
        .symbol_by_index(SymbolIndex(sym_index))
        .map_err(|_| String::from("ELF symbol truncated"))?;
    let name = symbol
        .name()
        .map_err(|_| String::from("ELF symbol name is not UTF-8"))?;
    let info = match symbol.flags() {
        SymbolFlags::Elf { st_info, .. } => st_info,
        _ => return Err(String::from("ELF symbol metadata missing")),
    };
    Ok(ParsedElfSymbol {
        name,
        info,
        section: symbol.section(),
        value: symbol.address(),
    })
}

fn rel_symbol_value(
    file: &ParsedElf<'_>,
    loaded: &[usize],
    sym_index: usize,
) -> Result<usize, String> {
    let sym = read_symbol(file, sym_index)?;
    let bind = sym.info >> 4;

    match sym.section {
        SymbolSection::Undefined => {
            if sym.name.is_empty() {
                return Ok(0);
            }
            if let Some(addr) = resolve_import(sym.name) {
                return Ok(addr);
            }
            if bind == STB_WEAK {
                return Ok(0);
            }
            Err(alloc::format!("unresolved import: {} (sym={} bind={})", sym.name, sym_index, bind))
        }
        SymbolSection::Absolute => usize::try_from(sym.value)
            .map_err(|_| String::from("ELF absolute symbol address out of range")),
        SymbolSection::Section(section_index) => {
            let Some(&base) = loaded.get(section_index.0) else {
                return Err(String::from("ELF symbol section out of range"));
            };
            if base == 0 {
                return Err(String::from("ELF symbol section not loaded"));
            }
            let value = usize::try_from(sym.value)
                .map_err(|_| String::from("ELF symbol value out of range"))?;
            base.checked_add(value)
                .ok_or_else(|| String::from("ELF symbol address overflow"))
        }
        _ => Err(String::from("unsupported ELF symbol section")),
    }
}

fn find_main_addr(
    file: &ParsedElf<'_>,
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

    require_symtab(file)?;
    let mut rust_main: Option<(usize, usize)> = None;
    for index in 0..file.elf_symbol_table().len() {
        let sym = read_symbol(file, index)?;
        let SymbolSection::Section(section_index) = sym.section else {
            continue;
        };
        if sym.info & 0x0f != 2 {
            continue;
        }
        if sym.name != "main" && !looks_like_rust_main_symbol(sym.name) {
            continue;
        }
        let base = *loaded
            .get(section_index.0)
            .ok_or_else(|| String::from("ELF main section out of range"))?;
        if base == 0 {
            return Err(String::from("ELF main section not loaded"));
        }
        let value = usize::try_from(sym.value)
            .map_err(|_| String::from("ELF main symbol value out of range"))?;
        let addr = base
            .checked_add(value)
            .ok_or_else(|| String::from("ELF main address overflow"))?;
        if sym.name == "main" {
            return Ok(addr);
        }
        let prefer_rust_main = match &rust_main {
            Some((_, best_len)) => sym.name.len() < *best_len,
            None => true,
        };
        if prefer_rust_main {
            rust_main = Some((addr, sym.name.len()));
        }
    }

    if let Some((addr, _)) = rust_main {
        return Ok(addr);
    }

    Err(String::from("ELF main symbol missing"))
}

fn looks_like_rust_main_symbol(name: &str) -> bool {
    (name.starts_with("_R") && name.ends_with("4main"))
        || (name.starts_with("_ZN") && name.contains("4main17h") && name.ends_with('E'))
}

fn best_entry_symbol<'a>(
    file: &ParsedElf<'a>,
) -> Result<Option<(&'static str, String, u16, u64)>, String> {
    require_symtab(file)?;
    let mut rust_main: Option<(String, u16, u64)> = None;

    for sym_index in 0..file.elf_symbol_table().len() {
        let sym = read_symbol(file, sym_index)?;
        let SymbolSection::Section(section_index) = sym.section else {
            continue;
        };
        if sym.info & 0x0f != 2 {
            continue;
        }
        let section_index = u16::try_from(section_index.0)
            .map_err(|_| String::from("ELF symbol section index out of range"))?;
        if sym.name == "main" {
            return Ok(Some(("exact", String::from(sym.name), section_index, sym.value)));
        }
        if looks_like_rust_main_symbol(sym.name) {
            let prefer = match &rust_main {
                Some((best_name, _, _)) => sym.name.len() < best_name.len(),
                None => true,
            };
            if prefer {
                rust_main = Some((String::from(sym.name), section_index, sym.value));
            }
        }
    }

    Ok(rust_main.map(|(name, section_index, value)| ("rust", name, section_index, value)))
}

fn abbreviate_symbol_name(name: &str) -> String {
    const HEAD: usize = 24;
    const TAIL: usize = 20;
    if name.len() <= HEAD + TAIL + 2 {
        return String::from(name);
    }
    alloc::format!("{}..{}", &name[..HEAD], &name[name.len() - TAIL..])
}

fn is_gotpc_rel_relocation(r_type: u32) -> bool {
    matches!(r_type, R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX)
}

fn relocation_type(relocation: &object::Relocation) -> Result<u32, String> {
    match relocation.flags() {
        RelocationFlags::Elf { r_type } => Ok(r_type),
        _ => Err(String::from("unsupported non-ELF relocation")),
    }
}

fn relocation_symbol(relocation: &object::Relocation) -> Result<usize, String> {
    match relocation.target() {
        RelocationTarget::Symbol(index) => Ok(index.0),
        RelocationTarget::Absolute => Ok(0),
        _ => Err(String::from("unsupported ELF relocation target")),
    }
}

fn validate_alloc_relocations(file: &ParsedElf<'_>) -> Result<(), String> {
    let endian = file.endian();
    let data = file.data();
    let sections = file.elf_section_table();
    let symtab_section = file.elf_symbol_table().section();
    for raw_section in sections.iter() {
        let section_type = raw_section.sh_type(endian);
        if section_type != object::elf::SHT_RELA && section_type != object::elf::SHT_REL {
            continue;
        }
        let target_index = raw_section.info_link(endian);
        let target = file
            .section_by_index(target_index)
            .map_err(|_| String::from("ELF relocation target out of range"))?;
        if !section_is_alloc(target.flags()) {
            continue;
        }
        if section_type != object::elf::SHT_RELA {
            return Err(String::from("unsupported ELF REL relocation"));
        }
        if raw_section.link(endian) != symtab_section {
            return Err(String::from("unsupported ELF relocation symbol table"));
        }
        let entsize = usize::try_from(raw_section.sh_entsize(endian))
            .map_err(|_| String::from("unsupported ELF relocation size"))?;
        if entsize != ELF64_RELA_LEN {
            return Err(String::from("unsupported ELF relocation size"));
        }
        raw_section
            .rela(endian, data)
            .map_err(|_| String::from("ELF relocation section truncated"))?
            .ok_or_else(|| String::from("ELF relocation section missing"))?;
    }
    Ok(())
}

fn collect_gotpc_rel_symbols(file: &ParsedElf<'_>) -> Result<Vec<usize>, String> {
    let mut symbols = BTreeMap::new();
    for target in file.sections() {
        if !section_is_alloc(target.flags()) {
            continue;
        }
        for (_, relocation) in target.relocations() {
            if relocation.has_implicit_addend() {
                return Err(String::from("unsupported ELF REL relocation"));
            }
            if is_gotpc_rel_relocation(relocation_type(&relocation)?) {
                symbols.insert(relocation_symbol(&relocation)?, ());
            }
        }
    }
    Ok(symbols.keys().copied().collect())
}

fn collect_pc_relative_import_symbols(
    file: &ParsedElf<'_>,
) -> Result<BTreeMap<usize, usize>, String> {
    let mut imports = BTreeMap::new();
    for target in file.sections() {
        if !section_is_alloc(target.flags()) {
            continue;
        }
        for (_, relocation) in target.relocations() {
            if relocation.has_implicit_addend() {
                return Err(String::from("unsupported ELF REL relocation"));
            }
            match relocation_type(&relocation)? {
                R_X86_64_PC32 | R_X86_64_PLT32 => {}
                _ => continue,
            }
            let r_sym = relocation_symbol(&relocation)?;
            let sym = read_symbol(file, r_sym)?;
            if sym.section != SymbolSection::Undefined || sym.name.is_empty() {
                continue;
            }
            if let Some(addr) = resolve_import(sym.name) {
                imports.insert(r_sym, addr);
                continue;
            }
            if sym.info >> 4 == STB_WEAK {
                continue;
            }
            return Err(alloc::format!(
                "unresolved import: {} (sym={} bind={})",
                sym.name,
                r_sym,
                sym.info >> 4
            ));
        }
    }
    Ok(imports)
}

unsafe fn write_import_thunk(thunk: *mut u8, target: usize) {
    // movabs r11, target; jmp r11. Keeps PC-relative calls inside the image.
    unsafe {
        *thunk.add(0) = 0x49;
        *thunk.add(1) = 0xBB;
        (thunk.add(2) as *mut u64).write_unaligned(target as u64);
        *thunk.add(10) = 0x41;
        *thunk.add(11) = 0xFF;
        *thunk.add(12) = 0xE3;
        for offset in 13..IMPORT_THUNK_SIZE {
            *thunk.add(offset) = 0x90;
        }
    }
}

fn relocation_place(
    target_base: usize,
    target_size: usize,
    offset: u64,
    write_size: usize,
) -> Result<usize, String> {
    let offset =
        usize::try_from(offset).map_err(|_| String::from("ELF relocation offset out of range"))?;
    let end = offset
        .checked_add(write_size)
        .ok_or_else(|| String::from("ELF relocation place overflow"))?;
    if end > target_size {
        return Err(String::from("ELF relocation write outside target section"));
    }
    target_base
        .checked_add(offset)
        .ok_or_else(|| String::from("ELF relocation place overflow"))
}

fn load_rel_image(bytes: &[u8]) -> Result<LoadedRelImage, String> {
    let file = parse_elf(bytes)?;
    if file.kind() != ObjectKind::Relocatable {
        return Err(String::from("only ELF REL images can be loaded"));
    }
    if file.architecture() != Architecture::X86_64 {
        return Err(String::from("unsupported ELF architecture"));
    }
    require_symtab(&file)?;
    validate_alloc_relocations(&file)?;
    let section_count = file.elf_section_table().len();
    let mut section_offsets = vec![0usize; section_count];
    let mut section_bases = vec![0usize; section_count];
    let mut total_size = 0usize;
    let mut max_align = 1usize;
    let got_symbols = collect_gotpc_rel_symbols(&file)?;
    let import_thunk_symbols = collect_pc_relative_import_symbols(&file)?;

    for section in file.sections() {
        if !section_is_alloc(section.flags()) {
            continue;
        }
        let index = section.index().0;
        let align = usize::try_from(section.align())
            .map_err(|_| String::from("ELF section alignment out of range"))?
            .max(1);
        if !align.is_power_of_two() {
            return Err(String::from("ELF section alignment is not a power of two"));
        }
        let section_size = usize::try_from(section.size())
            .map_err(|_| String::from("ELF section size out of range"))?;
        max_align = max_align.max(align);
        total_size = align_up(total_size, align)?;
        section_offsets[index] = total_size;
        if section_size != 0 {
            total_size = total_size
                .checked_add(section_size)
                .ok_or_else(|| String::from("ELF image too large"))?;
        }
    }

    if total_size == 0 {
        return Err(String::from("ELF image has no allocatable sections"));
    }

    let synthetic_got_offset = if got_symbols.is_empty() {
        None
    } else {
        max_align = max_align.max(8);
        total_size = align_up(total_size, 8)?;
        let offset = total_size;
        let got_bytes = got_symbols
            .len()
            .checked_mul(core::mem::size_of::<u64>())
            .ok_or_else(|| String::from("ELF synthetic GOT too large"))?;
        total_size = total_size
            .checked_add(got_bytes)
            .ok_or_else(|| String::from("ELF image too large"))?;
        Some(offset)
    };

    let synthetic_import_thunk_offset = if import_thunk_symbols.is_empty() {
        None
    } else {
        max_align = max_align.max(IMPORT_THUNK_ALIGN);
        total_size = align_up(total_size, IMPORT_THUNK_ALIGN)?;
        let offset = total_size;
        let thunk_bytes = import_thunk_symbols
            .len()
            .checked_mul(IMPORT_THUNK_SIZE)
            .ok_or_else(|| String::from("ELF import thunk table too large"))?;
        total_size = total_size
            .checked_add(thunk_bytes)
            .ok_or_else(|| String::from("ELF image too large"))?;
        Some(offset)
    };

    let layout = Layout::from_size_align(total_size, max_align)
        .map_err(|_| String::from("bad ELF layout"))?;
    if total_size > PORTAL_IMAGE_CAP_BYTES {
        return Err(alloc::format!(
            "portal image exceeds cap size={} cap={}",
            total_size,
            PORTAL_IMAGE_CAP_BYTES
        ));
    }
    let allocation = PortalImageAllocation::allocate(layout)?;
    let base = allocation.base;
    unsafe {
        core::ptr::write_bytes(base, 0, total_size);
    }

    for section in file.sections() {
        if !section_is_alloc(section.flags()) {
            continue;
        }
        let index = section.index().0;
        let section_size = usize::try_from(section.size())
            .map_err(|_| String::from("ELF section size out of range"))?;
        let section_base = unsafe { base.add(section_offsets[index]) };
        section_bases[index] = section_base as usize;
        if section_size == 0 {
            continue;
        }
        match section_type(&file, section.index())? {
            SHT_PROGBITS => {
                let src = section
                    .data()
                    .map_err(|_| String::from("ELF section truncated"))?;
                if src.len() != section_size {
                    return Err(String::from("ELF section size mismatch"));
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), section_base, section_size);
                }
            }
            SHT_NOBITS => {}
            _ => {}
        }
    }

    let mut synthetic_got_entries = BTreeMap::new();
    if let Some(got_offset) = synthetic_got_offset {
        for (slot, sym_index) in got_symbols.iter().copied().enumerate() {
            let entry = unsafe { base.add(got_offset + slot * core::mem::size_of::<u64>()) };
            let sym = rel_symbol_value(&file, section_bases.as_slice(), sym_index)?;
            unsafe {
                (entry as *mut u64).write_unaligned(sym as u64);
            }
            synthetic_got_entries.insert(sym_index, entry as usize);
        }
    }
    let mut synthetic_import_thunks = BTreeMap::new();
    if let Some(thunk_offset) = synthetic_import_thunk_offset {
        for (slot, (sym_index, target)) in import_thunk_symbols.iter().enumerate() {
            let thunk = unsafe { base.add(thunk_offset + slot * IMPORT_THUNK_SIZE) };
            unsafe {
                write_import_thunk(thunk, *target);
            }
            synthetic_import_thunks.insert(*sym_index, thunk as usize);
        }
    }

    for target in file.sections() {
        if !section_is_alloc(target.flags()) {
            continue;
        }
        let target_index = target.index().0;
        let target_base = *section_bases
            .get(target_index)
            .ok_or_else(|| String::from("ELF relocation target out of range"))?;
        if target_base == 0 {
            return Err(String::from("ELF relocation target not loaded"));
        }
        let target_size = usize::try_from(target.size())
            .map_err(|_| String::from("ELF relocation target size out of range"))?;
        for (r_offset, relocation) in target.relocations() {
            if relocation.has_implicit_addend() {
                return Err(String::from("unsupported ELF REL relocation"));
            }
            let r_type = relocation_type(&relocation)?;
            if r_type == R_X86_64_NONE {
                continue;
            }
            let write_size = match r_type {
                R_X86_64_64 => core::mem::size_of::<u64>(),
                R_X86_64_32S
                | R_X86_64_PC32
                | R_X86_64_PLT32
                | R_X86_64_GOTPCREL
                | R_X86_64_GOTPCRELX
                | R_X86_64_REX_GOTPCRELX => core::mem::size_of::<i32>(),
                _ => {
                    return Err(alloc::format!("unsupported ELF relocation type: {}", r_type));
                }
            };
            let place = relocation_place(target_base, target_size, r_offset, write_size)?;
            let r_sym = relocation_symbol(&relocation)?;
            let r_addend = relocation.addend();
            let sym = rel_symbol_value(&file, section_bases.as_slice(), r_sym)? as i64;
            let place_i64 = place as i64;
            unsafe {
                match r_type {
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
                        let target = synthetic_import_thunks
                            .get(&r_sym)
                            .copied()
                            .unwrap_or(sym as usize) as i64;
                        let value = target
                            .checked_add(r_addend)
                            .and_then(|v| v.checked_sub(place_i64))
                            .ok_or_else(|| String::from("R_X86_64_PC32 overflow"))?;
                        let value_i32 = i32::try_from(value)
                            .map_err(|_| String::from("R_X86_64_PC32 out of range"))?;
                        (place as *mut i32).write_unaligned(value_i32);
                    }
                    R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX => {
                        let got_entry = *synthetic_got_entries
                            .get(&r_sym)
                            .ok_or_else(|| String::from("R_X86_64_GOTPCREL missing GOT entry"))?
                            as i64;
                        let value = got_entry
                            .checked_add(r_addend)
                            .and_then(|v| v.checked_sub(place_i64))
                            .ok_or_else(|| String::from("R_X86_64_GOTPCREL overflow"))?;
                        let value_i32 = i32::try_from(value)
                            .map_err(|_| String::from("R_X86_64_GOTPCREL out of range"))?;
                        (place as *mut i32).write_unaligned(value_i32);
                    }
                    _ => unreachable!("relocation type checked above"),
                }
            }
        }
    }

    let backing = allocation.disarm();
    Ok(LoadedRelImage {
        base,
        used_len: total_size,
        backing,
        section_bases,
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
    match parse_elf(bytes).ok()?.kind() {
        ObjectKind::Relocatable => Some("REL"),
        ObjectKind::Executable => Some("EXEC"),
        ObjectKind::Dynamic => Some("DYN"),
        ObjectKind::Core => Some("CORE"),
        _ => Some("UNKNOWN"),
    }
}

pub(crate) fn elf_alloc_stats(bytes: &[u8]) -> Result<ElfAllocStats, String> {
    let file = parse_elf(bytes).map_err(String::from)?;
    Ok(elf_alloc_stats_from_file(&file))
}

fn elf_alloc_stats_from_file(file: &ParsedElf<'_>) -> ElfAllocStats {
    let mut stats = ElfAllocStats {
        sections: file.elf_section_table().len(),
        alloc_sections: 0,
        alloc_bytes: 0,
    };
    for section in file.sections() {
        if !section_is_alloc(section.flags()) {
            continue;
        }
        stats.alloc_sections += 1;
        stats.alloc_bytes = stats
            .alloc_bytes
            .saturating_add(usize::try_from(section.size()).unwrap_or(usize::MAX));
    }
    stats
}

pub(crate) fn elf_rel_debug_summary(bytes: &[u8], entry_hint: u64) -> Result<String, String> {
    let file = parse_elf(bytes).map_err(String::from)?;
    let stats = elf_alloc_stats_from_file(&file);

    let entry_symbol = match require_symtab(&file) {
        Ok(()) => match best_entry_symbol(&file) {
            Ok(Some((kind, name, section_index, value))) => alloc::format!(
                "entry_symbol={} sec={} value=0x{:x} kind={}",
                abbreviate_symbol_name(name.as_str()),
                section_index,
                value,
                kind
            ),
            Ok(None) => String::from("entry_symbol=missing"),
            Err(err) => alloc::format!("entry_symbol=scan-failed:{}", err),
        },
        Err(err) => alloc::format!("entry_symbol=scan-failed:{}", err),
    };

    Ok(alloc::format!(
        "ELF diag sections={} alloc_sections={} alloc_bytes={} entry_hint=sec:{}+0x{:x} {}",
        stats.sections,
        stats.alloc_sections,
        stats.alloc_bytes,
        entry_hint_section(entry_hint),
        entry_hint_offset(entry_hint),
        entry_symbol
    ))
}

pub(crate) fn elf_imports<'a>(bytes: &'a [u8]) -> Result<Vec<ElfImport<'a>>, &'static str> {
    let file = parse_elf(bytes)?;
    require_symtab(&file)?;
    let mut imports = Vec::new();
    for sym_index in 0..file.elf_symbol_table().len() {
        let sym = read_symbol(&file, sym_index).map_err(|_| "ELF symbol table malformed")?;
        let bind = sym.info >> 4;
        if sym.section != SymbolSection::Undefined
            || !(bind == STB_GLOBAL || bind == STB_WEAK)
            || sym.name.is_empty()
        {
            continue;
        }
        imports.push(ElfImport {
            name: sym.name,
            resolved_addr: resolve_known_import(sym.name),
        });
    }

    imports.sort_by(|a, b| a.name.cmp(b.name));
    imports.dedup_by(|a, b| a.name == b.name);
    Ok(imports)
}

fn resolve_known_import(name: &str) -> Option<usize> {
    if name.ends_with("___rustc26___rust_alloc_error_handler") {
        return Some(portal_rust_alloc_error_handler as *const () as usize);
    }
    if name.ends_with("___rustc35___rust_no_alloc_shim_is_unstable_v2") {
        return Some(portal_no_alloc_shim_is_unstable_v2 as *const () as usize);
    }
    if name.ends_with("___rustc12___rust_alloc") {
        return Some(portal_rust_alloc as *const () as usize);
    }
    if name.ends_with("___rustc14___rust_dealloc") {
        return Some(portal_rust_dealloc as *const () as usize);
    }
    if name.ends_with("___rustc14___rust_realloc") {
        return Some(portal_rust_realloc as *const () as usize);
    }
    if name.ends_with("___rustc19___rust_alloc_zeroed") {
        return Some(portal_rust_alloc_zeroed as *const () as usize);
    }

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
        "__assert_fail" => Some(trueos_qjs::trueos_shims::__assert_fail as *const () as usize),
        "__snprintf_chk" => Some(trueos_qjs::trueos_shims::__snprintf_chk as *const () as usize),
        "__vsnprintf_chk" => Some(trueos_qjs::trueos_shims::__vsnprintf_chk as *const () as usize),
        "malloc_usable_size" => {
            Some(trueos_qjs::trueos_shims::malloc_usable_size as *const () as usize)
        }
        "lrint" => Some(trueos_qjs::trueos_shims::lrint as *const () as usize),
        "modf" => Some(trueos_qjs::trueos_shims::modf as *const () as usize),
        "sinf" => Some(trueos_math::sinf as *const () as usize),
        "cosf" => Some(trueos_math::cosf as *const () as usize),
        "acosf" => Some(trueos_math::acosf as *const () as usize),
        "asinf" => Some(trueos_math::asinf as *const () as usize),
        "atanf" => Some(trueos_math::atanf as *const () as usize),
        "atan2f" => Some(trueos_math::atan2f as *const () as usize),
        "tanf" => Some(trueos_math::tanf as *const () as usize),
        "log2f" => Some(trueos_math::log2f as *const () as usize),
        "logf" => Some(trueos_math::logf as *const () as usize),
        "log10f" => Some(trueos_math::log10f as *const () as usize),
        "expf" => Some(trueos_math::expf as *const () as usize),
        "powf" => Some(trueos_math::powf as *const () as usize),
        "tanhf" => Some(trueos_math::tanhf as *const () as usize),
        "hypotf" => Some(trueos_math::hypotf as *const () as usize),
        "sin" => Some(trueos_math::sin as *const () as usize),
        "cos" => Some(trueos_math::cos as *const () as usize),
        "acos" => Some(trueos_math::acos as *const () as usize),
        "acosh" => Some(trueos_qjs::trueos_shims::acosh as *const () as usize),
        "asin" => Some(trueos_math::asin as *const () as usize),
        "asinh" => Some(trueos_qjs::trueos_shims::asinh as *const () as usize),
        "atan" => Some(trueos_math::atan as *const () as usize),
        "atanh" => Some(trueos_qjs::trueos_shims::atanh as *const () as usize),
        "tan" => Some(trueos_math::tan as *const () as usize),
        "sinh" => Some(trueos_math::sinh as *const () as usize),
        "cosh" => Some(trueos_math::cosh as *const () as usize),
        "log2" => Some(trueos_math::log2 as *const () as usize),
        "log" => Some(trueos_math::log as *const () as usize),
        "log10" => Some(trueos_math::log10 as *const () as usize),
        "log1p" => Some(trueos_math::log1p as *const () as usize),
        "exp" => Some(trueos_math::exp as *const () as usize),
        "expm1" => Some(trueos_math::expm1 as *const () as usize),
        "pow" => Some(trueos_math::pow as *const () as usize),
        "tanh" => Some(trueos_math::tanh as *const () as usize),
        "hypot" => Some(trueos_math::hypot as *const () as usize),
        "atan2" => Some(trueos_math::atan2 as *const () as usize),
        "trueos_cabi_wls_current_slot" => {
            Some(crate::stackkeeper::trueos_cabi_wls_current_slot as *const () as usize)
        }
        "_Unwind_Backtrace" => Some(portal_unwind_backtrace as *const () as usize),
        "_Unwind_FindEnclosingFunction" => {
            Some(portal_unwind_find_enclosing_function as *const () as usize)
        }
        "_Unwind_GetCFA" => Some(portal_unwind_get_cfa as *const () as usize),
        "_Unwind_GetIP" => Some(portal_unwind_get_ip as *const () as usize),
        "_Unwind_RaiseException" => Some(portal_unwind_raise_exception as *const () as usize),
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
        _ => resolve_ring_runtime_import(name)
            .or_else(|| resolve_runtime_abi_import(name))
            .or_else(|| resolve_cabi_import(name))
            .or_else(|| crate::unix_compat::resolve_import(name)),
    }
}

fn resolve_import(name: &str) -> Option<usize> {
    resolve_known_import(name).or_else(|| resolve_unresolved_import(name))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn resolve_runtime_abi_import(name: &str) -> Option<usize> {
    if crate::unix_compat::is_unix_import(name) {
        return None;
    }
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
        "exit" | "_exit" => Some(crate::std_abi_shim::exit as *const () as usize),
        "abort" => Some(trueos_qjs::trueos_shims::abort as *const () as usize),
        "trueos_cabi_dns_resolve_ipv4" => {
            Some(crate::std_abi_shim::trueos_cabi_dns_resolve_ipv4 as *const () as usize)
        }
        "trueos_vlayer_rapl_snapshot_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_rapl_snapshot_read as *const () as usize)
        }
        "trueos_vlayer_rapl_history_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_rapl_history_read as *const () as usize)
        }
        "trueos_vlayer_pci_snapshot_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_pci_snapshot_read as *const () as usize)
        }
        "trueos_vlayer_thermal_snapshot_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_thermal_snapshot_read as *const () as usize)
        }
        "trueos_vlayer_vram_snapshot_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_vram_snapshot_read as *const () as usize)
        }
        "trueos_vlayer_system_services_snapshot_read" => Some(
            crate::r::net::vlayer::trueos_vlayer_system_services_snapshot_read as *const ()
                as usize,
        ),
        "trueos_vlayer_printer_snapshot_read" => {
            Some(crate::r::net::vlayer::trueos_vlayer_printer_snapshot_read as *const () as usize)
        }
        "trueos_vlayer_print2d_submit" => {
            Some(crate::r::net::vlayer::trueos_vlayer_print2d_submit as *const () as usize)
        }
        "trueos_vlayer_print2d_status" => {
            Some(crate::r::net::vlayer::trueos_vlayer_print2d_status as *const () as usize)
        }
        "trueos_cabi_gridpaper_print_request_take" => Some(
            crate::r::gridpaper_service::trueos_cabi_gridpaper_print_request_take as *const ()
                as usize,
        ),
        "trueos_platform_monotonic_nanos" => {
            Some(crate::r::platform::trueos_platform_monotonic_nanos as *const () as usize)
        }
        "trueos_platform_unix_seconds" => {
            Some(crate::r::platform::trueos_platform_unix_seconds as *const () as usize)
        }
        "trueos_platform_cpu_count" => {
            Some(crate::r::platform::trueos_platform_cpu_count as *const () as usize)
        }
        "trueos_tokio_worker_carriers_enabled" => {
            Some(crate::r::platform::trueos_tokio_worker_carriers_enabled as *const () as usize)
        }
        "trueos_service_lane_submit_job" => {
            Some(crate::r::blocking::trueos_service_lane_submit_job as *const () as usize)
        }
        "trueos_tokio_spawn_blocking_job" => {
            Some(crate::r::blocking::trueos_tokio_spawn_blocking_job as *const () as usize)
        }
        "trueos_time_monotonic_nanos" => {
            Some(crate::std_abi_shim::trueos_time_monotonic_nanos as *const () as usize)
        }
        "trueos_time_unix_nanos" => {
            Some(crate::std_abi_shim::trueos_time_unix_nanos as *const () as usize)
        }
        "trueos_time_unix_seconds" => {
            Some(crate::std_abi_shim::trueos_time_unix_seconds as *const () as usize)
        }
        "trueos_tokio_platform_log_semantic_gap" => {
            Some(crate::r::platform::trueos_tokio_platform_log_semantic_gap as *const () as usize)
        }
        "trueos_tokio_platform_log" => {
            Some(crate::r::platform::trueos_tokio_platform_log as *const () as usize)
        }
        "trueos_tokio_platform_monotonic_nanos" => {
            Some(crate::r::platform::trueos_tokio_platform_monotonic_nanos as *const () as usize)
        }
        "trueos_tokio_platform_poll_once" => {
            Some(crate::r::platform::trueos_tokio_platform_poll_once as *const () as usize)
        }
        "trueos_tokio_platform_sleep_ms" => {
            Some(crate::r::platform::trueos_tokio_platform_sleep_ms as *const () as usize)
        }
        "trueos_tokio_platform_wait_observe" => {
            Some(crate::r::platform::trueos_tokio_platform_wait_observe as *const () as usize)
        }
        "trueos_tokio_platform_wait_after" => {
            Some(crate::r::platform::trueos_tokio_platform_wait_after as *const () as usize)
        }
        "trueos_tokio_platform_wait" => {
            Some(crate::r::platform::trueos_tokio_platform_wait as *const () as usize)
        }
        "trueos_tokio_platform_wake_one" => {
            Some(crate::r::platform::trueos_tokio_platform_wake_one as *const () as usize)
        }
        "trueos_tokio_platform_wake_all" => {
            Some(crate::r::platform::trueos_tokio_platform_wake_all as *const () as usize)
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
        "trueos_mio_selector_wake" => {
            Some(crate::mio_compat::trueos_mio_selector_wake as *const () as usize)
        }
        _ => None,
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn resolve_runtime_abi_import(_name: &str) -> Option<usize> {
    None
}

unsafe extern "C" fn portal_rust_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let Ok(layout) = Layout::from_size_align(size, align.max(1)) else {
        return core::ptr::null_mut();
    };

    let vm_id = portal_guest_alloc_vm_id();
    let ptr = if let Some(vm_id) = vm_id {
        unsafe { crate::allocators::alloc_raw_hv_guest(vm_id, layout) }
    } else {
        unsafe { crate::allocators::alloc_raw(layout) }
    };
    let trace_index = PORTAL_RUST_ALLOC_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if crate::log_os::flags::PORTAL_LOGS
        && (trace_index < 128
            || layout.size() >= 1024 * 1024
            || trace_index.is_power_of_two()
            || ptr.is_null())
    {
        let vm_for_stats = vm_id.unwrap_or(0);
        let stats = crate::allocators::hv_guest_heap_stats(vm_for_stats);
        crate::hv::hvlogf(format_args!(
            "portal: rust alloc seq={} vm={:?} size={} align={} ptr=0x{:016X} free_bytes={} largest_free={} free_blocks={} guest_heap=[0x{:016X}..0x{:016X})",
            trace_index,
            vm_id,
            layout.size(),
            layout.align(),
            ptr as usize,
            stats.free_bytes,
            stats.largest_free_block,
            stats.free_blocks,
            stats.heap_start,
            stats.heap_end,
        ));
    }
    ptr
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

pub(crate) fn build_process_env(
    archive: &str,
    app_fs_root: Option<&str>,
) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let app_home = app_fs_root
        .map(|root| alloc::format!("/{}", root.trim_matches('/')))
        .unwrap_or_else(|| String::from("/"));
    vars.insert(String::from("PWD"), app_home.clone());
    vars.insert(String::from("HOME"), app_home.clone());
    vars.insert(String::from("LANG"), String::from(crate::locale::current_language_code()));
    vars.insert(String::from("LANGUAGE"), String::from(crate::locale::current_language_code()));
    vars.insert(
        String::from("TRUEOS_LANGUAGE"),
        String::from(crate::locale::current_language_code()),
    );
    vars.insert(String::from("LC_ALL"), String::from(crate::locale::current_intl_locale_code()));
    vars.insert(
        String::from("LC_COLLATE"),
        String::from(crate::locale::current_intl_locale_code()),
    );
    vars.insert(String::from("LC_CTYPE"), String::from(crate::locale::current_intl_locale_code()));
    vars.insert(
        String::from("LC_MESSAGES"),
        String::from(crate::locale::current_intl_locale_code()),
    );
    vars.insert(
        String::from("LC_MONETARY"),
        String::from(crate::locale::current_intl_locale_code()),
    );
    vars.insert(
        String::from("LC_NUMERIC"),
        String::from(crate::locale::current_intl_locale_code()),
    );
    vars.insert(String::from("LC_TIME"), String::from(crate::locale::current_intl_locale_code()));
    vars.insert(
        String::from("TRUEOS_LOCALE"),
        String::from(crate::locale::current_intl_locale_code()),
    );
    vars.insert(String::from("TZ"), String::from(crate::locale::current_timezone_name()));
    vars.insert(
        String::from("TRUEOS_TIMEZONE"),
        String::from(crate::locale::current_timezone_name()),
    );
    let hostname = crate::net::adapter::get_hostname();
    vars.insert(String::from("HOSTNAME"), hostname.clone());
    vars.insert(String::from("TRUEOS_HOSTNAME"), hostname);
    // Make the guest's configured network identity available to blueprints.
    // Binding a listener to 0.0.0.0 is still the correct server behavior, but
    // applications need a stable way to advertise the address users can open
    // from the host.  Keep the primary address convenient and expose indexed
    // addresses for blueprints that have more than one NIC.
    let primary_device = crate::net::primary_device_index();
    vars.insert(String::from("TRUEOS_NET_PRIMARY_DEVICE"), alloc::format!("{primary_device}"));
    vars.insert(
        String::from("TRUEOS_NET_DEVICE_COUNT"),
        alloc::format!("{}", crate::net::device_count()),
    );
    if let Some(ip) = crate::net::adapter::ipv4_at(primary_device) {
        vars.insert(
            String::from("TRUEOS_NET_IPV4"),
            alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
        );
    }
    if let Some(router) = crate::net::adapter::primary_ipv4_router_snapshot() {
        vars.insert(
            String::from("TRUEOS_NET_IPV4_GATEWAY"),
            alloc::format!("{}.{}.{}.{}", router[0], router[1], router[2], router[3]),
        );
    }
    for index in 0..crate::net::device_count() {
        if let Some(ip) = crate::net::adapter::ipv4_at(index) {
            vars.insert(
                alloc::format!("TRUEOS_NET_IPV4_{index}"),
                alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
            );
        }
    }
    vars.insert(String::from("XDG_CONFIG_HOME"), String::from("/config"));
    vars.insert(String::from("XDG_CACHE_HOME"), String::from("/cache"));
    vars.insert(String::from("BAT_CONFIG_DIR"), String::from("/config/bat"));
    vars.insert(String::from("BAT_CACHE_PATH"), String::from("/cache/bat"));
    let archive_stem = safe_archive_stem(archive);
    if archive_stem == "bat" {
        vars.insert(
            String::from("BAT_OPTS"),
            String::from(
                "--color=never --paging=never --style=plain --decorations=never --terminal-width=100 --no-custom-assets",
            ),
        );
        vars.insert(String::from("BAT_PAGING"), String::from("never"));
        vars.insert(String::from("BAT_PAGER"), String::new());
        vars.insert(String::from("BAT_WIDTH"), String::from("100"));
    }
    if archive_stem == "prism_q_probe" {
        vars.insert(String::from("PRISM_MAX_SV_QUBITS"), String::from("26"));
        vars.insert(String::from("PRISM_MAX_PROB_QUBITS"), String::from("26"));
        vars.insert(String::from("PRISM_MAX_EXPORT_QUBITS"), String::from("26"));
    }
    vars.insert(String::from("TRUEOS_APP_ARCHIVE"), String::from(archive));
    if let Some(root) = app_fs_root {
        vars.insert(String::from("TRUEOS_APP_FS_ROOT"), String::from(root));
    }
    let app_common = app_fs_common_for_archive(archive);
    vars.insert(String::from("TRUEOS_APP_FS_COMMON"), app_common.clone());
    vars.insert(String::from("TRUEOS_APP_COMMON"), String::from("/common"));
    vars
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn safe_archive_stem(archive: &str) -> String {
    let mut out = String::new();
    let stem = archive.trim().trim_end_matches(".bp");
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    while out.starts_with('.') || out.starts_with('/') || out.starts_with('_') {
        out.remove(0);
    }
    while out.ends_with('.') || out.ends_with('/') || out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        String::from("app")
    } else {
        out
    }
}

pub(crate) fn app_fs_root_for_archive(archive: &str, _module_bytes: &[u8]) -> String {
    alloc::format!("apps/{}", safe_archive_stem(archive))
}

pub(crate) fn app_fs_common_for_archive(archive: &str) -> String {
    alloc::format!("apps/common/{}", safe_archive_stem(archive))
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
    match module.flags & BLUEPRINT_PAYLOAD_MASK {
        BLUEPRINT_PAYLOAD_RAW => Ok(module.payload.to_vec()),
        BLUEPRINT_PAYLOAD_7Z => crate::z7::extract_single_file_to_vec(module.payload)
            .map_err(|_| "7z payload decode failed"),
        _ => Err("unsupported blueprint payload flags"),
    }
}

fn embedded_asset_archive(bytes: &[u8]) -> Result<Option<&[u8]>, String> {
    let file = parse_elf(bytes).map_err(String::from)?;
    let mut archive = None;

    for section in file.sections() {
        let name = section
            .name()
            .map_err(|_| String::from("ELF section name is not UTF-8"))?;
        if name != EMBEDDED_ASSET_SECTION {
            continue;
        }
        if archive.is_some() {
            return Err(String::from("duplicate .trueos.assets section"));
        }
        if section_is_alloc(section.flags()) {
            return Err(String::from(".trueos.assets must not be allocatable"));
        }
        if section_type(&file, section.index())? != SHT_PROGBITS {
            return Err(String::from(".trueos.assets must be PROGBITS"));
        }
        let section_size = usize::try_from(section.size())
            .map_err(|_| String::from(".trueos.assets size is out of range"))?;
        let data = section
            .data()
            .map_err(|_| String::from(".trueos.assets section is truncated"))?;
        if data.len() != section_size {
            return Err(String::from(".trueos.assets section size mismatch"));
        }
        if data.is_empty() {
            return Err(String::from(".trueos.assets archive is empty"));
        }
        if data.len() > crate::allcaps::blueprint::ASSET_ARCHIVE_CAP_BYTES {
            return Err(alloc::format!(
                ".trueos.assets archive exceeds cap size={} cap={}",
                data.len(),
                crate::allcaps::blueprint::ASSET_ARCHIVE_CAP_BYTES
            ));
        }
        archive = Some(data);
    }

    Ok(archive)
}

fn take_asset_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], &'static str> {
    let end = cursor
        .checked_add(len)
        .ok_or("embedded asset offset overflow")?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or("embedded asset payload truncated")?;
    *cursor = end;
    Ok(slice)
}

fn validate_embedded_asset_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("embedded asset path is empty");
    }
    if path.len() > crate::allcaps::blueprint::ASSET_PATH_BYTES_CAP {
        return Err("embedded asset path exceeds length cap");
    }
    if path.trim() != path {
        return Err("embedded asset path has leading or trailing whitespace");
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("embedded asset path is not a portable relative path");
    }
    if path
        .as_bytes()
        .iter()
        .any(|byte| *byte < b' ' || *byte == 0x7f)
    {
        return Err("embedded asset path contains a control character");
    }

    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err("embedded asset path contains a non-normal component");
        }
        if component.len() > crate::allcaps::blueprint::ASSET_PATH_COMPONENT_BYTES_CAP {
            return Err("embedded asset path component exceeds length cap");
        }
    }
    Ok(())
}

fn parse_embedded_asset_table(
    decoded: &[u8],
) -> Result<(Vec<EmbeddedAssetEntry<'_>>, usize), String> {
    if decoded.len() > crate::allcaps::blueprint::ASSET_DECODE_CAP_BYTES {
        return Err(String::from("embedded asset table exceeds decoded size cap"));
    }
    if decoded.len() < ASSET_HEADER_LEN {
        return Err(String::from("embedded asset table header is truncated"));
    }
    if decoded.get(0..4) != Some(b"TRAS") {
        return Err(String::from("bad embedded asset table magic"));
    }
    if le_u16(decoded, 4) != Some(1) {
        return Err(String::from("unsupported embedded asset table version"));
    }
    if le_u16(decoded, 6) != Some(0) {
        return Err(String::from("unsupported embedded asset table flags"));
    }

    let count = le_u32(decoded, 8)
        .ok_or_else(|| String::from("embedded asset table header is truncated"))?
        as usize;
    if count > crate::allcaps::blueprint::ASSET_ENTRY_COUNT_CAP {
        return Err(alloc::format!(
            "embedded asset count exceeds cap count={} cap={}",
            count,
            crate::allcaps::blueprint::ASSET_ENTRY_COUNT_CAP
        ));
    }

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| String::from("embedded asset entry allocation failed"))?;
    let mut seen_paths = BTreeSet::new();
    let mut cursor = ASSET_HEADER_LEN;
    let mut total_data_len = 0usize;

    for index in 0..count {
        let header = take_asset_slice(decoded, &mut cursor, ASSET_ENTRY_HEADER_LEN)
            .map_err(|err| alloc::format!("embedded asset entry {}: {}", index, err))?;
        let path_len = le_u32(header, 0)
            .ok_or_else(|| alloc::format!("embedded asset entry {} header truncated", index))?
            as usize;
        let data_len_u64 = le_u64(header, 4)
            .ok_or_else(|| alloc::format!("embedded asset entry {} header truncated", index))?;
        if path_len == 0 || path_len > crate::allcaps::blueprint::ASSET_PATH_BYTES_CAP {
            return Err(alloc::format!(
                "embedded asset entry {} path length is out of range",
                index
            ));
        }
        if data_len_u64 > crate::allcaps::blueprint::ASSET_FILE_CAP_BYTES as u64 {
            return Err(alloc::format!("embedded asset entry {} file exceeds size cap", index));
        }
        let data_len = usize::try_from(data_len_u64)
            .map_err(|_| alloc::format!("embedded asset entry {} size is out of range", index))?;
        let expected_hash = &header[12..44];
        let path_bytes = take_asset_slice(decoded, &mut cursor, path_len)
            .map_err(|err| alloc::format!("embedded asset entry {} path: {}", index, err))?;
        let path = core::str::from_utf8(path_bytes)
            .map_err(|_| alloc::format!("embedded asset entry {} path is not UTF-8", index))?;
        validate_embedded_asset_path(path)
            .map_err(|err| alloc::format!("embedded asset entry {}: {}", index, err))?;
        if !seen_paths.insert(String::from(path)) {
            return Err(alloc::format!("embedded asset entry {} duplicates path {}", index, path));
        }

        let data = take_asset_slice(decoded, &mut cursor, data_len)
            .map_err(|err| alloc::format!("embedded asset entry {} data: {}", index, err))?;
        let digest = Sha256::digest(data);
        if digest.as_slice() != expected_hash {
            return Err(alloc::format!(
                "embedded asset entry {} hash mismatch path={}",
                index,
                path
            ));
        }
        total_data_len = total_data_len
            .checked_add(data_len)
            .ok_or_else(|| String::from("embedded asset total size overflow"))?;
        if total_data_len > crate::allcaps::blueprint::ASSET_DECODE_CAP_BYTES {
            return Err(String::from("embedded asset data exceeds total size cap"));
        }
        entries.push(EmbeddedAssetEntry { path, data });
    }

    if cursor != decoded.len() {
        return Err(alloc::format!(
            "embedded asset table has trailing bytes count={}",
            decoded.len() - cursor
        ));
    }
    for path in &seen_paths {
        for (offset, byte) in path.as_bytes().iter().copied().enumerate() {
            if byte == b'/' && seen_paths.contains(&path[..offset]) {
                return Err(alloc::format!(
                    "embedded asset path conflicts with parent file path={}",
                    path
                ));
            }
        }
    }

    Ok((entries, total_data_len))
}

fn joined_asset_path(root: &str, relative: &str) -> String {
    let mut path = String::with_capacity(root.len() + 1 + relative.len());
    path.push_str(root);
    if !path.ends_with('/') {
        path.push('/');
    }
    path.push_str(relative);
    path
}

pub(crate) fn materialize_embedded_assets(
    unpacked: &[u8],
    app_fs_root: &str,
) -> Result<Option<EmbeddedAssetStats>, String> {
    let Some(archive) = embedded_asset_archive(unpacked)? else {
        return Ok(None);
    };
    let decoded = crate::z7::extract_single_file_to_vec_bounded(
        archive,
        crate::allcaps::blueprint::ASSET_DECODE_CAP_BYTES,
        crate::allcaps::blueprint::ASSET_7Z_DICTIONARY_CAP_BYTES,
    )
    .map_err(|err| alloc::format!("embedded asset 7z decode failed: {:?}", err))?;
    let (entries, total_data_len) = parse_embedded_asset_table(decoded.as_slice())?;

    crate::r::io::kfs::create_dir_all(app_fs_root).map_err(|err| {
        alloc::format!("embedded asset root create failed path={} err={:?}", app_fs_root, err)
    })?;

    let mut created_parents = BTreeSet::new();
    for entry in &entries {
        if let Some((relative_parent, _)) = entry.path.rsplit_once('/')
            && created_parents.insert(relative_parent)
        {
            let parent = joined_asset_path(app_fs_root, relative_parent);
            crate::r::io::kfs::create_dir_all(parent.as_str()).map_err(|err| {
                alloc::format!(
                    "embedded asset directory create failed path={} err={:?}",
                    parent,
                    err
                )
            })?;
        }

        let path = joined_asset_path(app_fs_root, entry.path);
        let handle = crate::r::io::kfs::write_file_begin(path.as_str(), entry.data.len() as u64)
            .map_err(|err| {
                alloc::format!("embedded asset write begin failed path={} err={:?}", path, err)
            })?;
        for chunk in entry
            .data
            .chunks(crate::allcaps::blueprint::ASSET_WRITE_CHUNK_BYTES)
        {
            if let Err(err) = crate::r::io::kfs::write_file_chunk(handle, chunk) {
                let abort = crate::r::io::kfs::write_file_abort(handle);
                return Err(alloc::format!(
                    "embedded asset write chunk failed path={} err={:?} abort={:?}",
                    path,
                    err,
                    abort
                ));
            }
        }
        if let Err(err) = crate::r::io::kfs::write_file_finish(handle) {
            let abort = crate::r::io::kfs::write_file_abort(handle);
            return Err(alloc::format!(
                "embedded asset write finish failed path={} err={:?} abort={:?}",
                path,
                err,
                abort
            ));
        }
    }

    Ok(Some(EmbeddedAssetStats {
        entries: entries.len(),
        bytes: total_data_len,
    }))
}

pub(crate) fn invoke_host_rel(
    unpacked: &[u8],
    entry_hint: u64,
    process_args: Vec<String>,
    process_env: BTreeMap<String, String>,
    console_target: Option<crate::shell2::MatrixTarget>,
    app_fs_root: Option<String>,
) -> Result<(), String> {
    let image = load_rel_image(unpacked)?;
    let file = parse_elf(unpacked)?;
    let main_addr = find_main_addr(&file, image.section_bases.as_slice(), entry_hint)?;
    let entry_section = entry_hint_section(entry_hint) as usize;
    let entry_section_base = image.section_bases.get(entry_section).copied().unwrap_or(0);
    let vm_for_stats = portal_guest_alloc_vm_id().unwrap_or(0);
    let stats = crate::allocators::hv_guest_heap_stats(vm_for_stats);
    crate::hv::hvlogf(format_args!(
        "hv: rel image loaded vm={} base=0x{:016X} used_len=0x{:X} main=0x{:016X} entry_hint=sec:{}+0x{:X} entry_section_base=0x{:016X} free_bytes={} largest_free={} free_blocks={}",
        vm_for_stats,
        image.base as usize,
        image.used_len,
        main_addr,
        entry_section,
        entry_hint_offset(entry_hint),
        entry_section_base,
        stats.free_bytes,
        stats.largest_free_block,
        stats.free_blocks,
    ));
    set_rel_image_exec_policy(image.base as u64, image.used_len, true)?;
    let (_arg_storage, argv) = build_argv(process_args.as_slice());
    let main_fn: extern "C" fn(usize, *const *const c_char) =
        unsafe { core::mem::transmute(main_addr) };
    crate::r::io::env::with_launch_context_console_and_fs_root(
        process_args,
        process_env,
        console_target,
        app_fs_root,
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
    set_rel_image_exec_policy(image.base as u64, image.used_len, false)?;
    drop(image);
    Ok(())
}

fn set_rel_image_exec_policy(guest_va: u64, bytes: usize, executable: bool) -> Result<(), String> {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return Ok(());
    }
    let op = if executable {
        trueos_vm::vmcall::OP_BP_REL_IMAGE_EXEC_ENABLE
    } else {
        trueos_vm::vmcall::OP_BP_REL_IMAGE_EXEC_DISABLE
    };
    let (status, data) = trueos_vm::vmcall::call(op, guest_va, bytes as u64);
    let rc = (data as i64) as i32;
    if status != trueos_vm::vmcall::STATUS_OK || rc != 0 {
        return Err(alloc::format!(
            "REL execute policy {} failed status={} rc={} gva=0x{:016X} bytes={}",
            if executable { "enable" } else { "disable" },
            status,
            rc,
            guest_va,
            bytes,
        ));
    }
    Ok(())
}
