#!/usr/bin/env python3
"""Check Paper's real physical glyph sizing and shared grid-cell transport.

Compile the production size helper, boot script and grid glyph serializer with
only display discovery and the already-produced atlas supplied by a fixture.
This keeps the guest glyph header, the boot choice and cursor consumers tied
to the same physical metrics without requiring display hardware.
"""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
SERVICE = "src/r/services/shell3_font_service.rs"
DEMO = "src/spirit/paper_demo.rs"


def main() -> None:
    source = "extern crate alloc;\n"
    source += constant(DEMO, "GREETING_GRID_SCALE").replace("pub(super)", "pub(crate)") + "\n"
    source += item(DEMO, "launch_script")
    source += item(SERVICE, "grid_cell_extent")
    source += item(SERVICE, "grid_glyph_blocking")
    source += r'''
use std::cell::{Cell, RefCell};

mod ui4 {
    use super::{Cell, RefCell};
    thread_local! {
        pub static EXTENT: Cell<Option<(u32,u32)>> = const { Cell::new(Some((60,38))) };
        pub static REQUESTS: RefCell<Vec<(u32,u32)>> = const { RefCell::new(Vec::new()) };
    }
    pub fn physical_extent_pixels(width:u32,height:u32)->Option<(u32,u32)> {
        REQUESTS.with(|requests| requests.borrow_mut().push((width,height)));
        EXTENT.with(Cell::get)
    }
}

thread_local! { static READY:Cell<bool> = const { Cell::new(true) }; }
struct Atlas;
impl Atlas {
    fn glyph(&self, character:char)->Option<&'static [u8]> {
        match character { '§' => Some(&[0]), '?' => Some(&[100]), _ => None }
    }
}
struct Font { alpha: &'static Atlas }
struct FontSource;
static ATLAS:Atlas=Atlas;
static SHARED_FONT:Font=Font {alpha:&ATLAS};
static FONT:FontSource=FontSource;
impl FontSource {
    fn get(&self)->Option<&'static Font> { READY.with(Cell::get).then_some(&SHARED_FONT) }
}
fn sample_native(_: &Atlas, mask:&[u8], x:u32,y:u32,width:u32,_:u32)->u8 {
    (u32::from(mask[0])+x+y*width).min(255) as u8
}

#[test]
fn boot_and_guest_use_the_same_ten_mm_scale() {
    assert_eq!(launch_script(), "mode gridpaper\nsize 190x90\nstep 2\nwrap on\n");
    assert_eq!(grid_cell_extent(GREETING_GRID_SCALE),Some((21,38)));
    assert_eq!(ui4::REQUESTS.with(|requests| requests.borrow().clone()),vec![(10,10)]);
    ui4::REQUESTS.with(|requests| requests.borrow_mut().clear());
    let bytes=grid_glyph_blocking(GREETING_GRID_SCALE,'§').unwrap();
    assert_eq!(&bytes[..8], &[21,0,0,0,38,0,0,0]);
    assert_eq!(bytes.len(),8+21*38*4);
    assert_eq!(&bytes[8..12],&[255,255,255,255]);
    assert_eq!(&bytes[12..16],&[254,254,254,255]);
    assert_eq!(&bytes[8+21*4..12+21*4],&[234,234,234,255]);
    assert_eq!(ui4::REQUESTS.with(|requests| requests.borrow().clone()),vec![(10,10)]);
}

#[test]
fn character_advance_follows_em_height_instead_of_display_aspect_ratio() {
    ui4::EXTENT.with(|extent| extent.set(Some((999,44))));
    assert_eq!(grid_cell_extent(2),Some((24,44)));
    let bytes=grid_glyph_blocking(2,'§').unwrap();
    assert_eq!(&bytes[..8],&[24,0,0,0,44,0,0,0]);
    assert_eq!(bytes.len(),8+24*44*4);
}

#[test]
fn missing_physical_metrics_and_out_of_range_scales_do_not_invent_a_size() {
    for scale in [0,9,u32::MAX] { assert_eq!(grid_cell_extent(scale),None); }
    assert!(ui4::REQUESTS.with(|requests| requests.borrow().is_empty()));
    ui4::EXTENT.with(|extent| extent.set(None));
    assert_eq!(grid_cell_extent(2),None);
    assert_eq!(grid_glyph_blocking(2,'§'),Err("grid-font-physical-size"));
    ui4::EXTENT.with(|extent| extent.set(Some((0,0))));
    assert_eq!(grid_cell_extent(2),None);
    ui4::EXTENT.with(|extent| extent.set(Some((1,u32::MAX))));
    assert_eq!(grid_cell_extent(2),None);
}

#[test]
fn cold_atlas_is_unavailable_and_missing_characters_use_existing_question_mark() {
    READY.with(|ready| ready.set(false));
    assert_eq!(grid_glyph_blocking(2,'§'),Err("shell3-font-not-ready"));
    assert!(ui4::REQUESTS.with(|requests| requests.borrow().is_empty()));
    READY.with(|ready| ready.set(true));
    let bytes=grid_glyph_blocking(2,'λ').unwrap();
    assert_eq!(&bytes[8..12],&[155,155,155,255]);
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-gridpaper-font-size-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
