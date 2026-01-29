use alloc::vec;
use alloc::vec::Vec;

use crate::disc::block::{self, DeviceHandle};

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: alloc::alloc::Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = alloc::alloc::Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self { ptr, len, layout })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { alloc::alloc::dealloc(self.ptr, self.layout) };
        }
    }
}

fn write_blocks_aligned_with_log(
    handle: DeviceHandle,
    lba: u64,
    buf: &[u8],
    log: &mut dyn FnMut(&str),
) -> Result<(), block::Error> {
    let info = handle.info();
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(buf.len(), align).ok_or(block::Error::DmaUnavailable)?;
    tmp.as_mut_slice().copy_from_slice(buf);
    match handle.write_blocks(lba, tmp.as_mut_slice()) {
        Ok(()) => Ok(()),
        Err(e) => {
            log(
                alloc::format!(
                    "install: fat32: write failed lba={} bytes={} err={:?}",
                    lba,
                    buf.len(),
                    e
                )
                .as_str(),
            );
            Err(e)
        }
    }
}

fn name83(base: &str, ext: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    for (i, b) in base.as_bytes().iter().take(8).enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    for (i, b) in ext.as_bytes().iter().take(3).enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    out
}

fn dir_entry(name: [u8; 11], attr: u8, first_cluster: u32, size: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0..11].copy_from_slice(&name);
    e[11] = attr;

    // cluster high/low
    let hi = ((first_cluster >> 16) as u16).to_le_bytes();
    let lo = ((first_cluster & 0xFFFF) as u16).to_le_bytes();
    e[20..22].copy_from_slice(&hi);
    e[26..28].copy_from_slice(&lo);

    e[28..32].copy_from_slice(&size.to_le_bytes());
    e
}

fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    // Standard VFAT checksum over 8.3 name.
    let mut sum: u8 = 0;
    for &b in short_name.iter() {
        sum = (((sum & 1) << 7) | (sum >> 1)).wrapping_add(b);
    }
    sum
}

fn lfn_entry(sequence: u8, checksum: u8, chars: &[u16; 13]) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0] = sequence;
    e[11] = 0x0F; // LFN attribute
    e[12] = 0x00; // type
    e[13] = checksum;
    // e[26..28] first cluster low = 0

    let put = |dst: &mut [u8], src: &[u16]| {
        for (i, w) in src.iter().enumerate() {
            let b = w.to_le_bytes();
            dst[i * 2] = b[0];
            dst[i * 2 + 1] = b[1];
        }
    };

    put(&mut e[1..1 + 10], &chars[0..5]);
    put(&mut e[14..14 + 12], &chars[5..11]);
    put(&mut e[28..28 + 4], &chars[11..13]);
    e
}

fn lfn_entries_for_ascii_name(long_name: &str, short_name: &[u8; 11]) -> Vec<[u8; 32]> {
    // Directory order is: last LFN entry first (with 0x40 flag), down to 1, then the short entry.
    let checksum = lfn_checksum(short_name);
    let mut utf16: Vec<u16> = long_name.as_bytes().iter().map(|&b| b as u16).collect();
    utf16.push(0u16); // NUL terminator

    let total = utf16.len();
    let entries = (total + 12) / 13;

    let mut out = Vec::new();
    for i in (1..=entries).rev() {
        let start = (i - 1) * 13;
        let end = core::cmp::min(start + 13, total);
        let mut chunk = [0xFFFFu16; 13];
        let slice = &utf16[start..end];
        for (j, &w) in slice.iter().enumerate() {
            chunk[j] = w;
        }

        let mut seq = i as u8;
        if i == entries {
            seq |= 0x40;
        }
        out.push(lfn_entry(seq, checksum, &chunk));
    }
    out
}

fn clusters_for_bytes(bytes: usize, sectors_per_cluster: u32) -> u32 {
    let bytes_per_cluster = (sectors_per_cluster as usize).saturating_mul(512);
    if bytes_per_cluster == 0 {
        return 0;
    }
    if bytes == 0 {
        return 0;
    }
    let c = (bytes + bytes_per_cluster - 1) / bytes_per_cluster;
    core::cmp::max(1, c as u32)
}

