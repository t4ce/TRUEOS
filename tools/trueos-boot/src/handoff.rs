//! The small subset of the Limine v6 handoff ABI consumed by the kernel.
//!
//! This is a compatibility boundary, not a Limine implementation: the UEFI
//! loader supplies the information and the kernel keeps its existing requests.
//! Every returned pointer addresses retained pages through the HHDM. The arena
//! must remain identity mapped while this module writes it, and retained after
//! ExitBootServices. No allocator or firmware service is used by this module.

use core::{mem, ptr};

pub const USABLE: u64 = 0;
pub const RESERVED: u64 = 1;
pub const ACPI_RECLAIMABLE: u64 = 2;
pub const ACPI_NVS: u64 = 3;
pub const BAD_MEMORY: u64 = 4;
pub const BOOTLOADER_RECLAIMABLE: u64 = 5;
pub const EXECUTABLE_AND_MODULES: u64 = 6;
pub const FRAMEBUFFER: u64 = 7;
pub const MAPPED_RESERVED: u64 = 8;

const COMMON: [u64; 2] = [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b];
const BASE: [u64; 2] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc];
const BASE_REVISION: u64 = 6;
const MAX_PROTECTED: usize = 64;

const HHDM: usize = 0;
const MEMMAP: usize = 1;
const EXECUTABLE_ADDRESS: usize = 2;
const EXECUTABLE_FILE: usize = 3;
const MODULES: usize = 4;
const FRAMEBUFFERS: usize = 5;
const CMDLINE: usize = 6;
const DATE: usize = 7;
const RSDP: usize = 8;
const EFI: usize = 9;
const SMBIOS: usize = 10;
const STACK: usize = 11;
const MP: usize = 12;
const RESPONSE_COUNT: usize = 13;

const IDS: [[u64; 2]; RESPONSE_COUNT] = [
    [0x48dcf1cb8ad2b852, 0x63984e959a98244b],
    [0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    [0x71ba76863cc55f63, 0xb2644a48c516a487],
    [0xad97e90e83f1ed67, 0x31eb5d1c5ff23b69],
    [0x3e7e279702be32af, 0xca1c4f3bd1280cee],
    [0x9d5827dcd881dd75, 0xa3148604f6fab11b],
    [0x4b161536e598651e, 0xb390ad4a2f1f303a],
    [0x502746e184c088aa, 0xfbc5ec83e6327893],
    [0xc5e77b6b397e7b43, 0x27637845accdcf3c],
    [0x5ceba5163eaaf6d6, 0x0a6981610cf65fcc],
    [0x9e9046f11e095391, 0xaa4a520fefbde5ee],
    [0x224ef0460a8e8926, 0xe1cb0fc25f46ea3d],
    [0x95a67b819a1b857e, 0xa0b61b723b6a73e0],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    ArenaFull,
    BadAlignment,
    AddressOverflow,
    InteriorNul,
    TooManyProtectedRegions,
    MemoryMapFull,
    InvalidMemoryRange,
    InvalidRequestSection,
    UnsupportedBaseRevision,
    MissingBaseRevision,
    MissingRequiredRequest,
    StackTooSmall,
    NotPrepared,
}

/// A physical allocation kept intact for the kernel.
#[derive(Clone, Copy)]
pub struct Blob<'a> {
    pub phys: u64,
    pub len: usize,
    pub path: &'a str,
    pub cmdline: &'a str,
}

/// GOP mode normalized to the same RGB-mask description used by the kernel.
#[derive(Clone, Copy)]
pub struct Framebuffer {
    pub phys: u64,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

pub struct BootFacts<'a> {
    pub kernel: Blob<'a>,
    pub modules: &'a [Blob<'a>],
    /// Source of every downloaded file, or None when read from a local volume.
    pub tftp_server: Option<[u8; 4]>,
    pub kernel_physical_base: u64,
    pub kernel_virtual_base: u64,
    pub framebuffer: Option<Framebuffer>,
    /// Monitor data captured from the protocol on this framebuffer's GOP device.
    /// prepare() copies it into the retained arena before firmware can free it.
    pub framebuffer_edid: &'a [u8],
    /// These firmware addresses are physical; responses report HHDM pointers.
    pub rsdp: u64,
    pub efi_system_table: u64,
    pub smbios32: u64,
    pub smbios64: u64,
    pub timestamp: Option<i64>,
    pub stack_size: u64,
    /// Already-HHDM pointer to the SMP module's retained response, or zero.
    pub mp_response: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: u64,
}

