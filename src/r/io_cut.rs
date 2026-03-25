use alloc::vec::Vec;
use core::cmp::Ordering;
use parry2d::math::Point;

#[inline]
fn scissor_to_ndc(scissor: ScissorRect, vp_w: u32, vp_h: u32) -> Option<(f32, f32, f32, f32)> {
    if vp_w == 0 || vp_h == 0 {
        return None;
    }
    let x0 = scissor.x.min(vp_w) as f32;
    let y0 = scissor.y.min(vp_h) as f32;
    let x1 = scissor.x.saturating_add(scissor.width).min(vp_w) as f32;
    let y1 = scissor.y.saturating_add(scissor.height).min(vp_h) as f32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let w = vp_w as f32;
    let h = vp_h as f32;
    let left = (x0 / w) * 2.0 - 1.0;
    let right = (x1 / w) * 2.0 - 1.0;
    let top = 1.0 - (y0 / h) * 2.0;
    let bottom = 1.0 - (y1 / h) * 2.0;
    Some((left, right, bottom, top))
}

fn clip_poly_edge(input: &[RgbVtx], edge: u8, bound: f32, out: &mut Vec<RgbVtx>) {
    out.clear();
    if input.is_empty() {
        return;
    }

    let mut prev = input[input.len() - 1];
    let mut prev_in = match edge {
        0 => prev.x >= bound,
        1 => prev.x <= bound,
        2 => prev.y >= bound,
        _ => prev.y <= bound,
    };

    for &cur in input {
        let cur_in = match edge {
            0 => cur.x >= bound,
            1 => cur.x <= bound,
            2 => cur.y >= bound,
            _ => cur.y <= bound,
        };

        if cur_in != prev_in {
            let denom = match edge {
                0 | 1 => cur.x - prev.x,
                _ => cur.y - prev.y,
            };
            if denom.abs() > 1e-6 {
                let t = match edge {
                    0 | 1 => (bound - prev.x) / denom,
                    _ => (bound - prev.y) / denom,
                };
                out.push(interp_rgb(prev, cur, t));
            }
        }

        if cur_in {
            out.push(cur);
        }

        prev = cur;
        prev_in = cur_in;
    }
}

pub(super) fn clip_rgb_triangles_to_scissor(
    src: &[u8],
    scissor: ScissorRect,
    vp_w: u32,
    vp_h: u32,
) -> Vec<u8> {
    const VTX_SIZE: usize = 12;
    const TRI_SIZE: usize = VTX_SIZE * 3;

    let Some((left, right, bottom, top)) = scissor_to_ndc(scissor, vp_w, vp_h) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(src.len());
    let usable = src.len() - (src.len() % TRI_SIZE);
    let mut poly_a: Vec<RgbVtx> = Vec::with_capacity(8);
    let mut poly_b: Vec<RgbVtx> = Vec::with_capacity(8);

    let mut off = 0usize;
    while off + TRI_SIZE <= usable {
        let Some(v0) = read_rgb_vtx(src, off) else {
            break;
        };
        let Some(v1) = read_rgb_vtx(src, off + VTX_SIZE) else {
            break;
        };
        let Some(v2) = read_rgb_vtx(src, off + (2 * VTX_SIZE)) else {
            break;
        };
        off += TRI_SIZE;

        poly_a.clear();
        poly_a.push(v0);
        poly_a.push(v1);
        poly_a.push(v2);

        clip_poly_edge(&poly_a, 0, left, &mut poly_b);
        if poly_b.len() < 3 {
            continue;
        }
        clip_poly_edge(&poly_b, 1, right, &mut poly_a);
        if poly_a.len() < 3 {
            continue;
        }
        clip_poly_edge(&poly_a, 2, bottom, &mut poly_b);
        if poly_b.len() < 3 {
            continue;
        }
        clip_poly_edge(&poly_b, 3, top, &mut poly_a);
        if poly_a.len() < 3 {
            continue;
        }

        let base = poly_a[0];
        for i in 1..(poly_a.len() - 1) {
            push_rgb_vtx(&mut out, base);
            push_rgb_vtx(&mut out, poly_a[i]);
            push_rgb_vtx(&mut out, poly_a[i + 1]);
        }
    }

    out
}

#[inline]
fn convex_cross(o: Point<f32>, a: Point<f32>, b: Point<f32>) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

pub(crate) fn convex_hull_points(points: &[Point<f32>]) -> Vec<Point<f32>> {
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