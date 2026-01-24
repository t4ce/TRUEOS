use core::{char, ptr};

use crate::pci::mmio;

use spin::Once;

use crate::limine;

pub mod acpi;
pub mod tbl;

static LOG_ONCE: Once<()> = Once::new();

const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453_5953_2049_4249; // "IBI SYST"

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct EfiSystemTable {
    hdr: EfiTableHeader,
    firmware_vendor: *const u16,
    firmware_revision: u32,
    console_in_handle: usize,
    con_in: usize,
    console_out_handle: usize,
    con_out: usize,
    standard_error_handle: usize,
    std_err: usize,
    runtime_services: usize,
    boot_services: usize,
    number_of_table_entries: usize,
    configuration_table: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
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
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiConfigurationTable {
    vendor_guid: EfiGuid,
    vendor_table: usize,
}

fn cfg_guid_name(guid: &EfiGuid) -> Option<&'static str> {
    // Common config-table GUIDs.
    const ACPI_20: EfiGuid = EfiGuid::new(
        0x8868e871,
        0xe4f1,
        0x11d3,
        [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
    );
    const ACPI_10: EfiGuid = EfiGuid::new(
        0xeb9d2d30,
        0x2d88,
        0x11d3,
        [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
    );
    const SMBIOS: EfiGuid = EfiGuid::new(
        0xeb9d2d31,
        0x2d88,
        0x11d3,
        [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
    );
    const SMBIOS3: EfiGuid = EfiGuid::new(
        0xf2fd1544,
        0x9794,
        0x4a2c,
        [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94],
    );
    const MPS: EfiGuid = EfiGuid::new(
        0xeb9d2d2f,
        0x2d88,
        0x11d3,
        [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
    );
    const SAL: EfiGuid = EfiGuid::new(
        0xeb9d2d32,
        0x2d88,
        0x11d3,
        [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
    );
    const DTB: EfiGuid = EfiGuid::new(
        0xb1b621d5,
        0xf19c,
        0x41a5,
        [0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0],
    );

    // Additional UEFI config-table GUIDs (mostly EDK2).
    const LZMA_COMPRESSION: EfiGuid = EfiGuid::new(
        0xee4e5898,
        0x3914,
        0x4259,
        [0x9d, 0x6e, 0xdc, 0x7b, 0xd7, 0x94, 0x03, 0xcf],
    );
    const DXE_SERVICES_TABLE: EfiGuid = EfiGuid::new(
        0x05ad34ba,
        0x6f02,
        0x4214,
        [0x95, 0x2e, 0x4d, 0xa0, 0x39, 0x8e, 0x2b, 0xb9],
    );
    const HOB_LIST: EfiGuid = EfiGuid::new(
        0x7739f24c,
        0x93d7,
        0x11d4,
        [0x9a, 0x3a, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
    );
    const MEMORY_TYPE_INFO: EfiGuid = EfiGuid::new(
        0x4c19049f,
        0x4137,
        0x4dd3,
        [0x9c, 0x10, 0x8b, 0x97, 0xa8, 0x3f, 0xfd, 0xfa],
    );
    const DEBUG_IMAGE_INFO_TABLE: EfiGuid = EfiGuid::new(
        0x49152e77,
        0x1ada,
        0x4764,
        [0xb7, 0xa2, 0x7a, 0xfe, 0xfe, 0xd9, 0x5e, 0x8b],
    );
    const MEMORY_STATUS_CODE_RECORD: EfiGuid = EfiGuid::new(
        0x060cc026,
        0x4c0d,
        0x4dda,
        [0x8f, 0x41, 0x59, 0x5f, 0xef, 0x00, 0xa5, 0x02],
    );
    const MEMORY_ATTRIBUTES_TABLE: EfiGuid = EfiGuid::new(
        0xdcfa911d,
        0x26eb,
        0x469f,
        [0xa2, 0x20, 0x38, 0xb7, 0xdc, 0x46, 0x12, 0x20],
    );

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

    None
}

#[inline(always)]
fn guid_short(guid: &EfiGuid) -> (u16, u16) {
    let first4 = (guid.data1 >> 16) as u16;
    let last4 = u16::from_be_bytes([guid.data4[6], guid.data4[7]]);
    (first4, last4)
}

pub fn log_system_table_once() {
    LOG_ONCE.call_once(|| {
        let phys_or_virt = match limine::efi_system_table_address() {
            Some(addr) if addr != 0 => addr,
            _ => return,
        };

        let mapped = match mmio::map_limine_struct::<EfiSystemTable>(phys_or_virt) {
            Ok(m) => m,
            Err(err) => {
                crate::log!(
                    "UEFI: EFI system table map failed addr=0x{:016X} size=0x{:X} err={:?}\n",
                    phys_or_virt,
                    core::mem::size_of::<EfiSystemTable>(),
                    err
                );
                return;
            }
        };

        // Safety: The region is explicitly mapped and sized for EfiSystemTable.
        // We still do a minimal sanity check before reading further.
        let st = unsafe { mapped.as_ref() };
        if st.hdr.signature != EFI_SYSTEM_TABLE_SIGNATURE {
            crate::log!(
                "UEFI: EFI system table at 0x{:016X} signature mismatch 0x{:016X}\n",
                phys_or_virt,
                st.hdr.signature
            );
            return;
        }

        let vendor = (|| {
            let vendor_addr = st.firmware_vendor as u64;
            // Map at most 96 UTF-16 units (+ trailing NUL).
            let (vendor_ptr, _) = mmio::map_limine_slice::<u16>(vendor_addr, 97).ok()?;
            let vendor_ptr = vendor_ptr.as_ptr() as *const u16;
            unsafe { read_utf16z_lossy(vendor_ptr, 96) }
        })();
        if let Some(vendor) = vendor {
            crate::log!(
                "UEFI: SystemTable rev=0x{:08X} vendor='{}' fw_rev=0x{:08X} rt=0x{:016X} bs=0x{:016X} cfg_entries={} cfg=0x{:016X}\n",
                st.hdr.revision,
                vendor,
                st.firmware_revision,
                st.runtime_services as u64,
                st.boot_services as u64,
                st.number_of_table_entries as u64,
                st.configuration_table as u64,
            );
        } else {
            crate::log!(
                "UEFI: SystemTable rev=0x{:08X} fw_rev=0x{:08X} rt=0x{:016X} bs=0x{:016X} cfg_entries={} cfg=0x{:016X}\n",
                st.hdr.revision,
                st.firmware_revision,
                st.runtime_services as u64,
                st.boot_services as u64,
                st.number_of_table_entries as u64,
                st.configuration_table as u64,
            );
        }

        // Dump configuration table entries (GUID -> vendor_table pointer).
        let cfg_addr = st.configuration_table as u64;
        let cfg_entries = st.number_of_table_entries;
        if cfg_addr != 0 && cfg_entries != 0 {
            const MAX_CFG_DUMP: usize = 64;
            let dump_count = core::cmp::min(cfg_entries, MAX_CFG_DUMP);
            match mmio::map_limine_slice::<EfiConfigurationTable>(cfg_addr, dump_count) {
                Ok((cfg_ptr, _)) => {
                    let slice =
                        unsafe { core::slice::from_raw_parts(cfg_ptr.as_ptr(), dump_count) };
                    for (idx, entry) in slice.iter().enumerate() {
                        let name = cfg_guid_name(&entry.vendor_guid).unwrap_or("Unknown");
                        let (first4, last4) = guid_short(&entry.vendor_guid);
                        crate::log!(
                            "UEFI: cfg[{:<2}] {:04x}~{:04x} ({}) table=0x{:016X}\n",
                            idx as u64,
                            first4,
                            last4,
                            name,
                            entry.vendor_table as u64
                        );
                    }
                    if cfg_entries > dump_count {
                        crate::log!(
                            "UEFI: cfg dump truncated ({} of {})\n",
                            dump_count as u64,
                            cfg_entries as u64
                        );
                    }
                }
                Err(err) => {
                    crate::log!(
                        "UEFI: cfg table map failed addr=0x{:016X} entries={} err={:?}\n",
                        cfg_addr,
                        cfg_entries as u64,
                        err
                    );
                }
            }
        }
    });
}

unsafe fn read_utf16z_lossy(ptr16: *const u16, max_units: usize) -> Option<alloc::string::String> {
    if ptr16.is_null() {
        return None;
    }

    let mut out = alloc::string::String::new();
    for i in 0..max_units {
        let unit = ptr::read(ptr16.add(i));
        if unit == 0 {
            break;
        }
        // UEFI strings are UCS-2 in practice; treat as BMP.
        let ch = char::from_u32(unit as u32).unwrap_or('\u{FFFD}');
        out.push(ch);
    }

    Some(out)
}
