const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LONG_NAME: u8 = 0x0F;

pub(crate) fn bootx64_from_efi_img(img: &[u8]) -> Option<&[u8]> {
    let fs = Fat16View::new(img)?;
    let efi_cluster = fs.find_root_entry(b"EFI        ", ATTR_DIRECTORY)?.cluster;
    let boot_cluster = fs
        .find_dir_entry(efi_cluster, b"BOOT       ", ATTR_DIRECTORY)?
        .cluster;
    let bootx64 = fs.find_dir_entry(boot_cluster, b"BOOTX64 EFI", 0)?;
    fs.file_slice(bootx64.cluster, bootx64.size)
}

#[derive(Copy, Clone)]
struct Fat16View<'a> {
    img: &'a [u8],
    bytes_per_sector: usize,
    sectors_per_cluster: usize,
    root_entries: usize,
    fat_offset: usize,
    root_offset: usize,
    data_offset: usize,
}

#[derive(Copy, Clone)]
struct DirEntry {
    cluster: u16,
    size: usize,
}

impl<'a> Fat16View<'a> {
    fn new(img: &'a [u8]) -> Option<Self> {
        let bytes_per_sector = read_u16(img, 11)? as usize;
        let sectors_per_cluster = *img.get(13)? as usize;
        let reserved_sectors = read_u16(img, 14)? as usize;
        let fat_count = *img.get(16)? as usize;
        let root_entries = read_u16(img, 17)? as usize;
        let sectors_per_fat = read_u16(img, 22)? as usize;
        if bytes_per_sector == 0
            || sectors_per_cluster == 0
            || fat_count == 0
            || sectors_per_fat == 0
        {
            return None;
        }
        let fat_offset = reserved_sectors.checked_mul(bytes_per_sector)?;
        let root_offset = fat_offset.checked_add(
            fat_count
                .checked_mul(sectors_per_fat)?
                .checked_mul(bytes_per_sector)?,
        )?;
        let root_bytes = root_entries.checked_mul(32)?;
        let root_sectors = root_bytes.div_ceil(bytes_per_sector);
        let data_offset = root_offset.checked_add(root_sectors.checked_mul(bytes_per_sector)?)?;
        if img.get(fat_offset..fat_offset + 2).is_none()
            || img
                .get(root_offset..root_offset.checked_add(root_bytes)?)
                .is_none()
            || img.get(data_offset..).is_none()
        {
            return None;
        }
        Some(Self {
            img,
            bytes_per_sector,
            sectors_per_cluster,
            root_entries,
            fat_offset,
            root_offset,
            data_offset,
        })
    }

    fn find_root_entry(&self, name: &[u8; 11], required_attr: u8) -> Option<DirEntry> {
        self.find_entry_in_table(
            self.root_offset,
            self.root_entries.checked_mul(32)?,
            name,
            required_attr,
        )
    }

    fn find_dir_entry(
        &self,
        dir_cluster: u16,
        name: &[u8; 11],
        required_attr: u8,
    ) -> Option<DirEntry> {
        let mut cluster = dir_cluster;
        let cluster_bytes = self.cluster_bytes()?;
        for _ in 0..256 {
            let off = self.cluster_offset(cluster)?;
            if let Some(entry) = self.find_entry_in_table(off, cluster_bytes, name, required_attr) {
                return Some(entry);
            }
            cluster = self.next_cluster(cluster)?;
        }
        None
    }

    fn find_entry_in_table(
        &self,
        offset: usize,
        bytes: usize,
        name: &[u8; 11],
        required_attr: u8,
    ) -> Option<DirEntry> {
        let table = self.img.get(offset..offset.checked_add(bytes)?)?;
        for entry in table.chunks_exact(32) {
            if entry[0] == 0x00 {
                break;
            }
            if entry[0] == 0xE5 || (entry[11] & ATTR_LONG_NAME) == ATTR_LONG_NAME {
                continue;
            }
            if entry.get(0..11)? != name {
                continue;
            }
            if required_attr != 0 && (entry[11] & required_attr) == 0 {
                continue;
            }
            return Some(DirEntry {
                cluster: read_u16(entry, 26)?,
                size: read_u32(entry, 28)? as usize,
            });
        }
        None
    }

    fn file_slice(&self, cluster: u16, size: usize) -> Option<&'a [u8]> {
        let off = self.cluster_offset(cluster)?;
        self.img.get(off..off.checked_add(size)?)
    }

    fn next_cluster(&self, cluster: u16) -> Option<u16> {
        let off = self.fat_offset.checked_add(cluster as usize * 2)?;
        let next = read_u16(self.img, off)?;
        if next >= 0xFFF8 { None } else { Some(next) }
    }

    fn cluster_offset(&self, cluster: u16) -> Option<usize> {
        if cluster < 2 {
            return None;
        }
        self.data_offset.checked_add(
            (cluster as usize - 2)
                .checked_mul(self.sectors_per_cluster)?
                .checked_mul(self.bytes_per_sector)?,
        )
    }

    fn cluster_bytes(&self) -> Option<usize> {
        self.sectors_per_cluster.checked_mul(self.bytes_per_sector)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}
