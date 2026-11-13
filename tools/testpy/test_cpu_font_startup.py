#!/usr/bin/env python3
"""Test native-only font publication, failures and the production readiness gate.

The historical filename guards against reintroducing the retired CPU mitigation.
Hardware atlas production is the fixture boundary; startup and face policy run
from production source. Each test gets a fresh process for immutable publication.
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


def harness() -> str:
    source = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as trueos_executor;
pub struct Spawner;
pub struct SpawnError;
use std::sync::atomic::{AtomicBool, AtomicUsize, AtomicU8, Ordering};
#[macro_export] macro_rules! log_important { ($($token:tt)*) => {{}}; }
mod spin {
 pub struct Once<T>(std::sync::OnceLock<T>);
 impl<T> Once<T> {
  pub const fn new()->Self {Self(std::sync::OnceLock::new())}
  pub fn get(&self)->Option<&T> {self.0.get()}
  pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}
 }
 pub struct Mutex<T>(std::sync::Mutex<T>);
 impl<T> Mutex<T> {pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}}
}
mod r {pub mod readiness {
'''
    for name in ("BACKGROUND_AP_WORKER_READY", "UI4_COMPOSITOR_READY"):
        source += constant("src/r/readiness.rs", name) + "\n"
    source += r'''
 pub static BITS:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);
 pub fn is_set(mask:u32)->bool {BITS.load(std::sync::atomic::Ordering::Relaxed)&mask==mask}
}}
mod ui4 {
 thread_local! {
  pub static OUTPUT:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(Some((1920,1080)))};
  pub static PHYSICAL:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(None)};
  pub static PHYSICAL_REQUESTS:std::cell::RefCell<Vec<(u32,u32,u32)>>=const {std::cell::RefCell::new(Vec::new())};
 }
 pub fn output_dimensions()->Option<(u32,u32)> {OUTPUT.with(std::cell::Cell::get)}
 pub fn physical_extent_pixels_fraction(width:u32,height:u32,denominator:u32)->Option<(u32,u32)> {
  PHYSICAL_REQUESTS.with(|requests|requests.borrow_mut().push((width,height,denominator)));
  PHYSICAL.with(std::cell::Cell::get)
 }
}
static GPU_AVAILABLE:AtomicBool=AtomicBool::new(false);
static WARM_AVAILABLE:AtomicBool=AtomicBool::new(true);
static TGL_ACTIVE:AtomicBool=AtomicBool::new(false);
static PROBE_CALLS:AtomicUsize=AtomicUsize::new(0);
static PROBE_MODE:AtomicU8=AtomicU8::new(0);
static STARTUP_EVENTS:std::sync::Mutex<Vec<&'static str>>=std::sync::Mutex::new(Vec::new());
mod pci {
 pub fn experimental_tgl_9a49_active()->bool {crate::TGL_ACTIVE.load(crate::Ordering::Relaxed)}
}
mod intel {
 pub fn guc_submission_ready()->bool {crate::GPU_AVAILABLE.load(crate::Ordering::Relaxed)}
 pub mod render {
  pub fn warm_state()->Option<()> {
   (crate::GPU_AVAILABLE.load(crate::Ordering::Relaxed)
    &&crate::WARM_AVAILABLE.load(crate::Ordering::Relaxed)).then_some(())
  }
'''
    # Keep both acceptance predicates real; only the physical probe boundary
    # returns controlled readback/retirement results.
    contract = "src/intel/render/native_font_probe_contract.rs"
    probe = "src/intel/render/native_font_probe.rs"
    source += "mod native_font_probe_contract {\n" + item(contract, "Pixels")
    source += block((ROOT / contract).read_text(), "impl Pixels") + "\n}\n"
    source += item(probe, "NativeMicrofontR8Proof")
    source += block((ROOT / probe).read_text(), "impl NativeMicrofontR8Proof")
    source += r'''
  pub fn probe_native_microfont_r8()->Result<NativeMicrofontR8Proof,&'static str> {
   use crate::{Ordering,PROBE_CALLS,PROBE_MODE,STARTUP_EVENTS};
   assert!(!crate::shell3_font_service::font_published(),"probe precedes FONT publication");
   assert_eq!(crate::shell3_font_service::atlas_calls(),0,"probe precedes full atlas production");
   PROBE_CALLS.fetch_add(1,Ordering::Relaxed);
   STARTUP_EVENTS.lock().unwrap().push("preflight");
   let mode=PROBE_MODE.load(Ordering::Relaxed);
   if mode==5 {return Err("resident-scene-geometry-incomplete")}
   if mode==6 {return Err("render-busy")}
   Ok(NativeMicrofontR8Proof {
    physical_vendor:0x8086,physical_device:0x9a49,physical_revision:1,
    effective_device:0x4680,effective_revision:12,
    completed_draws:if mode==3 {0}else{1},release_sequence:if mode==2 {0}else{17},
    gpu_poll_us:21,resources_released:mode!=4,
    pixels:native_font_probe_contract::Pixels {
     white_inside:if mode==1 {63}else{64},black_outside:192,mismatched:u32::from(mode==1),
    },
   })
  }
 }
}
mod virtio_gpu_logo {
 pub static AVAILABLE:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
 pub fn output_dimensions()->Option<(u32,u32)> {AVAILABLE.load(crate::Ordering::Relaxed).then_some((1920,1080))}
}
mod microfont {pub const FWIDTH:usize=6;pub const FHEIGHT:usize=11;}
'''
    source += "#[path=" + json.dumps(str(ROOT / "src/r/spawn_spec.rs")) + "] mod spawn_spec;\n"
    source += "mod shell3_font_service {\nuse crate::{microfont,spin};\n"
    source += "#[path=" + json.dumps(str(ROOT / "src/r/services/shell3_hex_font.rs")) + "] pub(crate) mod hex;\n"
    source += r'''
 use crate::{AtomicBool,AtomicUsize,AtomicU8,Ordering};
 pub(crate) struct OwnedAlphaAtlas;
 static ALPHA:OwnedAlphaAtlas=OwnedAlphaAtlas;
 static NATIVE_CALLS:AtomicUsize=AtomicUsize::new(0);
 static NATIVE_HEIGHT:AtomicUsize=AtomicUsize::new(0);
 static ERROR:AtomicU8=AtomicU8::new(0);
 static MISSING_QUESTION:AtomicBool=AtomicBool::new(false);
 impl OwnedAlphaAtlas {
  fn glyph(&self,ch:char)->Option<&'static [u8]> {
   match ch {'A'=>Some(&[17,128]),'?'=> (!MISSING_QUESTION.load(Ordering::Relaxed)).then_some(&[34,255][..]),_=>None}
  }
  fn glyphs(&self)->impl Iterator<Item=(char,&'static [u8])> {std::iter::once(('A',&[17,128][..]))}
 }
 fn build_shell3_alpha_atlas(height:u32)->Result<&'static OwnedAlphaAtlas,&'static str> {
  assert!(!font_published(),"atlas construction precedes FONT publication");
  if crate::TGL_ACTIVE.load(Ordering::Relaxed) {
   assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
   assert_eq!(NATIVE_BOOT_PREFLIGHT.get(),Some(&Ok(())),"preflight succeeds before atlas construction");
  }
  crate::STARTUP_EVENTS.lock().unwrap().push("atlas");
  NATIVE_CALLS.fetch_add(1,Ordering::Relaxed);NATIVE_HEIGHT.store(height as usize,Ordering::Relaxed);
  match ERROR.load(Ordering::Relaxed) {0=>Ok(&ALPHA),1=>Err("microfont-atlas-quarantined"),_=>Err("render-busy")}
 }
 fn build_bitmap_alpha_atlas(_:u32)->Result<&'static OwnedAlphaAtlas,&'static str> {panic!("retired CPU atlas must never be selected")}
 #[derive(Default)] struct CellCache;
'''
    font_source = (ROOT / FONT).read_text()
    source += constant(FONT, "DEFAULT_UI4_FONT_SCALE") + "\n"
    source += constant(FONT, "ATLAS_EM_HALF_MM") + "\n"
    source += constant(FONT, "LAPTOP_SHELL_EM_HALF_MM") + "\n"
    for name in ("cell_extent", "cell_extent_for_em", "Font",
                 "laptop_context_bootstrap_enabled", "native_boot_preflight", "prepare_once"):
        source += item(FONT, name) + "\n"
    source += "static FONT:spin::Once<Font>=spin::Once::new();\n"
    source += re.search(r"^static NATIVE_BOOT_PREFLIGHT:.*;$", font_source, re.MULTILINE).group() + "\n"
    source += block(font_source, "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]\npub(crate) enum FontFace") + "\n"
    source += block(font_source, "#[derive(Clone, Copy)]\nenum CellMask") + "\n"
    source += item(FONT, "resolved_face") + item(FONT, "resolved_glyph")
    source += r'''
 pub(super) fn font_published()->bool {FONT.get().is_some()}
 pub(super) fn atlas_calls()->usize {NATIVE_CALLS.load(Ordering::Relaxed)}
 fn enable_tgl() {
  crate::TGL_ACTIVE.store(true,Ordering::Relaxed);
  crate::GPU_AVAILABLE.store(true,Ordering::Relaxed);
  assert!(laptop_context_bootstrap_enabled());
 }
 fn assert_preflight_failure_is_cached(mode:u8) {
  enable_tgl();crate::PROBE_MODE.store(mode,Ordering::Relaxed);
  let failure=Err("microfont-native-r8-preflight-failed");
  assert_eq!(prepare_once(),failure);
  assert!(!font_published());assert_eq!(atlas_calls(),0);
  crate::PROBE_MODE.store(0,Ordering::Relaxed);
  assert_eq!(native_boot_preflight(),failure);assert_eq!(prepare_once(),failure);
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
  assert_eq!(atlas_calls(),0);assert!(!font_published());
  assert_eq!(*crate::STARTUP_EVENTS.lock().unwrap(),["preflight"]);
 }
 #[test] fn every_output_uses_native_atlas_and_retains_one_publication() {
  crate::ui4::PHYSICAL.with(|p|p.set(Some((999,94))));
  prepare_once().unwrap();prepare_once().unwrap();
  assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),1);
  assert_eq!(NATIVE_HEIGHT.load(Ordering::Relaxed),94);
  assert!(core::ptr::eq(FONT.get().unwrap().alpha,&ALPHA));
 }
 #[test] fn native_errors_propagate_without_publishing_or_selecting_bitmap() {
  for (mode,message) in [(1,"microfont-atlas-quarantined"),(2,"render-busy")] {
   ERROR.store(mode,Ordering::Relaxed);
   assert_eq!(prepare_once(),Err(message));assert!(FONT.get().is_none());
  }
  assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),2);
  ERROR.store(0,Ordering::Relaxed);prepare_once().unwrap();
  assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),3);
 }
 #[test] fn missing_question_mark_prevents_publication() {
  MISSING_QUESTION.store(true,Ordering::Relaxed);
  assert_eq!(prepare_once(),Err("shell3-font-missing-question-mark"));assert!(FONT.get().is_none());
 }
 #[test] fn absent_display_does_not_invent_font_geometry() {
  crate::ui4::OUTPUT.with(|p|p.set(None));
  assert_eq!(prepare_once(),Err("shell3-font-physical-size-unavailable"));
  assert_eq!(NATIVE_CALLS.load(Ordering::Relaxed),0);
 }
 #[test] fn explicit_hex_requests_use_embedded_masks_and_raster_keeps_native_masks() {
  prepare_once().unwrap();let font=FONT.get().unwrap();
  for requested in [FontFace::Raster] {
   let (mask,ink)=resolved_face(font,requested,'A',[1,2,3,127]).unwrap();
   assert!(matches!(mask,CellMask::Raster(&[17,128])));assert_eq!(ink,[1,2,3,127]);
   let (mask,ink)=resolved_face(font,requested,'漢',[1,2,3,127]).unwrap();
   assert!(matches!(mask,CellMask::Raster(&[34,255])));assert_eq!(ink,[255,0,0,127]);
  }
  for character in ['A','漢'] {
   let (expected,expected_ink)=hex::resolve(character,[1,2,3,127]);
   let (mask,ink)=resolved_face(font,FontFace::Hex,character,[1,2,3,127]).unwrap();
   assert!(matches!(mask,CellMask::Hex(glyph) if glyph == expected));
   assert_eq!(ink,expected_ink);
  }
 }
 #[test] fn tgl_successful_preflight_runs_once_before_atlas_and_publication() {
  enable_tgl();native_boot_preflight().unwrap();native_boot_preflight().unwrap();
  crate::ui4::PHYSICAL.with(|p|p.set(Some((999,94))));
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
  assert!(!font_published());assert_eq!(atlas_calls(),0);
  prepare_once().unwrap();prepare_once().unwrap();
  assert!(font_published());assert_eq!(atlas_calls(),1);
  assert_eq!(NATIVE_HEIGHT.load(Ordering::Relaxed),94);
  assert_eq!(crate::ui4::PHYSICAL_REQUESTS.with(|requests|requests.borrow().clone()),
      [(30,30,4),(16,16,4)],"one 7.5-mm atlas; separate 4-mm presentation cells");
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
  assert_eq!(*crate::STARTUP_EVENTS.lock().unwrap(),["preflight","atlas"]);
 }
 #[test] fn tgl_full_atlas_retry_reuses_passed_preflight() {
  enable_tgl();ERROR.store(2,Ordering::Relaxed);
  assert_eq!(prepare_once(),Err("render-busy"));assert!(!font_published());
  ERROR.store(0,Ordering::Relaxed);prepare_once().unwrap();
  assert!(font_published());assert_eq!(atlas_calls(),2);
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
  assert_eq!(*crate::STARTUP_EVENTS.lock().unwrap(),["preflight","atlas","atlas"]);
 }
 #[test] fn tgl_bad_pixels_cannot_publish_despite_full_retirement_and_fence() {
  assert_preflight_failure_is_cached(1);
 }
 #[test] fn tgl_correct_pixels_without_release_fence_cannot_publish() {
  assert_preflight_failure_is_cached(2);
 }
 #[test] fn tgl_incomplete_draw_cannot_publish() {assert_preflight_failure_is_cached(3);}
 #[test] fn tgl_unreleased_resources_cannot_publish() {assert_preflight_failure_is_cached(4);}
 #[test] fn tgl_terminal_probe_error_is_cached_without_atlas_or_repeat_submission() {
  assert_preflight_failure_is_cached(5);
 }
 #[test] fn tgl_busy_from_an_attempted_probe_is_cached_without_resubmission() {
  assert_preflight_failure_is_cached(6);
 }
 #[test] fn tgl_early_gpu_readiness_is_retryable_and_does_not_cache_an_attempt() {
  crate::TGL_ACTIVE.store(true,Ordering::Relaxed);
  assert_eq!(prepare_once(),Err("render-busy"));
  assert!(NATIVE_BOOT_PREFLIGHT.get().is_none());
  crate::GPU_AVAILABLE.store(true,Ordering::Relaxed);
  crate::WARM_AVAILABLE.store(false,Ordering::Relaxed);
  assert_eq!(prepare_once(),Err("render-busy"));
  assert!(NATIVE_BOOT_PREFLIGHT.get().is_none());
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),0);assert_eq!(atlas_calls(),0);
  crate::WARM_AVAILABLE.store(true,Ordering::Relaxed);prepare_once().unwrap();
  assert!(font_published());assert_eq!(atlas_calls(),1);
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),1);
 }
 #[test] fn desktop_does_not_call_or_cache_the_laptop_probe() {
  crate::PROBE_MODE.store(5,Ordering::Relaxed);
  assert!(!laptop_context_bootstrap_enabled());
  native_boot_preflight().unwrap();prepare_once().unwrap();
  assert!(font_published());assert_eq!(atlas_calls(),1);
  assert_eq!(crate::PROBE_CALLS.load(Ordering::Relaxed),0);
  assert!(NATIVE_BOOT_PREFLIGHT.get().is_none());
  assert_eq!(*crate::STARTUP_EVENTS.lock().unwrap(),["atlas"]);
 }
}
mod spawn_service {
 use crate::spawn_spec::{SpawnAttempt,TaskSpec};
 static MICROFONT_WARM_STARTED:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
 fn spawn_microfont_warm(_:crate::Spawner)->SpawnAttempt {SpawnAttempt::Spawned}
'''
    source += item("src/r/services/spawn_service.rs", "microfont_warm_gate")
    specs = re.findall(r'    TaskSpec::enabled_gated\(\n        "microfont-warm",.*?\n    \),',
                       (ROOT / "src/r/services/spawn_service.rs").read_text(), re.DOTALL)
    assert len(specs) == 1
    source += "fn spec()->TaskSpec {" + specs[0].strip().removesuffix(",") + "}\n"
    source += r'''
 #[test] fn all_outputs_wait_for_compositor_and_native_gpu_readiness() {
  assert!(!microfont_warm_gate(),"geometry alone cannot start native rendering");
  crate::GPU_AVAILABLE.store(true,crate::Ordering::Relaxed);
  assert!(!microfont_warm_gate(),"GPU readiness alone does not grant compositor ownership");
  crate::r::readiness::BITS.store(crate::r::readiness::UI4_COMPOSITOR_READY,crate::Ordering::Relaxed);
  assert!(microfont_warm_gate());
  let spec=spec();assert_eq!(spec.required,crate::r::readiness::BACKGROUND_AP_WORKER_READY);
 }
 #[test] fn virgl_retains_its_existing_native_line_backend_gate() {
  crate::r::readiness::BITS.store(crate::r::readiness::UI4_COMPOSITOR_READY,crate::Ordering::Relaxed);
  assert!(!microfont_warm_gate());
  crate::virtio_gpu_logo::AVAILABLE.store(true,crate::Ordering::Relaxed);assert!(microfont_warm_gate());
 }
}
'''
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-native-font-startup-") as directory:
        code, binary = Path(directory) / "tests.rs", Path(directory) / "tests"
        code.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)], check=True)
        names = [line.removesuffix(": test") for line in
                 subprocess.check_output([str(binary), "--list"], text=True).splitlines()
                 if line.endswith(": test")]
        assert len(names) >= 7
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
