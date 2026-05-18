use core::sync::atomic::{AtomicU64, Ordering};
use limine::{BaseRevision, memmap as memory_map, request};

pub type FramebufferResponse = request::FramebufferResponse;
pub type MpResponse = request::MpResponse;
pub type MpCpu = limine::mp::MpInfo;
pub type BootloaderPerformanceResponse = request::BootloaderPerformanceResponse;
pub type BootloaderPerformanceRequest = request::BootloaderPerformanceRequest;
pub type EfiSystemTableResponse = request::EfiResponse;
pub type EfiSystemTableRequest = request::EfiRequest;

const UNSET_U64: u64 = u64::MAX;

static BOOT_TIMESTAMP_SECS_CACHE: AtomicU64 = AtomicU64::new(UNSET_U64);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[cfg(target_arch = "x86_64")]
#[used]
#[unsafe(link_section = ".limine_requests")]
pub static SMP_REQUEST: request::MpRequest = request::MpRequest::new(0);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static HHDM_REQUEST: request::HhdmRequest = request::HhdmRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static MEMMAP_REQUEST: request::MemmapRequest = request::MemmapRequest::new();

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
pub static MODULE_REQUEST: request::ModulesRequest = request::ModulesRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static STACK_SIZE_REQUEST: request::StackSizeRequest =
    request::StackSizeRequest::new(16 * 1024 * 1024);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static DATE_AT_BOOT_REQUEST: request::DateAtBootRequest = request::DateAtBootRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static BOOTLOADER_PERFORMANCE_REQUEST: BootloaderPerformanceRequest =
    BootloaderPerformanceRequest::new();

#[cfg(target_arch = "x86_64")]
#[used]
#[unsafe(link_section = ".limine_requests")]
pub static RSDP_REQUEST: request::RsdpRequest = request::RsdpRequest::new();

#[cfg(target_arch = "x86_64")]
#[used]
#[unsafe(link_section = ".limine_requests")]
pub static EFI_SYSTEM_TABLE_REQUEST: EfiSystemTableRequest = EfiSystemTableRequest::new();

pub fn hhdm_offset() -> Option<u64> {
    let resp = HHDM_REQUEST.response()?;
    Some(resp.offset)
}

pub fn memmap_entries() -> Option<&'static [&'static memory_map::Entry]> {
    let resp = MEMMAP_REQUEST.response()?;
    Some(resp.entries())
}

pub fn framebuffer_response() -> Option<&'static FramebufferResponse> {
    FRAMEBUFFER_REQUEST.response()
}

pub fn executable_address_bases() -> Option<(u64, u64)> {
    let resp = EXECUTABLE_ADDRESS_REQUEST.response()?;
    Some((resp.virtual_base, resp.physical_base))
}

pub fn module_bytes_by_string(expected: &[u8]) -> Option<&'static [u8]> {
    let resp = MODULE_REQUEST.response()?;
    for m in resp.modules().iter() {
        if m.cmdline().as_bytes() == expected {
            return bytes_from_limine_file(m);
        }
    }
    None
}

pub fn module_bytes_by_path_suffix(expected_suffix: &[u8]) -> Option<&'static [u8]> {
    let resp = MODULE_REQUEST.response()?;
    for m in resp.modules().iter() {
        if m.path().as_bytes().ends_with(expected_suffix) {
            return bytes_from_limine_file(m);
        }
    }
    None
}

pub fn kernel_file_bytes() -> Option<&'static [u8]> {
    let resp = EXECUTABLE_FILE_REQUEST.response()?;
    bytes_from_limine_file(resp.executable_file())
}

pub fn install_kernel_bytes() -> Option<&'static [u8]> {
    // Re-use the kernel executable file itself rather than a separate module
    kernel_file_bytes()
}