impl MemoryRegion {
    /// Loader allocations and boot-services data remain protected. Firmware
    /// table pointers can reference boot-services data after ExitBootServices.
    /// Boot-services code becomes usable after the successful exit; runtime
    /// pages never do.
    pub fn from_uefi(
        memory_type: u32,
        base: u64,
        pages: u64,
        attributes: u64,
    ) -> Result<Self, Error> {
        let length = pages.checked_mul(4096).ok_or(Error::AddressOverflow)?;
        base.checked_add(length).ok_or(Error::AddressOverflow)?;
        let kind = if attributes & (1 << 63) != 0 {
            MAPPED_RESERVED
        } else {
            match memory_type {
                1 | 2 | 4 => BOOTLOADER_RECLAIMABLE,
                3 | 7 => USABLE,
                8 => BAD_MEMORY,
                9 => ACPI_RECLAIMABLE,
                10 => ACPI_NVS,
                _ => RESERVED,
            }
        };
        Ok(Self { base, length, kind })
    }

    fn end(self) -> Result<u64, Error> {
        self.base.checked_add(self.length).ok_or(Error::AddressOverflow)
    }
}

#[derive(Clone, Copy)]
pub struct Allocation {
    pub physical: u64,
    pub virtual_address: u64,
    pub pointer: *mut u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FileWire {
    revision: u64,
    address: u64,
    size: u64,
    path: u64,
    cmdline: u64,
    media_type: u32,
    tftp_ip: u32,
    tftp_port: u32,
    partition_index: u32,
    mbr_disk_id: u32,
    uuids: [u8; 48],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FramebufferWire {
    address: u64,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
    reserved: [u8; 7],
    edid_size: u64,
    edid: u64,
}

pub struct Handoff {
    arena_phys: u64,
    arena_len: usize,
    used: usize,
    hhdm: u64,
    responses: [u64; RESPONSE_COUNT],
    map_entries: *mut MemoryRegion,
    map_pointers: u64,
    map_response: *mut [u64; 3],
    max_map_entries: usize,
    protected: [MemoryRegion; MAX_PROTECTED],
    protected_len: usize,
    stack_size: u64,
    prepared: bool,
}

#[derive(Debug, Default)]
pub struct PatchReport {
    pub answered: usize,
    pub unanswered: usize,
}

impl Handoff {
    /// # Safety
    /// The complete arena must be writable at its physical address. Its pages
    /// must be retained and mapped at both physical and HHDM addresses through
    /// the final kernel handoff. It must not alias another live Rust borrow.
    pub unsafe fn new(
        arena_phys: u64,
        arena_len: usize,
        hhdm: u64,
        max_map_entries: usize,
    ) -> Result<Self, Error> {
        if arena_phys & 7 != 0 || max_map_entries == 0 {
            return Err(Error::BadAlignment);
        }
        arena_phys.checked_add(arena_len as u64).ok_or(Error::AddressOverflow)?;
        hhdm.checked_add(arena_phys).and_then(|v| v.checked_add(arena_len as u64))
            .ok_or(Error::AddressOverflow)?;
        let mut this = Self {
            arena_phys, arena_len, used: 0, hhdm,
            responses: [0; RESPONSE_COUNT],
            map_entries: ptr::null_mut(), map_pointers: 0,
            map_response: ptr::null_mut(), max_map_entries,
            protected: [MemoryRegion::default(); MAX_PROTECTED], protected_len: 0,
            stack_size: 0, prepared: false,
        };
        let entries = this.allocate_bytes(
            max_map_entries.checked_mul(mem::size_of::<MemoryRegion>()).ok_or(Error::ArenaFull)?,
            mem::align_of::<MemoryRegion>(),
        )?;
        this.map_entries = entries.pointer.cast();
        let pointers = this.allocate_bytes(
            max_map_entries.checked_mul(8).ok_or(Error::ArenaFull)?, 8,
        )?;
        this.map_pointers = pointers.virtual_address;
        for i in 0..max_map_entries {
            unsafe {
                pointers.pointer.cast::<u64>().add(i).write(
                    entries.virtual_address + (i * mem::size_of::<MemoryRegion>()) as u64,
                );
            }
        }
        let response = this.allocate_bytes(24, 8)?;
        this.map_response = response.pointer.cast();
        unsafe { this.map_response.write([0, 0, this.map_pointers]); }
        this.responses[MEMMAP] = response.virtual_address;
        this.protect(MemoryRegion {
            base: arena_phys, length: arena_len as u64, kind: BOOTLOADER_RECLAIMABLE,
        })?;
        Ok(this)
    }

    /// Reserve zeroed retained arena storage before or after ExitBootServices.
    pub fn allocate_bytes(&mut self, length: usize, align: usize) -> Result<Allocation, Error> {
        if !align.is_power_of_two() {
            return Err(Error::BadAlignment);
        }
        let address = self.arena_phys.checked_add(self.used as u64).ok_or(Error::AddressOverflow)?;
        let physical = address.checked_add((align - 1) as u64).ok_or(Error::AddressOverflow)?
            & !((align - 1) as u64);
        let offset = usize::try_from(physical - self.arena_phys).map_err(|_| Error::ArenaFull)?;
        let end = offset.checked_add(length).ok_or(Error::ArenaFull)?;
        if end > self.arena_len {
            return Err(Error::ArenaFull);
        }
        let pointer = physical as *mut u8;
        unsafe { ptr::write_bytes(pointer, 0, length); }
        self.used = end;
        Ok(Allocation {
            physical,
            virtual_address: self.hhdm.checked_add(physical).ok_or(Error::AddressOverflow)?,
            pointer,
        })
    }

    pub fn store<T: Copy>(&mut self, value: T) -> Result<Allocation, Error> {
        let allocation = self.allocate_bytes(mem::size_of::<T>(), mem::align_of::<T>())?;
        unsafe { allocation.pointer.cast::<T>().write(value); }
        Ok(allocation)
    }

    fn virtual_address(&self, physical: u64) -> Result<u64, Error> {
        if physical == 0 { return Ok(0); }
        self.hhdm.checked_add(physical).ok_or(Error::AddressOverflow)
    }

    fn string(&mut self, string: &str) -> Result<u64, Error> {
        if string.as_bytes().contains(&0) {
            return Err(Error::InteriorNul);
        }
        let allocation = self.allocate_bytes(string.len().checked_add(1).ok_or(Error::ArenaFull)?, 1)?;
        unsafe { ptr::copy_nonoverlapping(string.as_ptr(), allocation.pointer, string.len()); }
        Ok(allocation.virtual_address)
    }

    fn protect(&mut self, region: MemoryRegion) -> Result<(), Error> {
        region.end()?;
        if region.length == 0 { return Ok(()); }
        if self.protected_len == self.protected.len() {
            return Err(Error::TooManyProtectedRegions);
        }
        self.protected[self.protected_len] = region;
        self.protected_len += 1;
        Ok(())
    }

    fn file(&mut self, blob: Blob<'_>, tftp_server: Option<[u8; 4]>) -> Result<u64, Error> {
        let address = self.virtual_address(blob.phys)?;
        let path = self.string(blob.path)?;
        let cmdline = self.string(blob.cmdline)?;
        let file = self.store(FileWire {
            revision: 0, address, size: blob.len as u64, path, cmdline,
            media_type: if tftp_server.is_some() { 2 } else { 0 },
            tftp_ip: tftp_server.map_or(0, u32::from_le_bytes),
            tftp_port: if tftp_server.is_some() { 69 } else { 0 }, partition_index: 0,
            mbr_disk_id: 0, uuids: [0; 48],
        })?;
        self.protect(MemoryRegion {
            base: blob.phys, length: blob.len as u64, kind: EXECUTABLE_AND_MODULES,
        })?;
        Ok(file.virtual_address)
    }

    /// Build every fixed-size response while firmware is still available.
    pub fn prepare(&mut self, facts: &BootFacts<'_>) -> Result<(), Error> {
        self.responses[HHDM] = self.store([0, self.hhdm])?.virtual_address;
        self.responses[EXECUTABLE_ADDRESS] = self.store([
            0, facts.kernel_physical_base, facts.kernel_virtual_base,
        ])?.virtual_address;
        let kernel_file = self.file(facts.kernel, facts.tftp_server)?;
        self.responses[EXECUTABLE_FILE] = self.store([0, kernel_file])?.virtual_address;
        let cmdline = self.string(facts.kernel.cmdline)?;
        self.responses[CMDLINE] = self.store([0, cmdline])?.virtual_address;
        let module_array = self.allocate_bytes(
            facts.modules.len().checked_mul(8).ok_or(Error::ArenaFull)?.max(8), 8,
        )?;
        for (i, module) in facts.modules.iter().enumerate() {
            let file = self.file(*module, facts.tftp_server)?;
            unsafe { module_array.pointer.cast::<u64>().add(i).write(file); }
        }
        self.responses[MODULES] = self.store([
            0, facts.modules.len() as u64, module_array.virtual_address,
        ])?.virtual_address;
        if let Some(framebuffer) = facts.framebuffer {
            let address = self.virtual_address(framebuffer.phys)?;
            // Reserve a non-null pointer even for an empty Rust slice.
            let edid = self.allocate_bytes(facts.framebuffer_edid.len().max(1), 1)?;
            unsafe { ptr::copy_nonoverlapping(facts.framebuffer_edid.as_ptr(), edid.pointer, facts.framebuffer_edid.len()); }
            let framebuffer_wire = self.store(FramebufferWire {
                address, width: framebuffer.width, height: framebuffer.height,
                pitch: framebuffer.pitch, bpp: framebuffer.bpp, memory_model: 1,
                red_mask_size: framebuffer.red_mask_size,
                red_mask_shift: framebuffer.red_mask_shift,
                green_mask_size: framebuffer.green_mask_size,
                green_mask_shift: framebuffer.green_mask_shift,
                blue_mask_size: framebuffer.blue_mask_size,
                blue_mask_shift: framebuffer.blue_mask_shift,
                reserved: [0; 7], edid_size: facts.framebuffer_edid.len() as u64,
                edid: edid.virtual_address,
            })?;
            let framebuffer_array = self.store([framebuffer_wire.virtual_address])?;
            self.responses[FRAMEBUFFERS] = self.store([
                0, 1, framebuffer_array.virtual_address,
            ])?.virtual_address;
            self.protect(MemoryRegion {
                base: framebuffer.phys,
                length: framebuffer.pitch.checked_mul(framebuffer.height).ok_or(Error::AddressOverflow)?,
                kind: FRAMEBUFFER,
            })?;
        }
        if let Some(timestamp) = facts.timestamp {
            self.responses[DATE] = self.store([0, timestamp as u64])?.virtual_address;
        }
        if facts.rsdp != 0 {
            let rsdp = self.virtual_address(facts.rsdp)?;
            self.responses[RSDP] = self.store([0, rsdp])?.virtual_address;
        }
        if facts.efi_system_table != 0 {
            let efi = self.virtual_address(facts.efi_system_table)?;
            self.responses[EFI] = self.store([0, efi])?.virtual_address;
        }
        if facts.smbios32 != 0 || facts.smbios64 != 0 {
            let smbios32 = self.virtual_address(facts.smbios32)?;
            let smbios64 = self.virtual_address(facts.smbios64)?;
            self.responses[SMBIOS] = self.store([0, smbios32, smbios64])?.virtual_address;
        }
        self.stack_size = facts.stack_size;
        self.responses[STACK] = self.store([0u64])?.virtual_address;
        self.responses[MP] = facts.mp_response;
        self.prepared = true;
        Ok(())
    }

    /// Call before patch_requests if AP startup changes the retained MP reply.
    pub fn set_mp_response(&mut self, virtual_address: u64) {
        self.responses[MP] = virtual_address;
    }

    /// Convert the *final* firmware map after successful ExitBootServices.
    /// Additional regions override UEFI types (e.g. the loaded kernel and page
    /// tables). Built-in protected ranges then override those extra regions.
    /// Splitting at all boundaries also handles GOP memory absent from the EFI
    /// map, without creating overlapping entries. No allocations occur here.
    pub fn finalize_memory_map(
        &mut self,
        regions: &[MemoryRegion],
        extra: &[MemoryRegion],
    ) -> Result<usize, Error> {
        let protected = &self.protected[..self.protected_len];
        let mut first = u64::MAX;
        for region in regions.iter().chain(extra).chain(protected) {
            if region.kind > MAPPED_RESERVED { return Err(Error::InvalidMemoryRange); }
            region.end()?;
            if region.length != 0 { first = first.min(region.base); }
        }
        let mut cursor = first;
        let mut count = 0;
        while cursor != u64::MAX {
            let mut next = u64::MAX;
            let mut kind = None;
            for region in regions.iter().chain(extra).chain(protected) {
                if region.length == 0 { continue; }
                let end = region.end()?;
                if region.base > cursor { next = next.min(region.base); }
                if end > cursor { next = next.min(end); }
                if region.base <= cursor && cursor < end { kind = Some(region.kind); }
            }
            if next == u64::MAX { break; }
            if let Some(kind) = kind {
                let entry = MemoryRegion { base: cursor, length: next - cursor, kind };
                let mut merged = false;
                if count > 0 {
                    let previous = unsafe { &mut *self.map_entries.add(count - 1) };
                    if previous.kind == kind && previous.end()? == cursor {
                        previous.length = previous.length.checked_add(entry.length).ok_or(Error::AddressOverflow)?;
                        merged = true;
                    }
                }
                if !merged {
                    if count == self.max_map_entries { return Err(Error::MemoryMapFull); }
                    unsafe { self.map_entries.add(count).write(entry); }
                    count += 1;
                }
            }
            cursor = next;
        }
        unsafe { (*self.map_response)[1] = count as u64; }
        Ok(count)
    }

    /// Answer known request records in the ELF's validated request section.
    ///
    /// # Safety
    /// `section` must be the writable, loaded physical backing of precisely the
    /// ELF `.limine_requests` section, not arbitrary kernel code or data. The
    /// section must remain exclusively borrowed until the kernel is entered.
    pub unsafe fn patch_requests(
        &self,
        section: *mut u8,
        section_len: usize,
    ) -> Result<PatchReport, Error> {
        if !self.prepared { return Err(Error::NotPrepared); }
        if section.is_null() || section as usize & 7 != 0 || section_len < 48 {
            return Err(Error::InvalidRequestSection);
        }
        let read = |offset: usize| unsafe { section.add(offset).cast::<u64>().read_unaligned() };
        let mut base_offset = None;
        for offset in (0..=section_len - 24).step_by(8) {
            if [read(offset), read(offset + 8)] == BASE {
                if base_offset.is_some() || read(offset + 16) != BASE_REVISION {
                    return Err(Error::UnsupportedBaseRevision);
                }
                base_offset = Some(offset);
            }
        }
        let base_offset = base_offset.ok_or(Error::MissingBaseRevision)?;
        let mut seen = [false; RESPONSE_COUNT];
        let mut report = PatchReport::default();
        // Validate first; a failed validation must not partially acknowledge a
        // base revision or leave half-patched request pointers in the image.
        for offset in (0..=section_len.saturating_sub(32)).step_by(8) {
            if [read(offset), read(offset + 8)] != COMMON { continue; }
            let id = [read(offset + 16), read(offset + 24)];
            let Some(index) = IDS.iter().position(|known| *known == id) else {
                report.unanswered += 1;
                continue;
            };
            let record_size = match index { MODULES => 64, MP | STACK => 56, _ => 48 };
            if offset.checked_add(record_size).is_none_or(|end| end > section_len) || seen[index] {
                return Err(Error::InvalidRequestSection);
            }
            if index == STACK && read(offset + 48) > self.stack_size {
                return Err(Error::StackTooSmall);
            }
            seen[index] = true;
            if self.responses[index] == 0 { report.unanswered += 1; }
            else { report.answered += 1; }
        }
        for required in [HHDM, MEMMAP, EXECUTABLE_ADDRESS, EXECUTABLE_FILE, STACK] {
            if !seen[required] { return Err(Error::MissingRequiredRequest); }
        }
        for offset in (0..=section_len.saturating_sub(48)).step_by(8) {
            if [read(offset), read(offset + 8)] != COMMON { continue; }
            let id = [read(offset + 16), read(offset + 24)];
            if let Some(index) = IDS.iter().position(|known| *known == id) {
                unsafe { section.add(offset + 40).cast::<u64>().write_unaligned(self.responses[index]); }
            }
        }
        unsafe {
            section.add(base_offset + 8).cast::<u64>().write_unaligned(BASE_REVISION);
            section.add(base_offset + 16).cast::<u64>().write_unaligned(0);
        }
        Ok(report)
    }
}

// These layouts are shared with limine 0.6.5 and the protocol C header.
const _: () = assert!(mem::size_of::<FileWire>() == 112);
const _: () = assert!(mem::size_of::<FramebufferWire>() == 64);
const _: () = assert!(mem::size_of::<MemoryRegion>() == 24);

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn arena() -> Vec<u64> {
        vec![0; 128 * 1024]
    }

    fn facts() -> BootFacts<'static> {
        BootFacts {
            kernel: Blob { phys: 0x800000, len: 12345, path: "/TRUEOS.elf", cmdline: "keyboard=de" },
            modules: &[], tftp_server: None, kernel_physical_base: 0x200000,
            kernel_virtual_base: 0xffffffff80100000,
            framebuffer: None, framebuffer_edid: &[], rsdp: 0, efi_system_table: 0,
            smbios32: 0, smbios64: 0, timestamp: Some(1234),
            stack_size: 16 * 1024 * 1024, mp_response: 0,
        }
    }

