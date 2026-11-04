#!/usr/bin/env python3
"""Exercise production viewport font selection and both-buffer invalidation.

Compile the actual HexRegion, Ui4Surface font methods and font-cache key with
small host-only storage stubs. No graphics backend or Cargo build is needed.
"""

import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def item(text: str, marker: str) -> str:
    start = text.index(marker)
    opening = text.index("{", start)
    depth = 1
    for position in range(opening + 1, len(text)):
        if text[position] == "{":
            depth += 1
        elif text[position] == "}":
            depth -= 1
            if depth == 0:
                return text[start:position + 1] + "\n"
    raise AssertionError("Unclosed production item: " + marker)


TESTS = r'''
#[cfg(test)]
mod paint_tests {
    use super::*;
    use edit_life::{CellRect, EditLife};

    fn settings() -> EditLife {
        EditLife {
            enabled: true, active: true,
            viewport: CellRect { x: 3, y: 2, width: 10, height: 6 },
            scroll_revision: 0, emission_revision: 0, exclusions: Vec::new(),
        }
    }

    fn surface() -> Ui4Surface {
        Ui4Surface {
            hex_region: None,
            frame_contents: [Some(vec![17, 19]), Some(vec![23, 29])],
            clear_buffers: [false; 2],
        }
    }

    fn drawn(surface: &mut Ui4Surface) {
        surface.frame_contents = [Some(vec![17, 19]), Some(vec![23, 29])];
        surface.clear_buffers = [false; 2];
    }

    fn invalidated(surface: &Ui4Surface) {
        assert_eq!(surface.frame_contents, [None, None]);
        assert_eq!(surface.clear_buffers, [true; 2]);
    }

    #[test]
    fn metadata_only_toggle_repaints_both_buffers_and_restores_raster() {
        let mut surface = surface();
        let mut settings = settings();
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(3, 2), FontFace::Hex);
        assert_eq!(surface.font_face(12, 7), FontFace::Hex);
        for (x, y) in [(2, 2), (13, 2), (3, 1), (3, 8), (0, 0)] {
            assert_eq!(surface.font_face(x, y), FontFace::Raster);
        }
        drawn(&mut surface);
        settings.enabled = false;
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(3, 2), FontFace::Raster);
        drawn(&mut surface);
        surface.set_hex_region(None);
        assert_eq!(surface.clear_buffers, [false; 2]);
    }

    #[test]
    fn pause_resume_and_revision_changes_keep_hex_and_completed_font_cache() {
        let mut surface = surface();
        let mut settings = settings();
        surface.set_hex_region(Some(&settings));
        drawn(&mut surface);
        settings.active = false;
        settings.scroll_revision += 1;
        settings.emission_revision += 1;
        surface.set_hex_region(Some(&settings));
        assert_eq!(surface.font_face(6, 5), FontFace::Hex);
        assert_eq!(surface.clear_buffers, [false; 2]);
        assert!(surface.frame_contents.iter().all(Option::is_some));
        settings.active = true;
        surface.set_hex_region(Some(&settings));
        assert_eq!(surface.clear_buffers, [false; 2]);
    }

    #[test]
    fn moving_overlay_uses_raster_and_invalidates_both_buffers() {
        let mut surface = surface();
        let mut settings = settings();
        surface.set_hex_region(Some(&settings));
        drawn(&mut surface);
        settings.active = false;
        settings.exclusions.push(CellRect { x: 5, y: 3, width: 2, height: 2 });
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(5, 3), FontFace::Raster);
        assert_eq!(surface.font_face(6, 4), FontFace::Raster);
        assert_eq!(surface.font_face(4, 3), FontFace::Hex);
        assert_eq!(surface.font_face(7, 4), FontFace::Hex);
        drawn(&mut surface);
        settings.exclusions[0].x = 8;
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(5, 3), FontFace::Hex);
        assert_eq!(surface.font_face(8, 3), FontFace::Raster);
        drawn(&mut surface);
        settings.exclusions.clear();
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(8, 3), FontFace::Hex);
    }

    #[test]
    fn gutter_and_viewport_changes_repaint_old_and_new_cell_faces() {
        let mut surface = surface();
        let mut settings = settings();
        surface.set_hex_region(Some(&settings));
        drawn(&mut surface);
        settings.viewport.x = 4;
        settings.viewport.width = 9;
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(3, 2), FontFace::Raster);
        assert_eq!(surface.font_face(4, 2), FontFace::Hex);
        drawn(&mut surface);
        settings.viewport.height = 0;
        surface.set_hex_region(Some(&settings));
        invalidated(&surface);
        assert_eq!(surface.font_face(4, 2), FontFace::Raster);
    }

    #[test]
    fn styled_cache_keeps_equal_raster_and_hex_characters_in_distinct_slots() {
        let raster = CellKey {
            face: FontFace::Raster, character: 'A', width: 17, height: 31,
            foreground: [17, 19, 23, 255], background: [0, 0, 0, 255], underline: false,
        };
        let hex = CellKey { face: FontFace::Hex, ..raster };
        let mut cache = std::collections::BTreeMap::new();
        cache.insert(raster, "raster coverage");
        cache.insert(hex, "hex coverage");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache[&raster], "raster coverage");
        assert_eq!(cache[&hex], "hex coverage");
    }
}
'''


def main() -> None:
    show = (ROOT / "src/shell3/show/show.rs").read_text()
    font = (ROOT / "src/r/services/shell3_font_service.rs").read_text()
    source = "extern crate alloc;\nextern crate self as trueos_terminal;\n"
    source += "#[path=" + json.dumps(str(ROOT / "crates/trueos-terminal/src/edit_life.rs")) + "] pub mod edit_life;\n"
    source += item(font, "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]\npub(crate) enum FontFace")
    source += item(font, "#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]\nstruct CellKey")
    source += "mod r { pub mod services { pub mod shell3_font_service { pub(crate) use crate::FontFace; } } }\n"
    source += item(show, "#[derive(Clone, Debug, PartialEq, Eq)]\nstruct HexRegion")
    source += item(show, "impl HexRegion")
    source += "struct Ui4Surface { hex_region: Option<HexRegion>, frame_contents: [Option<Vec<u8>>; 2], clear_buffers: [bool; 2] }\n"
    source += item(show, "impl Ui4Surface")
    source += TESTS
    with tempfile.TemporaryDirectory(prefix="trueos-edit-hex-paint-") as temporary:
        directory = Path(temporary)
        fixture = directory / "paint.rs"
        fixture.write_text(source)
        binary = directory / "paint"
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test", "-Adead_code",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(fixture), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
