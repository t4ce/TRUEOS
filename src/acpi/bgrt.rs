use spin::Once;

use crate::{debugconf, limine, pci::mmio, vga};

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

const MAX_W: usize = 256;
const MAX_H: usize = 256;
const MAX_PIXELS: usize = MAX_W * MAX_H;
const MAX_PALETTE: usize = 256;

#[link_section = ".bss"]
static mut BGRT_PIXELS: [u32; MAX_PIXELS] = [0; MAX_PIXELS];
static mut PALETTE: [u32; MAX_PALETTE] = [0; MAX_PALETTE];

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let Some(bgrt) = tables.find_table::<acpi::sdt::bgrt::Bgrt>() else {
            return;
        };

        let addr = bgrt.image_address;
        let (off_x, off_y) = bgrt.image_offset();

        // BGRT only defines image type and address/offset; size/format are in the pointed-to BMP.
        // IMPORTANT: This runs during early BSP bringup, so avoid heap allocations.
        let bmp = match bgrt.image_type() {
            acpi::sdt::bgrt::ImageType::Bitmap => {
                if let Some(info) = parse_bmp_header(addr) {
                    debugconf!(
                        "BGRT: addr=0x{:016X} off=({}, {}) size=0x{:X} fmt=bmp {}x{} {}bpp\n",
                        addr,
                        off_x,
                        off_y,
                        info.file_size,
                        info.width,
                        info.height,
                        info.bpp
                    );
                    Some(info)
                } else {
                    debugconf!(
                        "BGRT: addr=0x{:016X} off=({}, {}) size=? fmt=bmp\n",
                        addr,
                        off_x,
                        off_y
                    );
                    None
                }
            }
            acpi::sdt::bgrt::ImageType::Reserved => {
                debugconf!(
                    "BGRT: addr=0x{:016X} off=({}, {}) size=? fmt=reserved\n",
                    addr,
                    off_x,
                    off_y
                );
                None
            }
        };

        // If it is a BMP we understand, blit (cropped to 256x256) into the framebuffer.
        if let Some(bmp) = bmp {
            let (dst_x, dst_y) = vga::framebuffer_dimensions()
                .map(|(w, h)| (w as usize, h as usize))
                .unwrap_or((0, 0));
            let _ = blit_bmp_to_vga(addr, dst_x, dst_y, &bmp);
        }
    });
}

struct BmpInfo {
    file_size: u32,
    data_offset: u32,
    width: u32,
    height: u32,
    bpp: u16,
    compression: u32,
    top_down: bool,
}

fn parse_bmp_header(phys_addr: u64) -> Option<BmpInfo> {
    // Read enough for BITMAPFILEHEADER (14) + BITMAPINFOHEADER (40) + bitfields (12) + palette (256*4 worst-case).
    const READ_LEN: usize = 14 + 40 + 12 + (MAX_PALETTE * 4);

    if phys_addr < 0x1000 {
        return None;
    }

    // Avoid probing clearly unsafe physical ranges on baremetal.
    if !phys_range_looks_safe(phys_addr, READ_LEN as u64) {
        return None;
    }

    let mapped = mmio::map_mmio_region_exact(phys_addr, READ_LEN).ok()?;
    let base = mapped.as_ptr();

    let mut buf = [0u8; READ_LEN];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = unsafe { core::ptr::read_volatile(base.add(i) as *const u8) };
    }

    // Signature "BM".
    if buf[0] != b'B' || buf[1] != b'M' {
        return None;
    }

    let file_size = le_u32(&buf[2..6]);
    let data_offset = le_u32(&buf[10..14]);
    let dib_size = le_u32(&buf[14..18]);
    if dib_size < 40 {
        return None;
    }

    let width_i32 = le_i32(&buf[18..22]);
    let height_i32 = le_i32(&buf[22..26]);
    let planes = le_u16(&buf[26..28]);
    let bpp = le_u16(&buf[28..30]);
    let compression = le_u32(&buf[30..34]);

    if planes != 1 {
        return None;
    }

    let top_down = height_i32 < 0;
    let width = width_i32.unsigned_abs();
    let height = height_i32.unsigned_abs();

    // Basic sanity: data must start within the file.
    if data_offset >= file_size {
        return None;
    }

    Some(BmpInfo {
        file_size,
        data_offset,
        width,
        height,
        bpp,
        compression,
        top_down,
    })
}
fn blit_bmp_to_vga(phys_addr: u64, origin_x: usize, origin_y: usize, bmp: &BmpInfo) -> bool {
    let src_w = bmp.width as usize;
    let src_h = bmp.height as usize;
    if src_w == 0 || src_h == 0 {
        return false;
    }

    let copy_w = src_w.min(MAX_W);
    let copy_h = src_h.min(MAX_H);

    // Decide pixel reader based on bpp/compression.
    match (bmp.bpp, bmp.compression) {
        (24, 0) => blit_bmp24(phys_addr, origin_x, origin_y, bmp, copy_w, copy_h),
        (32, 0) => blit_bmp32(phys_addr, origin_x, origin_y, bmp, copy_w, copy_h, None),
        (32, 3) => {
            if let Some(masks) = read_masks(phys_addr, bmp) {
                blit_bmp32(
                    phys_addr,
                    origin_x,
                    origin_y,
                    bmp,
                    copy_w,
                    copy_h,
                    Some(masks),
                )
            } else {
                false
            }
        }
        (8, 0) => blit_bmp_indexed(phys_addr, origin_x, origin_y, bmp, copy_w, copy_h, 8),
        (4, 0) => blit_bmp_indexed(phys_addr, origin_x, origin_y, bmp, copy_w, copy_h, 4),
        _ => false,
    }
}

