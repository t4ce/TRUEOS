use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::f32::consts::PI;

use header::{FrameKind, FrameLens};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlapTarget {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowRange {
    pub start: usize,
    pub end: usize,
}

impl WindowRange {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug)]
pub struct Window {
    pub left: WindowRange,
    // left_slope_end == left.end
    left_slope_start: usize,
    pub right: WindowRange,
    // right_slope_start == right.start
    right_slope_end: usize,
    slope: Rc<Box<[f32]>>,
    pub overlap_target: OverlapTarget,
}

impl Window {
    fn new(left_len: usize, right_len: usize, slope: Rc<Box<[f32]>>) -> Self {
        let left_start = left_len / 2;
        let right_end = right_len / 2;
        let (left,
                left_slope_start,
                right,
                right_slope_end,
                overlap_target) = if left_len == right_len {
            // Long -> long or short -> short.
            (WindowRange {
                start: left_start,
                end: left_len,
            },
            left_start,
            WindowRange {
                start: 0,
                end: right_end,
            },
            right_end,
            OverlapTarget::Left)
        } else if left_len > right_len {
            // Long -> short.
            let left_point = left_len * 3 / 4;
            let right_point = right_len / 4;
            (WindowRange {
                start: left_start,
                end: left_point + right_point,
            },
            left_point - right_point,
            WindowRange {
                start: 0,
                end: right_end,
            },
            right_end,
            OverlapTarget::Left)
        } else {
            // Short -> long.
            let left_point = left_len / 4;
            let right_point = right_len / 4;
            (WindowRange {
                start: left_start,
                end: left_len,
            },
            left_start,
            WindowRange {
                start: right_point - left_point,
                end: right_end,
            },
            right_point + left_point,
            OverlapTarget::Right)
        };
        Window {
            left: left,
            left_slope_start: left_slope_start,
            right: right,
            right_slope_end: right_slope_end,
            slope: slope,
            overlap_target: overlap_target,
        }
    }

    pub fn len(&self) -> usize {
        match self.overlap_target {
            OverlapTarget::Left => self.left.len(),
            OverlapTarget::Right => self.right.len(),
        }
    }

    pub fn overlap(&self, left: &mut [f32], right: &mut [f32]) {
        let mut l_it = left[self.left_slope_start..self.left.end].iter_mut();
        let mut r_it = right[self.right.start..self.right_slope_end].iter_mut();
        let mut l_slope_it = self.slope.iter().rev();
        let mut r_slope_it = self.slope.iter();
        while let (Some(l), Some(r), Some(&l_slope), Some(&r_slope)) =
                (l_it.next(), r_it.next(), l_slope_it.next(), r_slope_it.next()) {
            let v = *l * l_slope + *r * r_slope;
            match self.overlap_target {
                OverlapTarget::Left => *l = v,
                OverlapTarget::Right => *r = v,
            }
        }
    }
}

#[derive(Debug)]
pub struct Windows {
    windows: [Window; 4],
}

impl Windows {
    pub fn new(frame_lens: FrameLens) -> Self {
        let short_slope = Rc::new(Self::make_slope(frame_lens.short() / 2));
        let long_slope = Rc::new(Self::make_slope(frame_lens.long() / 2));
        let windows = [
            Window::new(frame_lens.short(), frame_lens.short(), short_slope.clone()),
            Window::new(frame_lens.long(),  frame_lens.short(), short_slope.clone()),
            Window::new(frame_lens.short(), frame_lens.long(),  short_slope.clone()),
            Window::new(frame_lens.long(),  frame_lens.long(),  long_slope.clone()),
        ];
        Windows {
            windows: windows,
        }
    }

    pub fn get(&self, left_kind: FrameKind, right_kind: FrameKind) -> &Window {
        &self.windows[Self::window_idx(left_kind, right_kind)]
    }

    fn window_idx(left_kind: FrameKind, right_kind: FrameKind) -> usize {
        let l = left_kind as usize;
        let r = right_kind as usize;
        l | (r << 1)
    }

    fn make_slope(len: usize) -> Box<[f32]> {
        let mut r = Vec::with_capacity(len);
        let len = len as f32;
        for x in 0..r.capacity() {
            let inner = libm::sinf((x as f32 + 0.5_f32) / len * 0.5_f32 * PI);
            let y = libm::sinf(0.5_f32 * PI * inner * inner);
            r.push(y);
        }
        r.into_boxed_slice()
    }
}

#[cfg(test)]
mod tests {
    use header::{FrameKind, FrameLens};

    use super::*;

    #[test]
    fn windows() {
        let wins = Windows::new(FrameLens::new(512, 2048));

        let w = wins.get(FrameKind::Short, FrameKind::Short);
        assert_eq!(w.left, WindowRange { start: 256, end: 512 });
        assert_eq!(w.left_slope_start, 256);
        assert_eq!(w.right, WindowRange { start: 0, end: 256 });
        assert_eq!(w.right_slope_end, 256);
        assert_eq!(w.slope.len(), 256);
        assert_eq!(w.overlap_target, OverlapTarget::Left);

        let w = wins.get(FrameKind::Long, FrameKind::Long);
        assert_eq!(w.left, WindowRange { start: 1024, end: 2048 });
        assert_eq!(w.left_slope_start, 1024);
        assert_eq!(w.right, WindowRange { start: 0, end: 1024 });
        assert_eq!(w.right_slope_end, 1024);
        assert_eq!(w.slope.len(), 1024);
        assert_eq!(w.overlap_target, OverlapTarget::Left);

        let w = wins.get(FrameKind::Long, FrameKind::Short);
        assert_eq!(w.left, WindowRange { start: 1024, end: 1664 });
        assert_eq!(w.left_slope_start, 1408);
        assert_eq!(w.right, WindowRange { start: 0, end: 256 });
        assert_eq!(w.right_slope_end, 256);
        assert_eq!(w.slope.len(), 256);
        assert_eq!(w.overlap_target, OverlapTarget::Left);

        let w = wins.get(FrameKind::Short, FrameKind::Long);
        assert_eq!(w.left, WindowRange { start: 256, end: 512 });
        assert_eq!(w.left_slope_start, 256);
        assert_eq!(w.right, WindowRange { start: 384, end: 1024 });
        assert_eq!(w.right_slope_end, 640);
        assert_eq!(w.slope.len(), 256);
        assert_eq!(w.overlap_target, OverlapTarget::Right);
    }
}
