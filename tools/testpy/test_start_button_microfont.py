#!/usr/bin/env python3
"""Host-test the actual Microfont/button handoff and one-to-one mask placement."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item

SERVICE = "src/r/services/microfont_mask_service.rs"
BUTTON = "src/ui4/start_button.rs"


def async_item(name):
    source = (ROOT / SERVICE).read_text()
    start = source.index(f"pub(crate) async fn {name}(")
    end = re.search(r"^}\n", source[start:], re.MULTILINE)
    assert end is not None
    return source[start:start + end.end()]


HARNESS = r'''
#![allow(dead_code)]
use std::{future::Future, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering}}, task::{Context, Poll, Wake, Waker}};
#[macro_export] macro_rules! log_important { (target: $target:expr; $($args:tt)*) => {let _ = format_args!($($args)*);}; }
struct GlyphSlot(OnceLock<OwnedGlyphMask>);
impl GlyphSlot {
    fn get(&self) -> Option<&OwnedGlyphMask> {self.0.get()}
    fn call_once(&self, make:impl FnOnce()->OwnedGlyphMask) -> &OwnedGlyphMask {self.0.get_or_init(make)}
}
// Controllable wait boundary: completion can occur immediately before registration.
struct ReadyQueue {generation:AtomicU32,waker:Mutex<Option<Waker>>}
impl ReadyQueue {
    fn observe(&self)->u32 {self.generation.load(Ordering::Acquire)}
    fn notify_all(&self) {
        self.generation.fetch_add(1,Ordering::Release);
        if let Some(w)=self.waker.lock().unwrap().take(){w.wake();}
    }
    async fn wait_after(&self,observed:u32) {
        if COMPLETE_DURING_WAIT.swap(false,Ordering::AcqRel){warm_start_glyph().unwrap();}
        std::future::poll_fn(|cx| {
            let mut waker=self.waker.lock().unwrap();
            if self.observe()!=observed {Poll::Ready(())}
            else {*waker=Some(cx.waker().clone());Poll::Pending}
        }).await
    }
}
static START_GLYPH:GlyphSlot=GlyphSlot(OnceLock::new());
struct WarmLock(Mutex<()>);
impl WarmLock {fn try_lock(&self)->Option<std::sync::MutexGuard<'_,()>> {self.0.try_lock().ok()}}
static START_GLYPH_WARM:WarmLock=WarmLock(Mutex::new(()));
static START_GLYPH_READY:ReadyQueue=ReadyQueue {generation:AtomicU32::new(0),waker:Mutex::new(None)};
static COMPLETE_DURING_WAIT:AtomicBool=AtomicBool::new(false);
static FAIL_PRODUCTION:AtomicBool=AtomicBool::new(false);
static PRODUCTIONS:AtomicUsize=AtomicUsize::new(0);
mod ui4 {
    pub fn physical_extent_pixels(w:u32,h:u32)->Option<(u32,u32)> {
        assert_eq!((w,h),(50,50));Some((500,500))
    }
}
mod shell3_font_service {
    pub const DEFAULT_UI4_FONT_SCALE:u32=2;
    pub fn cell_extent(_:u32)->Option<(u32,u32)> {
        panic!("the button handoff harness supplies physical display metrics")
    }
}
fn build_home_button_mask(height:u32)->Result<OwnedGlyphMask,&'static str> {
    assert_eq!(height,500);PRODUCTIONS.fetch_add(1,Ordering::Relaxed);
    if FAIL_PRODUCTION.load(Ordering::Relaxed){return Err("production-failed");}
    Ok(OwnedGlyphMask {width:3,height:2,em_height_px:height,pixels:vec![1,2,3,4,5,6]})
}
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {fn wake(self:Arc<Self>){self.0.fetch_add(1,Ordering::Relaxed);}}
#[test] fn publication_wakes_button_and_is_retained_for_late_consumers() {
    let wake=Arc::new(WakeCount(AtomicUsize::new(0)));let waker=Waker::from(wake.clone());
    let mut cx=Context::from_waker(&waker);let mut button=Box::pin(wait_for_start_glyph());
    assert!(button.as_mut().poll(&mut cx).is_pending());
    FAIL_PRODUCTION.store(true,Ordering::Relaxed);
    assert_eq!(warm_start_glyph(),Err("production-failed"));
    assert!(START_GLYPH.get().is_none());assert_eq!(wake.0.load(Ordering::Relaxed),0);
    FAIL_PRODUCTION.store(false,Ordering::Relaxed);warm_start_glyph().unwrap();
    assert_eq!(wake.0.load(Ordering::Relaxed),1);
    let Poll::Ready(glyph)=button.as_mut().poll(&mut cx) else {panic!("button was not notified")};
    assert_eq!(glyph.em_height_px,500);assert_eq!(glyph.pixels,[1,2,3,4,5,6]);
    let mut late=Box::pin(wait_for_start_glyph());
    let Poll::Ready(again)=late.as_mut().poll(&mut cx) else {panic!("late button lost readiness")};
    assert!(std::ptr::eq(glyph,again));
    warm_start_glyph().unwrap();assert_eq!(PRODUCTIONS.load(Ordering::Relaxed),2);
    assert_eq!(wake.0.load(Ordering::Relaxed),1);
}
#[test] fn completion_before_wait_registration_does_not_lose_readiness() {
    COMPLETE_DURING_WAIT.store(true,Ordering::Release);
    let waker=Waker::from(Arc::new(WakeCount(AtomicUsize::new(0))));
    let mut cx=Context::from_waker(&waker);let mut button=Box::pin(wait_for_start_glyph());
    assert!(button.as_mut().poll(&mut cx).is_ready());
    assert_eq!(PRODUCTIONS.load(Ordering::Relaxed),1);
}
#[test] fn concurrent_warm_attempt_defers_without_producing_a_second_mask() {
    let held=START_GLYPH_WARM.try_lock().unwrap();
    assert_eq!(warm_start_glyph(),Err("microfont-start-glyph-busy"));
    assert!(START_GLYPH.get().is_none());assert_eq!(PRODUCTIONS.load(Ordering::Relaxed),0);
    drop(held);warm_start_glyph().unwrap();warm_start_glyph().unwrap();
    assert_eq!(PRODUCTIONS.load(Ordering::Relaxed),1);
}
#[test] fn button_centers_original_mask_without_resampling_or_copying() {
    let glyph=OwnedGlyphMask {width:3,height:2,em_height_px:500,pixels:vec![1,2,3,4,5,6]};
    let (w,h)=start_button_extent(glyph.width,glyph.height,2).unwrap();
    assert_eq!((w,h),(7,6));
    let mut foreground=Vec::new();
    for y in 0..h {for x in 0..w {
        let alpha=start_button_glyph_alpha(&glyph,w,h,x,y);
        if (2..5).contains(&x)&&(2..4).contains(&y){foreground.push(alpha)}else{assert_eq!(alpha,0)}
    }}
    assert_eq!(foreground,glyph.pixels);
    assert_eq!(start_button_extent(1,1,u32::MAX),None);
    assert_eq!(start_button_extent(u32::MAX,1,1),None);
}
'''


def main():
    code = HARNESS + item(SERVICE, "OwnedGlyphMask")
    code += constant(SERVICE, "START_GLYPH_EM_MM")
    code += item(SERVICE, "home_button_em_height").replace(
        "super::shell3_font_service", "shell3_font_service")
    code += item(SERVICE, "warm_start_glyph") + async_item("wait_for_start_glyph")
    code += item(BUTTON, "start_button_extent") + item(BUTTON, "start_button_glyph_alpha")
    with tempfile.TemporaryDirectory(prefix="trueos-start-button-microfont-") as directory:
        source = Path(directory) / "tests.rs"
        executable = Path(directory) / "tests"
        source.write_text(code)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
        # The production slot is process-lifetime, so each handoff scenario starts cold.
        for name in ("publication_wakes_button_and_is_retained_for_late_consumers",
                     "completion_before_wait_registration_does_not_lose_readiness",
                     "concurrent_warm_attempt_defers_without_producing_a_second_mask",
                     "button_centers_original_mask_without_resampling_or_copying"):
            subprocess.run([str(executable), "--exact", name], check=True)


if __name__ == "__main__":
    main()
