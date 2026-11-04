#!/usr/bin/env python3
"""Check production Edit capture, font tint, and its menu pause handoff."""

import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


CAPTURE_TESTS = r'''
#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::{CAPTURES, PAUSES, PUBLISHES, REMOVES, SOURCE, SOURCE_READS, VISITS};
    fn surface(width: u32) -> Ui4Surface {
        Ui4Surface {
            width,
            height: 1,
            scale: 2,
            frame: (),
            window: crate::ui4::WindowId,
            edit_life: None,
            edit_life_page: None,
        }
    }
    fn settings(width: u32) -> EditLife {
        EditLife {
            enabled: true,
            active: true,
            viewport: CellRect {
                x: 0,
                y: 0,
                width,
                height: 1,
            },
            scroll_revision: 0,
            emission_revision: 1,
            exclusions: Vec::new(),
        }
    }
    fn source(width: usize) {
        SOURCE.with(|s| *s.borrow_mut() = [17u8, 19, 23, 255].repeat(width));
    }
    fn work() -> (usize, usize, usize, usize) {
        (
            CAPTURES.with(|v| v.get()),
            SOURCE_READS.with(|v| v.get()),
            VISITS.with(|v| v.get()),
            PUBLISHES.with(|v| v.get()),
        )
    }
    fn ink() -> [(char, Option<crate::RgbaColor>); 1] {
        [('A', Some(crate::FOREGROUND))]
    }
    #[test]
    fn present_pauses_before_capture_and_resumes_clean_page_without_discarding_retained_input() {
        let mut surface = surface(256);
        let mut settings = settings(1);
        source(256);
        present(&mut surface, &[&ink()], Some(&settings), true);
        assert_eq!(work(), (1, 1, 1, 1));
        let retained = Arc::clone(surface.edit_life_page.as_ref().unwrap());

        // A menu redraw must still use the terminal source; none of its pixels
        // or glyphs may become a new immutable editor page.
        settings.active = false;
        SOURCE.with(|s| s.borrow_mut().fill(93));
        let menu = [('M', Some(crate::FOREGROUND))];
        present(&mut surface, &[&menu], Some(&settings), true);
        assert_eq!(work(), (1, 1, 1, 1));
        assert_eq!(PAUSES.with(|v| v.get()), 1);
        assert_eq!(REMOVES.with(|v| v.get()), 0);
        assert!(Arc::ptr_eq(&retained, surface.edit_life_page.as_ref().unwrap()));
        assert_eq!(surface.edit_life.as_ref(), Some(&settings));

        // Repaints within the menu also bypass source reads and the atlas.
        present(&mut surface, &[&menu], Some(&settings), true);
        assert_eq!(work(), (1, 1, 1, 1));
        assert!(Arc::ptr_eq(&retained, surface.edit_life_page.as_ref().unwrap()));

        // Closing a menu restores the document cells before publishing its
        // clean page. Even a metadata-only resume must capture once.
        settings.active = true;
        source(256);
        present(&mut surface, &[&ink()], Some(&settings), false);
        assert_eq!(work(), (2, 2, 2, 2));
        let resumed = surface.edit_life_page.as_ref().unwrap();
        assert!(!Arc::ptr_eq(&retained, resumed));
        assert_eq!(resumed.rgba, retained.rgba);
        assert_eq!(resumed.mask, retained.mask);
        assert_eq!(resumed.colors, retained.colors);
        assert_eq!(resumed.scroll_revision, retained.scroll_revision);
        assert_eq!(resumed.emission_revision, retained.emission_revision);
        present(&mut surface, &[&ink()], Some(&settings), false);
        assert_eq!(work(), (2, 2, 2, 2), "unchanged resumed page is not recaptured");

        settings.enabled = false;
        present(&mut surface, &[&ink()], Some(&settings), true);
        assert_eq!(work(), (2, 2, 2, 2));
        assert_eq!(REMOVES.with(|v| v.get()), 1);
        assert!(surface.edit_life_page.is_none());
        assert!(surface.edit_life.is_none());
    }
    #[test]
    fn present_with_any_popup_exclusion_pauses_without_initial_capture() {
        let mut surface = surface(256);
        let mut settings = settings(1);
        settings.exclusions.push(settings.viewport);
        source(256);
        present(&mut surface, &[&ink()], Some(&settings), true);
        assert_eq!(work(), (0, 0, 0, 0));
        assert_eq!(PAUSES.with(|v| v.get()), 1);
        assert!(surface.edit_life_page.is_none());
        assert_eq!(surface.edit_life.as_ref(), Some(&settings));

        settings.exclusions.clear();
        settings.active = false;
        present(&mut surface, &[&ink()], Some(&settings), true);
        assert_eq!(work(), (0, 0, 0, 0));
        assert_eq!(PAUSES.with(|v| v.get()), 2);

        settings.enabled = false;
        present(&mut surface, &[&ink()], Some(&settings), false);
        assert_eq!(work(), (0, 0, 0, 0));
        assert_eq!(REMOVES.with(|v| v.get()), 1);
        assert!(surface.edit_life.is_none());
        assert!(surface.edit_life_page.is_none());
    }
    #[test]
    fn paused_toggle_off_releases_the_retained_page() {
        let mut surface = surface(256);
        let mut settings = settings(1);
        source(256);
        present(&mut surface, &[&ink()], Some(&settings), true);
        let retained = Arc::downgrade(surface.edit_life_page.as_ref().unwrap());
        settings.active = false;
        present(&mut surface, &[&ink()], Some(&settings), true);
        assert!(retained.upgrade().is_some());
        settings.enabled = false;
        present(&mut surface, &[&ink()], Some(&settings), false);
        assert!(retained.upgrade().is_none());
        assert_eq!(work(), (1, 1, 1, 1));
        assert_eq!(REMOVES.with(|v| v.get()), 1);
    }
    #[test]
    fn captured_transparent_ink_matches_production_font_tint_for_every_alpha_and_coverage() {
        let surface = surface(256);
        source(256);
        for alpha in 0..=255u8 {
            let fg = [17, 129, 251, alpha];
            let line = [(
                'A',
                Some(crate::RgbaColor {
                    foreground: fg,
                    background: [0, 0, 0, 255],
                    underline: false,
                }),
            )];
            let page = capture(&surface, &[&line], &settings(1)).unwrap();
            for coverage in 0..=255u8 {
                let at = usize::from(coverage) * 4;
                assert_eq!(
                    &page.rgba[at..at + 4],
                    &crate::font_tint(coverage, fg, [0; 4]),
                    "coverage={coverage} alpha={alpha}"
                );
                assert_eq!(page.mask[usize::from(coverage)], coverage);
                assert_eq!(&page.colors[at..at + 4], &[fg[0], fg[1], fg[2], coverage]);
            }
        }
    }
    #[test]
    fn blank_capture_skips_atlas_preserves_opaque_exclusion_and_styled_background() {
        let surface = surface(768);
        source(768);
        VISITS.with(|v| v.set(0));
        let normal = crate::RgbaColor {
            foreground: [255; 4],
            background: [0, 0, 0, 255],
            underline: false,
        };
        let styled = crate::RgbaColor {
            background: [44, 44, 44, 255],
            ..normal
        };
        let line = [
            (' ', Some(normal)),
            (' ', Some(normal)),
            (' ', Some(styled)),
        ];
        let mut settings = settings(3);
        settings.exclusions.push(CellRect {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        });
        let page = capture(&surface, &[&line], &settings).unwrap();
        assert_eq!(VISITS.with(|v| v.get()), 0);
        assert!(page.rgba[..256 * 4].iter().all(|&v| v == 0));
        assert_eq!(&page.rgba[256 * 4..], &[17u8, 19, 23, 255].repeat(512));
        assert!(page.mask.iter().all(|&v| v == 0));
        assert!(page.colors.iter().all(|&v| v == 0));
    }
    #[test]
    fn underline_space_keeps_coverage_visit() {
        let surface = surface(256);
        source(256);
        VISITS.with(|v| v.set(0));
        let line = [(
            ' ',
            Some(crate::RgbaColor {
                foreground: [255; 4],
                background: [0, 0, 0, 255],
                underline: true,
            }),
        )];
        let page = capture(&surface, &[&line], &settings(1)).unwrap();
        assert_eq!(VISITS.with(|v| v.get()), 1);
        assert_eq!(page.mask[255], 255);
    }
    #[test]
    fn blank_span_clear_clips_viewport_and_overlapping_unsorted_exclusions() {
        let viewport = PixelRect {
            x: 1,
            y: 1,
            width: 6,
            height: 3,
        };
        let exclusions = [
            PixelRect {
                x: 4,
                y: 2,
                width: 2,
                height: 1,
            },
            PixelRect {
                x: 2,
                y: 1,
                width: 3,
                height: 2,
            },
            PixelRect {
                x: 4,
                y: 2,
                width: 3,
                height: 2,
            },
        ];
        let mut rgba = [17u8, 19, 23, 255].repeat(9 * 5);
        clear_cell(&mut rgba, 9, viewport, &exclusions, 0, 0, 9, 5);
        for y in 0..5 {
            for x in 0..9 {
                let cleared =
                    contains(viewport, x, y) && !exclusions.iter().any(|&r| contains(r, x, y));
                let at = (y * 9 + x) as usize * 4;
                assert_eq!(
                    &rgba[at..at + 4],
                    if cleared { &[0; 4] } else { &[17, 19, 23, 255] },
                    "x={x} y={y}"
                );
            }
        }
    }
}
'''

