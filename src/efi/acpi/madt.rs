use spin::Once;

use crate::pci::mmio;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

fn read_u8(base: *const u8, off: usize) -> u8 {
    unsafe { core::ptr::read_unaligned(base.add(off) as *const u8) }
}

fn read_u16(base: *const u8, off: usize) -> u16 {
    unsafe { core::ptr::read_unaligned(base.add(off) as *const u16) }
}

fn read_u32(base: *const u8, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(base.add(off) as *const u32) }
}

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let mut found = false;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() != "APIC" {
                continue;
            }
            found = true;
            let len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            if len < 44 {
                crate::log!("MADT: length too small (0x{:X})\n", len);
                continue;
            }

            let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) else {
                crate::log!("MADT: map failed phys=0x{:X} len=0x{:X}\n", phys, len);
                continue;
            };

            let base = mapped.as_ptr();
            let lapic_addr = read_u32(base, 36);
            let flags = read_u32(base, 40);
            crate::log!(
                "MADT: phys=0x{:X} len=0x{:X} lapic_addr=0x{:08X} flags=0x{:08X}\n",
                phys,
                len,
                lapic_addr,
                flags
            );

            let mut off = 44usize;
            let mut idx = 0usize;

            // Counts for quick sanity.
            let mut n_lapic = 0usize;
            let mut n_ioapic = 0usize;
            let mut n_iso = 0usize;
            let mut n_lapic_nmi = 0usize;
            let mut n_x2apic = 0usize;

            while off + 2 <= len {
                let t = read_u8(base, off) as u16;
                let l = read_u8(base, off + 1) as usize;
                if l < 2 || off + l > len {
                    break;
                }

                match t {
                    0 => {
                        // Processor Local APIC
                        if l >= 8 {
                            let acpi_proc_id = read_u8(base, off + 2);
                            let apic_id = read_u8(base, off + 3);
                            let lapic_flags = read_u32(base, off + 4);
                            crate::log!(
                                "MADT: [{:02}] LAPIC proc_id={} apic_id={} flags=0x{:08X}\n",
                                idx,
                                acpi_proc_id,
                                apic_id,
                                lapic_flags
                            );
                        }
                        n_lapic += 1;
                    }
                    1 => {
                        // I/O APIC
                        if l >= 12 {
                            let ioapic_id = read_u8(base, off + 2);
                            let ioapic_addr = read_u32(base, off + 4);
                            let gsi_base = read_u32(base, off + 8);
                            crate::log!(
                                "MADT: [{:02}] IOAPIC id={} addr=0x{:08X} gsi_base={}\n",
                                idx,
                                ioapic_id,
                                ioapic_addr,
                                gsi_base
                            );
                        }
                        n_ioapic += 1;
                    }
                    2 => {
                        // Interrupt Source Override
                        if l >= 10 {
                            let bus = read_u8(base, off + 2);
                            let source = read_u8(base, off + 3);
                            let gsi = read_u32(base, off + 4);
                            let iso_flags = read_u16(base, off + 8);
                            crate::log!(
                                "MADT: [{:02}] ISO bus={} source={} gsi={} flags=0x{:04X}\n",
                                idx,
                                bus,
                                source,
                                gsi,
                                iso_flags
                            );
                        }
                        n_iso += 1;
                    }
                    4 => {
                        // NMI Source
                        if l >= 8 {
                            let nmi_flags = read_u16(base, off + 2);
                            let gsi = read_u32(base, off + 4);
                            crate::log!(
                                "MADT: [{:02}] NMI_SOURCE gsi={} flags=0x{:04X}\n",
                                idx,
                                gsi,
                                nmi_flags
                            );
                        }
                    }
                    5 => {
                        // Local APIC NMI
                        if l >= 6 {
                            let acpi_proc_id = read_u8(base, off + 2);
                            let nmi_flags = read_u16(base, off + 3);
                            let lint = read_u8(base, off + 5);
                            crate::log!(
                                "MADT: [{:02}] LAPIC_NMI proc_id={} lint={} flags=0x{:04X}\n",
                                idx,
                                acpi_proc_id,
                                lint,
                                nmi_flags
                            );
                        }
                        n_lapic_nmi += 1;
                    }
                    9 => {
                        // Processor Local x2APIC
                        if l >= 16 {
                            let x2apic_id = read_u32(base, off + 4);
                            let x2apic_flags = read_u32(base, off + 8);
                            let acpi_uid = read_u32(base, off + 12);
                            crate::log!(
                                "MADT: [{:02}] X2APIC uid={} x2apic_id={} flags=0x{:08X}\n",
                                idx,
                                acpi_uid,
                                x2apic_id,
                                x2apic_flags
                            );
                        }
                        n_x2apic += 1;
                    }
                    10 => {
                        // x2APIC NMI
                        if l >= 12 {
                            let nmi_flags = read_u16(base, off + 2);
                            let acpi_uid = read_u32(base, off + 4);
                            let lint = read_u8(base, off + 8);
                            crate::log!(
                                "MADT: [{:02}] X2APIC_NMI uid={} lint={} flags=0x{:04X}\n",
                                idx,
                                acpi_uid,
                                lint,
                                nmi_flags
                            );
                        }
                    }
                    11 => {
                        // GIC CPU interface (ARM) - log basic length/type only
                        crate::log!("MADT: [{:02}] type=11 (GICC) len=0x{:X}\n", idx, l);
                    }
                    12 => {
                        // GIC Distributor (ARM)
                        crate::log!("MADT: [{:02}] type=12 (GICD) len=0x{:X}\n", idx, l);
                    }
                    _ => {
                        crate::log!("MADT: [{:02}] type={} len=0x{:X}\n", idx, t, l);
                    }
                }

                idx += 1;
                off += l;
            }

            crate::log!(
                "MADT: entries={} lapic={} x2apic={} ioapic={} iso={} lapic_nmi={}\n",
                idx,
                n_lapic,
                n_x2apic,
                n_ioapic,
                n_iso,
                n_lapic_nmi
            );
        }

        if !found {
            crate::log!("MADT: table not present\n");
        }
    });
}
