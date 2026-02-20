use core::ptr;
use limine::{memory_map, request, response, BaseRevision};

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static SMP_REQUEST: request::MpRequest = request::MpRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static HHDM_REQUEST: request::HhdmRequest = request::HhdmRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static MEMMAP_REQUEST: request::MemoryMapRequest = request::MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static FRAMEBUFFER_REQUEST: request::FramebufferRequest = request::FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static EXECUTABLE_ADDRESS_REQUEST: request::ExecutableAddressRequest =
    request::ExecutableAddressRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static EXECUTABLE_FILE_REQUEST: request::ExecutableFileRequest =
    request::ExecutableFileRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static MODULE_REQUEST: request::ModuleRequest = request::ModuleRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static STACK_SIZE_REQUEST: request::StackSizeRequest =
    request::StackSizeRequest::new().with_size(16 * 1024 * 1024);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static DATE_AT_BOOT_REQUEST: request::DateAtBootRequest = request::DateAtBootRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static BOOTLOADER_PERFORMANCE_REQUEST: BootloaderPerformanceRequest =
    BootloaderPerformanceRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static RSDP_REQUEST: request::RsdpRequest = request::RsdpRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static EFI_SYSTEM_TABLE_REQUEST: EfiSystemTableRequest = EfiSystemTableRequest::new();

pub fn hhdm_offset() -> Option<u64> {
    let resp = HHDM_REQUEST.get_response()?;
    Some(resp.offset())
}

pub fn memmap_entries() -> Option<&'static [&'static memory_map::Entry]> {
    let resp = MEMMAP_REQUEST.get_response()?;
    Some(resp.entries())
}

pub fn framebuffer_response() -> Option<&'static response::FramebufferResponse> {
    FRAMEBUFFER_REQUEST.get_response()
}

pub fn executable_address_bases() -> Option<(u64, u64)> {
    let resp = EXECUTABLE_ADDRESS_REQUEST.get_response()?;
    Some((resp.virtual_base(), resp.physical_base()))
}

pub fn module_bytes_by_string(expected: &[u8]) -> Option<&'static [u8]> {
    let resp = MODULE_REQUEST.get_response()?;
    for m in resp.modules().iter() {
        if m.string().to_bytes() == expected {
            return bytes_from_limine_file(m);
        }
    }
    None
}

pub fn kernel_file_bytes() -> Option<&'static [u8]> {
    let resp = EXECUTABLE_FILE_REQUEST.get_response()?;
    bytes_from_limine_file(resp.file())
}

pub fn install_kernel_bytes() -> Option<&'static [u8]> {
    // Re-use the kernel executable file itself rather than a separate module
    kernel_file_bytes()
}

pub fn install_bootx64_bytes() -> Option<&'static [u8]> {
    module_bytes_by_string(b"trueos.install.bootx64")
}

pub fn guest_kernel_bytes() -> Option<&'static [u8]> {
    // Re-use the kernel executable file itself rather than a separate module
    kernel_file_bytes()
}

fn bytes_from_limine_file(file: &limine::file::File) -> Option<&'static [u8]> {
    let addr = file.addr();
    let size = file.size() as usize;
    if addr.is_null() || size == 0 {
        return None;
    }

    // Limine implementations may report either a dereferenceable virtual address or a physical
    // address for bootloader-provided buffers. If it looks like a physical/HHDM address, translate
    // it through HHDM so we can safely read it later.
    let addr_u64 = addr as u64;
    let ptr = if let Some(phys) = try_as_phys_addr(addr_u64) {
        if let Some(hhdm) = hhdm_offset() {
            (hhdm + phys) as *const u8
        } else {
            addr as *const u8
        }
    } else {
        addr as *const u8
    };

    Some(unsafe { core::slice::from_raw_parts(ptr, size) })
}

pub fn smp_response() -> Option<&'static response::MpResponse> {
    SMP_REQUEST.get_response()
}

pub fn boot_timestamp_secs() -> Option<u64> {
    let resp = DATE_AT_BOOT_REQUEST.get_response()?;
    Some(resp.timestamp().as_secs())
}

pub fn bootloader_performance() -> Option<&'static BootloaderPerformanceResponse> {
    BOOTLOADER_PERFORMANCE_REQUEST.get_response()
}

pub fn rsdp_address() -> Option<u64> {
    let resp = RSDP_REQUEST.get_response()?;
    Some(resp.address() as u64)
}

