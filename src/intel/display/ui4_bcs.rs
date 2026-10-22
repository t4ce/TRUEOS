//! Slot-0 pixel ownership: restore transparent damage, then copy in broker z order.

use alloc::vec::Vec;

use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaSurface};

use super::{
    CompositionDamageRect, CompositionDamageRegion, RgbaOverlayTile, Ui4AsyncCompositionError,
};

/// Each copy has a matching source-cache flag. Copying all four channels,
/// including alpha zero, lets the display plane blend the winning pixel.
pub(super) fn plan_copies(
    base: GucBcs0RgbaSurface,
    destination: GucBcs0RgbaSurface,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
) -> Result<(Vec<GucBcs0RgbaCopy>, Vec<bool>), Ui4AsyncCompositionError> {
    let unavailable = Ui4AsyncCompositionError::Unavailable;
    if !valid_surface(base)
        || !valid_surface(destination)
        || base.width < destination.width
        || base.height < destination.height
        || overlaps(base, destination)
    {
        return Err(unavailable);
    }
    let damage =
        damage.clipped(CompositionDamageRect::new(0, 0, destination.width, destination.height));
    let mut copies = Vec::new();
    let mut scanout_cache = Vec::new();
    for rect in damage.rects() {
        copies.push(GucBcs0RgbaCopy {
            source: base,
            source_x: rect.x,
            source_y: rect.y,
            destination_x: rect.x,
            destination_y: rect.y,
            width: rect.width,
            height: rect.height,
        });
        scanout_cache.push(false);
    }
    for (index, tile) in tiles.iter().enumerate() {
        let bounds = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
        if tile.opacity == 0
            || !damage
                .rects()
                .iter()
                .any(|rect| bounds.intersection(*rect).is_some())
            || tiles[index + 1..]
                .iter()
                .any(|later| fully_covers(later, tile))
        {
            continue;
        }
        // BCS copies preserve RGBA; scaling and independent window opacity
        // cannot be represented by a rectangle copy.
        if tile.opacity != u8::MAX
            || tile.width == 0
            || tile.height == 0
            || tile.width != tile.source_width
            || tile.height != tile.source_height
        {
            return Err(unavailable);
        }
        let source = tile.gpgpu_surface.ok_or(unavailable)?;
        if !source.is_valid()
            || !matches!(source.storage_order, crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba)
            || source.width != tile.source_width
            || source.height != tile.source_height
            || source.pitch_bytes as usize != tile.pitch_bytes
        {
            return Err(unavailable);
        }
        let source = GucBcs0RgbaSurface {
            phys: source.phys,
            gpu: source.gpu,
            bytes: source.bytes,
            width: source.width,
            height: source.height,
            pitch_bytes: source.pitch_bytes,
        };
        if overlaps(source, destination) {
            return Err(unavailable);
        }
        for damaged in damage.rects() {
            let Some(draw) = bounds.intersection(*damaged) else {
                continue;
            };
            if tile.arc == 0 {
                push_copy(&mut copies, &mut scanout_cache, source, tile, draw);
                continue;
            }
            for y in draw.y..draw.y + draw.height {
                let local_y = y - tile.y;
                let middle = tile.width / 2;
                if !crate::ui4::window_arc::contains(
                    tile.width,
                    tile.height,
                    tile.arc,
                    middle,
                    local_y,
                ) {
                    continue;
                }
                // The rounded outline is symmetric and contiguous on each
                // row. Find its left edge without scanning producer pixels.
                let mut left = 0;
                let mut right = middle;
                while left < right {
                    let x = left + (right - left) / 2;
                    if crate::ui4::window_arc::contains(
                        tile.width,
                        tile.height,
                        tile.arc,
                        x,
                        local_y,
                    ) {
                        right = x;
                    } else {
                        left = x + 1;
                    }
                }
                let x = draw.x.max(tile.x.saturating_add(left));
                let end = (draw.x + draw.width).min(tile.x.saturating_add(tile.width - left));
                if end > x {
                    push_copy(
                        &mut copies,
                        &mut scanout_cache,
                        source,
                        tile,
                        CompositionDamageRect::new(x, y, end - x, 1),
                    );
                }
            }
        }
    }
    Ok((copies, scanout_cache))
}

fn fully_covers(later: &RgbaOverlayTile<'_>, lower: &RgbaOverlayTile<'_>) -> bool {
    if later.opacity == 0 {
        return false;
    }
    if later.x == lower.x
        && later.y == lower.y
        && later.width == lower.width
        && later.height == lower.height
        && later.arc == lower.arc
    {
        return true;
    }
    // Alpha does not uncover another tile in slot 0. Even a transparent
    // winning pixel hides an earlier owner, so invisible sources need no
    // import, scaling or opacity operation.
    later.arc == 0
        && later.x <= lower.x
        && later.y <= lower.y
        && u64::from(later.x) + u64::from(later.width)
            >= u64::from(lower.x) + u64::from(lower.width)
        && u64::from(later.y) + u64::from(later.height)
            >= u64::from(lower.y) + u64::from(lower.height)
}

fn push_copy(
    copies: &mut Vec<GucBcs0RgbaCopy>,
    scanout_cache: &mut Vec<bool>,
    source: GucBcs0RgbaSurface,
    tile: &RgbaOverlayTile<'_>,
    draw: CompositionDamageRect,
) {
    let source_x = draw.x - tile.x;
    let source_y = draw.y - tile.y;
    if let Some(previous) = copies.last_mut()
        && previous.source == source
        && scanout_cache.last() == Some(&tile.gpgpu_scanout_cache)
        && previous.source_x == source_x
        && previous.destination_x == draw.x
        && previous.width == draw.width
        && previous.source_y.checked_add(previous.height) == Some(source_y)
        && previous.destination_y.checked_add(previous.height) == Some(draw.y)
    {
        previous.height += draw.height;
        return;
    }
    copies.push(GucBcs0RgbaCopy {
        source,
        source_x,
        source_y,
        destination_x: draw.x,
        destination_y: draw.y,
        width: draw.width,
        height: draw.height,
    });
    scanout_cache.push(tile.gpgpu_scanout_cache);
}

fn valid_surface(surface: GucBcs0RgbaSurface) -> bool {
    crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        surface.gpu,
        surface.bytes,
        surface.width,
        surface.height,
        surface.pitch_bytes,
    )
    .is_some()
}

fn overlaps(left: GucBcs0RgbaSurface, right: GucBcs0RgbaSurface) -> bool {
    let overlap = |a: u64, a_bytes: usize, b: u64, b_bytes: usize| {
        a < b.saturating_add(b_bytes as u64) && b < a.saturating_add(a_bytes as u64)
    };
    overlap(left.phys, left.bytes, right.phys, right.bytes)
        || overlap(left.gpu, left.bytes, right.gpu, right.bytes)
}