fn read_masks(phys_addr: u64, _bmp: &BmpInfo) -> Option<(u32, u32, u32)> {
    // For BI_BITFIELDS, masks immediately follow the BITMAPINFOHEADER.
    let masks_off = phys_addr.checked_add(14 + 40)?;
    if !phys_range_looks_safe(masks_off, 12) {
        return None;
    }
    let mapped = mmio::map_mmio_region_exact(masks_off, 12).ok()?;
    let p = mapped.as_ptr();
    unsafe {
        let r = core::ptr::read_unaligned(p.add(0) as *const u32);
        let g = core::ptr::read_unaligned(p.add(4) as *const u32);
        let b = core::ptr::read_unaligned(p.add(8) as *const u32);
        Some((r, g, b))
    }
}

fn blit_bmp24(
    phys_addr: u64,
    origin_x: usize,
    origin_y: usize,
    bmp: &BmpInfo,
    copy_w: usize,
    copy_h: usize,
) -> bool {
    let width = bmp.width as usize;
    let height = bmp.height as usize;
    let row_bytes = width.saturating_mul(3);
    let row_stride = (row_bytes + 3) & !3;
    let pixel_bytes = row_stride.saturating_mul(height);
    let Some(data_phys) = phys_addr.checked_add(bmp.data_offset as u64) else {
        return false;
    };

    if !range_fits_in_file(bmp, pixel_bytes as u64) {
        return false;
    }
    if !phys_range_looks_safe(data_phys, pixel_bytes as u64) {
        return false;
    }

    let Some(mapped) = mmio::map_mmio_region_exact(data_phys, pixel_bytes).ok() else {
        return false;
    };
    let src = mapped.as_ptr();

    let expected = copy_w.saturating_mul(copy_h);
    if expected > MAX_PIXELS {
        return false;
    }

    unsafe {
        for y in 0..copy_h {
            let src_row = if bmp.top_down {
                y
            } else {
                (height - 1).saturating_sub(y)
            };
            let src_row_ptr = src.add(src_row.saturating_mul(row_stride));
            let dst_row_off = y.saturating_mul(copy_w);
            for x in 0..copy_w {
                let px = src_row_ptr.add(x.saturating_mul(3));
                let b = core::ptr::read_volatile(px.add(0));
                let g = core::ptr::read_volatile(px.add(1));
                let r = core::ptr::read_volatile(px.add(2));
                BGRT_PIXELS[dst_row_off.saturating_add(x)] =
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            }
        }
    }

    emit_image(origin_x, origin_y, copy_w, copy_h, expected)
}

fn blit_bmp32(
    phys_addr: u64,
    origin_x: usize,
    origin_y: usize,
    bmp: &BmpInfo,
    copy_w: usize,
    copy_h: usize,
    masks: Option<(u32, u32, u32)>,
) -> bool {
    let width = bmp.width as usize;
    let height = bmp.height as usize;
    let row_bytes = width.saturating_mul(4);
    let row_stride = row_bytes;
    let pixel_bytes = row_stride.saturating_mul(height);
    let Some(data_phys) = phys_addr.checked_add(bmp.data_offset as u64) else {
        return false;
    };

    if !range_fits_in_file(bmp, pixel_bytes as u64) {
        return false;
    }
    if !phys_range_looks_safe(data_phys, pixel_bytes as u64) {
        return false;
    }

    let Some(mapped) = mmio::map_mmio_region_exact(data_phys, pixel_bytes).ok() else {
        return false;
    };
    let src = mapped.as_ptr();

    let expected = copy_w.saturating_mul(copy_h);
    if expected > MAX_PIXELS {
        return false;
    }

    let (rm, gm, bm) = masks.unwrap_or((0x00FF_0000, 0x0000_FF00, 0x0000_00FF));
    let rshift = rm.trailing_zeros();
    let gshift = gm.trailing_zeros();
    let bshift = bm.trailing_zeros();

    unsafe {
        for y in 0..copy_h {
            let src_row = if bmp.top_down {
                y
            } else {
                (height - 1).saturating_sub(y)
            };
            let src_row_ptr = src.add(src_row.saturating_mul(row_stride));
            let dst_row_off = y.saturating_mul(copy_w);
            for x in 0..copy_w {
                let px =
                    core::ptr::read_volatile(src_row_ptr.add(x.saturating_mul(4)) as *const u32);
                let r = ((px & rm) >> rshift) as u32;
                let g = ((px & gm) >> gshift) as u32;
                let b = ((px & bm) >> bshift) as u32;
                BGRT_PIXELS[dst_row_off.saturating_add(x)] = (r << 16) | (g << 8) | b;
            }
        }
    }

    emit_image(origin_x, origin_y, copy_w, copy_h, expected)
}

