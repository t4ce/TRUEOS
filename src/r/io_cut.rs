use alloc::vec::Vec;
use core::cmp::Ordering;
use parry2d::math::Vector;

pub(super) fn clip_rgb_triangles_to_scissor(
    src: &[u8],
    scissor: ScissorRect,
    vp_w: u32,
    vp_h: u32,
) -> Vec<u8> {
    trueos_gfx_core::clip_rgb_triangles_to_scissor_bytes(
        src,
        trueos_gfx_core::ScissorRect {
            x: scissor.x,
            y: scissor.y,
            width: scissor.width,
            height: scissor.height,
        },
        vp_w,
        vp_h,
    )
}

#[inline]
fn convex_cross(o: Vector, a: Vector, b: Vector) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

pub(crate) fn convex_hull_points(points: &[Vector]) -> Vec<Vector> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.y.partial_cmp(&b.y).unwrap_or(Ordering::Equal))
    });
    sorted.dedup_by(|a, b| a.x == b.x && a.y == b.y);

    if sorted.len() <= 2 {
        return sorted;
    }

    let mut lower = Vec::with_capacity(sorted.len());
    for &p in &sorted {
        while lower.len() >= 2 {
            let n = lower.len();
            if convex_cross(lower[n - 2], lower[n - 1], p) > 0.0 {
                break;
            }
            lower.pop();
        }
        lower.push(p);
    }

    let mut upper = Vec::with_capacity(sorted.len());
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 {
            let n = upper.len();
            if convex_cross(upper[n - 2], upper[n - 1], p) > 0.0 {
                break;
            }
            upper.pop();
        }
        upper.push(p);
    }

    lower.pop();
    upper.pop();
    lower.extend_from_slice(upper.as_slice());
    lower
}
