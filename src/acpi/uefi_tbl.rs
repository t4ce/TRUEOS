use acpi::sdt::SdtHeader;
use spin::Once;

use crate::pci::mmio;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

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
                    let guid = unsafe { core::slice::from_raw_parts(base.add(36), 16) };
                    let cfg_ptr = unsafe { core::ptr::read_unaligned(base.add(52) as *const u64) };
                    crate::log!(
                        "UEFI: vendor_guid={:02X?} cfg_ptr=0x{:016X} len=0x{:X}\n",
                        guid,
                        cfg_ptr,
                        len
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
