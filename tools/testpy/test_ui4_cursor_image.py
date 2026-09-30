#!/usr/bin/env python3
"""Run UI4 custom-cursor production helpers on the host.

The kernel binary is freestanding and its ``cfg(test)`` build cannot link a
host test harness.  This tool extracts the production registration and slot-4
row-run helpers into a small Rust test crate with only their data dependencies
stubbed.  It deliberately invokes the real algorithms instead of duplicating
them in Python.
"""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def item(path: str, name: str) -> str:
    """Extract one unindented Rust item, failing when its source shape drifts."""
    source = (ROOT / path).read_text()
    declarations = list(
        re.finditer(
            rf"^(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?"
            rf"(?:fn|enum|struct|mod)\s+{re.escape(name)}\b",
            source,
            re.MULTILINE,
        )
    )
    if len(declarations) != 1:
        raise ValueError(f"{path}: expected one item {name}, got {len(declarations)}")
    start = declarations[0].start()
    attributes = re.search(r"(?:^#\[[^\n]*\]\n)+\Z", source[:start], re.MULTILINE)
    if attributes:
        start = attributes.start()
    ending = re.search(r"^}\s*\n", source[declarations[0].end() :], re.MULTILINE)
    if not ending:
        raise ValueError(f"{path}: no top-level closing brace for {name}")
    end = declarations[0].end() + ending.end()
    return source[start:end]


def constant(path: str, name: str) -> str:
    source = (ROOT / path).read_text()
    matches = list(
        re.finditer(
            rf"^(?:pub(?:\([^)]*\))?\s+)?const\s+{re.escape(name)}\b.*?;$",
            source,
            re.MULTILINE | re.DOTALL,
        )
    )
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one constant {name}, got {len(matches)}")
    return matches[0].group()


def harness_source() -> str:
    cursor = "src/ui4/cursor_frame_inout.rs"
    slot4 = "src/ui4/slot4_service.rs"
    cursor_items = "\n".join(
        [
            constant(cursor, "MAX_CURSOR_IMAGES_PER_FRAME"),
            constant(cursor, "MAX_CURSOR_IMAGE_DIMENSION"),
            item(cursor, "Ui4CursorImage"),
            item(cursor, "Ui4CursorRowRun"),
            item(cursor, "cursor_image_from_rgba"),
            item(cursor, "CursorFrameError"),
            item(cursor, "validate_cursor_image_input"),
        ]
    )
    slot4_item = item(slot4, "push_custom_cursor_image")
    return f'''#![allow(dead_code, unused_imports)]
extern crate alloc;

mod graphics {{
    pub mod primitives {{
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]
        pub struct Rgba8(pub u8, pub u8, pub u8, pub u8);
        impl Rgba8 {{
            pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {{ Self(r, g, b, a) }}
        }}
    }}
}}

mod intel {{
    use crate::graphics::primitives::Rgba8;
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct LiveOverlayRect {{
        pub x: u32, pub y: u32, pub width: u32, pub height: u32, pub color: Rgba8,
    }}
    impl LiveOverlayRect {{
        pub const fn new(x: u32, y: u32, width: u32, height: u32, color: Rgba8) -> Self {{
            Self {{ x, y, width, height, color }}
        }}
    }}
}}

mod ui4 {{
    use alloc::sync::Arc;
    use alloc::vec::Vec as AllocVec;
    {cursor_items}

    mod slot4 {{
        use alloc::vec::Vec;
        type Slot4Rects = Vec<crate::intel::LiveOverlayRect>;
        fn push_overlay_rect(
            rects: &mut Slot4Rects, x: u32, y: u32, width: u32, height: u32,
            color: crate::graphics::primitives::Rgba8,
        ) {{
            if width != 0 && height != 0 {{
                rects.push(crate::intel::LiveOverlayRect::new(x, y, width, height, color));
            }}
        }}
        {slot4_item}

        #[cfg(test)]
        mod tests {{
            use super::*;
            use crate::graphics::primitives::Rgba8;
            use super::super::{{cursor_image_from_rgba, validate_cursor_image_input, CursorFrameError}};

            #[test]
            fn validates_registration_bounds() {{
                assert_eq!(validate_cursor_image_input(0, 1, 1, 0, 0, 4), Err(CursorFrameError::InvalidImage));
                assert_eq!(validate_cursor_image_input(1, 65, 1, 0, 0, 260), Err(CursorFrameError::InvalidImage));
                assert_eq!(validate_cursor_image_input(1, 2, 2, 2, 0, 16), Err(CursorFrameError::InvalidImage));
                assert_eq!(validate_cursor_image_input(1, 2, 2, 1, 2, 16), Err(CursorFrameError::InvalidImage));
                assert_eq!(validate_cursor_image_input(1, 2, 2, 1, 1, 15), Err(CursorFrameError::InvalidImage));
                assert!(validate_cursor_image_input(16, 64, 64, 63, 63, 16_384).is_ok());
            }}

            #[test]
            fn alpha_is_removed_and_equal_pixels_are_coalesced() {{
                let image = cursor_image_from_rgba(4, 2, 0, 0, &[
                    1, 2, 3, 255, 1, 2, 3, 255, 0, 0, 0, 0, 4, 5, 6, 128,
                    0, 0, 0, 0, 7, 8, 9, 255, 7, 8, 9, 255, 0, 0, 0, 0,
                ]);
                assert_eq!(image.row_runs.len(), 3);
                assert_eq!((image.row_runs[0].row, image.row_runs[0].column, image.row_runs[0].width), (0, 0, 2));
                assert_eq!(image.row_runs[0].color, Rgba8::new(1, 2, 3, 255));
                assert_eq!((image.row_runs[2].row, image.row_runs[2].column, image.row_runs[2].width), (1, 1, 2));
            }}

            #[test]
            fn hotspot_clipping_uses_the_precomputed_runs() {{
                let image = cursor_image_from_rgba(4, 2, 1, 1, &[
                    200, 10, 20, 255, 200, 10, 20, 255, 0, 0, 0, 0, 1, 2, 3, 255,
                    0, 0, 0, 0, 10, 200, 20, 128, 10, 200, 20, 128, 0, 0, 0, 0,
                ]);
                let mut rects = Slot4Rects::new();
                push_custom_cursor_image(&mut rects, 1, 1, &image, 3, 2);
                assert_eq!(rects.len(), 2);
                assert_eq!((rects[0].x, rects[0].y, rects[0].width, rects[0].height), (0, 0, 2, 1));
                assert_eq!(rects[0].color, Rgba8::new(200, 10, 20, 255));
                assert_eq!((rects[1].x, rects[1].y, rects[1].width, rects[1].height), (1, 1, 2, 1));
                assert_eq!(rects[1].color, Rgba8::new(10, 200, 20, 128));
            }}

            #[test]
            fn maximum_image_with_different_adjacent_pixels_keeps_all_4096_runs() {{
                let mut rgba = Vec::with_capacity(64 * 64 * 4);
                for index in 0u16..4096 {{
                    rgba.extend_from_slice(&[(index & 0xff) as u8, (index >> 8) as u8, 17, 255]);
                }}
                let image = cursor_image_from_rgba(64, 64, 0, 0, &rgba);
                assert_eq!(image.row_runs.len(), 4096);
                let mut rects = Slot4Rects::new();
                push_custom_cursor_image(&mut rects, 0, 0, &image, 64, 64);
                assert_eq!(rects.len(), 4096);
                assert!(rects.iter().all(|rect| rect.width == 1 && rect.height == 1));
            }}
        }}
    }}
}}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-cursor-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
