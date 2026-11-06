#!/usr/bin/env python3
"""Exercise native desktop fonts and the narrowly scoped laptop CPU path.

Compile the production startup, display gate, retained text painter and canonical
bitmap module. Only the atlas producer and service infrastructure are doubled;
the bitmap producer's coverage is checked by test_bitmap_alpha_atlas.py.
Run each production-source test in a fresh process because font publication is
immutable. Desktop startup must call the native producer, and native failure
must propagate without publishing bitmap coverage.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item
from test_mouse_visual_cursors import block


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
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(value:T)->Self { Self(std::sync::Mutex::new(value)) }
        pub fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> {self.0.try_lock().ok()}
    }
}

mod r { pub mod readiness {
'''
    source += constant("src/r/readiness.rs", "BACKGROUND_AP_WORKER_READY") + "\n"
    source += constant("src/r/readiness.rs", "UI4_COMPOSITOR_READY") + "\n"
    source += "pub static BITS:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);\n"
    source += "pub fn is_set(required:u32)->bool { BITS.load(std::sync::atomic::Ordering::Relaxed)&required==required }\n"
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

mod pci {
    use std::sync::atomic::{AtomicU32, Ordering};
    pub static ID: AtomicU32 = AtomicU32::new(0x4680_8086);
    pub static REVISION: AtomicU32 = AtomicU32::new(0x0c);
    pub fn config_read_u32(_:u8,_:u8,_:u8,offset:u8) -> u32 {
        match offset {0=>ID.load(Ordering::Relaxed),8=>REVISION.load(Ordering::Relaxed),_=>panic!("unexpected PCI offset")}
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
    #[derive(Clone,Copy)]
    pub struct Dev {pub bus:u8,pub slot:u8,pub function:u8}
    pub static CLAIMED:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(true);
    pub fn claimed_device()->Option<Dev> {
        CLAIMED.load(std::sync::atomic::Ordering::Relaxed).then_some(Dev {bus:0,slot:2,function:0})
    }
    pub mod tgl_native_panel {
        use super::Dev;
        use std::sync::atomic::{AtomicBool,Ordering};
        pub static SCANOUT_LATCHED:AtomicBool=AtomicBool::new(false);
'''
    for name in ("ENABLED", "PHYSICAL_ID", "PHYSICAL_REVISION"):
        source += constant("src/intel/tgl_native_panel.rs", name) + "\n"
    for name in ("is_target", "ui4_handoff_active"):
        source += item("src/intel/tgl_native_panel.rs", name) + "\n"
    source += "}\n" + item("src/intel/mod.rs", "laptop_cpu_font_fallback_enabled") + "\n"
    source += r'''
    thread_local! {
        pub static FLUSH: Cell<Option<(usize,usize,usize,usize)>> = const { Cell::new(None) };
    }
    pub fn guc_submission_ready() -> bool {
        super::GPU_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        super::GPU_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub mod render { pub fn warm_state() -> Option<()> {
        super::super::GPU_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        super::super::GPU_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed).then_some(())
    } }
    pub fn dma_flush_strided_rows(address:*mut u8, width:usize, pitch:usize, height:usize) -> bool {
        FLUSH.with(|flush| flush.set(Some((address as usize,width,pitch,height))));
        true
    }
}
mod virtio_gpu_logo {
    pub static AVAILABLE:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub fn output_dimensions() -> Option<(u32,u32)> {
        super::GPU_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        AVAILABLE.load(std::sync::atomic::Ordering::Relaxed).then_some((1920,1080))
    }
}

fn set_laptop(enabled:bool) {
    use std::sync::atomic::Ordering;
    pci::ID.store(if enabled {0x9a49_8086} else {0x4680_8086},Ordering::Relaxed);
    pci::REVISION.store(if enabled {1} else {0x0c},Ordering::Relaxed);
    intel::tgl_native_panel::SCANOUT_LATCHED.store(enabled,Ordering::Relaxed);
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
pub(crate) struct OwnedAlphaAtlas { native: bool }
static ALPHA: OwnedAlphaAtlas = OwnedAlphaAtlas { native: false };
static NATIVE_ALPHA: OwnedAlphaAtlas = OwnedAlphaAtlas { native: true };
impl OwnedAlphaAtlas {
    pub(crate) fn glyph(&self, character:char) -> Option<&'static [u8]> {
        character.is_ascii().then_some(if self.native { &[17,128][..] } else { &[0,255][..] })
    }
    fn glyphs(&self) -> impl Iterator<Item=(char,&'static [u8])> {
        std::iter::once(('?',self.glyph('?').unwrap()))
    }
    pub(crate) fn coverage(&self, character:char, x:u32, y:u32, width:u32, height:u32) -> u8 {
        self.sample(self.glyph(character).or_else(||self.glyph('?')).unwrap(),x,y,width,height)
    }
    pub(crate) fn sample(&self, mask:&[u8], x:u32, y:u32, width:u32, height:u32) -> u8 {
        mask[((x+y) as usize)%mask.len()]
    }
}
static BITMAP_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static NATIVE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn build_bitmap_alpha_atlas(height:u32) -> Result<&'static OwnedAlphaAtlas,&'static str> {
    assert_eq!(height,22);
    BITMAP_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    Ok(&ALPHA)
}
fn build_shell3_alpha_atlas(_:u32) -> Result<&'static OwnedAlphaAtlas,&'static str> {
    NATIVE_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    if crate::NATIVE_ATLAS_QUARANTINED.load(std::sync::atomic::Ordering::Relaxed) {
        Err("microfont-atlas-quarantined")
    } else {
        Ok(&NATIVE_ALPHA)
    }
}
#[derive(Default)]
struct CellCache;
'''
    source += item(FONT, "Font") + "\n"
    source += "static FONT: spin::Once<Font> = spin::Once::new();\n"
    source += block(font_source, "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]\npub(crate) enum FontFace") + "\n"
    source += item(FONT, "cpu_bitmap_fallback_enabled") + "\n"
    source += item(FONT, "effective_face") + "\n"
    source += item(FONT, "prepare_once") + "\n"
    source += block(font_source, "#[derive(Clone, Copy)]\nenum CellMask") + "\n"
    source += item(FONT, "resolved_face") + "\n"
    source += item(FONT, "resolved_glyph") + "\n"
    source += r'''
pub(crate) fn ready_atlas() -> Option<&'static OwnedAlphaAtlas> {
    FONT.get().map(|font|font.alpha)
}
pub(crate) fn prepare_native_for_painter() {
    crate::set_laptop(false);
    crate::NATIVE_ATLAS_QUARANTINED.store(false,std::sync::atomic::Ordering::Relaxed);
    prepare_once().unwrap();
}

#[test]
fn cold_cpu_font_publishes_with_gpu_unavailable_and_native_atlas_quarantined() {
    use std::sync::atomic::Ordering;
    crate::set_laptop(true);
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
    let hex_atlas=font.hex.as_ref().expect("laptop must have its explicit bitmap path");
    assert_eq!(hex_atlas.coverage.len(),12*22*hex::GLYPH_COUNT);
    assert!(hex_atlas.coverage.contains(&255));
    let full=hex::resolve('█',[255;4]).0;
    for y in 0..22 { for x in 0..12 {
        assert_eq!(hex_atlas.sample(full,x,y,12,22,false),255);
    }}
    prepare_once().unwrap();
    assert_eq!(BITMAP_CALLS.load(Ordering::Relaxed),1);
    assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),0);
    assert_eq!(crate::GPU_CALLS.load(Ordering::Relaxed),0);
    let (mask, _) = resolved_face(font,FontFace::Hex,'A',[255;4]).unwrap();
    assert!(matches!(mask,CellMask::Hex(_)));
}

#[test]
fn desktop_publishes_native_font_without_allocating_bitmap_cells() {
    use std::sync::atomic::Ordering;
    crate::set_laptop(false);
    crate::NATIVE_ATLAS_QUARANTINED.store(false,Ordering::Relaxed);
    prepare_once().unwrap();
    let font=FONT.get().expect("desktop native atlas must publish");
    assert!(std::ptr::eq(font.alpha,&NATIVE_ALPHA));
    assert!(font.hex.is_none(),"desktop must not allocate bitmap cells");
    assert_eq!(BITMAP_CALLS.load(Ordering::Relaxed),0);
    assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),1);
    for requested in [FontFace::Raster,FontFace::Hex] {
        let (mask,foreground)=resolved_face(font,requested,'A',[23,47,71,255]).unwrap();
        assert!(matches!(mask,CellMask::Raster([17,128])),"requested {requested:?} exposed bitmap text");
        assert_eq!(foreground,[23,47,71,255]);
    }
    prepare_once().unwrap();
    assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),1);
}

#[test]
fn desktop_native_quarantine_propagates_without_silent_bitmap_fallback() {
    use std::sync::atomic::Ordering;
    crate::set_laptop(false);
    assert!(crate::NATIVE_ATLAS_QUARANTINED.load(Ordering::Relaxed));
    assert_eq!(prepare_once(),Err("microfont-atlas-quarantined"));
    assert!(FONT.get().is_none());
    assert_eq!(BITMAP_CALLS.load(Ordering::Relaxed),0);
    assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),1);
    assert!(crate::NATIVE_ATLAS_QUARANTINED.load(Ordering::Relaxed));
}

#[test]
fn bitmap_policy_requires_exact_physical_laptop_revision_and_latched_panel() {
    use std::sync::atomic::Ordering;
    crate::set_laptop(true);
    assert!(cpu_bitmap_fallback_enabled());
    for (id,revision,latched,claimed,expected) in [
        (0x4680_8086,0x0c,true,true,false),
        (0x9a49_8086,0x02,true,true,false),
        (0x9a49_8086,0x01,false,true,false),
        (0x9a49_8086,0x01,true,false,false),
        (0x9a49_1234,0x01,true,true,false),
        (0x9a49_8086,0x01,true,true,true),
    ] {
        crate::pci::ID.store(id,Ordering::Relaxed);
        crate::pci::REVISION.store(revision,Ordering::Relaxed);
        crate::intel::tgl_native_panel::SCANOUT_LATCHED.store(latched,Ordering::Relaxed);
        crate::intel::CLAIMED.store(claimed,Ordering::Relaxed);
        assert_eq!(cpu_bitmap_fallback_enabled(),expected,"id={id:x} rev={revision:x} latch={latched} claimed={claimed}");
    }
}
}

mod microfont_mask_service {
    use alloc::vec::Vec;
    use crate::spin;
    pub(crate) use crate::shell3_font_service::ready_atlas;
    static START_GLYPH:spin::Once<OwnedGlyphMask>=spin::Once::new();
    static START_GLYPH_WARM:spin::Mutex<()>=spin::Mutex::new(());
    static START_GLYPH_READY:Ready=Ready;
    struct Ready;
    impl Ready {fn notify_all(&self) {}}
    const START_GLYPH_EM_MM:u32=50;
    fn home_button_em_height()->Option<u32> {Some(147)}
    static NATIVE_HOME_CALLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
    pub(crate) static CPU_HOME_CALLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
    fn build_home_button_mask(height:u32)->Result<OwnedGlyphMask,&'static str> {
        NATIVE_HOME_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        if crate::NATIVE_ATLAS_QUARANTINED.load(std::sync::atomic::Ordering::Relaxed) {
            Err("native-home-quarantined")
        } else {
            Ok(OwnedGlyphMask {width:1,height:1,em_height_px:height,pixels:vec![17]})
        }
    }
'''
    source += item("src/r/services/microfont_mask_service.rs", "OwnedGlyphMask") + "\n"
    source += item("src/r/services/microfont_mask_service.rs", "warm_start_glyph") + "\n"
    source += r'''
    #[test]
    fn desktop_home_glyph_uses_native_line_asset_producer() {
        use std::sync::atomic::Ordering;
        crate::set_laptop(false);
        crate::NATIVE_ATLAS_QUARANTINED.store(false,Ordering::Relaxed);
        warm_start_glyph().unwrap();
        assert_eq!(START_GLYPH.get().unwrap().pixels,vec![17]);
        warm_start_glyph().unwrap();
        assert_eq!(NATIVE_HOME_CALLS.load(Ordering::Relaxed),1);
        assert_eq!(CPU_HOME_CALLS.load(Ordering::Relaxed),0);
    }
    #[test]
    fn laptop_home_glyph_uses_cpu_line_asset_without_native_work() {
        use std::sync::atomic::Ordering;
        crate::set_laptop(true);
        warm_start_glyph().unwrap();
        assert_eq!(START_GLYPH.get().unwrap().pixels,vec![255]);
        assert_eq!(NATIVE_HOME_CALLS.load(Ordering::Relaxed),0);
        assert_eq!(CPU_HOME_CALLS.load(Ordering::Relaxed),1);
    }
    #[test]
    fn desktop_home_native_failure_cannot_switch_to_cpu() {
        use std::sync::atomic::Ordering;
        crate::set_laptop(false);
        assert_eq!(warm_start_glyph(),Err("native-home-quarantined"));
        assert!(START_GLYPH.get().is_none());
        assert_eq!(NATIVE_HOME_CALLS.load(Ordering::Relaxed),1);
        assert_eq!(CPU_HOME_CALLS.load(Ordering::Relaxed),0);
    }
}
mod microfont_home_cpu {
    pub(crate) fn build(height:u32)->Result<super::microfont_mask_service::OwnedGlyphMask,&'static str> {
        super::microfont_mask_service::CPU_HOME_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        Ok(super::microfont_mask_service::OwnedGlyphMask {width:1,height:1,em_height_px:height,pixels:vec![255]})
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
    crate::set_laptop(true);
    crate::ui4::OUTPUT.with(|output| output.set(None));
    assert!(!microfont_warm_gate());
    crate::ui4::OUTPUT.with(|output| output.set(Some((1920,1080))));
    assert!(microfont_warm_gate());
    assert_eq!(crate::GPU_CALLS.load(std::sync::atomic::Ordering::Relaxed),0);
}

#[test]
fn font_task_can_start_before_the_ui4_compositor_is_ready() {
    crate::set_laptop(true);
    let spec=microfont_spec();
    let available=crate::r::readiness::BACKGROUND_AP_WORKER_READY;
    assert_eq!(available & crate::r::readiness::UI4_COMPOSITOR_READY,0);
    assert_eq!(available & spec.required,spec.required);
    assert!(!spec.disabled.load(std::sync::atomic::Ordering::Relaxed));
    crate::ui4::OUTPUT.with(|output| output.set(Some((1920,1080))));
    assert!((spec.gate)());
    assert_eq!(crate::GPU_CALLS.load(std::sync::atomic::Ordering::Relaxed),0);
}

#[test]
fn desktop_font_gate_waits_for_native_gpu_or_virtio_readiness() {
    use std::sync::atomic::Ordering;
    crate::set_laptop(false);
    assert!(!microfont_warm_gate(),"output geometry alone must not start desktop native rendering");
    crate::GPU_AVAILABLE.store(true,Ordering::Relaxed);
    assert!(!microfont_warm_gate(),"native GPU readiness must still wait for compositor ownership");
    crate::r::readiness::BITS.store(crate::r::readiness::UI4_COMPOSITOR_READY,Ordering::Relaxed);
    assert!(microfont_warm_gate());
    crate::GPU_AVAILABLE.store(false,Ordering::Relaxed);
    crate::virtio_gpu_logo::AVAILABLE.store(true,Ordering::Relaxed);
    assert!(microfont_warm_gate());
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
    crate::set_laptop(true);
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
fn desktop_diagnostics_wait_for_native_atlas_and_never_show_early_hex() {
    crate::set_laptop(false);
    let (width,height,pitch)=(96usize,64usize,112usize);
    let mut pixels=vec![GUARD;pitch*height+2];
    let s=surface(&mut pixels,width,height,pitch);
    crate::intel::FLUSH.with(|flush|flush.set(None));
    paint_text(s,b"USB",2,7);
    assert!(pixels.iter().all(|&pixel|pixel==GUARD),"desktop painted bitmap before native atlas existed");
    assert_eq!(crate::intel::FLUSH.with(std::cell::Cell::get),None);

    crate::shell3_font_service::prepare_native_for_painter();
    paint_text(s,b"USB",2,7);
    let (left,visible_width,visible_height)=(30,36,22);
    for y in 0..height { for x in 0..pitch {
        let expected=if x>=left && x<left+visible_width && y>=7 && y<7+visible_height {
            let alpha=[17u32,128][((x-left)%12+y-7)%2];
            0xff00_0000|((255-alpha)*0x010101)
        } else {GUARD};
        assert_eq!(pixels[1+y*pitch+x],expected,"desktop native diagnostic pixel {x},{y}");
    }}
    assert_eq!(pixels[0],GUARD);
    assert_eq!(pixels[pixels.len()-1],GUARD);
}

#[test]
fn cleared_diagnostic_plane_is_transparent_and_text_is_opaque_only_in_its_rectangle() {
    crate::set_laptop(true);
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
    crate::set_laptop(true);
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
        listing = subprocess.check_output([str(binary), "--list"], text=True)
        names = [line.removesuffix(": test") for line in listing.splitlines()
                 if line.endswith(": test")]
        assert len(names) >= 20, names
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
