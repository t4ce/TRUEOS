use spin::Once;

use crate::debugconf;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else { return; };

        let mut count = 0usize;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() == "SSDT" {
                count += 1;
                let len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) };
                debugconf!("SSDT: idx={} phys=0x{:X} len=0x{:X}\n", count - 1, phys, len);
            }
        }

        if count == 0 {
            debugconf!("SSDT: none present\n");
        }
    });
}
