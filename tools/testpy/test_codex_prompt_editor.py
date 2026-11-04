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


def attachment_message_source():
    result = r'''
mod r {
    pub mod path {
        pub struct FsPath(String);
        impl FsPath {
            pub fn parse(path: &str, _: bool) -> Result<Self, ()> { Ok(Self(path.into())) }
            pub fn to_relative_string(&self) -> String { self.0.clone() }
        }
    }
    pub mod fs {
        pub mod volatile_files {
            pub fn selected_disk(_: &str) -> Option<u8> { Some(0) }
        }
        pub mod trueosfs {
            pub struct Handle(Vec<u8>);
            impl Handle { pub fn data_len(&self) -> u64 { self.0.len() as u64 } }
            pub async fn file_read_open_async(_: u8, name: &str) -> Result<Option<Handle>, ()> {
                let bytes = match name {
                    "/first" => vec![b'a'; 600],
                    "/second" => vec![b'b'; 425],
                    "/fits" => vec![b'c'; 424],
                    "/large" => vec![b'd'; 2048],
                    "/binary" => vec![0, 255, 1],
                    _ => return Ok(None),
                };
                Ok(Some(Handle(bytes)))
            }
            pub async fn file_read_handle_range_async(handle: Handle, offset: u64, out: &mut [u8]) -> Result<Option<usize>, ()> {
                assert_eq!(offset, 0);
                assert!(out.len() <= 1024, "large attachments must stay references");
                assert_eq!(out.len(), handle.0.len(), "read the whole pinned small file");
                out.copy_from_slice(&handle.0);
                Ok(Some(out.len()))
            }
        }
    }
}
mod message_tests {
    use crate::{attachments, editor};
    use std::string::String;
    mod codex { pub const MAX_INPUT_BYTES: usize = 16 * 1024; }
    struct ActivePrompt { editor: editor::Editor, paths: Vec<String> }
'''
    result += block((ROOT / "src/ui4/codex_prompt.rs").read_text(), "async fn message(")
    result += r'''
    fn run<F: std::future::Future>(future: F) -> F::Output {
        struct NoWake;
        impl std::task::Wake for NoWake { fn wake(self: std::sync::Arc<Self>) {} }
        let waker = std::task::Waker::from(std::sync::Arc::new(NoWake));
        let mut context = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            _ => panic!("fixture futures must complete"),
        }
    }
    #[test]
    fn submitted_attachments_share_budget_and_leave_editor_and_large_files_alone() {
        let mut prompt = ActivePrompt { editor: editor::Editor::default(),
            paths: ["/first", "/second", "/fits", "/large", "/binary", "/folder", "/hello\nworld"]
                .map(String::from).to_vec() };
        for ch in "Please inspect".chars() { assert!(prompt.editor.insert(ch)); }
        let text = run(message(&prompt)).unwrap();
        assert_eq!(prompt.editor.text(), "Please inspect");
        assert!(text.starts_with("Please inspect"));
        assert!(text.contains(&"a".repeat(600)));
        assert!(text.contains(&"c".repeat(424)));
        assert!(!text.contains(&"b".repeat(425)));
        assert!(!text.contains(&"d".repeat(2048)));
        assert_eq!(text.matches("Small text attachment").count(), 2);
        for path in &prompt.paths { assert!(text.contains(&format!("{:?}", path))); }
    }
}
'''
    return result


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-codex-prompt-") as directory:
        directory = Path(directory)
        harness = directory / "harness.rs"
        editor = ROOT / "src/ui4/codex_prompt_editor.rs"
        attachments = ROOT / "src/ui4/codex_attachments.rs"
        harness.write_text(
            f'extern crate alloc;\n#[path = "{editor}"]\nmod editor;\n'
            + f'#[path = "{attachments}"]\nmod attachments;\n'
            + menu_source() + paint_source() + attachment_message_source(),
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
