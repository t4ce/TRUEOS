#!/usr/bin/env python3
"""Check the real UI4 software overlay rasterizer on the host.

The kernel cannot link as a host test crate. Extract its production raster
functions and data types, stub only the unused GPU/pipe handles, and compare
their output with a scalar reference. Run from any directory with
``python3 tools/testpy/test_ui4_overlay_raster.py``; only rustc is required.
"""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


TESTS = r"""
#[cfg(test)]
mod raster_tests {
    use super::*;

    const WIDTH: u32 = 13;
    const HEIGHT: u32 = 11;
    const PITCH: usize = 17;

    fn initial_pixels() -> Vec<u32> {
        (0..PITCH * HEIGHT as usize)
            .map(|i| {
                if i % PITCH >= WIDTH as usize {
                    0xfeed_face // Destination row padding must stay untouched.
                } else {
                    let alpha = [0, 1, 127, 255][i % 4];
                    u32::from_le_bytes([alpha / 3, alpha / 2, alpha, alpha])
                }
            })
            .collect()
    }

    fn surface(pixels: &mut [u32]) -> OverlaySurface {
        OverlaySurface {
            width: WIDTH,
            height: HEIGHT,
            pitch_bytes: (PITCH * 4) as u32,
            byte_len: pixels.len() * 4,
            virt: pixels.as_mut_ptr().cast(),
            phys: 0,
            gpu: 0,
            pipe: PipeInfo,
            plane_slot: 0,
            buffer_index: 0,
        }
    }

    fn source_pixels(width: u32, height: u32) -> Vec<u8> {
        let pitch = (width as usize + 3) * 4;
        let mut pixels = vec![0xa5; pitch * height as usize];
        let colors = [
            [0, 0, 0, 0],
            [203, 17, 89, 255],
            [13, 79, 127, 127],
            [1, 0, 1, 1],
            [254, 88, 0, 254],
            [4, 2, 0, 7],
        ];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * pitch + x * 4;
                pixels[offset..offset + 4].copy_from_slice(&colors[(x + y * 3) % colors.len()]);
            }
        }
        pixels
    }

    fn tile<'a>(pixels: &'a [u8], source_width: u32, source_height: u32) -> RgbaOverlayTile<'a> {
        RgbaOverlayTile {
            x: 2,
            y: 1,
            width: source_width,
            height: source_height,
            source_width,
            source_height,
            pitch_bytes: (source_width as usize + 3) * 4,
            pixels,
            gpgpu_surface: None,
            gpgpu_scanout_cache: false,
            opacity: 255,
            arc: 0,
            expected_rgba: None,
        }
    }

    fn in_rect(x: u32, y: u32, rect: CompositionDamageRect) -> bool {
        x >= rect.x && y >= rect.y
            && x < rect.x.saturating_add(rect.width)
            && y < rect.y.saturating_add(rect.height)
    }

    // A geometric reference using pixel centers and the nearest point in the
    // inner rectangle, independent of the integer production arc predicate.
    fn rounded_contains(tile: &RgbaOverlayTile<'_>, x: u32, y: u32) -> bool {
        let diameter = u64::from(tile.width.min(tile.height)) * u64::from(tile.arc) / 1000;
        let radius = diameter as f64 / 2.0;
        let px = f64::from(x) + 0.5;
        let py = f64::from(y) + 0.5;
        let nearest_x = px.clamp(radius, f64::from(tile.width) - radius);
        let nearest_y = py.clamp(radius, f64::from(tile.height) - radius);
        (px - nearest_x).powi(2) + (py - nearest_y).powi(2) <= radius.powi(2)
    }

    fn reference_over(destination: u32, source: [u8; 4], opacity: u8) -> u32 {
        let scaled = source.map(|channel| (u32::from(channel) * u32::from(opacity) + 127) / 255);
        let destination = destination.to_le_bytes();
        let mut result = [0; 4];
        for channel in 0..4 {
            result[channel] = (scaled[channel]
                + (u32::from(destination[channel]) * (255 - scaled[3]) + 127) / 255)
                .min(255) as u8;
        }
        u32::from_le_bytes(result)
    }

    fn reference_tile(pixels: &mut [u32], tile: &RgbaOverlayTile<'_>, clip: CompositionDamageRect) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if !in_rect(x, y, clip)
                    || !in_rect(x, y, CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height))
                {
                    continue;
                }
                let local_x = x - tile.x;
                let local_y = y - tile.y;
                if !rounded_contains(tile, local_x, local_y) {
                    continue;
                }
                let source_x = u64::from(local_x) * u64::from(tile.source_width) / u64::from(tile.width);
                let source_y = u64::from(local_y) * u64::from(tile.source_height) / u64::from(tile.height);
                let offset = source_y as usize * tile.pitch_bytes + source_x as usize * 4;
                let source = tile.pixels[offset..offset + 4].try_into().unwrap();
                let index = y as usize * PITCH + x as usize;
                pixels[index] = reference_over(pixels[index], source, tile.opacity);
            }
        }
    }

    fn reference_rect(pixels: &mut [u32], rect: LiveOverlayRect, clip: CompositionDamageRect) {
        let source = [rect.color.r, rect.color.g, rect.color.b]
            .map(|channel| ((u32::from(channel) * u32::from(rect.color.a) + 127) / 255) as u8);
        let source = [source[0], source[1], source[2], rect.color.a];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if in_rect(x, y, clip)
                    && in_rect(x, y, CompositionDamageRect::new(rect.x, rect.y, rect.width, rect.height))
                {
                    let index = y as usize * PITCH + x as usize;
                    pixels[index] = reference_over(pixels[index], source, 255);
                }
            }
        }
    }

    #[test]
    fn sprites_match_reference_with_opacity_scaling_rounding_and_damage() {
        let clips = [
            CompositionDamageRect::FULL,
            CompositionDamageRect::new(3, 2, 5, 6),
            CompositionDamageRect::new(11, 8, 9, 8),
            CompositionDamageRect::new(40, 40, 3, 3),
            CompositionDamageRect::new(0, 0, 0, 8),
        ];
        for (source_width, source_height) in [(4, 3), (7, 6)] {
            let source = source_pixels(source_width, source_height);
            for (width, height) in [(source_width, source_height), (9, 8), (3, 2), (5, 9)] {
                for (x, y) in [(0, 0), (2, 1), (10, 9), (u32::MAX - 1, 0)] {
                    for opacity in [0, 1, 63, 127, 128, 254, 255] {
                        for arc in [0, 250, 500, 1000] {
                            for clip in clips {
                                let mut tile = tile(&source, source_width, source_height);
                                tile.x = x;
                                tile.y = y;
                                tile.width = width;
                                tile.height = height;
                                tile.opacity = opacity;
                                tile.arc = arc;
                                let mut actual = initial_pixels();
                                let mut expected = actual.clone();
                                reference_tile(&mut expected, &tile, clip);
                                assert_eq!(
                                    copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut actual), &tile, clip),
                                    Some((0, 0, 0)),
                                );
                                assert_eq!(actual, expected,
                                    "source={source_width}x{source_height} destination={width}x{height}@{x},{y} opacity={opacity} arc={arc} clip={clip:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn disjoint_damage_matches_single_full_raster() {
        let source = source_pixels(5, 4);
        let mut tile = tile(&source, 5, 4);
        tile.width = 12;
        tile.height = 10;
        tile.opacity = 137;
        tile.arc = 1000;
        let mut full = initial_pixels();
        let mut split = full.clone();
        copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut full), &tile, CompositionDamageRect::FULL).unwrap();
        for clip in [
            CompositionDamageRect::new(0, 0, 7, 5),
            CompositionDamageRect::new(7, 0, WIDTH - 7, 5),
            CompositionDamageRect::new(0, 5, 7, HEIGHT - 5),
            CompositionDamageRect::new(7, 5, WIDTH - 7, HEIGHT - 5),
        ] {
            copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut split), &tile, clip).unwrap();
        }
        assert_eq!(split, full);
    }

    #[test]
    fn overlapping_sprites_and_rectangles_preserve_layer_order() {
        let source = source_pixels(4, 3);
        let clip = CompositionDamageRect::new(1, 1, 11, 9);
        let mut actual = initial_pixels();
        let mut expected = actual.clone();
        for layer in 0..4 {
            let rect = LiveOverlayRect {
                x: layer + 1,
                y: 2,
                width: 7,
                height: 6,
                color: Rgba8 { r: 220, g: 31, b: 174, a: [0, 1, 127, 255][layer as usize] },
            };
            blend_overlay_rect_rgba_clipped(surface(&mut actual), rect, clip);
            reference_rect(&mut expected, rect, clip);
            let mut tile = tile(&source, 4, 3);
            tile.x = layer;
            tile.y = layer + 1;
            tile.width = 8;
            tile.height = 7;
            tile.opacity = [255, 128, 1, 0][layer as usize];
            tile.arc = 500;
            copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut actual), &tile, clip).unwrap();
            reference_tile(&mut expected, &tile, clip);
            assert_eq!(actual, expected, "after layer {layer}");
        }
    }

    #[test]
    fn invalid_sources_fail_before_writing_even_at_zero_opacity() {
        let source = source_pixels(4, 3);
        for opacity in [0, 255] {
            for invalid_case in 0..8 {
                let mut tile = tile(&source, 4, 3);
                tile.opacity = opacity;
                let mut pixels = initial_pixels();
                let before = pixels.clone();
                let mut surface = surface(&mut pixels);
                match invalid_case {
                    0 => surface.virt = core::ptr::null_mut(),
                    1 => tile.width = 0,
                    2 => tile.height = 0,
                    3 => tile.source_width = 0,
                    4 => tile.source_height = 0,
                    5 => tile.pitch_bytes = 15,
                    6 => tile.pixels = &source[..tile.pitch_bytes * 2 + 15],
                    7 => tile.pitch_bytes = usize::MAX,
                    _ => unreachable!(),
                }
                assert!(copy_premultiplied_rgba_tile_into_overlay_clipped(surface, &tile, CompositionDamageRect::FULL).is_none(),
                    "invalid case {invalid_case}, opacity {opacity}");
                assert_eq!(pixels, before);
            }
        }
    }

    #[test]
    fn final_source_row_needs_no_trailing_padding() {
        let source = source_pixels(4, 3);
        let mut tile = tile(&source, 4, 3);
        tile.pixels = &source[..tile.pitch_bytes * 2 + 16];
        let mut actual = initial_pixels();
        let mut expected = actual.clone();
        copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut actual), &tile, CompositionDamageRect::FULL).unwrap();
        reference_tile(&mut expected, &tile, CompositionDamageRect::FULL);
        assert_eq!(actual, expected);
    }

    #[test]
    fn explicit_source_diagnostics_still_report_mismatches() {
        let source = [24, 40, 60, 120].repeat(6);
        let mut tile = tile(&source, 3, 2);
        tile.pitch_bytes = 12;
        tile.expected_rgba = Some(Rgba8 { r: 24, g: 40, b: 60, a: 120 });
        let mut actual = initial_pixels();
        assert_eq!(copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut actual), &tile, CompositionDamageRect::FULL), Some((6, 0, 0)));
        tile.expected_rgba.as_mut().unwrap().r = 25;
        assert_eq!(copy_premultiplied_rgba_tile_into_overlay_clipped(surface(&mut actual), &tile, CompositionDamageRect::new(3, 1, 2, 1)), Some((2, 2, 0)));
    }
}
"""


