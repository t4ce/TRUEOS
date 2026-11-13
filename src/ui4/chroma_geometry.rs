//! Pure point projection/easing, kept independent of GPU and display state.
pub(super) const ATLAS_WIDTH: usize = 128;
pub(super) const ATLAS_HEIGHT: usize = 64;

/// Physical Pipe A already applies A's cube to its input-colored dots. A B
/// preview instead supplies B's stored output; the physical A look still
/// affects the whole editor, and selecting B never replaces that look.
pub(super) fn node_color(input: u32, output: u32, selected_pipe: usize) -> [u8; 4] {
    let packed = if selected_pipe == 0 { input } else { output };
    let encode = |shift: u32| ((((packed >> shift) & 1023u32) * 255 + 511) / 1023) as u8;
    [encode(20), encode(10), encode(0), 255]
}

pub(super) fn atlas_uv(index: usize) -> [f32; 2] {
    [
        ((index % ATLAS_WIDTH) as f32 + 0.5) / ATLAS_WIDTH as f32,
        ((index / ATLAS_WIDTH) as f32 + 0.5) / ATLAS_HEIGHT as f32,
    ]
}

pub(super) struct Projection {
    width: f32,
    height: f32,
    cube_width: f32,
    scale: f32,
    /// sine/cosine of yaw followed by sine/cosine of pitch.
    rotation: [f32; 4],
}

impl Projection {
    pub(super) fn new(width: u32, height: u32, cube_width: u32, rotation: [f32; 4]) -> Self {
        Self {
            width: width as f32,
            height: height as f32,
            cube_width: cube_width as f32,
            scale: cube_width.min(height) as f32 * 0.58,
            rotation,
        }
    }

    /// Rebuild both position and original color address every time, since
    /// depth sorting permutes the preceding frame's vertex storage.
    pub(super) fn vertex(&self, index: usize, point: [f32; 3]) -> [f32; 5] {
        let [sy, cy, sp, cp] = self.rotation;
        let [x, y, z] = point.map(|channel| channel - 0.5);
        let rx = x * cy - z * sy;
        let rz = x * sy + z * cy;
        let ry = y * cp - rz * sp;
        let depth = y * sp + rz * cp;
        let perspective = 2.8 / (2.8 + depth);
        let px = self.cube_width * 0.50 + rx * self.scale * perspective;
        let py = self.height * 0.50 - ry * self.scale * perspective;
        let uv = atlas_uv(index);
        let mut vertex = [
            px * 2.0 / self.width - 1.0,
            1.0 - py * 2.0 / self.height,
            0.5 + depth * 0.25,
            uv[0],
            uv[1],
        ];
        if px < 2.0 || px > self.cube_width - 2.0 || py < 2.0 || py > self.height - 2.0 {
            // A whole-frame clip rectangle would otherwise include controls.
            vertex[0] = -2.0;
        }
        vertex
    }
}

pub(super) fn ease_node(position: &mut [f32; 3], target: [f32; 3], step: f32) -> bool {
    let mut moving = false;
    for axis in 0..3 {
        let delta = target[axis] - position[axis];
        if delta.abs() > 0.0001 {
            position[axis] += delta * step;
            moving = true;
        } else {
            position[axis] = target[axis];
        }
    }
    moving
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_colors_preview_b_outputs_without_double_applying_a() {
        let red = 1023 << 20;
        let gray = (512 << 20) | (512 << 10) | 512;
        assert_eq!(node_color(red, gray, 0), [255, 0, 0, 255]);
        assert_eq!(node_color(red, gray, 1), [128, 128, 128, 255]);
        assert_eq!(node_color(0x3fff_ffff, 0, 1), [0, 0, 0, 255]);
        assert_eq!(node_color(0, 0x3fff_ffff, 1), [255; 4]);
        for code in 0..=1023 {
            let expected = ((code * 255 + 511) / 1023) as u8;
            let packed = (code << 20) | (code << 10) | code;
            assert_eq!(
                node_color(0, packed, 1),
                [expected, expected, expected, 255]
            );
        }
    }

    #[test]
    fn rotated_nodes_keep_opaque_footprints_out_of_controls_at_supported_sizes() {
        for (width, height) in [(640, 360), (1920, 1080), (2560, 1440), (3840, 2160)] {
            let cube_width = width * 4 / 5;
            for yaw_step in -12..=12 {
                for pitch_step in -7..=7 {
                    let yaw = yaw_step as f32 * 0.3;
                    let pitch = pitch_step as f32 * 0.2;
                    let p = Projection::new(
                        width,
                        height,
                        cube_width,
                        [yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos()],
                    );
                    for r in 0..=4 {
                        for g in 0..=4 {
                            for b in 0..=4 {
                                let v =
                                    p.vertex(0, [r as f32 / 4.0, g as f32 / 4.0, b as f32 / 4.0]);
                                assert!(v.iter().all(|value| value.is_finite()));
                                assert!((0.0..=1.0).contains(&v[2]));
                                if v[0] == -2.0 {
                                    continue;
                                }
                                let px = (v[0] + 1.0) * width as f32 / 2.0;
                                let py = (1.0 - v[1]) * height as f32 / 2.0;
                                assert!(px >= 1.99 && px <= cube_width as f32 - 1.99);
                                assert!(py >= 1.99 && py <= height as f32 - 1.99);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn easing_finishes_and_stays_finished() {
        let target = [1.0, 0.0, 0.5];
        let mut position = [0.0, 1.0, 0.0];
        let mut frames = 0;
        while ease_node(&mut position, target, 0.21) {
            frames += 1;
            assert!(frames < 120);
        }
        assert_eq!(position, target);
        for _ in 0..10 {
            assert!(!ease_node(&mut position, target, 0.21));
        }
    }

    #[test]
    fn reprojection_restores_color_identity_after_sorting() {
        let mut vertices = (0..4913)
            .map(|index| {
                let projection = Projection::new(1920, 1080, 1536, [0.6, 0.8, 0.0, 1.0]);
                projection.vertex(index, [(index % 17) as f32 / 16.0, 0.5, 0.5])
            })
            .collect::<Vec<_>>();
        vertices.sort_unstable_by(|a, b| b[2].total_cmp(&a[2]));
        let projection = Projection::new(1920, 1080, 1536, [-0.6, 0.8, 0.0, 1.0]);
        for (index, vertex) in vertices.iter_mut().enumerate() {
            *vertex = projection.vertex(index, [0.5; 3]);
            assert_eq!([vertex[3], vertex[4]], atlas_uv(index));
        }
        let first = atlas_uv(0);
        let last = atlas_uv(vertices.len() - 1);
        assert!(first[0] > 0.0 && first[1] > 0.0);
        assert!(last[0] < 1.0 && last[1] < 1.0);
        assert!(vertices.len() <= ATLAS_WIDTH * ATLAS_HEIGHT);
    }
}