    fn requests() -> Vec<u64> {
        let mut words = vec![BASE[0], BASE[1], BASE_REVISION];
        for index in [HHDM, MEMMAP, EXECUTABLE_ADDRESS, EXECUTABLE_FILE, STACK] {
            words.extend_from_slice(&[COMMON[0], COMMON[1], IDS[index][0], IDS[index][1], 0, 0]);
            if index == STACK { words.push(16 * 1024 * 1024); }
        }
        words
    }

    #[test]
    fn populates_kernel_request_abi_and_retains_original_elf() {
        let mut backing = arena();
        let mut handoff = unsafe { Handoff::new(backing.as_mut_ptr() as u64, backing.len() * 8, 0, 128) }.unwrap();
        handoff.prepare(&facts()).unwrap();
        let mut request_section = requests();
        let report = unsafe { handoff.patch_requests(request_section.as_mut_ptr().cast(), request_section.len() * 8) }.unwrap();
        assert_eq!(report.answered, 5);
        assert_eq!(&request_section[..3], &[BASE[0], 6, 0]);
        let file_response = handoff.responses[EXECUTABLE_FILE] as *const u64;
        let file = unsafe { &*((*file_response.add(1)) as *const FileWire) };
        assert_eq!(file.address, 0x800000);
        assert_eq!(file.size, 12345);
        assert_eq!((file.media_type, file.tftp_ip, file.tftp_port), (0, 0, 0));
        let path = unsafe { core::ffi::CStr::from_ptr(file.path as _) };
        assert_eq!(path.to_bytes(), b"/TRUEOS.elf");
    }

