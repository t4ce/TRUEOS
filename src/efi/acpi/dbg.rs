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

fn read_u64(base: *const u8, off: usize) -> u64 {
    unsafe { core::ptr::read_unaligned(base.add(off) as *const u64) }
}

fn log_dbgp(phys: usize, len: usize, base: *const u8) {
    // DBGP: header (36) + interface_type (1) + reserved (3) + GAS (12) => total 52 (0x34)
    if len < 52 {
        crate::log!("DBGP: len too small (0x{:X})\n", len);
        return;
    }

    let iface = read_u8(base, 36);
    let gas_off = 40usize;
    let addr_space = read_u8(base, gas_off + 0);
    let bit_width = read_u8(base, gas_off + 1);
    let bit_off = read_u8(base, gas_off + 2);
    let access_size = read_u8(base, gas_off + 3);
    let address = read_u64(base, gas_off + 4);

    crate::log!(
        "DBGP: phys=0x{:X} len=0x{:X} iface_type=0x{:02X} GAS(space=0x{:02X} width={} off={} acc={} addr=0x{:016X})\n",
        phys,
        len,
        iface,
        addr_space,
        bit_width,
        bit_off,
        access_size,
        address
    );
}

fn log_dbg2(phys: usize, len: usize, base: *const u8) {
    // DBG2: header (36) + u32 info_offset + u32 info_count ...
    if len < 44 {
        crate::log!("DBG2: len too small (0x{:X})\n", len);
        return;
    }

    let info_offset = read_u32(base, 36) as usize;
    let info_count = read_u32(base, 40) as usize;
    crate::log!(
        "DBG2: phys=0x{:X} len=0x{:X} info_offset=0x{:X} info_count={}\n",
        phys,
        len,
        info_offset,
        info_count
    );

    if info_offset >= len {
        return;
    }

    // Each Debug Device Information structure begins at info_offset and has a u16 length at +2.
    // We walk `info_count` records, logging the port type/subtype and base address info if present.
    let mut off = info_offset;
    for idx in 0..info_count {
        if off + 4 > len {
            break;
        }

        let rev = read_u8(base, off + 0);
        let rec_len = read_u16(base, off + 2) as usize;
        if rec_len < 4 || off + rec_len > len {
            break;
        }

        // Layout fields we care about (ACPI DBG2 spec):
        // +0  u8  revision
        // +1  u8  reserved
        // +2  u16 length
        // +4  u8  num_gas
        // +5  u8  reserved
        // +6  u16 namepath_offset
        // +8  u16 namepath_length
        // +10 u16 oem_data_offset
        // +12 u16 oem_data_length
        // +14 u16 port_type
        // +16 u16 port_subtype
        // +18 u16 reserved
        // +20 u16 base_address_offset
        // +22 u16 address_size_offset
        let num_gas = if rec_len >= 5 { read_u8(base, off + 4) } else { 0 };
        let port_type = if rec_len >= 16 { read_u16(base, off + 14) } else { 0 };
        let port_subtype = if rec_len >= 18 { read_u16(base, off + 16) } else { 0 };
        let base_addr_off = if rec_len >= 22 {
            read_u16(base, off + 20) as usize
        } else {
            0
        };

        crate::log!(
            "DBG2: [{}] rev={} len=0x{:X} num_gas={} port_type=0x{:04X} subtype=0x{:04X}\n",
            idx,
            rev,
            rec_len,
            num_gas,
            port_type,
            port_subtype
        );

        // If base address offset looks sane, dump first GAS and its address.
        if num_gas > 0 && base_addr_off != 0 {
            let gas_abs = off.saturating_add(base_addr_off);
            if gas_abs + 12 <= off + rec_len {
                let addr_space = read_u8(base, gas_abs + 0);
                let bit_width = read_u8(base, gas_abs + 1);
                let bit_off = read_u8(base, gas_abs + 2);
                let access_size = read_u8(base, gas_abs + 3);
                let address = read_u64(base, gas_abs + 4);
                crate::log!(
                    "DBG2:     GAS0(space=0x{:02X} width={} off={} acc={} addr=0x{:016X})\n",
                    addr_space,
                    bit_width,
                    bit_off,
                    access_size,
                    address
                );
            }
        }

        off += rec_len;
    }
}

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let mut found_dbg2 = false;
        let mut found_dbgp = false;

        for (phys, hdr) in tables.table_headers() {
            let sig = hdr.signature.as_str();
            if sig != "DBG2" && sig != "DBGP" {
                continue;
            }

            let len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) else {
                crate::log!("{}: map failed phys=0x{:X} len=0x{:X}\n", sig, phys, len);
                continue;
            };

            let base = mapped.as_ptr();
            match sig {
                "DBGP" => {
                    found_dbgp = true;
                    log_dbgp(phys, len, base);
                }
                "DBG2" => {
                    found_dbg2 = true;
                    log_dbg2(phys, len, base);
                }
                _ => {}
            }
        }

        if !found_dbgp {
            crate::log!("DBGP: table not present\n");
        }
        if !found_dbg2 {
            crate::log!("DBG2: table not present\n");
        }
    });
}
