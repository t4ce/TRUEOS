#!/usr/bin/env python3
"""Run the production shared CPU font atlas without a GPU or kernel runtime.

Compile the real atlas type, methods, bitmap initializer and native entry point.
Only the native hardware boundary, Once storage and raster identities are host
fixtures. Every test runs in a fresh process because the atlas is immutable.
"""

import json
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
    for name in ("ALPHA_ATLAS", "CPU_ALPHA_ATLAS"):
        declaration = re.search(rf"^static {name}:.*?;$", source, re.MULTILINE)
        assert declaration, name
        production += declaration.group() + "\n"
    production = production.replace("super::shell3_font_service::hex", "crate::hex")
    production = production.replace("super::shell3_font_service::cpu_bitmap_fallback_enabled",
                                    "crate::cpu_bitmap_fallback_enabled")
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
static CPU_BITMAP_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
fn cpu_bitmap_fallback_enabled() -> bool {
    CPU_BITMAP_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}
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
''' + "#[path=" + json.dumps(str(ROOT / "src/r/services/shell3_hex_font.rs")) + "] mod hex;\n" + production + r'''
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
    fn raw_bit(words: &[u32; 4], x: u32, y: u32) -> u8 {
        let bits = u128::from(words[0]) | (u128::from(words[1]) << 32)
            | (u128::from(words[2]) << 64) | (u128::from(words[3]) << 96);
        if bits & (1 << (65 - y * 6 - x)) == 0 { 0 } else { 255 }
    }
    #[test] fn all_canonical_glyphs_are_exact_complete_cells() {
        assert!(ready_atlas().is_none());
        let atlas = build_bitmap_alpha_atlas(11).unwrap();
        assert!(ALPHA_ATLAS.get().is_none());
        assert!(core::ptr::eq(ready_atlas().unwrap(), atlas));
        assert_eq!((atlas.glyph_width, atlas.glyph_height, atlas.gutter_px), (6, 11, 0));
        assert_eq!(atlas.glyphs().count(), hex::GLYPH_COUNT);
        for (&cp, words) in hex::CODEPOINTS.iter().zip(&hex::MASKS) {
            let character = char::from_u32(cp).unwrap();
            let mask = atlas.glyph(character).unwrap();
            assert_eq!(mask.len(), 66);
            for y in 0..11 { for x in 0..6 {
                assert_eq!(atlas.sample(mask, x, y, 6, 11), raw_bit(words, x, y), "U+{cp:X}");
            }}
        }
        for character in ['█', '⣿', '←', '§', 'ẽ', 'ỹ', '🯹'] { assert!(atlas.glyph(character).is_some()); }
        assert_eq!(&atlas.glyph('█').unwrap()[64..], &[255, 255]);
        assert!(atlas.glyph('漢').is_none());
        for y in 0..11 { for x in 0..6 {
            assert_eq!(atlas.coverage('漢', x, y, 6, 11), atlas.coverage('?', x, y, 6, 11));
        }}
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    #[test] fn physical_size_is_baked_once_and_resampling_stays_bounded() {
        let atlas = build_bitmap_alpha_atlas(31).unwrap();
        assert_eq!((atlas.glyph_width, atlas.glyph_height), (17, 31));
        assert_eq!(atlas.pixels.len(), hex::GLYPH_COUNT * 17 * 31);
        for (&cp, words) in hex::CODEPOINTS.iter().zip(&hex::MASKS) {
            let mask = atlas.glyph(char::from_u32(cp).unwrap()).unwrap();
            for y in 0..31 { for x in 0..17 {
                assert_eq!(atlas.sample(mask, x, y, 17, 31), raw_bit(words, x * 6 / 17, y * 11 / 31));
            }}
            for y in 0..9 { for x in 0..5 { let _ = atlas.sample(mask, x, y, 5, 9); }}
        }
        assert!(core::ptr::eq(atlas, build_bitmap_alpha_atlas(31).unwrap()));
        assert_eq!(build_bitmap_alpha_atlas(22).err(), Some("microfont-bitmap-atlas-already-sized"));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    #[test] fn size_errors_do_not_publish_an_atlas() {
        for height in [0, 1, 10, 254, u32::MAX] {
            assert_eq!(build_bitmap_alpha_atlas(height).err(), Some("microfont-bitmap-atlas-size"));
            assert!(ready_atlas().is_none());
        }
        let atlas = build_bitmap_alpha_atlas(253).unwrap();
        assert_eq!((atlas.glyph_width, atlas.glyph_height), (138, 253));
        let mask = atlas.glyph('A').unwrap();
        for (x, y, width, height) in [(0, 0, 0, 1), (0, 0, 1, 0), (6, 0, 6, 11), (0, 11, 6, 11)] {
            assert_eq!(atlas.sample(mask, x, y, width, height), 0);
            assert_eq!(atlas.coverage('A', x, y, width, height), 0);
        }
    }
    #[test] fn native_and_bitmap_publication_remain_independent() {
        let bitmap = build_bitmap_alpha_atlas(11).unwrap();
        assert_eq!(build_shell3_alpha_atlas(11).err(), Some("native-unavailable"));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(core::ptr::eq(ready_atlas().unwrap(), bitmap));
        let native = ALPHA_ATLAS.call_once(native_fixture);
        assert!(core::ptr::eq(ready_atlas().unwrap(), native));
        assert!(core::ptr::eq(build_shell3_alpha_atlas(11).unwrap(), native));
        assert!(core::ptr::eq(build_bitmap_alpha_atlas(11).unwrap(), bitmap));
        assert_eq!(build_shell3_alpha_atlas(22).err(), Some("microfont-atlas-already-sized"));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
    #[test] fn native_layout_and_filtering_are_unchanged() {
        let atlas = native_fixture();
        let mask = atlas.glyph('A').unwrap();
        for y in 0..11 { for x in 0..6 {
            assert_eq!(atlas.sample(mask, x, y, 6, 11), (x + y * 6 + 1) as u8);
        }}
        assert_eq!(atlas.sample(mask, 0, 0, 3, 11), 2); // (1 + 2) / 2, rounded
        assert_eq!(atlas.sample(mask, 1, 0, 3, 11), 4);
        assert!(atlas.glyph('漢').is_none());
        assert!(atlas.glyph('ỹ').is_none()); // bitmap coverage cannot create native slots
        assert!(atlas.glyphs().all(|(character, _)| character != 'ỹ'));
    }
    #[test] fn desktop_rejects_bitmap_initialization_before_publication() {
        CPU_BITMAP_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        for height in [0, 11, 31, u32::MAX] {
            assert_eq!(build_bitmap_alpha_atlas(height).err(), Some("microfont-bitmap-atlas-disabled"));
            assert!(CPU_ALPHA_ATLAS.get().is_none());
            assert!(ready_atlas().is_none());
        }
        let native = ALPHA_ATLAS.call_once(native_fixture);
        assert!(core::ptr::eq(ready_atlas().unwrap(), native));
        assert!(core::ptr::eq(build_shell3_alpha_atlas(11).unwrap(), native));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    #[test] fn desktop_hides_preexisting_bitmap_and_keeps_native_priority() {
        let bitmap = build_bitmap_alpha_atlas(11).unwrap();
        assert!(core::ptr::eq(ready_atlas().unwrap(), bitmap));
        CPU_BITMAP_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(ready_atlas().is_none());
        assert_eq!(build_bitmap_alpha_atlas(11).err(), Some("microfont-bitmap-atlas-disabled"));
        let native = ALPHA_ATLAS.call_once(native_fixture);
        assert!(core::ptr::eq(ready_atlas().unwrap(), native));
        CPU_BITMAP_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(core::ptr::eq(ready_atlas().unwrap(), native));
        assert_eq!(NATIVE_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    #[test] fn text_copy_uses_shared_bitmap_tiles_and_clips() {
        let atlas = build_bitmap_alpha_atlas(11).unwrap();
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
        assert len(names) == 8
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