HARNESS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as trueos_terminal;
extern crate self as trueos_time;
use std::cell::{Cell, RefCell};
thread_local! {
    static SOURCE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static VISITS: Cell<usize> = const { Cell::new(0) };
    static CAPTURES: Cell<usize> = const { Cell::new(0) };
    static SOURCE_READS: Cell<usize> = const { Cell::new(0) };
    static PAUSES: Cell<usize> = const { Cell::new(0) };
    static PUBLISHES: Cell<usize> = const { Cell::new(0) };
    static REMOVES: Cell<usize> = const { Cell::new(0) };
}
pub struct Instant;
impl Instant {
    pub fn now() -> Self {
        Self
    }
    pub fn as_millis(&self) -> u64 {
        1000
    }
}
#[macro_export]
macro_rules! log_info {
    ($($t:tt)*) => {};
}
#[macro_export]
macro_rules! log_warn {
    ($($t:tt)*) => {};
}
#[path = "@TERMINAL_PATH@"]
pub mod edit_life;
#[derive(Clone, Copy)]
pub struct RgbaColor {
    foreground: [u8; 4],
    background: [u8; 4],
    underline: bool,
}
impl RgbaColor {
    fn rgba(self) -> [u8; 4] {
        self.foreground
    }
    fn background(self) -> Option<[u8; 4]> {
        Some(self.background)
    }
    fn underline(self) -> bool {
        self.underline
    }
}
const FOREGROUND: RgbaColor = RgbaColor {
    foreground: [255; 4],
    background: [0, 0, 0, 255],
    underline: false,
};
const BACKGROUND: [u8; 4] = [0, 0, 0, 255];
struct Ui4Surface {
    frame: (),
    window: ui4::WindowId,
    width: u32,
    height: u32,
    scale: u32,
    edit_life: Option<edit_life::EditLife>,
    edit_life_page: Option<alloc::sync::Arc<ui4::edit_life::Page>>,
}
mod ui4 {
    #[derive(Clone, Copy)]
    pub struct WindowId;
    impl WindowId {
        pub fn raw(self) -> u64 {
            0
        }
    }
    pub struct View {
        pub virt: *const u8,
        pub pitch: u32,
    }
    pub fn acquire_published_frame(_: ()) -> Result<(), ()> {
        Ok(())
    }
    pub fn release_published_frame(_: ()) -> Result<(), ()> {
        Ok(())
    }
    pub fn published_rgba_view(_: ()) -> Result<View, ()> {
        crate::SOURCE_READS.with(|v| v.set(v.get() + 1));
        crate::SOURCE.with(|s| {
            let s = s.borrow();
            Ok(View {
                virt: s.as_ptr(),
                pitch: s.len() as u32,
            })
        })
    }
    pub mod edit_life {
        pub const MAX_PAGE_PIXELS: usize = 4096 * 2160;
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub struct PixelRect {
            pub x: u32,
            pub y: u32,
            pub width: u32,
            pub height: u32,
        }
        pub struct Page {
            pub width: u32,
            pub height: u32,
            pub rgba: Vec<u8>,
            pub mask: Vec<u8>,
            pub colors: Vec<u8>,
            pub viewport: PixelRect,
            pub scroll_revision: u64,
            pub emission_revision: u64,
        }
        pub fn remove(_: super::WindowId) {
            crate::REMOVES.with(|v| v.set(v.get() + 1));
        }
        pub fn pause(_: super::WindowId, _: ()) {
            crate::PAUSES.with(|v| v.set(v.get() + 1));
        }
        pub fn publish_page(_: super::WindowId, _: (), _: alloc::sync::Arc<Page>) {
            crate::PUBLISHES.with(|v| v.set(v.get() + 1));
        }
    }
}
mod r {
    pub mod services {
        pub mod shell3_font_service {
            pub fn cell_extent(_: u32) -> Option<(u32, u32)> {
                crate::CAPTURES.with(|v| v.set(v.get() + 1));
                Some((256, 1))
            }
            pub fn with_cell_coverage(
                _: char,
                w: u32,
                h: u32,
                fg: [u8; 4],
                _: bool,
                mut pixel: impl FnMut(u32, u32, u8, [u8; 4]),
            ) -> Result<(), &'static str> {
                crate::VISITS.with(|v| v.set(v.get() + 1));
                for y in 0..h {
                    for x in 0..w {
                        pixel(x, y, x as u8, fg)
                    }
                }
                Ok(())
            }
        }
    }
}
// @TINT@
#[path = "capture.rs"]
mod capture;
'''


def font_tint() -> str:
    """Use the font producer's implementation as the independent pixel oracle."""
    source = (ROOT / "src/r/services/shell3_font_service.rs").read_text()
    start = source.index("fn tint(")
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end].replace("fn tint(", "fn font_tint(", 1)


def main() -> None:
    capture = (ROOT / "src/shell3/show/edit_life.rs").read_text() + CAPTURE_TESTS
    terminal_path = json.dumps(str(ROOT / "crates/trueos-terminal/src/edit_life.rs"))
    harness = HARNESS.replace('"@TERMINAL_PATH@"', terminal_path)
    harness = harness.replace("// @TINT@", font_tint())
    with tempfile.TemporaryDirectory(prefix="trueos-edit-capture-") as directory:
        work = Path(directory)
        code = work / "main.rs"
        binary = work / "tests"
        (work / "capture.rs").write_text(capture)
        code.write_text(harness)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
