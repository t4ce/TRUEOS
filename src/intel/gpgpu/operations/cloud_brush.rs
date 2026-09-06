// Shared normalized brush history for the High Wisps shader.
pub(crate) struct CppCloudBrush {
    pub(crate) points: [u32; crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY],
    pub(crate) count: usize,
    next: usize,
    pub(crate) last: Option<(i32, i32)>,
}

impl CppCloudBrush {
    pub(crate) const fn new() -> Self {
        Self {
            points: [0; crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY],
            count: 0,
            next: 0,
            last: None,
        }
    }

    fn push(&mut self, local_x: i32, local_y: i32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let x = local_x.clamp(0, width.saturating_sub(1) as i32) as u32;
        let y = local_y.clamp(0, height.saturating_sub(1) as i32) as u32;
        let packed_x = x.saturating_mul(u16::MAX as u32) / width.saturating_sub(1).max(1);
        let packed_y = y.saturating_mul(u16::MAX as u32) / height.saturating_sub(1).max(1);
        self.points[self.next] = packed_x | (packed_y << 16);
        self.next = (self.next + 1) % self.points.len();
        self.count = self.count.saturating_add(1).min(self.points.len());
    }

    pub(crate) fn drag_to(&mut self, local_x: i32, local_y: i32, width: u32, height: u32) {
        let Some((from_x, from_y)) = self.last else {
            self.push(local_x, local_y, width, height);
            self.last = Some((local_x, local_y));
            return;
        };
        let dx = local_x.saturating_sub(from_x);
        let dy = local_y.saturating_sub(from_y);
        let distance = dx.unsigned_abs().max(dy.unsigned_abs());
        let spacing = width.min(height).saturating_div(24).max(1);
        let steps = distance
            .div_ceil(spacing)
            .max(1)
            .min(self.points.len() as u32);
        for step in 1..=steps {
            let x = i64::from(from_x) + i64::from(dx) * i64::from(step) / i64::from(steps);
            let y = i64::from(from_y) + i64::from(dy) * i64::from(step) / i64::from(steps);
            self.push(x as i32, y as i32, width, height);
        }
        self.last = Some((local_x, local_y));
    }
}