fn compute_fat32_layout(total_sectors: u32, sectors_per_cluster: u32) -> Option<(u16, u32, u32)> {
    // Returns (reserved_sectors, fat_sectors, first_data_sector)
    // Keep reserved fixed at 32.
    let reserved: u16 = 32;
    let fats: u32 = 2;

    if total_sectors < 10_000 {
        return None;
    }

    let mut fat_sectors: u32 = 1;
    for _ in 0..16 {
        let first_data_sector = (reserved as u32).saturating_add(fats.saturating_mul(fat_sectors));
        if first_data_sector >= total_sectors {
            return None;
        }
        let data_sectors = total_sectors - first_data_sector;
        let clusters = data_sectors / sectors_per_cluster;
        if clusters < 65_536 {
            // too small for FAT32
            return None;
        }

        let fat_bytes_needed = (clusters as u64 + 2).saturating_mul(4);
        let fat_sectors_needed = ((fat_bytes_needed + 511) / 512) as u32;
        if fat_sectors_needed == fat_sectors {
            return Some((reserved, fat_sectors, first_data_sector));
        }
        fat_sectors = fat_sectors_needed;
    }

    None
}

fn pick_fat32_geometry(
    total_sectors: u32,
    bootx64_len: usize,
    kernel_len: usize,
    bios_sys_len: usize,
    payload_len: usize,
    limine_conf_len: usize,
) -> Option<(u32, u16, u32, u32)> {
    // Return (sectors_per_cluster, reserved_sectors, fat_sectors, first_data_sector)
    // Try small clusters first so we can support smaller ESPs while still being FAT32.
    const CANDIDATES: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

    for spc in CANDIDATES {
        let Some((reserved, fat_sectors, first_data_sector)) = compute_fat32_layout(total_sectors, spc) else {
            continue;
        };

        if first_data_sector >= total_sectors {
            continue;
        }
        let data_sectors = total_sectors - first_data_sector;
        let total_clusters = data_sectors / spc;
        if total_clusters < 65_536 {
            continue;
        }

        // Directory clusters: root + EFI + BOOT + install.
        let dir_clusters: u32 = 4;
        let need_bootx64 = clusters_for_bytes(bootx64_len, spc);
        let need_kernel = clusters_for_bytes(kernel_len, spc);
        let need_bios = clusters_for_bytes(bios_sys_len, spc);
        let need_payload = clusters_for_bytes(payload_len, spc);
        let need_conf = clusters_for_bytes(limine_conf_len, spc);
        let slack: u32 = 32;

        // Cluster numbers start at 2; usable cluster count is (total_clusters).
        // We use a simple sequential allocation scheme.
        let needed = dir_clusters
            .saturating_add(need_bootx64)
            .saturating_add(need_kernel)
            .saturating_add(need_bios)
            .saturating_add(need_conf)
            .saturating_add(need_payload)
            .saturating_add(slack);

        if needed < total_clusters {
            return Some((spc, reserved, fat_sectors, first_data_sector));
        }
    }

    None
}

pub struct EspImage<'a> {
    pub bootx64_efi: &'a [u8],
    pub kernel_elf: &'a [u8],
    pub limine_bios_sys: Option<&'a [u8]>,
    pub payload_iso: Option<&'a [u8]>,
    pub limine_conf: &'a [u8],
}

/// Format the given partition as a minimal FAT32 ESP and write:
/// - /EFI/BOOT/BOOTX64.EFI
/// - /TRUEOS.ELF
/// - /limine.conf
/// - /limine-bios.sys (optional, but required for BIOS boot)
/// - /install/PAYLOAD.ISO (optional)
///
/// This intentionally does NOT use the `fatfs` crate; it writes the on-disk structures directly.
pub fn format_and_populate_esp_fat32(
    esp: DeviceHandle,
    image: EspImage<'_>,
) -> Result<(), block::Error> {
    format_and_populate_esp_fat32_with_log(esp, image, &mut |_| {})
}

