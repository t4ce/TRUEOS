use spin::Once;

use alloc::vec::Vec;
use heapless::String;

use crate::pci::mmio;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn ascii_trim<const N: usize>(bytes: &[u8]) -> String<N> {
    let mut tmp: String<N> = String::new();
    for &b in bytes {
        let ch = if (0x20..=0x7E).contains(&b) { b as char } else { '?' };
        if tmp.push(ch).is_err() {
            break;
        }
    }

    let trimmed = tmp.as_str().trim_end_matches(' ');
    let mut out: String<N> = String::new();
    let _ = out.push_str(trimmed);
    out
}

#[derive(Clone)]
struct SsdtInfo {
    phys: usize,
    len: usize,
    aml_hash: u32,
    checksum_ok: bool,
    revision: u8,
    oem_id: String<8>,
    oem_table_id: String<12>,
    oem_revision: u32,
    creator_id: String<8>,
    creator_revision: u32,
}

fn parse_ssdt(phys: usize, len: usize) -> Option<SsdtInfo> {
    if len < 36 {
        return None;
    }

    let Ok(mapped) = mmio::map_mmio_region_exact(phys as u64, len) else {
        return None;
    };

    let base = mapped.as_ptr();
    let bytes = unsafe { core::slice::from_raw_parts(base, len) };

    // ACPI SDT header layout:
    // 0..4 signature, 4..8 length, 8 revision, 9 checksum, 10..16 oem_id,
    // 16..24 oem_table_id, 24..28 oem_revision, 28..32 creator_id,
    // 32..36 creator_revision.
    let revision = bytes[8];
    let oem_id = ascii_trim::<8>(&bytes[10..16]);
    let oem_table_id = ascii_trim::<12>(&bytes[16..24]);
    let oem_revision = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let creator_id = ascii_trim::<8>(&bytes[28..32]);
    let creator_revision = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);

    let checksum_ok = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0;
    let aml_hash = fnv1a32(&bytes[36..]);

    Some(SsdtInfo {
        phys,
        len,
        aml_hash,
        checksum_ok,
        revision,
        oem_id,
        oem_table_id,
        oem_revision,
        creator_id,
        creator_revision,
    })
}

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let mut ssdts: Vec<SsdtInfo> = Vec::new();
        let mut idx = 0usize;
        for (phys, hdr) in tables.table_headers() {
            if hdr.signature.as_str() != "SSDT" {
                continue;
            }

            let len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hdr.length)) } as usize;
            let Some(info) = parse_ssdt(phys, len) else {
                crate::log!("SSDT: idx={} phys=0x{:X} len=0x{:X} (map/parse failed)\n", idx, phys, len);
                idx += 1;
                continue;
            };
            ssdts.push(info);
            idx += 1;
        }

        if ssdts.is_empty() {
            crate::log!("SSDT: none present\n");
            return;
        }

        // Sort by (hash,len,oem_table_id) so duplicates cluster.
        ssdts.sort_by(|a, b| {
            (a.aml_hash, a.len, a.oem_table_id.as_str()).cmp(&(b.aml_hash, b.len, b.oem_table_id.as_str()))
        });

        let total = ssdts.len();
        let mut unique = 0usize;
        let mut duplicate_instances = 0usize;

        let mut i = 0usize;
        while i < ssdts.len() {
            let key_hash = ssdts[i].aml_hash;
            let key_len = ssdts[i].len;
            let key_table = ssdts[i].oem_table_id.clone();

            let mut run = 1usize;
            while i + run < ssdts.len()
                && ssdts[i + run].aml_hash == key_hash
                && ssdts[i + run].len == key_len
                && ssdts[i + run].oem_table_id == key_table
            {
                run += 1;
            }

            unique += 1;
            if run > 1 {
                duplicate_instances += run - 1;
            }

            let first = &ssdts[i];
            crate::log!(
                "SSDT: hash=0x{:08X} len=0x{:X} count={} rev={} checksum_ok={} oem='{}' table='{}' oem_rev=0x{:X} creator='{}' creator_rev=0x{:X} phys0=0x{:X}\n",
                first.aml_hash,
                first.len,
                run,
                first.revision,
                first.checksum_ok,
                first.oem_id.as_str(),
                first.oem_table_id.as_str(),
                first.oem_revision,
                first.creator_id.as_str(),
                first.creator_revision,
                first.phys
            );

            i += run;
        }

        crate::log!(
            "SSDT: total={} unique={} dup_instances={}\n",
            total,
            unique,
            duplicate_instances
        );
    });
}
