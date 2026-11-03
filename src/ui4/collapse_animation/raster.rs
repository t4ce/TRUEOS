//! Pixel-only collapse helpers, independent of broker and display ownership.

pub(super) const DURATION_MS: u64 = 240;
pub(super) const ICON_SIZE: u32 = 64;
const ONE: u64 = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Geometry {
    pub width: u32,
    pub height: u32,
    pub snapshot_width: u32,
    pub snapshot_height: u32,
    eased: u64,
    pub icon_mix: u8,
}

impl Geometry {
    pub fn at(width: u32, height: u32, elapsed_ms: u64) -> Self {
        let progress = elapsed_ms.min(DURATION_MS) * ONE / DURATION_MS;
        let remaining = ONE - progress;
        let eased = ONE - remaining * remaining * remaining / (ONE * ONE);
        let longest = width.max(height).max(1);
        let edge = u64::from(longest) * (ONE - eased) + u64::from(ICON_SIZE) * eased;
        let scale = |extent: u32| {
            ((u64::from(extent) * edge + u64::from(longest) * ONE / 2)
                / (u64::from(longest) * ONE))
                .max(1) as u32
        };
        let snapshot_width = scale(width);
        let snapshot_height = scale(height);
        Self {
            width: snapshot_width.max(ICON_SIZE),
            height: snapshot_height.max(ICON_SIZE),
            snapshot_width,
            snapshot_height,
            eased,
            icon_mix: (progress.saturating_sub(640) * 255 / (ONE - 640)) as u8,
        }
    }

    /// Interpolate the center toward the live tile, then keep the backing on
    /// scanout. Dimensions describe an exact-sized backing, never a scaler.
    pub fn position(
        self,
        initial: (i32, i32, u32, u32),
        target_center_twice: (i64, i64),
        output: (u32, u32),
    ) -> (i32, i32) {
        let axis = |origin: i32, extent: u32, target: i64, current: u32, output: u32| {
            let initial = i64::from(origin) * 2 + i64::from(extent);
            let center = initial + (target - initial) * self.eased as i64 / ONE as i64;
            ((center - i64::from(current)) / 2)
                .clamp(0, i64::from(output.saturating_sub(current))) as i32
        };
        (
            axis(initial.0, initial.2, target_center_twice.0, self.width, output.0),
            axis(initial.1, initial.3, target_center_twice.1, self.height, output.1),
        )
    }
}

#[derive(Clone, Copy)]
pub(super) struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
    pub pixels: &'a [u8],
}

impl Image<'_> {
    pub fn sample(self, x: u32, y: u32, width: u32, height: u32) -> [u8; 4] {
        let sx = ((u64::from(x) * 2 + 1) * u64::from(self.width)
            / (u64::from(width) * 2)) as usize;
        let sy = ((u64::from(y) * 2 + 1) * u64::from(self.height)
            / (u64::from(height) * 2)) as usize;
        let offset = sy * self.pitch + sx * 4;
        self.pixels[offset..offset + 4].try_into().unwrap()
    }
}

pub(super) fn blend(snapshot: [u8; 4], icon: [u8; 4], mix: u8) -> [u8; 4] {
    core::array::from_fn(|channel| {
        ((u32::from(snapshot[channel]) * u32::from(255 - mix)
            + u32::from(icon[channel]) * u32::from(mix)
            + 127)
            / 255) as u8
    })
}

