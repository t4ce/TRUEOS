#!/usr/bin/env python3
"""Exercise production compact Edit capture and its menu/pause handoff."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

STUBS = r"""
extern crate alloc;
extern crate self as trueos_time;
extern crate self as trueos_terminal;
use core::cell::Cell;
thread_local! {
    static CAPTURES: Cell<usize> = const { Cell::new(0) };
    static PUBLISHES: Cell<usize> = const { Cell::new(0) };
    static PAUSES: Cell<usize> = const { Cell::new(0) };
    static REMOVES: Cell<usize> = const { Cell::new(0) };
    static FAILED: Cell<bool> = const { Cell::new(false) };
}
#[macro_export] macro_rules! log_info { ($($t:tt)*) => {}; }
#[macro_export] macro_rules! log_warn { ($($t:tt)*) => {}; }
pub struct Instant;
impl Instant { pub fn now() -> Self { Self } pub fn as_millis(&self) -> u64 { 1000 } }
#[derive(Clone, Copy)]
pub struct RgbaColor { foreground: [u8;4], background: [u8;4], underline: bool }
impl RgbaColor {
    fn rgba(self) -> [u8;4] { self.foreground }
    fn background(self) -> Option<[u8;4]> { Some(self.background) }
    fn underline(self) -> bool { self.underline }
}
const BACKGROUND: [u8;4] = [0,0,0,255];
const FOREGROUND: RgbaColor = RgbaColor { foreground: [255;4], background: BACKGROUND, underline:false };
struct Ui4Surface {
    width:u32, height:u32, scale:u32, frame:(), window:ui4::WindowId,
    edit_life: Option<edit_life::EditLife>,
    edit_life_page: Option<alloc::sync::Arc<ui4::edit_life::Page>>,
}
mod r { pub mod services { pub mod shell3_font_service {
    pub fn cell_extent(_:u32) -> Option<(u32,u32)> {
        crate::CAPTURES.with(|v| v.set(v.get()+1)); Some((6,11))
    }
    #[path="@FONT@"] pub mod hex;
}}}
mod ui4 {
    #[derive(Clone,Copy,PartialEq,Eq)] pub struct WindowId;
    impl WindowId { pub fn raw(self) -> u32 { 1 } }
    pub mod edit_life {
        use alloc::vec::Vec;
        pub(crate) const MAX_PAGE_PIXELS:usize = 4096*2160;
        @PAGE@
        pub fn publish_page(_:super::WindowId,_:(),page:alloc::sync::Arc<Page>) {
            assert!(page.valid()); crate::PUBLISHES.with(|v| v.set(v.get()+1));
        }
        pub fn pause(_:super::WindowId,_:()) { crate::PAUSES.with(|v| v.set(v.get()+1)); }
        pub fn remove(_:super::WindowId) { crate::REMOVES.with(|v| v.set(v.get()+1)); }
        pub fn is_failed(_:super::WindowId) -> bool { crate::FAILED.with(core::cell::Cell::get) }
    }
}
#[path="@PARSER@"] mod edit_life;
mod capture {
    @CAPTURE@
    @TESTS@
}
"""

TESTS = r"""
#[cfg(test)] mod compact_tests {
    use super::*;
    use crate::{CAPTURES,PUBLISHES,PAUSES,REMOVES};
    fn surface(width:u32,height:u32) -> Ui4Surface {
        Ui4Surface { width,height,scale:2,frame:(),window:crate::ui4::WindowId,
            edit_life:None,edit_life_page:None }
    }
    fn settings() -> EditLife {
        EditLife { enabled:true,active:true,viewport:CellRect{x:0,y:0,width:3,height:1},
            scroll_revision:7,emission_revision:9,exclusions:Vec::new() }
    }
    fn work() -> (usize,usize) { (CAPTURES.with(Cell::get),PUBLISHES.with(Cell::get)) }
    use core::cell::Cell;
    #[test] fn capture_retains_only_viewport_cells_and_exact_font_ids_colors_flags() {
        let surface=surface(30,33);
        let mut settings=settings();
        settings.viewport=CellRect{x:1,y:1,width:3,height:1};
        let selected=crate::RgbaColor{background:[8,40,80,255],..crate::FOREGROUND};
        let underline=crate::RgbaColor{underline:true,..crate::FOREGROUND};
        let chrome=[('M',None);5];
        let page=[('G',None),('A',Some(crate::FOREGROUND)),('█',Some(selected)),(' ',Some(underline)),('S',None)];
        let captured=capture(&surface,&[&chrome,&page,&chrome],&settings).unwrap();
        assert_eq!((captured.columns,captured.rows,captured.cell_width,captured.cell_height),(3,1,6,11));
        assert_eq!(captured.viewport,PixelRect{x:6,y:11,width:18,height:11});
        assert_eq!(captured.cells.len(),3);
        assert_eq!(core::mem::size_of_val(captured.cells.as_slice()),48);
        let font=&crate::r::services::shell3_font_service::hex::MASKS;
        let a=crate::r::services::shell3_font_service::hex::resolve('A',[255;4]).0;
        assert_eq!(captured.cells[0],[a,u32::MAX,0xff000000,0]);
        assert!(font[a as usize].iter().any(|&word| word!=0));
        assert_eq!(captured.cells[1][2],u32::from_le_bytes(selected.background));
        assert_eq!(captured.cells[1][3],1);
        assert_eq!(captured.cells[2][0],crate::r::services::shell3_font_service::hex::SPACE_GLYPH);
        assert_eq!(captured.cells[2][3],2);
        assert_eq!((captured.scroll_revision,captured.emission_revision),(7,9));
    }
    #[test] fn clipped_last_cell_keeps_full_cell_mapping_and_missing_rows_are_blank() {
        let surface=surface(17,10);
        let settings=settings();
        let line=[('A',None),('B',None),('█',None)];
        let page=capture(&surface,&[&line],&settings).unwrap();
        assert_eq!((page.viewport.width,page.viewport.height),(17,10));
        assert_eq!((page.columns,page.rows,page.cell_width,page.cell_height),(3,1,6,11));
        assert_eq!(page.cells[2][0],crate::r::services::shell3_font_service::hex::resolve('█',[255;4]).0);
        let blank=capture(&surface,&[],&settings).unwrap();
        assert!(blank.cells.iter().all(|cell| cell[0]==crate::r::services::shell3_font_service::hex::SPACE_GLYPH));
    }
    #[test] fn unsupported_unicode_resolves_to_same_red_bitmap_replacement() {
        let surface=surface(18,11);
        let line=[('漢',Some(crate::RgbaColor{foreground:[4,5,6,129],..crate::FOREGROUND}))];
        let page=capture(&surface,&[&line],&settings()).unwrap();
        assert_eq!(page.cells[0][0],crate::r::services::shell3_font_service::hex::FALLBACK_GLYPH);
        assert_eq!(page.cells[0][1],u32::from_le_bytes([255,0,0,129]));
    }
    #[test] fn pause_skips_capture_retains_page_and_resume_captures_once_without_reset() {
        let mut surface=surface(18,11);
        let mut settings=settings();
        let clean=[('A',None)];
        present(&mut surface,&[&clean],Some(&settings),true);
        assert_eq!(work(),(1,1));
        let retained=Arc::clone(surface.edit_life_page.as_ref().unwrap());
        settings.active=false;
        let menu=[('M',None)];
        for _ in 0..3 { present(&mut surface,&[&menu],Some(&settings),true); }
        assert_eq!(work(),(1,1));
        assert_eq!(PAUSES.with(Cell::get),3);
        assert!(Arc::ptr_eq(&retained,surface.edit_life_page.as_ref().unwrap()));
        settings.active=true;
        present(&mut surface,&[&clean],Some(&settings),false);
        assert_eq!(work(),(2,2));
        let resumed=surface.edit_life_page.as_ref().unwrap();
        assert_eq!(resumed.cells,retained.cells);
        assert_eq!((resumed.scroll_revision,resumed.emission_revision),(7,9));
        present(&mut surface,&[&clean],Some(&settings),false);
        assert_eq!(work(),(2,2));
        settings.active=false;
        present(&mut surface,&[&menu],Some(&settings),true);
        settings.enabled=false;
        present(&mut surface,&[&clean],Some(&settings),false);
        assert_eq!(REMOVES.with(Cell::get),1);
        assert!(surface.edit_life_page.is_none());
    }
    #[test] fn any_popup_exclusion_pauses_before_initial_capture() {
        let mut surface=surface(18,11);
        let mut settings=settings();
        settings.exclusions.push(settings.viewport);
        present(&mut surface,&[],Some(&settings),true);
        assert_eq!(work(),(0,0));
        assert_eq!(PAUSES.with(Cell::get),1);
        assert!(surface.edit_life_page.is_none());
        settings.exclusions.clear();settings.active=false;
        present(&mut surface,&[],Some(&settings),true);
        assert_eq!(work(),(0,0));
    }
    #[test] fn failed_producer_bypasses_capture_on_typing_and_menu_resume() {
        let mut surface=surface(18,11);
        let mut settings=settings();
        let clean=[('A',None)];
        present(&mut surface,&[&clean],Some(&settings),true);
        let retained=Arc::clone(surface.edit_life_page.as_ref().unwrap());
        crate::FAILED.with(|v| v.set(true));
        let changed=[('B',None)];
        for active in [true,false,true] {
            settings.active=active;
            settings.emission_revision+=1;
            present(&mut surface,&[&changed],Some(&settings),true);
        }
        assert_eq!(work(),(1,1));
        assert_eq!(PAUSES.with(Cell::get),3);
        assert!(Arc::ptr_eq(&retained,surface.edit_life_page.as_ref().unwrap()));
        settings.enabled=false;
        present(&mut surface,&[&clean],Some(&settings),false);
        assert_eq!(REMOVES.with(Cell::get),1);
        assert!(surface.edit_life_page.is_none());
    }
}
"""


def main() -> None:
    source = (ROOT / "src/ui4/edit_life.rs").read_text()
    start = source.index("#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]")
    page = source[start:source.index("\n#[derive(Clone)]\nstruct Registration", start)]
    fixture = (STUBS.replace("@FONT@", str(ROOT / "src/r/services/shell3_hex_font.rs"))
               .replace("@PAGE@", page)
               .replace("@PARSER@", str(ROOT / "crates/trueos-terminal/src/edit_life.rs"))
               .replace("@CAPTURE@", (ROOT / "src/shell3/show/edit_life.rs").read_text().replace("//!", "//"))
               .replace("@TESTS@", TESTS))
    with tempfile.TemporaryDirectory(prefix="trueos-edit-compact-capture-") as temporary:
        source = Path(temporary) / "tests.rs"
        source.write_text(fixture)
        binary = Path(temporary) / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
