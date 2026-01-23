use alloc::vec::Vec;
use alloc::alloc::{alloc, dealloc, Layout};

use fatfs::{
    format_volume, FileSystem, FormatVolumeOptions, FsOptions, IoBase, Read, Seek, SeekFrom, Write,
};

use crate::disc::block;
use embassy_time::{Duration as EmbassyDuration, Timer};

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

    if format_volume(&mut ramdisk, FormatVolumeOptions::new()).is_err() {
        return;
    }
    let Ok(fs) = FileSystem::new(ramdisk, FsOptions::new()) else {
        return;
    };

    let root = fs.root_dir();
    if let Ok(mut file) = root.create_file("helloworld") {
        let _ = file.write_all(b"hello from fatfs in-memory demo");
    }

    if let Ok(stats) = fs.stats() {
        crate::log!(
            "fatfs demo: clusters total={} free={}",
            stats.total_clusters(),
            stats.free_clusters()
        );
    }
}

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            ptr,
            len,
            layout,
        })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { dealloc(self.ptr, self.layout) };
        }
    }
}

struct BlockDeviceIo {
    dev: block::DeviceHandle,
    pos: u64,
    block_size: usize,
    scratch: AlignedBuf,
    read_count: u64,
    last_logged_lba: u64,
}

impl BlockDeviceIo {
    fn new(dev: block::DeviceHandle) -> Result<Self, block::Error> {
        let info = dev.info();
        let block_size = info.block_size as usize;
        if block_size == 0 {
            return Err(block::Error::InvalidParam);
        }
        let align = info.dma_alignment.max(1) as usize;
        let mut scratch = AlignedBuf::new(block_size, align).ok_or(block::Error::DmaUnavailable)?;
        for b in scratch.as_mut_slice().iter_mut() {
            *b = 0;
        }
        Ok(Self {
            dev,
            pos: 0,
            block_size,
            scratch,
            read_count: 0,
            last_logged_lba: u64::MAX,
        })
    }

    fn io_read_block(&mut self, lba: u64) -> Result<(), block::Error> {
        self.read_count = self.read_count.wrapping_add(1);
        // Keep this intentionally low-noise: FAT mount may read lots of LBAs.
        if self.read_count <= 8 {
            crate::log!("fatfs demo: io_read_block #{} lba={}\n", self.read_count, lba);
            self.last_logged_lba = lba;
        }
        self.dev.read_blocks(lba, self.scratch.as_mut_slice())
    }
}

impl IoBase for BlockDeviceIo {
    type Error = block::Error;
}

impl Read for BlockDeviceIo {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;

            self.io_read_block(lba)?;

            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);
            buf[total..total + want]
                .copy_from_slice(&self.scratch.as_mut_slice()[block_off..block_off + want]);

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }
}

impl Write for BlockDeviceIo {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.dev.supports_write() {
            return Err(block::Error::NotSupported);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;
            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);

            if want != self.block_size {
                self.io_read_block(lba)?;
            }

            self.scratch.as_mut_slice()[block_off..block_off + want]
                .copy_from_slice(&buf[total..total + want]);
            self.dev.write_blocks(lba, self.scratch.as_mut_slice())?;

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.dev.flush()
    }
}

impl Seek for BlockDeviceIo {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let cap_bytes = (self.dev.block_count() as u128) * (self.block_size as u128);
        let cap_i64 = core::cmp::min(cap_bytes, i64::MAX as u128) as i64;

        let next = match pos {
            SeekFrom::Start(off) => {
                if (off as u128) > cap_bytes {
                    return Err(block::Error::OutOfBounds);
                }
                off as i64
            }
            SeekFrom::End(delta) => cap_i64.checked_add(delta).ok_or(block::Error::OutOfBounds)?,
            SeekFrom::Current(delta) => (self.pos as i64)
                .checked_add(delta)
                .ok_or(block::Error::OutOfBounds)?,
        };

