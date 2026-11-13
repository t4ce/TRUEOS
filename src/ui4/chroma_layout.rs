//! Pure layout and hit testing for the CHROMATIC editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    fn contains(self, x: i32, y: i32) -> bool {
        i64::from(x) >= i64::from(self.x)
            && i64::from(y) >= i64::from(self.y)
            && i64::from(x) < i64::from(self.x + self.width)
            && i64::from(y) < i64::from(self.y + self.height)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub width: u32,
    pub height: u32,
    pub cube_width: u32,
    pub controls_x: u32,
    pub controls_width: u32,
    pub curve: Rect,
    pub(super) margin: u32,
    pub(super) slider_start: u32,
    pub(super) row_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}
impl Layout {
    pub fn new(width: u32, height: u32) -> Result<Self, &'static str> {
        if width < 640 || height < 360 || width > 16384 || height > 16384 {
            return Err("chroma-control-resolution");
        }
        let controls_width = width / 5;
        let controls_x = width - controls_width;
        let margin = (controls_width / 16).clamp(6, 24);
        let track_width = controls_width - 2 * margin;
        let cell_height = (height / 50).clamp(11, 24).min(track_width * 11 / (14 * 6));
        let cell_width = (cell_height * 6 / 11).max(1);
        let curve = Rect {
            x: controls_x + margin,
            y: height / 7,
            width: track_width,
            height: height / 5,
        };
        let slider_start = curve.y + curve.height + height / 18;
        let row_height = (height - slider_start - height / 12) / 5;
        Ok(Self {
            width,
            height,
            cube_width: controls_x,
            controls_x,
            controls_width,
            curve,
            margin,
            slider_start,
            row_height,
            cell_width,
            cell_height,
        })
    }
    pub fn slider_track(&self, index: usize) -> Rect {
        Rect {
            x: self.controls_x + self.margin,
            y: self.slider_start + index as u32 * self.row_height + self.cell_height + 8,
            width: self.controls_width - 2 * self.margin,
            height: 4,
        }
    }
    pub fn slider_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..5).find(|&i| {
            let r = self.slider_track(i);
            Rect {
                x: r.x.saturating_sub(5),
                y: r.y.saturating_sub(10),
                width: r.width + 10,
                height: 24,
            }
            .contains(x, y)
        })
    }
    pub fn slider_value(&self, index: usize, x: i32) -> f32 {
        let r = self.slider_track(index);
        let t = ((x as f32 - r.x as f32) / (r.width - 1) as f32).clamp(0.0, 1.0);
        match index {
            1 | 2 => t * 2.0 - 1.0,
            3 => t * 1.8,
            _ => t,
        }
    }
    pub fn slider_bar(&self, index: usize, value: f32) -> Rect {
        let r = self.slider_track(index);
        let t = match index {
            1 | 2 => (value + 1.0) / 2.0,
            3 => value / 1.8,
            _ => value,
        }
        .clamp(0.0, 1.0);
        let bar_width = (self.controls_width / 40).clamp(3, 8);
        let x = r.x + ((r.width - bar_width) as f32 * t + 0.5) as u32;
        Rect {
            x,
            y: r.y.saturating_sub(8),
            width: bar_width,
            height: 20,
        }
    }
    pub fn curve_point(&self, index: usize, value: f32) -> (u32, u32) {
        (
            self.curve.x + (self.curve.width - 1) * index.min(6) as u32 / 6,
            self.curve.y
                + ((1.0 - value.clamp(-1.0, 1.0)) * 0.5 * (self.curve.height - 1) as f32 + 0.5)
                    as u32,
        )
    }
    pub fn curve_handle_at(&self, x: i32, y: i32, curve: &[f32; 7]) -> Option<usize> {
        let radius = (self.controls_width / 24).clamp(7, 14) as i64;
        (0..7).find(|&i| {
            let p = self.curve_point(i, curve[i]);
            let dx = i64::from(x) - i64::from(p.0);
            let dy = i64::from(y) - i64::from(p.1);
            dx * dx + dy * dy <= radius * radius
        })
    }
    pub fn curve_value(&self, y: i32) -> f32 {
        (1.0 - 2.0 * (y as f32 - self.curve.y as f32) / (self.curve.height - 1) as f32)
            .clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolution_limits_and_outside_hit_tests_are_explicit() {
        for (width, height) in [(0, 0), (639, 360), (640, 359), (16385, 1080), (1920, 16385)] {
            assert!(Layout::new(width, height).is_err());
        }
        let l = Layout::new(641, 361).unwrap();
        assert_eq!(l.controls_x + l.controls_width, 641);
        assert_eq!(l.slider_at(-1, -1), None);
        assert_eq!(l.slider_at(0, 180), None);
        assert_eq!(l.slider_at(640, 360), None);
        assert_eq!(l.curve_value(-1000), 1.0);
        assert_eq!(l.curve_value(10000), -1.0);
    }
    #[test]
    fn hue_handle_hit_tests_follow_the_current_curve() {
        let l = Layout::new(1920, 1080).unwrap();
        let curve = [1.0, 0.5, -0.5, -1.0, 0.0, 0.25, 1.0];
        for index in 0..7 {
            let p = l.curve_point(index, curve[index]);
            assert_eq!(
                l.curve_handle_at(p.0 as i32, p.1 as i32, &curve),
                Some(index)
            );
        }
        assert_eq!(l.curve_handle_at(0, 0, &curve), None);
    }
    #[test]
    fn sliders_and_curves_map_edges_without_leaving_frame() {
        for (w, h) in [(640, 360), (1920, 1080), (3840, 2160)] {
            let l = Layout::new(w, h).unwrap();
            assert_eq!(l.cube_width + l.controls_width, w);
            for i in 0..5 {
                let r = l.slider_track(i);
                assert_eq!(l.slider_at(r.x as i32, r.y as i32), Some(i));
                let min = if i == 1 || i == 2 { -1.0 } else { 0.0 };
                let max = if i == 3 { 1.8 } else { 1.0 };
                assert!((l.slider_value(i, r.x as i32) - min).abs() < 0.0001);
                assert!((l.slider_value(i, (r.x + r.width - 1) as i32) - max).abs() < 0.0001);
                let bar = l.slider_bar(i, max);
                assert!(bar.x + bar.width <= w && bar.y + bar.height <= h);
            }
            assert_eq!(l.curve_value(l.curve.y as i32), 1.0);
            assert_eq!(l.curve_value((l.curve.y + l.curve.height - 1) as i32), -1.0);
        }
    }
}
