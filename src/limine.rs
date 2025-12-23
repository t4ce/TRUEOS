use limine::{memory_map, request, response, BaseRevision};

#[used]
#[link_section = ".limine_requests"]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".limine_requests"]
pub static SMP_REQUEST: request::MpRequest = request::MpRequest::new();

#[used]
#[link_section = ".limine_requests"]
pub static HHDM_REQUEST: request::HhdmRequest = request::HhdmRequest::new();

#[used]
#[link_section = ".limine_requests"]
pub static MEMMAP_REQUEST: request::MemoryMapRequest = request::MemoryMapRequest::new();

pub fn hhdm_offset() -> Option<u64> {
    let resp = unsafe { HHDM_REQUEST.get_response()? };
    Some(resp.offset())
}

pub fn memmap_entries() -> Option<&'static [&'static memory_map::Entry]> {
    let resp = unsafe { MEMMAP_REQUEST.get_response()? };
    Some(resp.entries())
}

pub fn smp_response() -> Option<&'static response::MpResponse> {
    unsafe { SMP_REQUEST.get_response() }
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