pub fn efi_system_table_address() -> Option<u64> {
    let resp = EFI_SYSTEM_TABLE_REQUEST.get_response()?;
    Some(resp.address)
}

/// Returns true if `phys` lies within any Limine-reported memory map range.
///
/// Note: This is a containment check only. It does not imply the region is safe to keep
/// borrowing forever; treat BOOTLOADER_RECLAIMABLE etc as reclaimable.
pub fn memmap_contains_phys(phys: u64) -> bool {
    let Some(entries) = memmap_entries() else {
        return false;
    };
    for entry in entries {
        let base = entry.base;
        let end = entry.base.saturating_add(entry.length);
        if phys >= base && phys < end {
            return true;
        }
    }
    false
}

/// Try to interpret an address as something that can be treated as physical memory.
///
/// Accepts:
/// - A raw physical address present in the Limine memory map
/// - An HHDM address (HHDM+phys) where `phys` is present in the Limine memory map
///
/// This is intended as a practical kernel-side contract for consuming Limine/firmware pointers:
/// turn them into a physical address first, then map explicitly before dereferencing.
pub fn try_as_phys_addr(addr: u64) -> Option<u64> {
    if memmap_contains_phys(addr) {
        return Some(addr);
    }

    let hhdm = hhdm_offset()?;
    let phys = addr.checked_sub(hhdm)?;
    if memmap_contains_phys(phys) {
        Some(phys)
    } else {
        None
    }
}

pub fn memmap_type_name(entry_type: memory_map::EntryType) -> &'static str {
    use memory_map::EntryType as T;
    match entry_type {
        T::USABLE => "USABLE",
        T::RESERVED => "RESERVED",
        T::ACPI_RECLAIMABLE => "ACPI_RECLAIMABLE",
        T::ACPI_NVS => "ACPI_NVS",
        T::BAD_MEMORY => "BAD_MEMORY",
        T::BOOTLOADER_RECLAIMABLE => "BOOTLOADER_RECLAIMABLE",
        T::EXECUTABLE_AND_MODULES => "EXECUTABLE_AND_MODULES",
        T::FRAMEBUFFER => "FRAMEBUFFER",
        _ => "OTHER",
    }
}

#[repr(C)]
pub struct BootloaderPerformanceResponse {
    revision: u64,
    reset_usec: u64,
    init_usec: u64,
    exec_usec: u64,
}

impl BootloaderPerformanceResponse {
    pub fn reset_usec(&self) -> u64 {
        self.reset_usec
    }

    pub fn init_usec(&self) -> u64 {
        self.init_usec
    }

    pub fn exec_usec(&self) -> u64 {
        self.exec_usec
    }
}

#[repr(C)]
pub struct BootloaderPerformanceRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut BootloaderPerformanceResponse,
}

unsafe impl Sync for BootloaderPerformanceRequest {}

impl BootloaderPerformanceRequest {
    pub const fn new() -> Self {
        Self {
            id: [
                0xc7b1dd30df4c8b88,
                0x0a82e883a194f07b,
                0x6b50ad9bf36d13ad,
                0xdc4c7e88fc759e17,
            ],
            revision: 0,
            response: ptr::null_mut(),
        }
    }

    pub fn get_response(&self) -> Option<&'static BootloaderPerformanceResponse> {
        let resp = self.response;
        if resp.is_null() {
            None
        } else {
            Some(unsafe { &*resp })
        }
    }
}

#[repr(C)]
pub struct EfiSystemTableResponse {
    revision: u64,
    pub address: u64,
}

#[repr(C)]
pub struct EfiSystemTableRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut EfiSystemTableResponse,
}

unsafe impl Sync for EfiSystemTableRequest {}

impl EfiSystemTableRequest {
    pub const fn new() -> Self {
        // LIMINE_EFI_SYSTEM_TABLE_REQUEST_ID { LIMINE_COMMON_MAGIC, 0x5ceba5163eaaf6d6, 0x0a6981610cf65fcc }
        Self {
            id: [
                0xc7b1dd30df4c8b88,
                0x0a82e883a194f07b,
                0x5ceba5163eaaf6d6,
                0x0a6981610cf65fcc,
            ],
            revision: 0,
            response: ptr::null_mut(),
        }
    }

    pub fn get_response(&self) -> Option<&'static EfiSystemTableResponse> {
        let resp = self.response;
        if resp.is_null() {
            None
        } else {
            Some(unsafe { &*resp })
        }
    }
}
