use acpi::sdt::SdtHeader;
use spin::Once;

use crate::debugconf;
use crate::pci::mmio;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else { return; };

        let mut found = false;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() != "FPDT" {
                continue;
            }
            found = true;
            let len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            if let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) {
                let base = mapped.as_ptr();
                // FPDT contains a series of performance records. Log first record header if present.
                if len >= 44 {
                    let rec_type = unsafe { core::ptr::read_unaligned(base.add(36) as *const u16) };
                    let rec_len = unsafe { core::ptr::read_unaligned(base.add(38) as *const u16) } as usize;
                    debugconf!(
                        "FPDT: len=0x{:X} first_record_type=0x{:04X} first_record_len=0x{:X}\n",
                        len,
                        rec_type,
                        rec_len
                    );
                } else {
                    debugconf!("FPDT: length too small (0x{:X})\n", len);
                }
            } else {
                debugconf!("FPDT: map failed phys=0x{:X} len=0x{:X}\n", phys, len);
            }
        }

        if !found {
            debugconf!("FPDT: table not present\n");
        }
    });
}