/// Blend premultiplied pixels. The captured frame keeps one aspect ratio,
/// while the final native-sized icon fades into the centered 64px canvas.
pub(super) fn render(
    snapshot: Image<'_>,
    icon: Image<'_>,
    geometry: Geometry,
    destination: &mut [u8],
    pitch: usize,
) {
    destination.fill(0);
    let source_left = (geometry.width - geometry.snapshot_width) / 2;
    let source_top = (geometry.height - geometry.snapshot_height) / 2;
    let icon_left = (geometry.width - ICON_SIZE) / 2;
    let icon_top = (geometry.height - ICON_SIZE) / 2;
    for y in 0..geometry.height {
        for x in 0..geometry.width {
            let source = if x >= source_left && y >= source_top
                && x - source_left < geometry.snapshot_width
                && y - source_top < geometry.snapshot_height
            {
                snapshot.sample(
                    x - source_left,
                    y - source_top,
                    geometry.snapshot_width,
                    geometry.snapshot_height,
                )
            } else {
                [0; 4]
            };
            let compact = if geometry.icon_mix != 0 && x >= icon_left && y >= icon_top
                && x - icon_left < ICON_SIZE && y - icon_top < ICON_SIZE
            {
                icon.sample(x - icon_left, y - icon_top, ICON_SIZE, ICON_SIZE)
            } else {
                [0; 4]
            };
            let offset = y as usize * pitch + x as usize * 4;
            destination[offset..offset + 4].copy_from_slice(&blend(source, compact, geometry.icon_mix));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangular_frames_shrink_uniformly_into_a_square_icon_canvas() {
        for (width, height) in [(960, 600), (600, 960), (1_920, 1_080), (160, 32)] {
            let mut previous = width.max(height);
            for elapsed in 0..=DURATION_MS {
                let geometry = Geometry::at(width, height, elapsed);
                assert!(geometry.snapshot_width.max(geometry.snapshot_height) <= previous);
                previous = geometry.snapshot_width.max(geometry.snapshot_height);
                let error = (i64::from(geometry.snapshot_width) * i64::from(height)
                    - i64::from(geometry.snapshot_height) * i64::from(width)).abs();
                assert!(error <= i64::from(width + height), "aspect ratio changed");
            }
            let end = Geometry::at(width, height, DURATION_MS);
            assert_eq!((end.width, end.height), (64, 64));
            assert_eq!(end.snapshot_width.max(end.snapshot_height), 64);
            assert_eq!(end.icon_mix, 255);
        }
    }

    #[test]
    fn the_final_center_tracks_the_current_cursor_tile_and_clips_at_edges() {
        let initial = (100, 200, 960, 600);
        assert_eq!(Geometry::at(960, 600, 0).position(initial, (1_000, 1_000), (1_920, 1_080)), (100, 200));
        let end = Geometry::at(960, 600, DURATION_MS);
        assert_eq!(end.position(initial, (1_000, 1_000), (1_920, 1_080)), (468, 468));
        assert_eq!(end.position(initial, (1_400, 1_600), (1_920, 1_080)), (668, 768));
        assert_eq!(end.position(initial, (0, 2_160), (1_920, 1_080)), (0, 1_016));
    }

    #[test]
    fn crossfade_preserves_premultiplied_alpha() {
        assert_eq!(blend([100, 0, 0, 128], [0, 50, 0, 64], 128), [50, 25, 0, 96]);
        for mix in 0..=255 {
            let pixel = blend([100, 0, 0, 128], [0, 50, 0, 64], mix);
            assert!(pixel[..3].iter().all(|channel| *channel <= pixel[3]));
        }
    }

    #[test]
    fn the_final_raster_is_exactly_the_icon_with_pitched_rows() {
        let snapshot = [200, 100, 0, 255];
        let icon = [0, 50, 0, 64, 0, 0, 30, 32];
        let geometry = Geometry::at(960, 600, DURATION_MS);
        let pitch = 64 * 4 + 16;
        let mut output = vec![0xAA; pitch * 64];
        render(
            Image { width: 1, height: 1, pitch: 4, pixels: &snapshot },
            Image { width: 2, height: 1, pitch: 8, pixels: &icon },
            geometry, &mut output, pitch,
        );
        for row in output.chunks_exact(pitch) {
            assert!(row[..32 * 4].chunks_exact(4).all(|pixel| pixel == &icon[..4]));
            assert!(row[32 * 4..64 * 4].chunks_exact(4).all(|pixel| pixel == &icon[4..]));
            assert!(row[64 * 4..].iter().all(|byte| *byte == 0));
        }
    }
}
