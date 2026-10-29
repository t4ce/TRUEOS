#!/usr/bin/env python3
"""Exercise the production prompt's UTF-8 editor without booting a display."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]


def block(source, marker):
    """Import one named inline production test, retaining its exact body."""
    assert source.count(marker) == 1, marker
    start = source.index(marker)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def menu_source():
    broker = "src/ui4/input_broker.rs"
    constants = (
        "CONTEXT_MENU_BORDER_PX", "CONTEXT_MENU_OFFSET_PX", "CONTEXT_MENU_WIDTH_PX",
        "CONTEXT_MENU_ROW_GAP_PX", "CONTEXT_MENU_ROW_HEIGHT_PX",
        "DESKTOP_CONTEXT_MENU_ENTRY_COUNT", "DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX",
    )
    result = "mod microfont {\n" + constant("vendor/microfont/src/lib.rs", "FHEIGHT") + "}\n"
    result += "mod color_picker {\n" + constant("src/ui4/color_picker.rs", "PICKER_WIDTH") + "}\n"
    result += "mod menu {\nuse crate::microfont;\n"
    result += "\n".join(constant(broker, name) for name in constants)
    result += "\n".join(item(broker, name) for name in (
        "DesktopContextMenuAction", "Ui4VisualRect", "context_menu_rect",
        "desktop_context_menu_action_at", "visual_rect_contains",
    ))
    result += "mod tests { use super::*;\n#[test]\n"
    result += block((ROOT / broker).read_text(),
                    "fn desktop_context_menu_exposes_codex_directly_below_link()")
    result += """
    #[test]
    fn fourth_menu_row_fits_at_the_screen_corner() {
        for (width,height) in [(1920,1080),(1024,768),(256,128)] {
            let menu = context_menu_rect((width-1,height-1),width,height);
            assert!(menu.x+menu.width <= width);
            assert!(menu.y+menu.height <= height);
            let x=menu.x+menu.width/2;
            let y=menu.y+DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX
                +3*CONTEXT_MENU_ROW_HEIGHT_PX+CONTEXT_MENU_ROW_HEIGHT_PX/2;
            assert_eq!(desktop_context_menu_action_at(menu,x,y),
                Some(DesktopContextMenuAction::Codex));
            assert_eq!(desktop_context_menu_action_at(menu,menu.x+menu.width,y),None);
        }
    }
    }}
    """
    return result


def paint_source():
    prompt = "src/ui4/codex_prompt.rs"
    result = "mod paint { use crate::editor;\n"
    result += "\n".join(item(prompt, name) for name in ("cell_bounds", "draw_glyph"))
    result += """
    #[test]
    fn glyph_white_border_preserves_ruling_and_frame_padding() {
        // A glyph with a white border and black interior, at a partial cell
        // on the frame edge. Pitch padding must remain completely untouched.
        let mut glyph=alloc::vec::Vec::new();
        glyph.extend_from_slice(&5u32.to_le_bytes());
        glyph.extend_from_slice(&5u32.to_le_bytes());
        for y in 0..5 { for x in 0..5 {
            let value=if x==0 || y==0 || x==4 || y==4 {255} else {0};
            glyph.extend_from_slice(&[value,value,value,255]);
        }}
        let mut pixels=alloc::vec![17u8; 8*4*6];
        draw_glyph(&mut pixels,8*4,6,6,3,3,3,3,&glyph);
        for y in 0..6 { for x in 0..8 {
            let offset=(y*8+x)*4;
            if (4..6).contains(&x) && (4..6).contains(&y) {
                assert_eq!(&pixels[offset..offset+4],&[0,0,0,255]);
            } else {
                assert_eq!(&pixels[offset..offset+4],&[17;4]);
            }
        }}
        assert_eq!(cell_bounds(18,8,719,341),(681,303,38,38));
    }
    }
    """
    return result


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-codex-prompt-") as directory:
        directory = Path(directory)
        harness = directory / "harness.rs"
        editor = ROOT / "src/ui4/codex_prompt_editor.rs"
        harness.write_text(
            f'extern crate alloc;\n#[path = "{editor}"]\nmod editor;\n'
            + menu_source() + paint_source(),
            encoding="utf-8",
        )
        binary = directory / "prompt-editor-tests"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "-O", str(harness), "-o", str(binary)],
            check=True,
            cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True, cwd=ROOT)


if __name__ == "__main__":
    main()
