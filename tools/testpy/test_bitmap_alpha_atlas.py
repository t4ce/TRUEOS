#!/usr/bin/env python3
"""Test the retired bitmap entry point and the production native atlas contract.

No hardware executes: fixtures provide an already-produced native atlas or a
native-production failure. Publication, filtering, fallback glyphs and text-mask
copying remain production source.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import item
from test_mouse_visual_cursors import block


ROOT = Path(__file__).resolve().parents[2]
ATLAS = "src/r/services/microfont_mask_service.rs"


def harness() -> str:
    source = (ROOT / ATLAS).read_text()
    start = source.index("pub(crate) const ATLAS_COLUMNS")
    end = source.index("#[derive(Clone, Copy, Debug, PartialEq)]")
    production = "use alloc::vec::Vec;\n" + source[start:end]
    for name in ("glyph_slot", "AlphaAtlasSource", "OwnedAlphaAtlas", "ready_atlas",
                 "build_bitmap_alpha_atlas", "build_shell3_alpha_atlas", "copy_text_mask"):
        production += item(ATLAS, name) + "\n"
    production += block(source, "impl OwnedAlphaAtlas {") + "\n"
    for name in ("ALPHA_ATLAS",):
        declaration = re.search(rf"^static {name}:.*?;$", source, re.MULTILINE)
        assert declaration, name
        production += declaration.group() + "\n"
    return r'''
extern crate alloc;
#[macro_export] macro_rules! log_important { ($($tokens:tt)*) => {}; }
mod spin {
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {
        pub const fn new() -> Self { Self(std::sync::OnceLock::new()) }
        pub fn get(&self) -> Option<&T> { self.0.get() }
        pub fn call_once(&self, initialize: impl FnOnce() -> T) -> &T {
            self.0.get_or_init(initialize)
        }
    }
}
mod microfont {
    pub mod raster_line {
        pub const GLYPH_CHARACTERS: &[char] = &['A', '?', '§', '←'];
        pub fn glyph_strips(ch: char) -> Option<()> {
            GLYPH_CHARACTERS.contains(&ch).then_some(())
        }
    }
    pub mod raster_quad {
        pub const GLYPH_CHARACTERS: &[char] = &[];
        pub fn glyph_quads(_: char) -> Option<()> { None }
    }
}
static NATIVE_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[derive(Clone, Copy, Default)] struct AtlasConfig { glyph_height_px: Option<u32>, line_width_px: f32 }
impl AtlasConfig {
    fn cell_extent(&self) -> (u32, u32) { (8, self.glyph_height_px.unwrap() + 2) }
    fn gutter(&self) -> u32 { 1 }
}
struct Surface { phys: u64, width: u32, height: u32, pitch_bytes: u32 }
struct NativeAtlas;
impl NativeAtlas { fn surface(&self) -> Surface { panic!("native hardware must not execute") } }
fn with_warmed_atlas<R>(_: AtlasConfig, _: impl FnOnce(&mut NativeAtlas) -> Result<R, &'static str>)
    -> Result<R, &'static str> {
    NATIVE_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Err("native-unavailable")
}
mod phys { pub fn phys_to_virt(_: usize) -> usize { panic!("no physical GPU mapping") } }
mod intel {
    pub struct DmaFlushRows;
    impl DmaFlushRows { pub fn new(_: *mut u8, _: usize, _: usize, _: usize) -> Self { Self } }
    pub fn dma_flush_strided_row_spans(_: &[DmaFlushRows]) -> bool { panic!("no GPU DMA") }
}
''' + production + r'''
#[cfg(test)] mod atlas_tests {
    use super::*;
    fn native_fixture() -> OwnedAlphaAtlas {
        let mut pixels = vec![0; SLOT_COUNT * 8 * 13];
        for character in ['A', '?', '§', '←'] {
            let slot = glyph_slot(character).unwrap();
            for y in 0..11usize { for x in 0..6usize {
                pixels[slot * 8 * 13 + (y + 1) * 8 + x + 1] = (x + y * 6 + 1) as u8;
            }}
        }
        OwnedAlphaAtlas { em_height_px: 11, line_width_px: 2.75,
            glyph_width: 8, glyph_height: 13, gutter_px: 1,
            source: AlphaAtlasSource::Native, pixels }
    }
    #[test] fn retired_bitmap_entry_never_allocates_or_publishes_coverage() {
        for height in [0, 1, 11, 31, 253, u32::MAX] {
            assert_eq!(build_bitmap_alpha_atlas(height).err(), Some("microfont-bitmap-atlas-disabled"));
            assert!(ALPHA_ATLAS.get().is_none());
            assert!(ready_atlas().is_none());
        }
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    #[test] fn native_failure_cannot_publish_a_second_font_identity() {
        assert_eq!(build_shell3_alpha_atlas(11).err(), Some("native-unavailable"));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(ready_atlas().is_none());
        assert_eq!(build_bitmap_alpha_atlas(11).err(), Some("microfont-bitmap-atlas-disabled"));
        assert!(ready_atlas().is_none());
        let native = ALPHA_ATLAS.call_once(native_fixture);
        assert!(core::ptr::eq(ready_atlas().unwrap(), native));
        assert!(core::ptr::eq(build_shell3_alpha_atlas(11).unwrap(), native));
        assert_eq!(build_shell3_alpha_atlas(22).err(), Some("microfont-atlas-already-sized"));
    }
    #[test] fn native_layout_and_area_filtering_are_unchanged() {
        let atlas = native_fixture();
        let mask = atlas.glyph('A').unwrap();
        for y in 0..11 { for x in 0..6 {
            assert_eq!(atlas.sample(mask, x, y, 6, 11), (x + y * 6 + 1) as u8);
        }}
        assert_eq!(atlas.sample(mask, 0, 0, 3, 11), 2);
        assert_eq!(atlas.sample(mask, 1, 0, 3, 11), 4);
        assert!(atlas.glyph('漢').is_none());
        assert!(atlas.glyph('ỹ').is_none());
        assert!(atlas.glyphs().all(|(character, _)| character != 'ỹ'));
        for y in 0..11 { for x in 0..6 {
            assert_eq!(atlas.coverage('漢',x,y,6,11),atlas.coverage('?',x,y,6,11));
        }}
    }
    #[test] fn invalid_sampling_extents_do_not_read_coverage() {
        let atlas = native_fixture();
        let mask = atlas.glyph('A').unwrap();
        for (x,y,width,height) in [(0,0,0,1),(0,0,1,0),(6,0,6,11),(0,11,6,11)] {
            assert_eq!(atlas.sample(mask,x,y,width,height),0);
            assert_eq!(atlas.coverage('A',x,y,width,height),0);
        }
    }
    #[test] fn text_copy_uses_native_tiles_and_clips() {
        let atlas = ALPHA_ATLAS.call_once(native_fixture);
        let mut destination = [19; 8 * 12];
        copy_text_mask(&mut destination, 8, 12, 5, 2, "A漢", 6, 11).unwrap();
        for y in 0..12 { for x in 0..8 {
            let expected = if x >= 5 && y >= 2 { atlas.coverage('A', (x - 5) as u32, (y - 2) as u32, 6, 11) } else { 19 };
            assert_eq!(destination[y * 8 + x], expected);
        }}
        assert_eq!(copy_text_mask(&mut destination, 0, 12, 0, 0, "A", 6, 11), Err("microfont-text-shape"));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}

'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-bitmap-alpha-atlas-") as directory:
        work = Path(directory)
        source, binary = work / "test.rs", work / "test"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", "-Adead_code",
                        "--target=x86_64-unknown-linux-gnu", "-C", "linker=" + str(ROOT / "tools/rust-host-ld"),
                        str(source), "-o", str(binary)], check=True, cwd=ROOT)
        listing = subprocess.check_output([str(binary), "--list"], text=True)
        names = [line.removesuffix(": test") for line in listing.splitlines()
                 if line.startswith("atlas_tests::") and line.endswith(": test")]
        assert len(names) == 5
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
