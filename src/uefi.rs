use core::{char, ptr};

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

        let table_ptr = match to_virt_ptr::<EfiSystemTable>(phys_or_virt) {
            Some(p) => p,
            None => {
                crate::log!("UEFI: EFI system table at 0x{:016X} (no HHDM; not parsing)\n", phys_or_virt);
                return;
            }
        };

        // Safety: Limine says the system table exists iff response is present.
        // We still defensively do a minimal sanity check before reading further.
        let st = unsafe { &*table_ptr };
        if st.hdr.signature != EFI_SYSTEM_TABLE_SIGNATURE {
            crate::log!(
                "UEFI: EFI system table at 0x{:016X} signature mismatch 0x{:016X}\n",
                phys_or_virt,
                st.hdr.signature
            );
            return;
        }

        let vendor = unsafe { read_utf16z_lossy(st.firmware_vendor, 96) };
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

fn to_virt_ptr<T>(addr: u64) -> Option<*const T> {
    // Limine base rev >=3 may return a physical address.
    // If it "looks" like a low physical address, prefer HHDM+phys.
    let is_likely_phys = addr < 0x0000_8000_0000_0000;

    if is_likely_phys {
        let hhdm = limine::hhdm_offset()?;
        let virt = addr.wrapping_add(hhdm);
        Some(virt as *const T)
    } else {
        Some(addr as *const T)
    }
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
