#!/usr/bin/env python3
"""Check the production TRUEOS sprite-start policy without GPU/asset loading."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT.parent / "voxy/src/scene/terrain/mod.rs"


def main():
    source = SOURCE.read_text()
    start = source.index("    pub fn prepare(max_texture_size: u32)")
    end = source.index("    fn prepare_started(max_texture_size: u32)", start)
    policy = source[start:end]
    with tempfile.TemporaryDirectory(prefix="voxy-deferred-sprites-") as directory:
        folder = Path(directory)
        rust = folder / "tests.rs"
        rust.write_text(r'''
use std::cell::RefCell;
thread_local! { static STARTED: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) }; }
struct Renderer(u32);
impl Renderer { fn max_texture_size(&self) -> u32 { self.0 } }
struct SpriteRenderContext(u32);
type SpriteRenderContextLazy = Box<dyn FnMut(&mut Renderer) -> SpriteRenderContext>;
impl SpriteRenderContext {
''' + policy + r'''
    // Substitute asset/GPU preparation only; the start policy above is copied
    // unchanged from production and compiled with its TRUEOS cfg enabled.
    fn prepare_started(limit: u32) -> SpriteRenderContextLazy {
        STARTED.with(|started| started.borrow_mut().push(limit));
        Box::new(move |_| SpriteRenderContext(limit))
    }
}
#[test]
fn quitting_without_world_entry_starts_no_sprite_work() {
    let pending = SpriteRenderContext::prepare(4096);
    STARTED.with(|started| assert!(started.borrow().is_empty()));
    drop(pending);
    STARTED.with(|started| assert!(started.borrow().is_empty()));
}
#[test]
fn first_world_use_starts_once_with_the_negotiated_device_limit() {
    let mut pending = SpriteRenderContext::prepare(4096);
    let mut renderer = Renderer(1024);
    assert_eq!(pending(&mut renderer).0, 1024);
    assert_eq!(pending(&mut renderer).0, 1024);
    STARTED.with(|started| assert_eq!(*started.borrow(), vec![1024]));
}
#[test]
fn preparation_hint_remains_an_upper_bound() {
    let mut pending = SpriteRenderContext::prepare(512);
    assert_eq!(pending(&mut Renderer(4096)).0, 512);
    STARTED.with(|started| assert_eq!(*started.borrow(), vec![512]));
}
''')
        binary = folder / "tests"
        subprocess.run([
            "rustc", "--test", "--edition=2024", "--target=x86_64-unknown-linux-gnu",
            "--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags",
            str(rust), "-o", str(binary),
        ], check=True, timeout=60)
        subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
