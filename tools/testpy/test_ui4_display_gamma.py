#!/usr/bin/env python3
"""Exercise the production gamma/fade ABI with an inert MMIO palette."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def function(source, name):
    start = source.index(name)
    start = source.rfind('\n', 0, start) + 1
    opening = source.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def main():
    blueprint = (ROOT / 'src/ui4/blueprint_text.rs').read_text()
    picker = (ROOT / 'src/ui4/color_picker.rs').read_text()
    start = blueprint.index('struct DisplayFadeState')
    end = blueprint.index('static NEXT_BLUEPRINT_FONT_SPRITE_ID', start)
    state = blueprint[start:end]
    methods = picker[picker.index('impl PipeGammaSnapshot {'):picker.index('static PIPE_GAMMA')]
    source = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
type WindowOwner = u32;
static OWNER: AtomicU32 = AtomicU32::new(1);
static FAIL: AtomicBool = AtomicBool::new(false);
mod hv { pub fn current_hull_guest_context_vm_id() -> Option<u32> { None } }
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_UI4_SCENE_DISPLAY_FADE: u32 = 0x220;
    pub const OP_BP_UI4_SCENE_SET_DISPLAY_GAMMA_RAMP: u32 = 0x211;
} }
#[path = "__CURVE__"] mod display_fade_curve;
mod color_picker {
    use super::*;
    const POST_CSC_GAMMA_ENABLE: u32 = 1 << 30;
    const GAMMA_MODE_MASK: u32 = 3;
    const GAMMA_MODE_10_BIT: u32 = 1;
    const PRECISION_PALETTE_ENTRIES: usize = 1024;
    #[derive(Clone, Debug, PartialEq)]
    pub struct PipeGammaSnapshot { pub mode: u32, pub precision_palette: [u32; 1024] }
    __METHODS__
    pub static HW: Mutex<PipeGammaSnapshot> = Mutex::new(PipeGammaSnapshot {
        mode: 0x40000001, precision_palette: [0x15555; 1024],
    });
    pub fn read_pipe_a_gamma() -> Option<PipeGammaSnapshot> { Some(HW.lock().clone()) }
    pub fn program_pipe_a_precision_gamma(entries: &[u32; 1024]) -> bool {
        if FAIL.load(Ordering::Relaxed) { return false; }
        let snapshot = HW.lock().with_precision_palette(*entries);
        *HW.lock() = snapshot;
        true
    }
    pub fn fade_pipe_a_gamma(snapshot: &PipeGammaSnapshot, amount: i32) -> bool {
        if FAIL.load(Ordering::Relaxed) { return false; }
        if amount == 0 { *HW.lock() = snapshot.clone(); return true; }
        if !snapshot.supports_precision_transfer() { return false; }
        let entries = display_fade_curve::palette(&snapshot.precision_palette, amount, snapshot.mode & POST_CSC_GAMMA_ENABLE != 0);
        program_pipe_a_precision_gamma(&entries)
    }
}
mod blueprint_text {
    use super::*;
    const ERROR_INVALID: i32 = -1;
    const ERROR_CONTEXT: i32 = -2;
    const ERROR_NOT_FOUND: i32 = -3;
    const ERROR_UI4: i32 = -4;
    const ERROR_BUSY: i32 = -5;
    static SURFACES: Mutex<()> = Mutex::new(());
    fn blueprint_owner() -> Option<u32> { Some(OWNER.load(Ordering::Relaxed)) }
    fn surface_mut(s: &mut (), _: u32, window: u32) -> Option<&mut ()> { (window < 3).then_some(s) }
    fn guest_status(_: u32, _: u64, _: u64, _: &[u8]) -> i32 { -4 }
    __STATE__
    __GAMMA__
    __FADE__
    pub fn close(owner: u32, window: Option<u32>) { release_display_color(owner, window); }
    pub fn reset() { *DISPLAY_GAMMA.lock() = None; *DISPLAY_FADE.lock() = None; }
}
use blueprint_text::{trueos_cabi_ui4_scene_set_display_gamma_ramp as gamma, trueos_cabi_ui4_scene_display_fade_v1 as fade};
fn reset() -> color_picker::PipeGammaSnapshot {
    blueprint_text::reset(); OWNER.store(1, Ordering::Relaxed); FAIL.store(false, Ordering::Relaxed);
    let original = color_picker::PipeGammaSnapshot { mode: 0x40000001, precision_palette: [0x15555; 1024] };
    *color_picker::HW.lock() = original.clone(); original
}
fn ramp() -> [u16; 768] { core::array::from_fn(|i| (i % 256) as u16 * 257) }
#[test]
fn gamma_updates_compose_with_portal_fade_and_close_restores_original() {
    let original = reset(); let identity = ramp();
    assert_eq!(gamma(1, identity.as_ptr()), 0);
    assert_eq!(fade(1, -32768), 0);
    let constant = core::array::from_fn::<_, 768, _>(|i| match i / 256 { 0 => 65535, 1 => 32768, _ => 0 });
    assert_eq!(gamma(1, constant.as_ptr()), 0);
    let base = [(1023 << 20) | (512 << 10); 1024];
    assert_eq!(color_picker::HW.lock().precision_palette, display_fade_curve::palette(&base, -32768, true));
    assert_eq!(fade(1, 0), 0);
    assert_eq!(color_picker::HW.lock().precision_palette, base);
    assert_eq!(fade(1, 65535), 0);
    assert_eq!(color_picker::HW.lock().precision_palette, [0x3fffffff; 1024]);
    blueprint_text::close(1, Some(2));
    assert_ne!(*color_picker::HW.lock(), original);
    blueprint_text::close(1, Some(1));
    assert_eq!(*color_picker::HW.lock(), original);
}
#[test]
fn fade_started_before_gamma_and_owner_teardown_restore_original() {
    let original = reset();
    assert_eq!(fade(1, 32768), 0);
    assert_eq!(gamma(1, ramp().as_ptr()), 0);
    blueprint_text::close(1, None);
    assert_eq!(*color_picker::HW.lock(), original);
}
#[test]
fn other_windows_cannot_overwrite_a_transfer_or_its_fade() {
    reset(); assert_eq!(gamma(1, ramp().as_ptr()), 0);
    let before = color_picker::HW.lock().clone();
    OWNER.store(2, Ordering::Relaxed);
    assert_eq!(gamma(2, ramp().as_ptr()), -5);
    assert_eq!(fade(2, 100), -5);
    assert_eq!(fade(2, 0), -5);
    blueprint_text::close(2, None);
    assert_eq!(*color_picker::HW.lock(), before);
}
#[test]
fn invalid_calls_do_not_acquire_ownership_and_programming_errors_retain_restoration() {
    let original = reset();
    assert_eq!(gamma(1, core::ptr::null()), -1);
    assert_eq!(fade(1, 65536), -1);
    OWNER.store(2, Ordering::Relaxed);
    assert_eq!(gamma(2, ramp().as_ptr()), 0);
    blueprint_text::close(2, None);
    assert_eq!(*color_picker::HW.lock(), original);
    reset(); FAIL.store(true, Ordering::Relaxed);
    assert_eq!(gamma(1, ramp().as_ptr()), -4);
    assert_eq!(fade(1, 100), -4);
    FAIL.store(false, Ordering::Relaxed); OWNER.store(2, Ordering::Relaxed);
    assert_eq!(gamma(2, ramp().as_ptr()), -5);
    blueprint_text::close(1, None);
    assert_eq!(*color_picker::HW.lock(), original);
    assert_eq!(gamma(2, ramp().as_ptr()), 0);
    blueprint_text::close(2, None);
    assert_eq!(*color_picker::HW.lock(), original);
}

'''
    replacements = {
        '__CURVE__': str(ROOT / 'src/ui4/display_fade_curve.rs'),
        '__METHODS__': methods,
        '__STATE__': state,
        '__GAMMA__': function(blueprint, 'pub extern "C" fn trueos_cabi_ui4_scene_set_display_gamma_ramp'),
        '__FADE__': function(blueprint, 'pub extern "C" fn trueos_cabi_ui4_scene_display_fade_v1'),
    }
    for key, value in replacements.items():
        source = source.replace(key, value)
    with tempfile.TemporaryDirectory(prefix='trueos-display-gamma-') as tmp:
        path = Path(tmp) / 'test.rs'
        path.write_text(source)
        binary = Path(tmp) / 'test'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
