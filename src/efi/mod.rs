use crate::pci::mmio;

use crate::limine;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

pub mod acpi;
pub mod smbios;
// pub mod acpi_uefi;  asdasd

const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453_5953_2049_4249; // "IBI SYST"

static EFI_RESET_BOOT_LOGGED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: usize,
    pub con_in: usize,
    pub console_out_handle: usize,
    pub con_out: usize,
    pub standard_error_handle: usize,
    pub std_err: usize,
    pub runtime_services: usize,
    pub boot_services: usize,
    pub number_of_table_entries: usize,
    pub configuration_table: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiGuid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl EfiGuid {
    const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }

    pub(crate) fn from_uefi_bytes(bytes: [u8; 16]) -> Self {
        // UEFI GUIDs are laid out as {u32,u16,u16,[u8;8]} in little-endian for the first
        // three fields.
        let data1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let data2 = u16::from_le_bytes([bytes[4], bytes[5]]);
        let data3 = u16::from_le_bytes([bytes[6], bytes[7]]);
        let mut data4 = [0u8; 8];
        data4.copy_from_slice(&bytes[8..16]);
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }

    pub(crate) fn fmt_canonical(&self) -> alloc::string::String {
        use alloc::format;
        format!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiConfigurationTable {
    pub vendor_guid: EfiGuid,
    pub vendor_table: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct EfiSystemTableValidation {
    pub physical_address: u64,
    pub header_size: usize,
    pub stored_crc32: u32,
    pub computed_crc32: u32,
    pub crc_valid: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EfiConfigurationTableError {
    SystemTableUnavailable,
    NoEntries,
    TooManyEntries,
    AddressInvalid,
    RangeInvalid,
    MapFailed,
}

pub fn cfg_guid_name(guid: &EfiGuid) -> Option<&'static str> {
    // Common config-table GUIDs.
    const ACPI_20: EfiGuid =
        EfiGuid::new(0x8868e871, 0xe4f1, 0x11d3, [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81]);
    const ACPI_10: EfiGuid =
        EfiGuid::new(0xeb9d2d30, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
    const SMBIOS: EfiGuid =
        EfiGuid::new(0xeb9d2d31, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
    const SMBIOS3: EfiGuid =
        EfiGuid::new(0xf2fd1544, 0x9794, 0x4a2c, [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94]);
    const MPS: EfiGuid =
        EfiGuid::new(0xeb9d2d2f, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
    const SAL: EfiGuid =
        EfiGuid::new(0xeb9d2d32, 0x2d88, 0x11d3, [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
    const DTB: EfiGuid =
        EfiGuid::new(0xb1b621d5, 0xf19c, 0x41a5, [0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0]);

    // Additional UEFI config-table GUIDs (mostly EDK2).
    const LZMA_COMPRESSION: EfiGuid =
        EfiGuid::new(0xee4e5898, 0x3914, 0x4259, [0x9d, 0x6e, 0xdc, 0x7b, 0xd7, 0x94, 0x03, 0xcf]);
    const DXE_SERVICES_TABLE: EfiGuid =
        EfiGuid::new(0x05ad34ba, 0x6f02, 0x4214, [0x95, 0x2e, 0x4d, 0xa0, 0x39, 0x8e, 0x2b, 0xb9]);
    const HOB_LIST: EfiGuid =
        EfiGuid::new(0x7739f24c, 0x93d7, 0x11d4, [0x9a, 0x3a, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
    const MEMORY_TYPE_INFO: EfiGuid =
        EfiGuid::new(0x4c19049f, 0x4137, 0x4dd3, [0x9c, 0x10, 0x8b, 0x97, 0xa8, 0x3f, 0xfd, 0xfa]);
    const DEBUG_IMAGE_INFO_TABLE: EfiGuid =
        EfiGuid::new(0x49152e77, 0x1ada, 0x4764, [0xb7, 0xa2, 0x7a, 0xfe, 0xfe, 0xd9, 0x5e, 0x8b]);
    const MEMORY_STATUS_CODE_RECORD: EfiGuid =
        EfiGuid::new(0x060cc026, 0x4c0d, 0x4dda, [0x8f, 0x41, 0x59, 0x5f, 0xef, 0x00, 0xa5, 0x02]);
    const MEMORY_ATTRIBUTES_TABLE: EfiGuid =
        EfiGuid::new(0xdcfa911d, 0x26eb, 0x469f, [0xa2, 0x20, 0x38, 0xb7, 0xdc, 0x46, 0x12, 0x20]);
    const SYSTEM_RESOURCE_TABLE: EfiGuid =
        EfiGuid::new(0xb122a263, 0x3661, 0x4f68, [0x99, 0x29, 0x78, 0xf8, 0xb0, 0xd6, 0x21, 0x80]);
    const PROPERTIES_TABLE: EfiGuid =
        EfiGuid::new(0x880aaca3, 0x4adc, 0x4a04, [0x90, 0x79, 0xb7, 0x47, 0x34, 0x08, 0x25, 0xe5]);
    const TPM_FINAL_EVENTS_TABLE: EfiGuid =
        EfiGuid::new(0x1e2ed096, 0x30e2, 0x4254, [0xbd, 0x89, 0x86, 0x3b, 0xbe, 0xf8, 0x23, 0x25]);

    if guid.data1 == ACPI_20.data1
        && guid.data2 == ACPI_20.data2
        && guid.data3 == ACPI_20.data3
        && guid.data4 == ACPI_20.data4
    {
        return Some("ACPI 2.0");
    }
    if guid.data1 == ACPI_10.data1
        && guid.data2 == ACPI_10.data2
        && guid.data3 == ACPI_10.data3
        && guid.data4 == ACPI_10.data4
    {
        return Some("ACPI 1.0");
    }
    if guid.data1 == SMBIOS.data1
        && guid.data2 == SMBIOS.data2
        && guid.data3 == SMBIOS.data3
        && guid.data4 == SMBIOS.data4
    {
        return Some("SMBIOS");
    }
    if guid.data1 == SMBIOS3.data1
        && guid.data2 == SMBIOS3.data2
        && guid.data3 == SMBIOS3.data3
        && guid.data4 == SMBIOS3.data4
    {
        return Some("SMBIOS3");
    }
    if guid.data1 == MPS.data1
        && guid.data2 == MPS.data2
        && guid.data3 == MPS.data3
        && guid.data4 == MPS.data4
    {
        return Some("MPS");
    }
    if guid.data1 == SAL.data1
        && guid.data2 == SAL.data2
        && guid.data3 == SAL.data3
        && guid.data4 == SAL.data4
    {
        return Some("SAL");
    }
    if guid.data1 == DTB.data1
        && guid.data2 == DTB.data2
        && guid.data3 == DTB.data3
        && guid.data4 == DTB.data4
    {
        return Some("DTB");
    }

    if guid.data1 == LZMA_COMPRESSION.data1
        && guid.data2 == LZMA_COMPRESSION.data2
        && guid.data3 == LZMA_COMPRESSION.data3
        && guid.data4 == LZMA_COMPRESSION.data4
    {
        return Some("LZMA compression");
    }
    if guid.data1 == DXE_SERVICES_TABLE.data1
        && guid.data2 == DXE_SERVICES_TABLE.data2
        && guid.data3 == DXE_SERVICES_TABLE.data3
        && guid.data4 == DXE_SERVICES_TABLE.data4
    {
        return Some("DXE services table");
    }
    if guid.data1 == HOB_LIST.data1
        && guid.data2 == HOB_LIST.data2
        && guid.data3 == HOB_LIST.data3
        && guid.data4 == HOB_LIST.data4
    {
        return Some("HOB list");
    }
    if guid.data1 == MEMORY_TYPE_INFO.data1
        && guid.data2 == MEMORY_TYPE_INFO.data2
        && guid.data3 == MEMORY_TYPE_INFO.data3
        && guid.data4 == MEMORY_TYPE_INFO.data4
    {
        return Some("Memory type info");
    }
    if guid.data1 == DEBUG_IMAGE_INFO_TABLE.data1
        && guid.data2 == DEBUG_IMAGE_INFO_TABLE.data2
        && guid.data3 == DEBUG_IMAGE_INFO_TABLE.data3
        && guid.data4 == DEBUG_IMAGE_INFO_TABLE.data4
    {
        return Some("Debug image info table");
    }
    if guid.data1 == MEMORY_STATUS_CODE_RECORD.data1
        && guid.data2 == MEMORY_STATUS_CODE_RECORD.data2
        && guid.data3 == MEMORY_STATUS_CODE_RECORD.data3
        && guid.data4 == MEMORY_STATUS_CODE_RECORD.data4
    {
        return Some("Memory status code record");
    }
    if guid.data1 == MEMORY_ATTRIBUTES_TABLE.data1
        && guid.data2 == MEMORY_ATTRIBUTES_TABLE.data2
        && guid.data3 == MEMORY_ATTRIBUTES_TABLE.data3
        && guid.data4 == MEMORY_ATTRIBUTES_TABLE.data4
    {
        return Some("Memory attributes table");
    }
    if guid.data1 == SYSTEM_RESOURCE_TABLE.data1
        && guid.data2 == SYSTEM_RESOURCE_TABLE.data2
        && guid.data3 == SYSTEM_RESOURCE_TABLE.data3
        && guid.data4 == SYSTEM_RESOURCE_TABLE.data4
    {
        return Some("EFI system resource table");
    }
    if guid.data1 == PROPERTIES_TABLE.data1
        && guid.data2 == PROPERTIES_TABLE.data2
        && guid.data3 == PROPERTIES_TABLE.data3
        && guid.data4 == PROPERTIES_TABLE.data4
    {
        return Some("EFI properties table");
    }
    if guid.data1 == TPM_FINAL_EVENTS_TABLE.data1
        && guid.data2 == TPM_FINAL_EVENTS_TABLE.data2
        && guid.data3 == TPM_FINAL_EVENTS_TABLE.data3
        && guid.data4 == TPM_FINAL_EVENTS_TABLE.data4
    {
        return Some("TPM final events table");
    }

    None
}

pub fn system_table() -> Option<&'static EfiSystemTable> {
    let phys_or_virt = match limine::efi_system_table_address() {
        Some(addr) if addr != 0 => addr,
        _ => return None,
    };
    let phys = limine::try_as_phys_addr(phys_or_virt)?;
    if !limine::memmap_contains_phys_range(phys, core::mem::size_of::<EfiSystemTable>()) {
        return None;
    }
    let mapped = mmio::map_limine_struct::<EfiSystemTable>(phys).ok()?;
    let st = unsafe { mapped.as_ref() };
    if st.hdr.signature != EFI_SYSTEM_TABLE_SIGNATURE {
        return None;
    }
    Some(st)
}

/// Validate the complete EFI system-table header, including its CRC32.
pub fn system_table_validation() -> Option<EfiSystemTableValidation> {
    const MAX_HEADER_BYTES: usize = 4096;

    let raw = limine::efi_system_table_address()?;
    let physical_address = limine::try_as_phys_addr(raw)?;
    if !limine::memmap_contains_phys_range(
        physical_address,
        core::mem::size_of::<EfiTableHeader>(),
    ) {
        return None;
    }
    let header_ptr = mmio::map_limine_struct::<EfiTableHeader>(physical_address).ok()?;
    let header = unsafe { header_ptr.as_ref() };
    if header.signature != EFI_SYSTEM_TABLE_SIGNATURE {
        return None;
    }
    let header_size = header.header_size as usize;
    if header_size < core::mem::size_of::<EfiSystemTable>() || header_size > MAX_HEADER_BYTES {
        return None;
    }
    if !limine::memmap_contains_phys_range(physical_address, header_size) {
        return None;
    }
    let ptr = mmio::map_mmio_region_exact(physical_address, header_size).ok()?;
    let bytes = unsafe { core::slice::from_raw_parts(ptr.as_ptr(), header_size) };
    let mut crc_bytes = Vec::from(bytes);
    crc_bytes.get_mut(16..20)?.fill(0);
    let computed_crc32 = crc32fast::hash(&crc_bytes);
    Some(EfiSystemTableValidation {
        physical_address,
        header_size,
        stored_crc32: header.crc32,
        computed_crc32,
        crc_valid: computed_crc32 == header.crc32,
    })
}

/// Decode the firmware-vendor UTF-16 string with a hard upper bound.
pub fn firmware_vendor_string() -> Option<String> {
    const MAX_VENDOR_CODE_UNITS: usize = 256;

    let st = system_table()?;
    let raw = st.firmware_vendor as u64;
    if raw == 0 {
        return None;
    }
    let phys = limine::try_as_phys_addr(raw)?;
    let byte_len = MAX_VENDOR_CODE_UNITS * core::mem::size_of::<u16>();
    if !limine::memmap_contains_phys_range(phys, byte_len) {
        return None;
    }
    let ptr = mmio::map_mmio_region_exact(phys, byte_len).ok()?;
    let units =
        unsafe { core::slice::from_raw_parts(ptr.as_ptr() as *const u16, MAX_VENDOR_CODE_UNITS) };
    let len = units.iter().position(|unit| *unit == 0)?;
    Some(String::from_utf16_lossy(&units[..len]))
}

/// Return the EFI configuration-table array after validating its count and
/// complete physical range.
pub fn configuration_tables() -> Result<&'static [EfiConfigurationTable], EfiConfigurationTableError>
{
    const MAX_CONFIGURATION_TABLES: usize = 4096;

    let st = system_table().ok_or(EfiConfigurationTableError::SystemTableUnavailable)?;
    let count = st.number_of_table_entries;
    if count == 0 {
        return Err(EfiConfigurationTableError::NoEntries);
    }
    if count > MAX_CONFIGURATION_TABLES {
        return Err(EfiConfigurationTableError::TooManyEntries);
    }
    let raw = st.configuration_table as u64;
    let phys = limine::try_as_phys_addr(raw).ok_or(EfiConfigurationTableError::AddressInvalid)?;
    let byte_len = core::mem::size_of::<EfiConfigurationTable>()
        .checked_mul(count)
        .ok_or(EfiConfigurationTableError::TooManyEntries)?;
    if !limine::memmap_contains_phys_range(phys, byte_len) {
        return Err(EfiConfigurationTableError::RangeInvalid);
    }
    let (ptr, _) = mmio::map_limine_slice::<EfiConfigurationTable>(phys, count)
        .map_err(|_| EfiConfigurationTableError::MapFailed)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), count) })
}

#[repr(C)]
pub struct EfiRuntimeServices {
    pub hdr: EfiTableHeader,
    pub get_time: usize,
    pub set_time: usize,
    pub get_wakeup_time: usize,
    pub set_wakeup_time: usize,
    pub set_virtual_address_map: usize,
    pub convert_pointer: usize,
    pub get_variable: usize,
    pub get_next_variable_name: usize,
    pub set_variable: usize,
    pub get_next_high_mono_count: usize,
    pub reset_system: usize,
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EfiResetType {
    Cold = 0,
    Warm = 1,
    Shutdown = 2,
    PlatformSpecific = 3,
}

impl EfiResetType {
    const fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::Shutdown => "shutdown",
            Self::PlatformSpecific => "platform-specific",
        }
    }
}

pub(crate) fn log_reset_runtime_once() {
    if EFI_RESET_BOOT_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(st) = system_table() else {
        crate::log!("efi/reset: runtime-services present=0 reason=system-table-missing\n");
        return;
    };
    let Ok(rt_ptr) = mmio::map_limine_struct::<EfiRuntimeServices>(st.runtime_services as u64)
    else {
        crate::log!(
            "efi/reset: runtime-services present=0 reason=runtime-map-failed phys=0x{:X}\n",
            st.runtime_services
        );
        return;
    };
    let rt = unsafe { rt_ptr.as_ref() };
    let reset_phys = rt.reset_system as u64;
    let reset_present = reset_phys != 0;
    let acpi20_guid = EfiGuid::from_uefi_bytes([
        0x71, 0xE8, 0x68, 0x88, 0xF1, 0xE4, 0xD3, 0x11, 0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88,
        0x81,
    ]);
    let reset_types = [
        EfiResetType::Cold,
        EfiResetType::Warm,
        EfiResetType::Shutdown,
        EfiResetType::PlatformSpecific,
    ];
    crate::log!(
        "efi/reset: runtime-services present=1 reset_system_present={} reset_system_phys=0x{:X} reset_types={},{},{},{} guid_probe={} guid_name={} action=log-only\n",
        reset_present as u8,
        reset_phys,
        reset_types[0].label(),
        reset_types[1].label(),
        reset_types[2].label(),
        reset_types[3].label(),
        acpi20_guid.fmt_canonical(),
        cfg_guid_name(&acpi20_guid).unwrap_or("unknown"),
    );
}

pub unsafe fn runtime_services_reset(reset_type: EfiResetType) {
    let Some(st) = system_table() else { return };
    // st.runtime_services is physical.
    // Map the RuntimeServices table structure.
    let Ok(rt_ptr) = mmio::map_limine_struct::<EfiRuntimeServices>(st.runtime_services as u64)
    else {
        return;
    };
    let rt = rt_ptr.as_ref();

    // rt.reset_system is physical (function pointer).
    // Convert to virtual via HHDM (assuming Limine provides it and it's executable).
    let Some(hhdm) = limine::hhdm_offset() else {
        return;
    };

    let fn_phys = rt.reset_system as u64;
    let fn_virt = hhdm + fn_phys;

    let reset_fn: unsafe extern "efiapi" fn(EfiResetType, usize, usize, *const u8) -> ! =
        core::mem::transmute(fn_virt as usize);

    reset_fn(reset_type, 0, 0, core::ptr::null());
}