fn blit_bmp_indexed(
    phys_addr: u64,
    origin_x: usize,
    origin_y: usize,
    bmp: &BmpInfo,
    copy_w: usize,
    copy_h: usize,
    bpp: u8,
) -> bool {
    let width = bmp.width as usize;
    let height = bmp.height as usize;
    let row_bits = width.saturating_mul(bpp as usize);
    let row_stride = ((row_bits + 31) / 32) * 4; // padded to 4-byte boundary
    let pixel_bytes = row_stride.saturating_mul(height);
    let Some(data_phys) = phys_addr.checked_add(bmp.data_offset as u64) else {
        return false;
    };

    if !range_fits_in_file(bmp, pixel_bytes as u64) {
        return false;
    }
    if !phys_range_looks_safe(data_phys, pixel_bytes as u64) {
        return false;
    }

    // Palette begins at header end; for indexed BI_RGB, palette entries are 4 bytes (B,G,R,0).
    let palette_entries = 1usize << bpp;
    let palette_bytes = palette_entries.saturating_mul(4).min(MAX_PALETTE * 4);
    let Some(palette_phys) = phys_addr.checked_add(14 + 40) else {
        return false;
    };
    if !phys_range_looks_safe(palette_phys, palette_bytes as u64) {
        return false;
    }
    let Some(pal_map) = mmio::map_mmio_region_exact(palette_phys, palette_bytes).ok() else {
        return false;
    };
    let pal_ptr = pal_map.as_ptr();
    unsafe {
        for i in 0..palette_entries.min(MAX_PALETTE) {
            let ent = core::ptr::read_unaligned(pal_ptr.add(i * 4) as *const u32);
            let b = ent & 0xFF;
            let g = (ent >> 8) & 0xFF;
            let r = (ent >> 16) & 0xFF;
            PALETTE[i] = (r << 16) | (g << 8) | b;
        }
    }

    let Some(mapped) = mmio::map_mmio_region_exact(data_phys, pixel_bytes).ok() else {
        return false;
    };
    let src = mapped.as_ptr();

    let expected = copy_w.saturating_mul(copy_h);
    if expected > MAX_PIXELS {
        return false;
    }

    unsafe {
        for y in 0..copy_h {
            let src_row = if bmp.top_down {
                y
            } else {
                (height - 1).saturating_sub(y)
            };
            let row_ptr = src.add(src_row.saturating_mul(row_stride));
            let dst_row_off = y.saturating_mul(copy_w);
            match bpp {
                8 => {
                    for x in 0..copy_w {
                        let idx = core::ptr::read_volatile(row_ptr.add(x)) as usize;
                        BGRT_PIXELS[dst_row_off.saturating_add(x)] = PALETTE[idx % MAX_PALETTE];
                    }
                }
                4 => {
                    for x in 0..copy_w {
                        let byte = core::ptr::read_volatile(row_ptr.add(x / 2));
                        let idx = if x % 2 == 0 { byte >> 4 } else { byte & 0x0F } as usize;
                        BGRT_PIXELS[dst_row_off.saturating_add(x)] = PALETTE[idx % MAX_PALETTE];
                    }
                }
                _ => {}
            }
        }
    }

    emit_image(origin_x, origin_y, copy_w, copy_h, expected)
}

fn emit_image(
    origin_x: usize,
    origin_y: usize,
    width: usize,
    height: usize,
    expected: usize,
) -> bool {
    let img = unsafe {
        vga::Image {
            width,
            height,
            pixels: &BGRT_PIXELS[..expected],
        }
    };
    vga::blit_image(origin_x, origin_y, &img)
}

fn range_fits_in_file(bmp: &BmpInfo, needed: u64) -> bool {
    let start = bmp.data_offset as u64;
    match start.checked_add(needed) {
        Some(end) => end <= bmp.file_size as u64,
        None => false,
    }
}

fn phys_range_looks_safe(base: u64, len: u64) -> bool {
    let end = match base.checked_add(len) {
        Some(v) => v,
        None => return false,
    };

    let Some(entries) = limine::memmap_entries() else {
        // If we can't validate, assume unsafe.
        return false;
    };

    for e in entries {
        let e_base = e.base;
        let e_end = match e.base.checked_add(e.length) {
            Some(v) => v,
            None => continue,
        };
        if base >= e_base && end <= e_end {
            use ::limine::memory_map::EntryType as T;
            return !matches!(e.entry_type, T::RESERVED | T::BAD_MEMORY);
        }
    }

    false
}

fn le_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_i32(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
