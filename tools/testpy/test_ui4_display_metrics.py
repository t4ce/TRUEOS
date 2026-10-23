#!/usr/bin/env python3
"""Exercise actual output sizing policy with native/virtual readiness controls."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
mod intel {
    std::thread_local! {pub static EXTENT:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(None)};}
    pub fn physical_extent_pixels(_:u32,_:u32)->Option<(u32,u32)> {EXTENT.get()}
}
mod virtio_gpu_logo {
    std::thread_local! {pub static OUTPUT:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(None)};}
    pub fn output_dimensions()->Option<(u32,u32)> {OUTPUT.get()}
}
#[path=__SOURCE__] mod display_metrics;
use display_metrics::physical_extent_pixels;
#[test] fn native_edid_result_keeps_priority_over_virtual_policy() {
    intel::EXTENT.set(Some((53,71)));
    virtio_gpu_logo::OUTPUT.set(Some((1920,1080)));
    assert_eq!(physical_extent_pixels(5,5),Some((53,71)));
}
#[test] fn virtual_size_requires_ready_output_and_ignores_window_resolution() {
    assert_eq!(physical_extent_pixels(5,5),None);
    virtio_gpu_logo::OUTPUT.set(Some((1920,1080)));
    assert_eq!(physical_extent_pixels(5,5),Some((19,19)));
    assert_eq!(physical_extent_pixels(50,50),Some((189,189)));
    virtio_gpu_logo::OUTPUT.set(Some((3840,2160)));
    assert_eq!(physical_extent_pixels(5,5),Some((19,19)));
    virtio_gpu_logo::OUTPUT.set(None);
    assert_eq!(physical_extent_pixels(5,5),None);
}
#[test] fn invalid_virtual_rectangles_fail_closed() {
    virtio_gpu_logo::OUTPUT.set(Some((1920,1080)));
    assert_eq!(physical_extent_pixels(0,5),None);
    assert_eq!(physical_extent_pixels(5,0),None);
    assert_eq!(physical_extent_pixels(u32::MAX,5),None);
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-display-metrics-") as directory:
        harness = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        harness.write_text(HARNESS.replace(
            "__SOURCE__", json.dumps(str(ROOT / "src/ui4/display_metrics.rs"))))
        subprocess.run([os.environ.get("RUSTC", "rustc"), "--edition=2024", "--test",
                        str(harness), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