pub fn install_bootx64_bytes() -> Option<&'static [u8]> {
    if let Some(bootx64) = module_bytes_by_string(b"trueos.install.bootx64") {
        return Some(bootx64);
    }
    let efi_img = module_bytes_by_string(b"trueos.install.efi_img")?;
    crate::efi_img::bootx64_from_efi_img(efi_img)
}

pub fn guest_kernel_bytes() -> Option<&'static [u8]> {
    // Re-use the kernel executable file itself rather than a separate module
    kernel_file_bytes()
}

fn bytes_from_limine_file(file: &limine::file::File) -> Option<&'static [u8]> {
    let data = file.data();
    if data.is_empty() {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(data.as_ptr(), data.len()) })
}

#[cfg(target_arch = "x86_64")]
pub fn smp_response() -> Option<&'static MpResponse> {
    SMP_REQUEST.response()
}

#[cfg(not(target_arch = "x86_64"))]
pub fn smp_response() -> Option<&'static MpResponse> {
    None
}

#[cfg(target_arch = "x86_64")]
pub fn mp_cpu_id(cpu: &MpCpu) -> u32 {
    cpu.lapic_id
}

#[cfg(not(target_arch = "x86_64"))]
pub fn mp_cpu_id(cpu: &MpCpu) -> u32 {
    if cpu.processor_id != 0 {
        cpu.processor_id
    } else {
        cpu.mpidr as u32
    }
}

pub fn prime_bootloader_caches() {
    let _ = cache_boot_timestamp_secs();
}

pub fn boot_timestamp_secs() -> Option<u64> {
    let cached = BOOT_TIMESTAMP_SECS_CACHE.load(Ordering::Acquire);
    if cached != UNSET_U64 {
        return Some(cached);
    }
    cache_boot_timestamp_secs()
}

fn cache_boot_timestamp_secs() -> Option<u64> {
    let resp = DATE_AT_BOOT_REQUEST.response()?;
    let secs = resp.timestamp as u64;
    BOOT_TIMESTAMP_SECS_CACHE.store(secs, Ordering::Release);
    Some(secs)
}

pub fn bootloader_performance() -> Option<&'static BootloaderPerformanceResponse> {
    BOOTLOADER_PERFORMANCE_REQUEST.response()
}

#[cfg(target_arch = "x86_64")]
pub fn efi_system_table_response() -> Option<&'static EfiSystemTableResponse> {
    EFI_SYSTEM_TABLE_REQUEST.response()
}

#[cfg(not(target_arch = "x86_64"))]
pub fn efi_system_table_response() -> Option<&'static EfiSystemTableResponse> {
    None
}

#[cfg(target_arch = "x86_64")]
pub fn rsdp_address() -> Option<u64> {
    let resp = RSDP_REQUEST.response()?;
    Some(resp.address as u64)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn rsdp_address() -> Option<u64> {
    None
}

pub fn efi_system_table_address() -> Option<u64> {
    let resp = efi_system_table_response()?;
    Some(resp.address as u64)
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

pub fn memmap_type_name(entry_type: u64) -> &'static str {
    match entry_type {
        memory_map::MEMMAP_USABLE => "USABLE",
        memory_map::MEMMAP_RESERVED => "RESERVED",
        memory_map::MEMMAP_ACPI_RECLAIMABLE => "ACPI_RECLAIMABLE",
        memory_map::MEMMAP_ACPI_NVS => "ACPI_NVS",
        memory_map::MEMMAP_BAD_MEMORY => "BAD_MEMORY",
        memory_map::MEMMAP_BOOTLOADER_RECLAIMABLE => "BOOTLOADER_RECLAIMABLE",
        memory_map::MEMMAP_EXECUTABLE_AND_MODULES => "EXECUTABLE_AND_MODULES",
        memory_map::MEMMAP_FRAMEBUFFER => "FRAMEBUFFER",
        memory_map::MEMMAP_MAPPED_RESERVED => "MAPPED_RESERVED",
        _ => "OTHER",
    }
}
