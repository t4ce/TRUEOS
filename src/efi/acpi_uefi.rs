use spin::Once;

use crate::efi::acpi::ensure_tables;
use crate::efi::EfiGuid;
use crate::pci::mmio;

static LOG_ONCE: Once<()> = Once::new();

/// Logs the ACPI table with signature "UEFI" (if present).
///
/// Note: This is not the UEFI System Table. It's an ACPI table container that
/// typically embeds a GUID + payload.
pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let mut found = false;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() != "UEFI" {
                continue;
            }
            found = true;
            let len =
                unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            if let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) {
                let base = mapped.as_ptr();
                if len >= 60 {
                    let guid_bytes = unsafe { core::slice::from_raw_parts(base.add(36), 16) };
                    let guid = EfiGuid::from_uefi_bytes([
                        guid_bytes[0], guid_bytes[1], guid_bytes[2], guid_bytes[3],
                        guid_bytes[4], guid_bytes[5], guid_bytes[6], guid_bytes[7],
                        guid_bytes[8], guid_bytes[9], guid_bytes[10], guid_bytes[11],
                        guid_bytes[12], guid_bytes[13], guid_bytes[14], guid_bytes[15],
                    ]);
                    // ACPI "UEFI" table is a generic container:
                    // - 36-byte ACPI header
                    // - 16-byte GUID identifier
                    // - u16 data_offset
                    // - u16 data_length
                    // The data blob often begins with an address/pointer for the GUID payload.
                    let data_off =
                        unsafe { core::ptr::read_unaligned(base.add(52) as *const u16) } as usize;
                    let data_len =
                        unsafe { core::ptr::read_unaligned(base.add(54) as *const u16) } as usize;

                    let ptr_guess = if data_off != 0 && data_off + 8 <= len {
                        unsafe { core::ptr::read_unaligned(base.add(data_off) as *const u64) }
                    } else {
                        0
                    };
                    crate::log!(
                        "UEFI: ACPI table 'UEFI' phys=0x{:016X} len=0x{:X} guid={} data_off=0x{:X} data_len=0x{:X} ptr_guess=0x{:016X}\n",
                        phys as u64,
                        len,
                        guid.fmt_canonical(),
                        data_off as u64,
                        data_len as u64,
                        ptr_guess
                    );
                } else {
                    crate::log!("UEFI: length too small (0x{:X})\n", len);
                }
            } else {
                crate::log!("UEFI: map failed phys=0x{:X} len=0x{:X}\n", phys, len);
            }
        }

        if !found {
            crate::log!("UEFI: table not present\n");
        }
    });
}
