//! Bounded rectangular damage shared by the UI4 broker and compositor.
//!
//! Regions remain disjoint. This matters for premultiplied-alpha composition:
//! processing an overlapping pixel twice would blend it twice. Capacity is
//! deliberately fixed so a producer cannot turn damage tracking into an
//! unbounded kernel allocation.

pub(crate) const DAMAGE_REGION_CAPACITY: usize = 16;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DamageRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DamageRect {
    pub(crate) const FULL: Self = Self {
        x: 0,
        y: 0,
        width: u32::MAX,
        height: u32::MAX,
    };

    pub(crate) const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn valid(self) -> bool {
        self.width != 0 && self.height != 0
    }

    pub(crate) fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .max(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .max(other.y.saturating_add(other.height));
        Self::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    pub(crate) fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self
            .x
            .saturating_add(self.width)
            .min(other.x.saturating_add(other.width));
        let bottom = self
            .y
            .saturating_add(self.height)
            .min(other.y.saturating_add(other.height));
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }

    fn area(self) -> u128 {
        u128::from(self.width) * u128::from(self.height)
    }

    fn overlaps(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    fn merges_without_inflation(self, other: Self) -> bool {
        self.union(other).area() == self.area().saturating_add(other.area())
    }

    fn should_merge(self, other: Self) -> bool {
        self.overlaps(other) || self.merges_without_inflation(other)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DamageRegion {
    rects: [DamageRect; DAMAGE_REGION_CAPACITY],
    len: u8,
}

impl DamageRegion {
    pub(crate) const EMPTY: Self = Self {
        rects: [DamageRect::new(0, 0, 0, 0); DAMAGE_REGION_CAPACITY],
        len: 0,
    };
    pub(crate) const FULL: Self = {
        let mut rects = [DamageRect::new(0, 0, 0, 0); DAMAGE_REGION_CAPACITY];
        rects[0] = DamageRect::FULL;
        Self { rects, len: 1 }
    };

    pub(crate) fn from_rect(rect: DamageRect) -> Self {
        let mut region = Self::EMPTY;
        region.add(rect);
        region
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }

    pub(crate) fn rects(&self) -> &[DamageRect] {
        &self.rects[..self.len()]
    }

    pub(crate) fn add_region(&mut self, other: Self) {
        for rect in other.rects() {
            self.add(*rect);
        }
    }

    /// Add one rectangle while retaining a disjoint, bounded representation.
    ///
    /// Overlap and rectangular, zero-inflation adjacency are normalized first.
    /// At capacity the pair with the smallest bounding-union area inflation is
    /// coalesced. The operation is conservative: it may overdraw, never omit.
    pub(crate) fn add(&mut self, mut rect: DamageRect) {
        if !rect.valid() {
            return;
        }
        if rect == DamageRect::FULL {
            *self = Self::EMPTY;
            self.push_unchecked(rect);
            return;
        }
        if self.rects().contains(&DamageRect::FULL) {
            return;
        }

        let mut index = 0;
        while index < self.len() {
            if rect.should_merge(self.rects[index]) {
                rect = rect.union(self.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        if self.len() < DAMAGE_REGION_CAPACITY {
            self.push_unchecked(rect);
            return;
        }

        let incoming = self.len();
        let mut best = (0usize, 1usize, u128::MAX);
        for left in 0..=incoming {
            for right in left + 1..=incoming {
                let a = if left == incoming {
                    rect
                } else {
                    self.rects[left]
                };
                let b = if right == incoming {
                    rect
                } else {
                    self.rects[right]
                };
                let inflation = a
                    .union(b)
                    .area()
                    .saturating_sub(a.area().saturating_add(b.area()));
                if inflation < best.2 {
                    best = (left, right, inflation);
                }
            }
        }

        let (left, right, _) = best;
        if right == incoming {
            let merged = self.remove(left).union(rect);
            self.add(merged);
        } else {
            let b = self.remove(right);
            let a = self.remove(left);
            self.add(a.union(b));
            self.add(rect);
        }
    }

    pub(crate) fn clipped(self, bounds: DamageRect) -> Self {
        let mut clipped = Self::EMPTY;
        for rect in self.rects() {
            if let Some(rect) = rect.intersection(bounds) {
                clipped.add(rect);
            }
        }
        clipped
    }

    pub(crate) fn bounding_rect(self) -> Option<DamageRect> {
        self.rects().iter().copied().reduce(DamageRect::union)
    }

    fn push_unchecked(&mut self, rect: DamageRect) {
        let index = self.len();
        self.rects[index] = rect;
        self.len += 1;
    }

    fn remove(&mut self, index: usize) -> DamageRect {
        let removed = self.rects[index];
        let old_len = self.len();
        for slot in index..old_len.saturating_sub(1) {
            self.rects[slot] = self.rects[slot + 1];
        }
        self.len -= 1;
        self.rects[self.len()] = DamageRect::new(0, 0, 0, 0);
        removed
    }
}

impl Default for DamageRegion {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_and_zero_inflation_adjacency_are_normalized() {
        let mut region = DamageRegion::EMPTY;
        region.add(DamageRect::new(0, 0, 10, 10));
        region.add(DamageRect::new(8, 0, 4, 10));
        region.add(DamageRect::new(12, 0, 3, 10));
        assert_eq!(region.rects(), &[DamageRect::new(0, 0, 15, 10)]);
    }

    #[test]
    fn separated_rectangles_remain_separate() {
        let mut region = DamageRegion::EMPTY;
        region.add(DamageRect::new(0, 0, 4, 4));
        region.add(DamageRect::new(100, 100, 4, 4));
        assert_eq!(region.len(), 2);
    }

    #[test]
    fn capacity_coalesces_the_least_inflating_pair() {
        let mut region = DamageRegion::EMPTY;
        for index in 0..DAMAGE_REGION_CAPACITY {
            region.add(DamageRect::new(index as u32 * 10, 0, 1, 1));
        }
        region.add(DamageRect::new(152, 0, 1, 1));
        assert_eq!(region.len(), DAMAGE_REGION_CAPACITY);
        assert!(region.rects().contains(&DamageRect::new(150, 0, 3, 1)));
    }

    #[test]
    fn clipping_preserves_disjoint_damage() {
        let mut region = DamageRegion::EMPTY;
        region.add(DamageRect::new(0, 0, 10, 10));
        region.add(DamageRect::new(20, 20, 10, 10));
        let clipped = region.clipped(DamageRect::new(5, 5, 20, 20));
        assert_eq!(clipped.rects(), &[DamageRect::new(5, 5, 5, 5), DamageRect::new(20, 20, 5, 5)]);
    }
}
