use core::{char, ptr};

use crate::pci::mmio;

use spin::Once;

use crate::limine;

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

pub fn log_system_table_once() {
    LOG_ONCE.call_once(|| {
        let phys_or_virt = match limine::efi_system_table_address() {
            Some(addr) if addr != 0 => addr,
            _ => return,
        };

        let Some(st_phys) = limine::try_as_phys_addr(phys_or_virt) else {
            crate::log!(
                "UEFI: EFI system table at 0x{:016X} (not a mappable physical/HHDM address; not parsing)\n",
                phys_or_virt
            );
            return;
        };

        let Ok(mapped) = mmio::map_mmio_region_exact(st_phys, core::mem::size_of::<EfiSystemTable>()) else {
            crate::log!(
                "UEFI: EFI system table map failed phys=0x{:016X} size=0x{:X}\n",
                st_phys,
                core::mem::size_of::<EfiSystemTable>()
            );
            return;
        };

        // Safety: The region is explicitly mapped and sized for EfiSystemTable.
        // We still do a minimal sanity check before reading further.
        let st = unsafe { &*(mapped.as_ptr() as *const EfiSystemTable) };
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
            let vendor_phys = limine::try_as_phys_addr(vendor_addr)?;
            let bytes = 2usize
                .checked_mul(96usize.checked_add(1)?)
                .unwrap_or(0);
            if bytes == 0 {
                return None;
            }
            let mapped = mmio::map_mmio_region_exact(vendor_phys, bytes).ok()?;
            let vendor_ptr = mapped.as_ptr() as *const u16;
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