        if next < 0 {
            return Err(block::Error::OutOfBounds);
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

fn pick_usbms_device() -> Option<block::DeviceHandle> {
    for h in block::device_handles().into_iter() {
        let info = h.info();
        if info.label.as_deref() == Some("usbms") {
            return Some(h);
        }
    }
    None
}

fn run_fatfs_demo_on_device(handle: block::DeviceHandle) {
    let info = handle.info();
    crate::log!(
        "fatfs demo: using device id={} blocks={} block_size={}\n",
        info.id.raw(),
        info.block_count,
        info.block_size
    );
    
    // Prove the block path is functional before we do anything destructive.
    // A successful read of LBA0 (even if it returns zeros) demonstrates end-to-end BOT READ(10).
    let align = info.dma_alignment.max(1) as usize;
    if let Some(mut probe) = AlignedBuf::new(info.block_size as usize, align) {
        match handle.read_blocks(0, probe.as_mut_slice()) {
            Ok(()) => {
                let b = probe.as_mut_slice();
                crate::log!(
                    "fatfs demo: read lba0 ok bytes[0..16]=[{:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}]\n",
                    b[0],
                    b[1],
                    b[2],
                    b[3],
                    b[4],
                    b[5],
                    b[6],
                    b[7],
                    b[8],
                    b[9],
                    b[10],
                    b[11],
                    b[12],
                    b[13],
                    b[14],
                    b[15]
                );
            }
            Err(e) => {
                crate::log!("fatfs demo: read lba0 FAILED err={:?}\n", e);
            }
        }
    } else {
        crate::log!("fatfs demo: read lba0 SKIPPED (no aligned DMA buffer)\n");
    }

    let io = match BlockDeviceIo::new(handle) {
        Ok(io) => io,
        Err(e) => {
            crate::log!("fatfs demo: failed to init device io: {:?}\n", e);
            return;
        }
    };

    // Prefer non-destructive behavior: attempt to mount first.
    crate::log!("fatfs demo: mount begin\n");
    let fs = match FileSystem::new(io, FsOptions::new()) {
        Ok(fs) => fs,
        Err(e) => {
            crate::log!("fatfs demo: mount failed ({:?}); formatting usbms (destructive)\n", e);

            let mut io = match BlockDeviceIo::new(handle) {
                Ok(io) => io,
                Err(e) => {
                    crate::log!("fatfs demo: failed to init device io for format: {:?}\n", e);
                    return;
                }
            };

            if let Err(e) = format_volume(&mut io, FormatVolumeOptions::new()) {
                crate::log!("fatfs demo: format failed ({:?})\n", e);
                return;
            }

            match FileSystem::new(io, FsOptions::new()) {
                Ok(fs) => fs,
                Err(e) => {
                    crate::log!("fatfs demo: mount after format failed ({:?})\n", e);
                    return;
                }
            }
        }
    };

    crate::log!("fatfs demo: mount ok\n");

    // Keep this 8.3-safe (<=8.3) to avoid short-name alias confusion.
    const DEMO_FILE_NAME: &str = "trueos.txt";
    crate::log!("fatfs demo: demo file name={}\n", DEMO_FILE_NAME);

    {
        let root = fs.root_dir();

        crate::log!("fatfs demo: dir list begin\n");
        let mut listed = 0u32;
        for entry in root.iter() {
            match entry {
                Ok(entry) => {
                    crate::log!("fatfs demo: dir entry: {}\n", entry.file_name());
                    listed = listed.wrapping_add(1);
                    if listed >= 16 {
                        crate::log!("fatfs demo: dir list truncated\n");
                        break;
                    }
                }
                Err(e) => {
                    crate::log!("fatfs demo: dir list entry failed ({:?})\n", e);
                    break;
                }
            }
        }
        crate::log!("fatfs demo: dir list end (count={})\n", listed);

        fn log_read<T: Read>(label: &str, file: &mut T)
        where
            T::Error: core::fmt::Debug,
        {
            let mut buf = [0u8; 256];
            match file.read(&mut buf) {
                Ok(n) => {
                    let shown = core::cmp::min(n, 128);
                    crate::log!("fatfs demo: {} read ok n={} shown={} [", label, n, shown);
                    for i in 0..shown {
                        crate::log!("{:02X}", buf[i]);
                        if i + 1 != shown {
                            crate::log!(" ");
                        }
                    }
                    if n > shown {
                        crate::log!(" ...");
                    }
                    crate::log!("]\n");
                }
                Err(e) => crate::log!("fatfs demo: {} read failed ({:?})\n", label, e),
            }
        }

        crate::log!("fatfs demo: open existing begin\n");
        match root.open_file(DEMO_FILE_NAME) {
            Ok(mut file) => {
                crate::log!("fatfs demo: open existing ok\n");
                log_read("existing", &mut file);
            }
            Err(fatfs::Error::NotFound) => {
                crate::log!("fatfs demo: existing file missing; create begin\n");
                match root.create_file(DEMO_FILE_NAME) {
                    Ok(mut file) => {
                        crate::log!("fatfs demo: create ok; write_all begin\n");
                        if let Err(e) = file.write_all("TRUEOS§".as_bytes()) {
                            crate::log!("fatfs demo: write_all failed ({:?})\n", e);
                        }
                        let _ = file.flush();

                        let _ = file.seek(SeekFrom::Start(0));
                        log_read("after_create", &mut file);
                    }
                    Err(e) => crate::log!("fatfs demo: create_file failed ({:?})\n", e),
                }
            }
            Err(e) => crate::log!("fatfs demo: open existing failed ({:?})\n", e),
        };
    }
}

#[embassy_executor::task]
pub async fn fatfs_usb_demo_task() {
    // Wait for the USB mass storage block device to appear.
    for _ in 0..200 {
        if let Some(dev) = pick_usbms_device() {
            run_fatfs_demo_on_device(dev);
            return;
        }
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
    crate::log!("fatfs demo: usbms device not found\n");
}
