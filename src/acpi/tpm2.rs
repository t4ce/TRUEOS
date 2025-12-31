use acpi::sdt::SdtHeader;
use spin::Once;

use crate::debugconf;
use crate::pci::mmio;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

#[repr(C, packed)]
struct Tpm2 {
    header: SdtHeader,
    platform_class: u16,
    reserved: u16,
    control_area: u64,
    start_method: u32,
    // spec-defined optional parameters follow; we ignore for now.
}

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let mut found = false;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() != "TPM2" {
                continue;
            }
            found = true;
            let len =
                unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            if let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) {
                if len >= core::mem::size_of::<Tpm2>() {
                    let ptr = mapped.as_ptr() as *const Tpm2;
                    let t = unsafe { core::ptr::read_unaligned(ptr) };
                    let platform_class = t.platform_class;
                    let control_area = t.control_area;
                    let start_method = t.start_method;
                    debugconf!(
                        "TPM2: class={} control_area=0x{:X} start_method={} len=0x{:X}\n",
                        platform_class,
                        control_area,
                        start_method,
                        len
                    );
                } else {
                    debugconf!("TPM2: length too small (0x{:X})\n", len);
                }
            } else {
                debugconf!("TPM2: map failed phys=0x{:X} len=0x{:X}\n", phys, len);
            }
        }

        if !found {
            debugconf!("TPM2: table not present\n");
        }
    });
}
