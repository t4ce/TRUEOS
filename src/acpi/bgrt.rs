use spin::Once;

use crate::{
    debugconf,
    pci::mmio,
};

use super::ensure_tables;

static BGRT_LOG_ONCE: Once<()> = Once::new();

#[derive(Clone, Copy, Debug)]
struct BmpInfo {
    width: u32,
    height: u32,
    bpp: u16,
    compression: u32,
}

pub fn log_once() {
    BGRT_LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let Some(bgrt_mapping) = tables.find_table::<::acpi::sdt::bgrt::Bgrt>() else {
            return;
        };

        // `Bgrt` is `#[repr(C, packed)]`; avoid unaligned field refs by copying it out.
        let bgrt = unsafe { core::ptr::read_unaligned(bgrt_mapping.virtual_start.as_ptr()) };

        let addr = bgrt.image_address;
        let off_x = bgrt.image_offset_x;
        let off_y = bgrt.image_offset_y;

        if addr == 0 {
            return;
        }

        match bgrt.image_type() {
            ::acpi::sdt::bgrt::ImageType::Bitmap => {
                let bmp = parse_bmp_header_phys(addr);
                if let Some(bmp) = bmp {
                    debugconf!(
                        "BGRT: addr=0x{:016X} off=({}, {}) size={}x{} fmt=bpp{} comp{}\n",
                        addr,
                        off_x,
                        off_y,
                        bmp.width,
                        bmp.height,
                        bmp.bpp,
                        bmp.compression,
                    );
                } else {
                    // Table present but BMP header unreadable/unexpected.
                    debugconf!(
                        "BGRT: addr=0x{:016X} off=({}, {}) size=? fmt=bmp?\n",
                        addr,
                        off_x,
                        off_y
                    );
                }
            }
            ::acpi::sdt::bgrt::ImageType::Reserved => {
                debugconf!(
                    "BGRT: addr=0x{:016X} off=({}, {}) size=? fmt=type{}\n",
                    addr,
                    off_x,
                    off_y,
                    bgrt.image_type,
                );
            }
        }
    });
}

fn parse_bmp_header_phys(phys_addr: u64) -> Option<BmpInfo> {
    // Read enough for BITMAPFILEHEADER (14) + BITMAPINFOHEADER (40).
    let mapped = mmio::map_mmio_region_exact(phys_addr, 64).ok()?;
    let p = mapped.as_ptr();

    let sig = unsafe { read_u16(p.add(0)) };
    if sig != 0x4D42 {
        return None;
    }

    let dib_size = unsafe { read_u32(p.add(14)) };
    if dib_size < 40 {
        return None;
    }

    let width = unsafe { read_i32(p.add(18)) };
    let height = unsafe { read_i32(p.add(22)) };
    let bpp = unsafe { read_u16(p.add(28)) };
    let compression = unsafe { read_u32(p.add(30)) };

    let width = u32::try_from(width.abs()).ok()?;
    let height = u32::try_from(height.abs()).ok()?;

    Some(BmpInfo {
        width,
        height,
        bpp,
        compression,
    })
}

#[inline(always)]
unsafe fn read_u16(p: *const u8) -> u16 {
    u16::from_le(core::ptr::read_unaligned(p as *const u16))
}

#[inline(always)]
unsafe fn read_u32(p: *const u8) -> u32 {
    u32::from_le(core::ptr::read_unaligned(p as *const u32))
}

#[inline(always)]
unsafe fn read_i32(p: *const u8) -> i32 {
    i32::from_le(core::ptr::read_unaligned(p as *const i32))
}
