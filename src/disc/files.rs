use alloc::vec::Vec;

use fatfs::{
    format_volume, FileSystem, FormatVolumeOptions, FsOptions, IoBase, Read, Seek, SeekFrom, Write,
};

#[derive(Debug)]
struct RamDisk {
    data: Vec<u8>,
    pos: usize,
}

impl RamDisk {
    fn new(size: usize) -> Self {
        Self {
            data: alloc::vec![0_u8; size],
            pos: 0,
        }
    }
}

impl IoBase for RamDisk {
    type Error = ();
}

impl Read for RamDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let available = self.data.len() - self.pos;
        let to_copy = available.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

impl Write for RamDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Err(());
        }
        let available = self.data.len() - self.pos;
        let to_copy = available.min(buf.len());
        if to_copy == 0 {
            return Err(());
        }
        self.data[self.pos..self.pos + to_copy].copy_from_slice(&buf[..to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for RamDisk {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let len = self.data.len() as i64;
        let next = match pos {
            SeekFrom::Start(offset) => {
                if offset > self.data.len() as u64 {
                    return Err(());
                }
                offset as i64
            }
            SeekFrom::End(delta) => len.checked_add(delta).ok_or(())?,
            SeekFrom::Current(delta) => (self.pos as i64).checked_add(delta).ok_or(())?,
        };
        if next < 0 || next as usize > self.data.len() {
            return Err(());
        }
        self.pos = next as usize;
        Ok(self.pos as u64)
    }
}

pub fn create_demo_file() {
    crate::log!("fatfs demo: ");
    const RAMDISK_BYTES: usize = 1024 * 1024; // 1 MiB scratch volume
    let mut ramdisk = RamDisk::new(RAMDISK_BYTES);

    if let Err(_) = format_volume(&mut ramdisk, FormatVolumeOptions::new()) {
        return;
    }

    let fs = match FileSystem::new(ramdisk, FsOptions::new()) {
        Ok(fs) => fs,
        Err(_) => {
            return;
        }
    };

    let root = fs.root_dir();
    match root.create_file("helloworld") {
        Ok(mut file) => {
            if let Err(_) = file.write_all(b"hello from fatfs in-memory demo") {
                return;
            }
        }
        Err(_) => {
            return;
        }
    }

    if let Ok(stats) = fs.stats() {
        crate::log!(
            "fatfs demo: clusters total={} free={}",
            stats.total_clusters(),
            stats.free_clusters()
        );
    }
}