pub fn format_and_populate_esp_fat32_with_log(
    esp: DeviceHandle,
    image: EspImage<'_>,
    log: &mut dyn FnMut(&str),
) -> Result<(), block::Error> {
    if esp.parent().is_none() {
        // Must be a partition device, not the whole disk.
        return Err(block::Error::InvalidParam);
    }
    if !esp.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let info = esp.info();
    if info.block_size != 512 {
        return Err(block::Error::NotSupported);
    }

    let total_sectors = core::cmp::min(info.block_count, u32::MAX as u64) as u32;

    let bios_sys_len = image.limine_bios_sys.map_or(0, |p| p.len());
    let payload_len = image.payload_iso.map_or(0, |p| p.len());
    let limine_conf_len = image.limine_conf.len();
    let Some((sectors_per_cluster, reserved, fat_sectors, first_data_sector)) = pick_fat32_geometry(
        total_sectors,
        image.bootx64_efi.len(),
        image.kernel_elf.len(),
        bios_sys_len,
        payload_len,
        limine_conf_len,
    ) else {
        return Err(block::Error::OutOfBounds);
    };

    let bytes_per_cluster = (sectors_per_cluster as usize) * 512;

    // Fixed directory cluster assignments.
    let cl_root: u32 = 2;
    let cl_efi: u32 = 3;
    let cl_boot: u32 = 4;
    let cl_install: u32 = 5;

    let mut next_cluster: u32 = 6;

    let bootx64_clusters = clusters_for_bytes(image.bootx64_efi.len(), sectors_per_cluster);
    let bootx64_start = next_cluster;
    next_cluster = next_cluster.saturating_add(bootx64_clusters);

    let kernel_clusters = clusters_for_bytes(image.kernel_elf.len(), sectors_per_cluster);
    let kernel_start = next_cluster;
    next_cluster = next_cluster.saturating_add(kernel_clusters);

    let conf_clusters = clusters_for_bytes(image.limine_conf.len(), sectors_per_cluster);
    let conf_start = next_cluster;
    next_cluster = next_cluster.saturating_add(conf_clusters);

    let bios_sys_clusters = clusters_for_bytes(bios_sys_len, sectors_per_cluster);
    let bios_sys_start = next_cluster;
    next_cluster = next_cluster.saturating_add(bios_sys_clusters);

    let payload_clusters = clusters_for_bytes(payload_len, sectors_per_cluster);
    let payload_start = next_cluster;
    next_cluster = next_cluster.saturating_add(payload_clusters);

    // Cluster count check against data area.
    let data_sectors = total_sectors - first_data_sector;
    let total_clusters = data_sectors / sectors_per_cluster;
    if (next_cluster as u64) >= (total_clusters as u64 + 2) {
        return Err(block::Error::OutOfBounds);
    }

    // Build FAT tables.
    let fat_bytes = (fat_sectors as usize) * 512;
    let mut fat = vec![0u8; fat_bytes];

    let mut set_fat = |cluster: u32, val: u32| {
        let off = (cluster as usize) * 4;
        if off + 4 <= fat.len() {
            fat[off..off + 4].copy_from_slice(&val.to_le_bytes());
        }
    };

    // Reserved entries.
    set_fat(0, 0x0FFFFFF8);
    set_fat(1, 0x0FFFFFFF);

    // Directories (single cluster, EOC).
    for &c in &[cl_root, cl_efi, cl_boot, cl_install] {
        set_fat(c, 0x0FFFFFFF);
    }

    // Helper to chain file clusters.
    let mut chain = |start: u32, count: u32| {
        if count == 0 {
            return;
        }
        for i in 0..count {
            let c = start + i;
            let next = if i + 1 == count { 0x0FFFFFFF } else { c + 1 };
            set_fat(c, next);
        }
    };

    chain(bootx64_start, bootx64_clusters);
    chain(kernel_start, kernel_clusters);
    chain(conf_start, conf_clusters);
    chain(bios_sys_start, bios_sys_clusters);
    chain(payload_start, payload_clusters);

    // --- Write reserved region: boot sector, FSInfo, backup ---
    let mut boot = [0u8; 512];
    boot[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    boot[3..11].copy_from_slice(b"MSWIN4.1");
    boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot[13] = sectors_per_cluster as u8;
    boot[14..16].copy_from_slice(&reserved.to_le_bytes());
    boot[16] = 2;
    boot[17..19].copy_from_slice(&0u16.to_le_bytes());
    boot[19..21].copy_from_slice(&0u16.to_le_bytes());
    boot[21] = 0xF8;
    boot[22..24].copy_from_slice(&0u16.to_le_bytes());
    boot[24..26].copy_from_slice(&63u16.to_le_bytes());
    boot[26..28].copy_from_slice(&255u16.to_le_bytes());
    boot[28..32].copy_from_slice(&0u32.to_le_bytes());
    boot[32..36].copy_from_slice(&total_sectors.to_le_bytes());
    boot[36..40].copy_from_slice(&fat_sectors.to_le_bytes());
    boot[40..42].copy_from_slice(&0u16.to_le_bytes());
    boot[42..44].copy_from_slice(&0u16.to_le_bytes());
    boot[44..48].copy_from_slice(&cl_root.to_le_bytes());
    boot[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo sector
    boot[50..52].copy_from_slice(&6u16.to_le_bytes()); // backup boot sector
    // boot[52..64] reserved zeros

    boot[64] = 0x80;
    boot[66] = 0x29;
    let mut vol_id = [0u8; 4];
    if !crate::rng::fill_bytes(&mut vol_id) {
        vol_id = 0x12345678u32.to_le_bytes();
    }
    boot[67..71].copy_from_slice(&vol_id);
    boot[71..82].copy_from_slice(b"TRUEOS ESP ");
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55;
    boot[511] = 0xAA;

    let mut fsinfo = [0u8; 512];
    fsinfo[0..4].copy_from_slice(&0x41615252u32.to_le_bytes());
    fsinfo[484..488].copy_from_slice(&0x61417272u32.to_le_bytes());
    fsinfo[488..492].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    fsinfo[492..496].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    fsinfo[508..512].copy_from_slice(&0xAA550000u32.to_le_bytes());

    write_blocks_aligned_with_log(esp, 0, &boot, log)?;
    write_blocks_aligned_with_log(esp, 1, &fsinfo, log)?;
    write_blocks_aligned_with_log(esp, 6, &boot, log)?;
    write_blocks_aligned_with_log(esp, 7, &fsinfo, log)?;

    // Zero remaining reserved sectors.
    let mut zero = [0u8; 512];
    zero.fill(0);
    for lba in 2u64..(reserved as u64) {
        if lba == 6 || lba == 7 {
            continue;
        }
        write_blocks_aligned_with_log(esp, lba, &zero, log)?;
    }

    // --- Write FATs ---
    let fat1_lba = reserved as u64;
    for i in 0..fat_sectors as u64 {
        let start = (i as usize) * 512;
        let end = start + 512;
        write_blocks_aligned_with_log(esp, fat1_lba + i, &fat[start..end], log)?;
    }
    let fat2_lba = fat1_lba + fat_sectors as u64;
    for i in 0..fat_sectors as u64 {
        let start = (i as usize) * 512;
        let end = start + 512;
        write_blocks_aligned_with_log(esp, fat2_lba + i, &fat[start..end], log)?;
    }

    // Helper to write a whole cluster.
    let mut cluster_buf = vec![0u8; bytes_per_cluster];
    let mut write_cluster = |cluster: u32, data: &[u8]| -> Result<(), block::Error> {
        cluster_buf.fill(0);
        let take = core::cmp::min(cluster_buf.len(), data.len());
        cluster_buf[..take].copy_from_slice(&data[..take]);
        let first_sector = (cluster - 2) as u64 * (sectors_per_cluster as u64) + (first_data_sector as u64);
        for s in 0..(sectors_per_cluster as u64) {
            let off = (s as usize) * 512;
            write_blocks_aligned_with_log(esp, first_sector + s, &cluster_buf[off..off + 512], log)?;
        }
        Ok(())
    };

    // --- Directories ---
    // Root dir
    {
        let mut dir = [0u8; 512];
        let mut off = 0;

        let mut push = |e: &[u8; 32]| {
            if off + 32 <= dir.len() {
                dir[off..off + 32].copy_from_slice(e);
                off += 32;
            }
        };

        push(&dir_entry(name83("EFI", ""), 0x10, cl_efi, 0));
        push(&dir_entry(name83("INSTALL", ""), 0x10, cl_install, 0));
        push(&dir_entry(
            name83("TRUEOS", "ELF"),
            0x20,
            kernel_start,
            image.kernel_elf.len() as u32,
        ));
        push(&dir_entry(
            name83("LIMINE", "CONF"),
            0x20,
            conf_start,
            image.limine_conf.len() as u32,
        ));

        if let Some(bios_sys) = image.limine_bios_sys {
            if !bios_sys.is_empty() {
                // Limine expects this filename; use VFAT LFN entries with a deterministic 8.3 alias.
                let short = name83("LIMINEBI", "SYS");
                for lfn in lfn_entries_for_ascii_name("limine-bios.sys", &short) {
                    push(&lfn);
                }
                push(&dir_entry(short, 0x20, bios_sys_start, bios_sys.len() as u32));
            }
        }

        write_cluster(cl_root, &dir)?;
    }

    // EFI dir
    {
        let mut dir = [0u8; 512];
        let mut off = 0;
        for e in [
            dir_entry(name83(".", ""), 0x10, cl_efi, 0),
            dir_entry(name83("..", ""), 0x10, cl_root, 0),
            dir_entry(name83("BOOT", ""), 0x10, cl_boot, 0),
        ] {
            dir[off..off + 32].copy_from_slice(&e);
            off += 32;
        }
        write_cluster(cl_efi, &dir)?;
    }

    // BOOT dir
    {
        let mut dir = [0u8; 512];
        let mut off = 0;
        for e in [
            dir_entry(name83(".", ""), 0x10, cl_boot, 0),
            dir_entry(name83("..", ""), 0x10, cl_efi, 0),
            dir_entry(
                name83("BOOTX64", "EFI"),
                0x20,
                bootx64_start,
                image.bootx64_efi.len() as u32,
            ),
        ] {
            dir[off..off + 32].copy_from_slice(&e);
            off += 32;
        }
        write_cluster(cl_boot, &dir)?;
    }

    // INSTALL dir
    {
        let mut dir = [0u8; 512];
        let mut off = 0;
        for e in [
            dir_entry(name83(".", ""), 0x10, cl_install, 0),
            dir_entry(name83("..", ""), 0x10, cl_root, 0),
        ] {
            dir[off..off + 32].copy_from_slice(&e);
            off += 32;
        }

        if let Some(payload) = image.payload_iso {
            if !payload.is_empty() {
                let e = dir_entry(name83("PAYLOAD", "ISO"), 0x20, payload_start, payload.len() as u32);
                dir[off..off + 32].copy_from_slice(&e);
            }
        }
        write_cluster(cl_install, &dir)?;
    }

    // --- File data ---
    let mut write_file = |start_cluster: u32, clusters: u32, data: &[u8]| -> Result<(), block::Error> {
        let mut remaining = data;
        for i in 0..clusters {
            let c = start_cluster + i;
            let take = core::cmp::min(bytes_per_cluster, remaining.len());
            write_cluster(c, &remaining[..take])?;
            remaining = &remaining[take..];

            // Formatting/writing the ESP can take a while; keep the shell responsive.
            crate::time::poll_executor();
        }
        Ok(())
    };

    write_file(bootx64_start, bootx64_clusters, image.bootx64_efi)?;
    write_file(kernel_start, kernel_clusters, image.kernel_elf)?;
    write_file(conf_start, conf_clusters, image.limine_conf)?;
    if let Some(bios_sys) = image.limine_bios_sys {
        if !bios_sys.is_empty() {
            write_file(bios_sys_start, bios_sys_clusters, bios_sys)?;
        }
    }
    if let Some(payload) = image.payload_iso {
        if !payload.is_empty() {
            write_file(payload_start, payload_clusters, payload)?;
        }
    }

    esp.flush()?;
    Ok(())
}
