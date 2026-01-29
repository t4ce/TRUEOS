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
            if hdr.signature.as_str() != "DMAR" {
                continue;
            }
            found = true;
            let len =
                unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            if let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) {
                let base = mapped.as_ptr();
                // DMAR header fields after SDT header (36 bytes): host address width, flags, reserved2 (2), reserved3 (4).
                if len >= 44 {
                    let host_aw = unsafe { core::ptr::read_unaligned(base.add(36) as *const u8) };
                    let flags = unsafe { core::ptr::read_unaligned(base.add(37) as *const u8) };
                    crate::log!(
                        "DMAR: host_addr_width={} flags=0x{:02X} len=0x{:X}\n",
                        host_aw & 0x3F,
                        flags,
                        len
                    );

                    // Count remapping structures by walking type/len records starting at offset 44.
                    let mut off = 44usize;
                    let mut count = 0usize;
                    while off + 4 <= len {
                        let _t = unsafe { core::ptr::read_unaligned(base.add(off) as *const u16) };
                        let l =
                            unsafe { core::ptr::read_unaligned(base.add(off + 2) as *const u16) }
                                as usize;
                        if l < 4 || off + l > len {
                            break;
                        }
                        count += 1;
                        off += l;
                    }
                    crate::log!("DMAR: remap_structs={}\n", count);
                } else {
                    crate::log!("DMAR: length too small (0x{:X})\n", len);
                }
            } else {
                crate::log!("DMAR: map failed phys=0x{:X} len=0x{:X}\n", phys, len);
            }
        }

        if !found {
            crate::log!("DMAR: table not present\n");
        }
    });
}
