//! Pure, bounded geometry for the interaction plane's button feedback.
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        i64::from(x) >= i64::from(self.x)
            && i64::from(y) >= i64::from(self.y)
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let b = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));
        (r > i64::from(x) && b > i64::from(y)).then(|| Self {
            x,
            y,
            width: (r - i64::from(x)) as u32,
            height: (b - i64::from(y)) as u32,
        })
    }
}

/// Pick the endpoint with the larger luminance distance; avoid inversion's
/// near-zero contrast around midgray. Alpha is fixed, independent of CSS alpha.
pub fn contrast(border: [u8; 4]) -> [u8; 4] {
    let luma = 299 * u32::from(border[0]) + 587 * u32::from(border[1]) + 114 * u32::from(border[2]);
    let value = if luma < 127_500 { 255 } else { 0 };
    [value, value, value, 128]
}

/// Disjoint strips: corners never receive two 50%-alpha overlays. The outline
/// is three pixels wide and follows the original rectangle, not its clip.
/// Eight-pixel dashes advance one perimeter pixel per 15 Hz tick.
pub fn outline(rect: Rect, pressed: bool, phase: u32) -> Vec<Rect> {
    let mut out = Vec::new();
    if rect.width == 0 || rect.height == 0 || rect.width > 8192 || rect.height > 8192 {
        return out;
    }
    let t = 3u32.min(rect.width).min(rect.height);
    if rect.width <= 2 * t || rect.height <= 2 * t {
        out.push(rect);
        return out;
    }
    let mut distance = 0u32;
    let sides = [
        (
            Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: t,
            },
            true,
            false,
        ),
        (
            Rect {
                x: rect.x + (rect.width - t) as i32,
                y: rect.y + t as i32,
                width: t,
                height: rect.height - t,
            },
            false,
            false,
        ),
        (
            Rect {
                x: rect.x,
                y: rect.y + (rect.height - t) as i32,
                width: rect.width - t,
                height: t,
            },
            true,
            true,
        ),
        (
            Rect {
                x: rect.x,
                y: rect.y + t as i32,
                width: t,
                height: rect.height.saturating_sub(2 * t),
            },
            false,
            true,
        ),
    ];
    for (side, horizontal, reverse) in sides {
        if side.width == 0 || side.height == 0 {
            continue;
        }
        let len = if horizontal { side.width } else { side.height };
        if pressed {
            out.push(side);
        } else {
            let mut pos = 0;
            while pos < len {
                let pattern = (distance + pos + phase % 16) % 16;
                let run = (if pattern < 8 {
                    8 - pattern
                } else {
                    16 - pattern
                })
                .min(len - pos);
                if pattern < 8 {
                    let offset = if reverse { len - pos - run } else { pos };
                    out.push(if horizontal {
                        Rect {
                            x: side.x + offset as i32,
                            width: run,
                            ..side
                        }
                    } else {
                        Rect {
                            y: side.y + offset as i32,
                            height: run,
                            ..side
                        }
                    });
                }
                pos += run;
            }
        }
        distance += len;
    }
    out
}

/// Subtract an opaque window rectangle into at most four disjoint pieces.
pub fn subtract(rect: Rect, cover: Rect) -> Vec<Rect> {
    let Some(hit) = rect.intersection(cover) else {
        return alloc::vec![rect];
    };
    let right = rect.x + rect.width as i32;
    let bottom = rect.y + rect.height as i32;
    let hr = hit.x + hit.width as i32;
    let hb = hit.y + hit.height as i32;
    let mut out = Vec::with_capacity(4);
    if hit.y > rect.y {
        out.push(Rect {
            height: (hit.y - rect.y) as u32,
            ..rect
        });
    }
    if hb < bottom {
        out.push(Rect {
            y: hb,
            height: (bottom - hb) as u32,
            ..rect
        });
    }
    if hit.x > rect.x {
        out.push(Rect {
            x: rect.x,
            y: hit.y,
            width: (hit.x - rect.x) as u32,
            height: hit.height,
        });
    }
    if hr < right {
        out.push(Rect {
            x: hr,
            y: hit.y,
            width: (right - hr) as u32,
            height: hit.height,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    fn pixels(rects: &[Rect]) -> BTreeSet<(i32, i32)> {
        let mut out = BTreeSet::new();
        for r in rects {
            for y in r.y..r.y + r.height as i32 {
                for x in r.x..r.x + r.width as i32 {
                    assert!(out.insert((x, y)), "alpha would stack");
                }
            }
        }
        out
    }
    #[test]
    fn contrast_is_visible_for_black_white_and_midgray() {
        assert_eq!(contrast([0, 0, 0, 255]), [255, 255, 255, 128]);
        assert_eq!(contrast([255; 4]), [0, 0, 0, 128]);
        assert_eq!(contrast([128; 4]), [0, 0, 0, 128]);
        assert_eq!(contrast([120; 4]), [255, 255, 255, 128]);
    }
    #[test]
    fn pressed_is_complete_with_no_alpha_overlap_even_for_tiny_buttons() {
        for w in 1..35 {
            for h in 1..20 {
                let rect = Rect {
                    x: 4,
                    y: 7,
                    width: w,
                    height: h,
                };
                let p = pixels(&outline(rect, true, 0));
                let t = 3.min(w).min(h);
                for y in 0..h {
                    for x in 0..w {
                        assert_eq!(
                            p.contains(&(4 + x as i32, 7 + y as i32)),
                            x < t || x >= w - t || y < t || y >= h - t
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn dashes_move_without_changing_bounds_and_wrap_after_sixteen_ticks() {
        let r = Rect {
            x: 9,
            y: 6,
            width: 80,
            height: 24,
        };
        let full = pixels(&outline(r, true, 0));
        let a = pixels(&outline(r, false, 0));
        let b = pixels(&outline(r, false, 1));
        assert_ne!(a, b);
        assert!(a.is_subset(&full) && b.is_subset(&full));
        assert_eq!(a, pixels(&outline(r, false, 16)));
    }
    #[test]
    fn clipping_does_not_invent_an_edge_and_subtraction_preserves_visible_pixels() {
        let r = Rect {
            x: -10,
            y: 0,
            width: 40,
            height: 20,
        };
        let clip = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let clipped: Vec<_> = outline(r, true, 0)
            .into_iter()
            .filter_map(|r| r.intersection(clip))
            .collect();
        let p = pixels(&clipped);
        assert!(!p.contains(&(0, 10)));
        let cover = Rect {
            x: 4,
            y: 3,
            width: 12,
            height: 9,
        };
        let base = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 20,
        };
        let remainder = pixels(&subtract(base, cover));
        for y in 0..20 {
            for x in 0..30 {
                assert_eq!(remainder.contains(&(x, y)), !cover.contains(x, y));
            }
        }
    }
}
