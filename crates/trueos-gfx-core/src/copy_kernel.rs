use alloc::vec::Vec;

use crate::{Error, Result, Rgba8, UiRect, UiSurfaceFormat};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyBlend {
    Opaque,
    SrcAlpha,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyPoint {
    pub x: i32,
    pub y: i32,
}

impl CopyPoint {
    #[inline]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyKernelOptions {
    pub blend: CopyBlend,
}

impl CopyKernelOptions {
    #[inline]
    pub const fn opaque() -> Self {
        Self {
            blend: CopyBlend::Opaque,
        }
    }

    #[inline]
    pub const fn src_alpha() -> Self {
        Self {
            blend: CopyBlend::SrcAlpha,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopySurface<'a> {
    pub bytes: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: usize,
    pub format: UiSurfaceFormat,
}

#[derive(Debug)]
pub struct CopySurfaceMut<'a> {
    pub bytes: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: usize,
    pub format: UiSurfaceFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopySpan {
    pub dst_x: u32,
    pub dst_y: u32,
    pub src_x: u32,
    pub src_y: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CopyKernelStats {
    pub input_rects: usize,
    pub clipped_rects: usize,
    pub spans: usize,
    pub pixels: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClippedRect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Interval {
    x0: u32,
    x1: u32,
}

pub fn copy_region_from_rects(
    src: CopySurface<'_>,
    dst: CopySurfaceMut<'_>,
    src_origin: CopyPoint,
    dst_origin: CopyPoint,
    rects: &[UiRect],
    options: CopyKernelOptions,
) -> Result<CopyKernelStats> {
    validate_surface(src.bytes.len(), src.width, src.height, src.pitch_bytes, src.format)?;
    validate_surface(dst.bytes.len(), dst.width, dst.height, dst.pitch_bytes, dst.format)?;

    let spans = build_copy_spans(
        src.width, src.height, dst.width, dst.height, src_origin, dst_origin, rects,
    )?;
    let mut stats = CopyKernelStats {
        input_rects: rects.len(),
        clipped_rects: 0,
        spans: spans.len(),
        pixels: 0,
    };

    for span in spans {
        copy_span(src, &mut *dst.bytes, dst.pitch_bytes, dst.format, span, options)?;
        stats.pixels = stats.pixels.saturating_add(span.len as usize);
    }
    stats.clipped_rects =
        clipped_rects(src.width, src.height, dst.width, dst.height, src_origin, dst_origin, rects)
            .len();
    Ok(stats)
}

pub fn build_copy_spans(
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    src_origin: CopyPoint,
    dst_origin: CopyPoint,
    rects: &[UiRect],
) -> Result<Vec<CopySpan>> {
    let clipped =
        clipped_rects(src_width, src_height, dst_width, dst_height, src_origin, dst_origin, rects);
    if clipped.is_empty() {
        return Ok(Vec::new());
    }

    let min_y = clipped.iter().map(|r| r.y0).min().unwrap_or(0);
    let max_y = clipped.iter().map(|r| r.y1).max().unwrap_or(0);
    let mut spans = Vec::new();
    let mut intervals = Vec::new();

    for y in min_y..max_y {
        intervals.clear();
        for rect in &clipped {
            if y >= rect.y0 && y < rect.y1 {
                intervals.push(Interval {
                    x0: rect.x0,
                    x1: rect.x1,
                });
            }
        }
        if intervals.is_empty() {
            continue;
        }

        intervals.sort_by_key(|interval| interval.x0);
        let mut merged = intervals[0];
        for interval in intervals.iter().skip(1) {
            if interval.x0 <= merged.x1 {
                merged.x1 = merged.x1.max(interval.x1);
            } else {
                push_span(&mut spans, merged, y, src_origin, dst_origin)?;
                merged = *interval;
            }
        }
        push_span(&mut spans, merged, y, src_origin, dst_origin)?;
    }

    Ok(spans)
}

fn clipped_rects(
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    src_origin: CopyPoint,
    dst_origin: CopyPoint,
    rects: &[UiRect],
) -> Vec<ClippedRect> {
    let src_x0_in_dst = i64::from(dst_origin.x) - i64::from(src_origin.x);
    let src_y0_in_dst = i64::from(dst_origin.y) - i64::from(src_origin.y);
    let min_x = 0i64.max(src_x0_in_dst);
    let min_y = 0i64.max(src_y0_in_dst);
    let max_x = i64::from(dst_width).min(src_x0_in_dst + i64::from(src_width));
    let max_y = i64::from(dst_height).min(src_y0_in_dst + i64::from(src_height));

    let mut out = Vec::new();
    if min_x >= max_x || min_y >= max_y {
        return out;
    }

    for rect in rects {
        if rect.is_empty() {
            continue;
        }
        let x0 = i64::from(rect.x).max(min_x);
        let y0 = i64::from(rect.y).max(min_y);
        let x1 = i64::from(rect.x)
            .saturating_add(i64::from(rect.w))
            .min(max_x);
        let y1 = i64::from(rect.y)
            .saturating_add(i64::from(rect.h))
            .min(max_y);
        if x0 < x1 && y0 < y1 {
            out.push(ClippedRect {
                x0: x0 as u32,
                y0: y0 as u32,
                x1: x1 as u32,
                y1: y1 as u32,
            });
        }
    }
    out
}

fn push_span(
    spans: &mut Vec<CopySpan>,
    interval: Interval,
    dst_y: u32,
    src_origin: CopyPoint,
    dst_origin: CopyPoint,
) -> Result<()> {
    if interval.x0 >= interval.x1 {
        return Ok(());
    }
    let src_x = i64::from(src_origin.x) + i64::from(interval.x0) - i64::from(dst_origin.x);
    let src_y = i64::from(src_origin.y) + i64::from(dst_y) - i64::from(dst_origin.y);
    let src_x = u32::try_from(src_x).map_err(|_| Error::Invalid)?;
    let src_y = u32::try_from(src_y).map_err(|_| Error::Invalid)?;
    spans.push(CopySpan {
        dst_x: interval.x0,
        dst_y,
        src_x,
        src_y,
        len: interval.x1 - interval.x0,
    });
    Ok(())
}

fn validate_surface(
    byte_len: usize,
    width: u32,
    height: u32,
    pitch_bytes: usize,
    _format: UiSurfaceFormat,
) -> Result<()> {
    let row_bytes = (width as usize).checked_mul(4).ok_or(Error::Invalid)?;
    if width == 0 || height == 0 || pitch_bytes < row_bytes {
        return Err(Error::Invalid);
    }
    let min_len = pitch_bytes
        .checked_mul(height as usize)
        .ok_or(Error::Invalid)?;
    if byte_len < min_len {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn copy_span(
    src: CopySurface<'_>,
    dst_bytes: &mut [u8],
    dst_pitch_bytes: usize,
    dst_format: UiSurfaceFormat,
    span: CopySpan,
    options: CopyKernelOptions,
) -> Result<()> {
    let byte_len = (span.len as usize).checked_mul(4).ok_or(Error::Invalid)?;
    let src_off = pixel_offset(src.pitch_bytes, span.src_x, span.src_y)?;
    let dst_off = pixel_offset(dst_pitch_bytes, span.dst_x, span.dst_y)?;

    if options.blend == CopyBlend::Opaque && src.format == dst_format {
        dst_bytes[dst_off..dst_off + byte_len]
            .copy_from_slice(&src.bytes[src_off..src_off + byte_len]);
        return Ok(());
    }

    for i in 0..span.len as usize {
        let src_pixel =
            read_pixel(src.bytes, src_off.saturating_add(i.saturating_mul(4)), src.format);
        let dst_pixel_off = dst_off.saturating_add(i.saturating_mul(4));
        let out = match options.blend {
            CopyBlend::Opaque => src_pixel,
            CopyBlend::SrcAlpha => {
                let dst_pixel = read_pixel(dst_bytes, dst_pixel_off, dst_format);
                src_over(src_pixel, dst_pixel)
            }
        };
        write_pixel(dst_bytes, dst_pixel_off, dst_format, out);
    }
    Ok(())
}

#[inline]
fn pixel_offset(pitch_bytes: usize, x: u32, y: u32) -> Result<usize> {
    (y as usize)
        .checked_mul(pitch_bytes)
        .and_then(|row| row.checked_add((x as usize).checked_mul(4)?))
        .ok_or(Error::Invalid)
}

#[inline]
fn read_pixel(bytes: &[u8], off: usize, format: UiSurfaceFormat) -> Rgba8 {
    match format {
        UiSurfaceFormat::Rgba8888 => {
            Rgba8::new(bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3])
        }
        UiSurfaceFormat::Xrgb8888 => {
            let word =
                u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            Rgba8::new(
                ((word >> 16) & 0xFF) as u8,
                ((word >> 8) & 0xFF) as u8,
                (word & 0xFF) as u8,
                255,
            )
        }
        UiSurfaceFormat::Xbgr8888 => {
            let word =
                u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            Rgba8::new(
                (word & 0xFF) as u8,
                ((word >> 8) & 0xFF) as u8,
                ((word >> 16) & 0xFF) as u8,
                255,
            )
        }
    }
}

#[inline]
fn write_pixel(bytes: &mut [u8], off: usize, format: UiSurfaceFormat, pixel: Rgba8) {
    match format {
        UiSurfaceFormat::Rgba8888 => {
            bytes[off] = pixel.r;
            bytes[off + 1] = pixel.g;
            bytes[off + 2] = pixel.b;
            bytes[off + 3] = pixel.a;
        }
        UiSurfaceFormat::Xrgb8888 => {
            let word = ((pixel.r as u32) << 16) | ((pixel.g as u32) << 8) | pixel.b as u32;
            bytes[off..off + 4].copy_from_slice(&word.to_le_bytes());
        }
        UiSurfaceFormat::Xbgr8888 => {
            let word = ((pixel.b as u32) << 16) | ((pixel.g as u32) << 8) | pixel.r as u32;
            bytes[off..off + 4].copy_from_slice(&word.to_le_bytes());
        }
    }
}

#[inline]
fn div255(value: u32) -> u8 {
    ((value + 127) / 255) as u8
}

fn src_over(src: Rgba8, dst: Rgba8) -> Rgba8 {
    if src.a == 0 {
        return dst;
    }
    if src.a == 255 {
        return src;
    }
    let inv = 255u32 - src.a as u32;
    Rgba8 {
        r: div255(src.r as u32 * src.a as u32 + dst.r as u32 * inv),
        g: div255(src.g as u32 * src.a as u32 + dst.g as u32 * inv),
        b: div255(src.b as u32 * src.a as u32 + dst.b as u32 * inv),
        a: (src.a as u32 + div255(dst.a as u32 * inv) as u32).min(255) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_bytes(pixels: &[Rgba8]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in pixels {
            out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
        }
        out
    }

    #[test]
    fn spans_are_row_sorted_union_with_holes() {
        let rects = [
            UiRect::new(1, 1, 4, 2),
            UiRect::new(3, 2, 4, 2),
            UiRect::new(1, 5, 2, 1),
            UiRect::new(5, 5, 2, 1),
        ];
        let spans =
            build_copy_spans(16, 16, 16, 16, CopyPoint::new(0, 0), CopyPoint::new(0, 0), &rects)
                .unwrap();

        let compact: Vec<(u32, u32, u32)> =
            spans.iter().map(|s| (s.dst_y, s.dst_x, s.len)).collect();
        assert_eq!(compact, vec![(1, 1, 4), (2, 1, 6), (3, 3, 4), (5, 1, 2), (5, 5, 2),]);
    }

    #[test]
    fn overlapping_rects_copy_each_pixel_once() {
        let width = 6;
        let height = 4;
        let mut src_pixels = Vec::new();
        for i in 0..(width * height) {
            src_pixels.push(Rgba8::new(i as u8, 0, 0, 255));
        }
        let src_bytes = rgba_bytes(&src_pixels);
        let mut dst = vec![0u8; src_bytes.len()];
        let rects = [UiRect::new(1, 1, 3, 2), UiRect::new(2, 1, 3, 2)];

        let stats = copy_region_from_rects(
            CopySurface {
                bytes: &src_bytes,
                width,
                height,
                pitch_bytes: width as usize * 4,
                format: UiSurfaceFormat::Rgba8888,
            },
            CopySurfaceMut {
                bytes: &mut dst,
                width,
                height,
                pitch_bytes: width as usize * 4,
                format: UiSurfaceFormat::Rgba8888,
            },
            CopyPoint::new(0, 0),
            CopyPoint::new(0, 0),
            &rects,
            CopyKernelOptions::opaque(),
        )
        .unwrap();

        assert_eq!(stats.spans, 2);
        assert_eq!(stats.pixels, 8);
        for y in 0..height {
            for x in 0..width {
                let off = (y as usize * width as usize + x as usize) * 4;
                if (1..=2).contains(&y) && (1..=4).contains(&x) {
                    assert_eq!(&dst[off..off + 4], &src_bytes[off..off + 4]);
                } else {
                    assert_eq!(&dst[off..off + 4], &[0, 0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn alpha_stamp_blends_and_preserves_holes() {
        let width = 4;
        let height = 3;
        let src = rgba_bytes(&[Rgba8::new(200, 0, 0, 128); 12]);
        let mut dst = rgba_bytes(&[Rgba8::new(0, 0, 100, 255); 12]);
        let rects = [UiRect::new(0, 0, 1, 3), UiRect::new(2, 0, 2, 3)];

        copy_region_from_rects(
            CopySurface {
                bytes: &src,
                width,
                height,
                pitch_bytes: width as usize * 4,
                format: UiSurfaceFormat::Rgba8888,
            },
            CopySurfaceMut {
                bytes: &mut dst,
                width,
                height,
                pitch_bytes: width as usize * 4,
                format: UiSurfaceFormat::Rgba8888,
            },
            CopyPoint::new(0, 0),
            CopyPoint::new(0, 0),
            &rects,
            CopyKernelOptions::src_alpha(),
        )
        .unwrap();

        for y in 0..height {
            let hole = (y as usize * width as usize + 1) * 4;
            assert_eq!(&dst[hole..hole + 4], &[0, 0, 100, 255]);
        }
        assert_eq!(&dst[0..4], &[100, 0, 50, 255]);
    }
}
