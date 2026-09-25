//! UI4 frame outline shared by hit testing and compositor admission.

/// A thousand is a full half-short-side radius: a square becomes a circle.
pub(crate) const ARC_MAX: u16 = 1000;

/// Twice the corner radius, allowing exact half-pixel radii for odd extents.
pub(crate) const fn radius_twice(width: u32, height: u32, arc: u16) -> u64 {
    (width.min(height) as u64 * arc as u64) / ARC_MAX as u64
}

/// Test a destination pixel center against the rounded frame outline.
pub(crate) const fn contains(width: u32, height: u32, arc: u16, x: u32, y: u32) -> bool {
    if x >= width || y >= height {
        return false;
    }
    let radius = radius_twice(width, height, arc);
    if radius == 0 {
        return true;
    }
    let px = x as u64 * 2 + 1;
    let py = y as u64 * 2 + 1;
    let right = width as u64 * 2 - radius;
    let bottom = height as u64 * 2 - radius;
    let dx = if px < radius {
        radius - px
    } else {
        px.saturating_sub(right)
    };
    let dy = if py < radius {
        radius - py
    } else {
        py.saturating_sub(bottom)
    };
    (dx as u128 * dx as u128 + dy as u128 * dy as u128)
        <= radius as u128 * radius as u128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_circle_and_capsule() {
        assert!(contains(8, 8, 0, 0, 0));
        assert!(!contains(8, 8, ARC_MAX, 0, 0));
        assert!(contains(8, 8, ARC_MAX, 4, 0));
        assert!(contains(8, 8, ARC_MAX, 0, 4));
        assert!(contains(12, 6, ARC_MAX, 6, 0));
        assert!(!contains(12, 6, ARC_MAX, 0, 0));
        assert!(contains(5, 5, ARC_MAX, 2, 0));
    }
}