    #[test]
    fn downloaded_kernel_and_modules_report_the_tftp_source() {
        let mut backing = arena();
        let mut handoff = unsafe { Handoff::new(backing.as_mut_ptr() as u64, backing.len() * 8, 0, 128) }.unwrap();
        let mut input = facts();
        let modules = [Blob { phys: 0x900000, len: 4096, path: "/EFI/BOOT/BOOTX64.EFI", cmdline: "trueos.install.bootx64" }];
        input.modules = &modules;
        input.tftp_server = Some([192, 168, 1, 20]);
        handoff.prepare(&input).unwrap();
        let kernel_response = handoff.responses[EXECUTABLE_FILE] as *const u64;
        let modules_response = handoff.responses[MODULES] as *const u64;
        unsafe {
            let kernel = &*((*kernel_response.add(1)) as *const FileWire);
            let modules = *modules_response.add(2) as *const u64;
            let module = &*((*modules) as *const FileWire);
            for file in [kernel, module] {
                assert_eq!(file.media_type, 2);
                assert_eq!(file.tftp_ip.to_le_bytes(), [192, 168, 1, 20]);
                assert_eq!(file.tftp_port, 69);
            }
        }
    }

    #[test]
    fn framebuffer_edid_is_copied_into_retained_handoff_storage() {
        let mut backing = arena();
        let mut handoff = unsafe { Handoff::new(backing.as_mut_ptr() as u64, backing.len() * 8, 0, 128) }.unwrap();
        let mut edid = [0u8; 128];
        edid[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
        edid[21] = 60;
        edid[22] = 34;
        edid[127] = 0u8.wrapping_sub(edid[..127].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)));
        let original = edid;
        let mut input = facts();
        input.framebuffer = Some(Framebuffer {
            phys: 0xc0000000, width: 2560, height: 1440, pitch: 2560 * 4, bpp: 32,
            red_mask_size: 8, red_mask_shift: 16, green_mask_size: 8, green_mask_shift: 8,
            blue_mask_size: 8, blue_mask_shift: 0,
        });
        input.framebuffer_edid = &edid;
        handoff.prepare(&input).unwrap();
        edid.fill(0); // Firmware's original buffer is no longer needed.
        unsafe {
            let response = handoff.responses[FRAMEBUFFERS] as *const u64;
            let pointers = *response.add(2) as *const u64;
            let framebuffer = &*((*pointers) as *const FramebufferWire);
            assert_eq!((framebuffer.width, framebuffer.height), (2560, 1440));
            assert_eq!(framebuffer.edid_size, 128);
            let retained = core::slice::from_raw_parts(framebuffer.edid as *const u8, 128);
            assert_eq!(retained, &original);
            assert!((backing.as_ptr() as usize..backing.as_ptr() as usize + backing.len() * 8).contains(&(retained.as_ptr() as usize)));
        }
    }

    #[test]
    fn reserves_loader_pages_and_splits_unsorted_memory_and_overlays() {
        let mut backing = arena();
        let mut handoff = unsafe { Handoff::new(backing.as_mut_ptr() as u64, backing.len() * 8, 0, 128) }.unwrap();
        handoff.prepare(&facts()).unwrap();
        let regions = [
            MemoryRegion::from_uefi(7, 0x400000, 1024, 0).unwrap(),
            MemoryRegion::from_uefi(2, 0x200000, 512, 0).unwrap(),
            MemoryRegion::from_uefi(7, 0x100000, 256, 0).unwrap(),
        ];
        let extra = [MemoryRegion { base: 0x500000, length: 0x100000, kind: EXECUTABLE_AND_MODULES }];
        let count = handoff.finalize_memory_map(&regions, &extra).unwrap();
        let map = unsafe { core::slice::from_raw_parts(handoff.map_entries, count) };
        assert!(map.windows(2).all(|pair| pair[0].end().unwrap() <= pair[1].base));
        assert!(map.contains(&MemoryRegion { base: 0x200000, length: 0x200000, kind: BOOTLOADER_RECLAIMABLE }));
        assert!(map.contains(&extra[0]));
        assert!(map.contains(&MemoryRegion { base: 0x400000, length: 0x100000, kind: USABLE }));
        assert!(map.contains(&MemoryRegion { base: 0x600000, length: 0x200000, kind: USABLE }));
        assert!(map.iter().any(|r| r.base == 0x800000 && r.kind == EXECUTABLE_AND_MODULES));
    }

    #[test]
    fn malformed_requests_do_not_partially_acknowledge_base_revision() {
        let mut backing = arena();
        let mut handoff = unsafe { Handoff::new(backing.as_mut_ptr() as u64, backing.len() * 8, 0, 128) }.unwrap();
        handoff.prepare(&facts()).unwrap();
        let mut request_section = requests();
        request_section.pop(); // Truncate the stack request payload.
        let before = request_section.clone();
        let result = unsafe { handoff.patch_requests(request_section.as_mut_ptr().cast(), request_section.len() * 8) };
        assert!(matches!(result, Err(Error::InvalidRequestSection)));
        assert_eq!(request_section, before);
    }

    #[test]
    fn overflowing_memory_ranges_are_rejected() {
        assert_eq!(MemoryRegion::from_uefi(7, 0x1000, u64::MAX, 0), Err(Error::AddressOverflow));
        assert_eq!(MemoryRegion::from_uefi(7, u64::MAX - 4095, 1, 0), Err(Error::AddressOverflow));
    }

    #[test]
    fn retains_firmware_data_and_runtime_pages_after_exit_boot_services() {
        assert_eq!(MemoryRegion::from_uefi(4, 0x1000, 1, 0).unwrap().kind, BOOTLOADER_RECLAIMABLE);
        assert_eq!(MemoryRegion::from_uefi(3, 0x2000, 1, 0).unwrap().kind, USABLE);
        assert_eq!(MemoryRegion::from_uefi(7, 0x3000, 1, 1 << 63).unwrap().kind, MAPPED_RESERVED);
    }
}
