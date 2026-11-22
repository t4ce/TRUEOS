#!/usr/bin/env python3
"""Exercise Shell3's production cell diffs on the host."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    shell = "src/shell3/shell3.rs"
    update = "src/shell3/update.rs"
    shell_source = (ROOT / shell).read_text()
    impl_start = shell_source.index("impl RgbaColor {")
    color_impl = shell_source[impl_start : shell_source.index("\n}", impl_start) + 2]
    source = "\n".join(
        [
            "#![allow(dead_code)]",
            f'#[path = "{ROOT / "src/ui4/damage.rs"}"] mod damage;',
            """mod r {
    pub mod services {
        pub mod shell3_font_service {
            pub fn cell_extent(_: u32) -> Option<(u32, u32)> { Some((8, 16)) }
        }
    }
}""",
            "mod fixture {",
            "type RenderedLine = Vec<(char, Option<RgbaColor>)>;",
            item(shell, "RgbaColor"),
            color_impl,
            item(shell, "SpecialRows"),
            item(shell, "StripSide"),
            item(update, "SegmentUpdate"),
            constant(update, "MATRIX_BACKGROUND"),
            *(item(update, name) for name in (
                "diff_rendered_lines",
                "diff_paint_lines",
                "diff_lines",
                "diff_visible_segment",
                "row_from_index",
            )),
            r"""
fn cells(text: &str) -> RenderedLine {
    text.chars().map(|ch| (ch, None)).collect()
}

fn spans(updates: &[SegmentUpdate]) -> Vec<(usize, usize, &str)> {
    updates.iter().map(|update| (update.offset, update.remove, &*update.text)).collect()
}

#[test]
fn waveform_and_spectrum_keep_the_unchanged_divider_out_of_paint() {
    let old = [cells("│⠄⠤││▄▅│")];
    let mut new = [cells("│⠂⠴││█▅│")];
    new[0][6].1 = Some(RgbaColor::Pink);
    let painted = diff_paint_lines(Some(&old), &new);
    assert_eq!(spans(&painted), [(1, 2, "⠂⠴"), (5, 2, "█▅")]);
    assert_eq!(painted[1].colors, [None, Some(RgbaColor::Pink)]);
    assert_eq!(
        spans(&diff_rendered_lines(Some(&old), &new)),
        [(1, 6, "⠂⠴││█▅")],
    );
    assert!(diff_paint_lines(Some(&new), &new).is_empty());
}

#[test]
fn resizing_and_disappearing_rows_keep_the_existing_cell_coverage() {
    let old = [cells("│⠂⠴│"), cells("─")];
    let new = [cells("│⠂")];
    let painted = diff_paint_lines(Some(&old), &new);
    assert_eq!(spans(&painted), [(2, 2, ""), (0, 1, "")]);
    assert_eq!(painted[1].row, SpecialRows::StatusRow);
    assert_eq!(
        spans(&diff_paint_lines(Some(&new), &old)),
        [(2, 0, "⠴│"), (0, 0, "─")],
    );
}

#[test]
fn a_fresh_surface_only_paints_occupied_or_styled_cells() {
    let mut line = cells(" ╭   │ ");
    line[3].1 = Some(RgbaColor::Underlined { foreground: [255; 4] });
    assert_eq!(
        spans(&diff_paint_lines(None, &[line])),
        [(1, 0, "╭"), (3, 0, " "), (5, 0, "│")],
    );
}
""",
            "mod show { use crate::damage::DamageRect;",
            item("src/shell3/show/show.rs", "damage_for_segments"),
            r"""
#[test]
fn damage_includes_changes_already_in_the_acquired_buffer() {
    use super::{cells, diff_paint_lines, diff_rendered_lines};
    let displayed_front = [cells("│⠂⠤││▄▅│")];
    let acquired_back = [cells("│⠄⠤││█▅│")];
    let next = [cells("│⠂⠤││█▅│")];
    let painted = diff_paint_lines(Some(&acquired_back), &next);
    let front_updates = diff_rendered_lines(Some(&displayed_front), &next);
    assert_eq!(
        damage_for_segments(&painted, &front_updates, 64, 16, 1),
        Some(DamageRect::new(8, 0, 40, 16)),
    );
}
""",
            "}",
            "}",
        ]
    )
    with tempfile.TemporaryDirectory(prefix="shell3-paint-damage-") as temp:
        path = Path(temp)
        harness = path / "harness.rs"
        harness.write_text(source)
        binary = path / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", harness, "-o", binary], check=True)
        subprocess.run([binary], check=True)


if __name__ == "__main__":
    main()