def harness_source() -> str:
    display = "src/intel/display.rs"
    declarations = [item("crates/trueos-graphics/primitives.rs", "Rgba8")]
    declarations.extend(
        item(display, name)
        for name in (
            "OverlaySurface",
            "RgbaOverlayTile",
            "LiveOverlayRect",
            "copy_premultiplied_rgba_tile_into_overlay_clipped",
            "tile_source_coordinate",
            "apply_tile_opacity",
            "premul_u8",
            "overlay_scanout_pixel_rgba_premul",
            "fill_overlay_rect",
            "blend_overlay_rect_rgba_clipped",
            "clip_composition_damage",
            "intersect_composition_damage",
        )
    )
    damage = (ROOT / "src/ui4/damage.rs").as_posix()
    arc = (ROOT / "src/ui4/window_arc.rs").as_posix()
    return (
        "#![allow(dead_code, unfulfilled_lint_expectations)]\n"
        "#[derive(Copy, Clone)] struct PipeInfo;\n"
        "mod intel { pub mod gpgpu { pub struct GpgpuRgba8Surface; } }\n"
        "mod ui4 {\n"
        f'    #[path = "{damage}"] pub mod damage;\n'
        f'    #[path = "{arc}"] pub mod window_arc;\n'
        "}\n"
        "type CompositionDamageRect = ui4::damage::DamageRect;\n"
        + "\n".join(declarations)
        + TESTS
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-overlay-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "overlay_raster.rs"
        binary = directory / "overlay_raster_tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "-O", str(source), "-o", str(binary)],
            cwd=directory,
            check=True,
        )
        subprocess.run([str(binary)], cwd=directory, check=True)


if __name__ == "__main__":
    main()
