#!/usr/bin/env python3
"""Exercise CPU font publication and early diagnostic text without a GPU.

Compile the production startup, display gate, retained text painter and canonical
bitmap module. Only the atlas producer and service infrastructure are doubled;
the bitmap producer's coverage is checked by test_bitmap_alpha_atlas.py.
Any attempt to enter the native font backend or query GPU readiness fails here.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
FONT = "src/r/services/shell3_font_service.rs"
LOG = "src/r/services/microfont_log_service.rs"


def surface_impl() -> str:
    source = (ROOT / LOG).read_text()
    start = source.index("impl Surface {")
    end = source.index("\n}\n", start) + 3
    return source[start:end]


def clear_surface() -> str:
    source = (ROOT / LOG).read_text()
    start = source.index("unsafe fn clear_surface(")
    end = source.index("\n}\n", start) + 3
    return source[start:end]


def microfont_spec() -> str:
    source = (ROOT / "src/r/services/spawn_service.rs").read_text()
    specs = re.findall(
        r'    TaskSpec::enabled_gated\(\n        "microfont-warm",.*?\n    \),',
        source, re.DOTALL,
    )
    if len(specs) != 1:
        raise ValueError(f"expected one microfont-warm task specification, got {len(specs)}")
    return specs[0].strip().removesuffix(",")


def harness() -> str:
    source = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as trueos_executor;
pub struct Spawner;
pub struct SpawnError;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[macro_export]
macro_rules! log_important { ($($token:tt)*) => {{}}; }

mod spin {
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {
        pub const fn new() -> Self { Self(std::sync::OnceLock::new()) }
        pub fn get(&self) -> Option<&T> { self.0.get() }
        pub fn call_once(&self, build: impl FnOnce() -> T) -> &T { self.0.get_or_init(build) }
    }
    pub type Mutex<T> = std::sync::Mutex<T>;
}

mod r { pub mod readiness {
'''
    source += constant("src/r/readiness.rs", "BACKGROUND_AP_WORKER_READY") + "\n"
    source += constant("src/r/readiness.rs", "UI4_COMPOSITOR_READY") + "\n"
    source += "}}\n#[path = " + json.dumps(str(ROOT / "src/r/spawn_spec.rs")) + "]\nmod spawn_spec;\n"
    source += r'''

mod ui4 {
    use super::Cell;
    thread_local! {
        pub static OUTPUT: Cell<Option<(u32,u32)>> = const { Cell::new(Some((1920,1080))) };
        pub static PHYSICAL: Cell<Option<(u32,u32)>> = const { Cell::new(None) };
    }
    pub fn output_dimensions() -> Option<(u32,u32)> { OUTPUT.with(Cell::get) }
    pub fn physical_extent_pixels_fraction(_:u32, _:u32, _:u32) -> Option<(u32,u32)> {
        PHYSICAL.with(Cell::get)
    }
}

static GPU_AVAILABLE: AtomicBool = AtomicBool::new(false);
static NATIVE_ATLAS_QUARANTINED: AtomicBool = AtomicBool::new(true);
static GPU_CALLS: AtomicUsize = AtomicUsize::new(0);
fn gpu_call() -> ! {
    GPU_CALLS.fetch_add(1, Ordering::Relaxed);
    panic!("CPU font startup entered a GPU-dependent path");
}
mod intel {
    use super::Cell;
    thread_local! {
        pub static FLUSH: Cell<Option<(usize,usize,usize,usize)>> = const { Cell::new(None) };
    }
    pub fn guc_submission_ready() -> bool { super::gpu_call() }
    pub mod render { pub fn warm_state() -> Option<()> { super::super::gpu_call() } }
    pub fn dma_flush_strided_rows(address:*mut u8, width:usize, pitch:usize, height:usize) -> bool {
        FLUSH.with(|flush| flush.set(Some((address as usize,width,pitch,height))));
        true
    }
}
mod virtio_gpu_logo {
    pub fn output_dimensions() -> Option<(u32,u32)> { super::gpu_call() }
}

mod microfont {
'''
    source += "include!(" + json.dumps(str(ROOT / "vendor/microfont/src/cp850_characters.rs")) + ");\n"
    source += constant("vendor/microfont/src/lib.rs", "FWIDTH") + "\n"
    source += constant("vendor/microfont/src/lib.rs", "FHEIGHT") + "\n"
    source += item("vendor/microfont/src/cp850.rs", "glyph_character")
    source += "}\nmod shell3_font_service {\n"
    source += "use alloc::{boxed::Box, vec::Vec};\nuse crate::{microfont, spin};\n"
    source += "#[path = " + json.dumps(str(ROOT / "src/r/services/shell3_hex_font.rs")) + "]\npub(crate) mod hex;\n"
    source += constant(FONT, "DEFAULT_UI4_FONT_SCALE") + "\n"
    source += item(FONT, "cell_extent") + "\n"
    source += item(FONT, "HexAtlas") + "\n"
    font_source = (ROOT / FONT).read_text()
    impl_start = font_source.index("impl HexAtlas {")
    impl_end = font_source.index("\n}\n", impl_start) + 3
    source += font_source[impl_start:impl_end]
    source += r'''
// This double tests publication/ordering, not atlas production. The production
// bitmap atlas and its sampling are compiled by the companion coverage test.
struct OwnedAlphaAtlas;
static ALPHA: OwnedAlphaAtlas = OwnedAlphaAtlas;
impl OwnedAlphaAtlas {
    fn glyph(&self, character:char) -> Option<&'static [u8]> {
        (character == '?').then_some(&[0,255][..])
    }
    fn glyphs(&self) -> impl Iterator<Item=(char,&'static [u8])> {
        std::iter::once(('?',self.glyph('?').unwrap()))
    }
}
static BITMAP_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn build_bitmap_alpha_atlas(height:u32) -> Result<&'static OwnedAlphaAtlas,&'static str> {
    assert_eq!(height,22);
    BITMAP_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    Ok(&ALPHA)
}
fn build_shell3_alpha_atlas(_:u32) -> Result<&'static OwnedAlphaAtlas,&'static str> {
    crate::gpu_call()
}
#[derive(Default)]
struct CellCache;
'''
    source += item(FONT, "Font") + "\n"
    source += "static FONT: spin::Once<Font> = spin::Once::new();\n"
    source += item(FONT, "prepare_once") + "\n"
    source += r'''
#[test]
fn cold_cpu_font_publishes_with_gpu_unavailable_and_native_atlas_quarantined() {
    use std::sync::atomic::Ordering;
    assert!(!crate::GPU_AVAILABLE.load(Ordering::Relaxed));
    assert!(crate::NATIVE_ATLAS_QUARANTINED.load(Ordering::Relaxed));
    assert!(FONT.get().is_none());
    crate::ui4::OUTPUT.with(|output| output.set(None));
    assert_eq!(prepare_once(), Err("shell3-font-physical-size-unavailable"));
    assert_eq!(BITMAP_CALLS.load(Ordering::Relaxed),0);
    assert!(FONT.get().is_none());
    crate::ui4::OUTPUT.with(|output| output.set(Some((1920,1080))));
    assert_eq!(cell_extent(DEFAULT_UI4_FONT_SCALE),Some((12,22)));
    prepare_once().unwrap();
    let font=FONT.get().expect("CPU font must become ready");
    assert!(font.alpha.glyph('?').unwrap().contains(&255));
    assert_eq!(font.hex.coverage.len(),12*22*hex::GLYPH_COUNT);
    assert!(font.hex.coverage.contains(&255));
    let full=hex::resolve('█',[255;4]).0;
    for y in 0..22 { for x in 0..12 {
        assert_eq!(font.hex.sample(full,x,y,12,22,false),255);
    }}
    prepare_once().unwrap();
    assert_eq!(BITMAP_CALLS.load(Ordering::Relaxed),1);
    assert_eq!(crate::GPU_CALLS.load(Ordering::Relaxed),0);
}
}

mod spawn_service {
use crate::spawn_spec::{SpawnAttempt,TaskSpec};
static MICROFONT_WARM_STARTED:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
fn spawn_microfont_warm(_:crate::Spawner)->SpawnAttempt { SpawnAttempt::Spawned }
'''
    source += item("src/r/services/spawn_service.rs", "microfont_warm_gate")
    source += "fn microfont_spec()->TaskSpec {\n" + microfont_spec() + "\n}\n"
    source += r'''
#[test]
fn cpu_font_gate_needs_an_output_but_no_gpu_readiness() {
    crate::ui4::OUTPUT.with(|output| output.set(None));
    assert!(!microfont_warm_gate());
    crate::ui4::OUTPUT.with(|output| output.set(Some((1920,1080))));
    assert!(microfont_warm_gate());
    assert_eq!(crate::GPU_CALLS.load(std::sync::atomic::Ordering::Relaxed),0);
}

#[test]
fn font_task_can_start_before_the_ui4_compositor_is_ready() {
    let spec=microfont_spec();
    let available=crate::r::readiness::BACKGROUND_AP_WORKER_READY;
    assert_eq!(available & crate::r::readiness::UI4_COMPOSITOR_READY,0);
    assert_eq!(available & spec.required,spec.required);
    assert!(!spec.disabled.load(std::sync::atomic::Ordering::Relaxed));
    crate::ui4::OUTPUT.with(|output| output.set(Some((1920,1080))));
    assert!((spec.gate)());
    assert_eq!(crate::GPU_CALLS.load(std::sync::atomic::Ordering::Relaxed),0);
}
}

mod microfont_log_service {
use crate::microfont;
'''
    source += constant(LOG, "SCALE") + "\n"
    source += constant(LOG, "BG") + "\n"
    source += item(LOG, "Surface") + "\n" + surface_impl()
    source += clear_surface() + "\n"
    source += item(LOG, "paint_text") + "\n"
    source += r'''
const GUARD:u32=0x1234_5678;

fn surface(pixels:&mut [u32],width:usize,height:usize,pitch:usize)->Surface {
    Surface { phys:4096,gpu:8192,virt:unsafe {pixels.as_mut_ptr().add(1)} as usize,
        byte_len:pitch*height*4,width:width as u32,height:height as u32,
        pitch_bytes:(pitch*4) as u32 }
}

// Expected bits are read directly from the canonical 66-bit cells rather than
// invoking the production sampler which the painter itself uses.
fn expected_coverage(byte:u8,x:usize,y:usize,scale:usize)->u32 {
    use crate::shell3_font_service::hex;
    let character=microfont::glyph_character(byte).unwrap_or(' ');
    let glyph=hex::CODEPOINTS.binary_search(&(character as u32))
        .map_or(hex::FALLBACK_GLYPH as usize,|glyph|glyph);
    let bit=65-(y/scale)*6-x/scale;
    ((hex::MASKS[glyph][bit/32]>>(bit%32))&1)*255
}

fn assert_paint(width:usize,height:usize,padding:usize,text:&[u8],scale:usize,top:usize) {
    let pitch=width+padding;
    let mut pixels=vec![GUARD;pitch*height+2];
    let s=surface(&mut pixels,width,height,pitch);
    assert!(s.valid());
    crate::intel::FLUSH.with(|flush|flush.set(None));
    paint_text(s,text,scale,top);
    let cell_width=6*scale;
    let text_width=text.len()*cell_width;
    let left=width.saturating_sub(text_width)/2;
    let visible_width=text_width.min(width-left);
    let visible_height=(11*scale).min(height.saturating_sub(top));
    for y in 0..height { for x in 0..pitch {
        let expected=if x>=left && x<left+visible_width && y>=top && y<top+visible_height {
            let local_x=x-left;
            let alpha=expected_coverage(text[local_x/cell_width],local_x%cell_width,y-top,scale);
            0xff00_0000|((255-alpha)*0x010101)
        } else {GUARD};
        assert_eq!(pixels[1+y*pitch+x],expected,"pixel {x},{y}, {width}x{height}, scale={scale}, top={top}");
    }}
    assert_eq!(pixels[0],GUARD);
    assert_eq!(pixels[pixels.len()-1],GUARD);
    let expected_flush=if visible_width==0 || visible_height==0 {None} else {
        Some((s.virt+top*pitch*4+left*4,visible_width*4,pitch*4,visible_height))
    };
    assert_eq!(crate::intel::FLUSH.with(std::cell::Cell::get),expected_flush);
}

#[test]
fn early_diagnostics_use_canonical_bits_and_preserve_clipping_padding_and_guards() {
    let text=b"USB \xDB\xCE\x01?";
    for (width,height,padding) in [(96,44,0),(125,60,11),(160,80,7),(80,45,16)] {
        for scale in [1,2,4,8] { for top in [0,height-3,height,height+1] {
            assert_paint(width,height,padding,text,scale,top);
        }}
    }
    assert_paint(96,44,4,b"",1,0);
    assert_paint(96,44,4,b"USB",0,0);
    assert_eq!(crate::GPU_CALLS.load(std::sync::atomic::Ordering::Relaxed),0);
}

#[test]
fn cleared_diagnostic_plane_is_transparent_and_text_is_opaque_only_in_its_rectangle() {
    let (width,height,pitch)=(96usize,64usize,112usize);
    let mut pixels=vec![GUARD;pitch*height+2];
    let s=surface(&mut pixels,width,height,pitch);
    assert!(s.valid());
    unsafe {clear_surface(s);}
    for y in 0..height {for x in 0..pitch {
        assert_eq!(pixels[1+y*pitch+x],if x<width {0} else {GUARD});
    }}
    let (text,scale,top)=(b"USB",2usize,7usize);
    paint_text(s,text,scale,top);
    let cell_width=6*scale;
    let left=(width-text.len()*cell_width)/2;
    for y in 0..height {for x in 0..pitch {
        let expected=if x>=width {GUARD}
            else if (left..left+text.len()*cell_width).contains(&x)
                && (top..top+11*scale).contains(&y) {
                let local_x=x-left;
                let alpha=expected_coverage(text[local_x/cell_width],local_x%cell_width,y-top,scale);
                0xff00_0000|((255-alpha)*0x010101)
            } else {0};
        assert_eq!(pixels[1+y*pitch+x],expected,"pixel {x},{y}");
    }}
    // Repainting starts with another transparent clear, removing old text.
    unsafe {clear_surface(s);}
    for y in 0..height {for x in 0..pitch {
        assert_eq!(pixels[1+y*pitch+x],if x<width {0} else {GUARD});
    }}
    assert_eq!(pixels[0],GUARD);
    assert_eq!(pixels[pixels.len()-1],GUARD);
}

#[test]
fn empty_zero_and_overflowing_extents_never_write_or_flush() {
    let mut pixels=vec![GUARD;112*64+2];
    let s=surface(&mut pixels,96,64,112);
    let mut cases=vec![(b"".as_slice(),1,0),(b"USB".as_slice(),0,0),
        (b"USB".as_slice(),usize::MAX,0),(b"USB".as_slice(),1,usize::MAX),
        (b"USB".as_slice(),u32::MAX as usize,0)];
    if usize::BITS>32 { cases.push((b"USB".as_slice(),1usize.checked_shl(32).unwrap(),0)); }
    for (text,scale,top) in cases {
        crate::intel::FLUSH.with(|flush|flush.set(None));
        paint_text(s,text,scale,top);
        assert!(pixels.iter().all(|pixel|*pixel==GUARD),"scale={scale}, top={top}");
        assert_eq!(crate::intel::FLUSH.with(std::cell::Cell::get),None);
    }
}

#[test]
fn surface_handoff_rejects_bad_pitch_allocation_alignment_and_dimensions() {
    let mut pixels=vec![GUARD;112*64+2];
    let s=surface(&mut pixels,96,64,112);
    assert!(s.valid());
    for invalid in [
        Surface {virt:0,..s},Surface {virt:s.virt+1,..s},
        Surface {phys:0,..s},Surface {phys:s.phys+1,..s},
        Surface {gpu:s.gpu+1,..s},Surface {pitch_bytes:95*4,..s},
        Surface {pitch_bytes:s.pitch_bytes+1,..s},Surface {byte_len:s.byte_len-1,..s},
        Surface {byte_len:usize::MAX,..s},Surface {virt:usize::MAX-3,..s},
        Surface {width:71,..s},Surface {height:43,..s},
    ] {assert!(!invalid.valid(),"{invalid:?}");}
}
}
'''
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-cpu-font-startup-") as temporary:
        directory = Path(temporary)
        code = directory / "tests.rs"
        binary = directory / "tests"
        code.write_text(harness())
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(code), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
