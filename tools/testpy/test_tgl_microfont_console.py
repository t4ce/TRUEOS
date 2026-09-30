#!/usr/bin/env python3
"""Host tests for the CPU log, not a full kernel or hardware test.

Requires rustup stable, microfont 3.7.8 and the sibling TRUEOS-Blueprints log-os
crate used by the kernel. No hardware or test-rig endpoints are contacted.
"""
from pathlib import Path
import json
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def function(source: str, name: str) -> str:
    found = list(re.finditer(r"\bfn " + re.escape(name) + r"\(", source))
    assert len(found) == 1, name
    start = found[0].start()
    end = source.find("\n}", start)
    assert end > start, name
    return source[start:end + 2]


def main() -> None:
    log = (ROOT / "src/log_os.rs").read_text()
    intel = (ROOT / "src/intel/mod.rs").read_text()
    panel = (ROOT / "src/intel/tgl_native_panel.rs").read_text()
    display = (ROOT / "src/intel/display.rs").read_text()
    slot4 = (ROOT / "src/ui4/slot4_service.rs").read_text()
    assert "pub(crate) mod microfont_console;" in log
    assert "[&microfont_console::SINK, &TCP_LOG_SINK, &EMULATOR_UART_LOG_SINK]" in log
    boot = function(intel, "init_once")
    assert boot.index("enable_for_pci_identity") < boot.index("init_physical_gt_once")
    native = function(panel, "init_once")
    assert native.index("RETAINED_FRAME.call_once") < native.index("microfont_console::attach")
    assert native.index("microfont_console::attach") < native.index("let ctl_before")
    assert "super::display::init_primary_boot_surface(dev)" in native
    assert "SPIRIT_RESEALED: bool = false" in panel
    bootstrap = function(display, "bootstrap_ui4_rgba8_plane_stack_once")
    assert "microfont_overlay::bootstrap_surface" in bootstrap
    assert "if !microfont_overlay::owns_plane(surface.pipe, surface.plane_slot)" in bootstrap
    assert "ensure_overlay_surface_for_pipe" in bootstrap
    assert "wait_for_plane_stack_live" in bootstrap
    assert "publish_ui4_output_capabilities" in bootstrap
    ensure = function(display, "ensure_overlay_surface_for_pipe")
    assert ensure.index("microfont_overlay::owns_plane") < ensure.index("overlay_surface_pool")
    flip = function(display, "queue_ui4_plane_surface_flip")
    assert "microfont_overlay::owns_plane(PIPES[0]" in flip
    assert "PlaneSurfaceFlipQueueResult::Rejected" in flip
    service = function(slot4, "ui4_slot4_service_task")
    assert service.index("microfont_console::owns_plane") < service.index("service online")
    print("PASS: capture before GT, CPU-only retained surface, actual UI4 continuation, exclusive log plane", flush=True)

    core = Path(os.environ.get("TRUEOS_LOG_OS_DIR", str(ROOT.parent / "TRUEOS-Blueprints/crates/log-os"))).resolve()
    if not (core / "Cargo.toml").is_file():
        raise SystemExit(f"missing existing kernel log-os dependency: {core}")
    console = json.dumps(str(ROOT / "src/log_os/microfont_console.rs"))
    overlay = json.dumps(str(ROOT / "src/intel/display/microfont_overlay.rs"))
    harness = r'''
#![allow(dead_code)]
mod ui4 { pub const INTERACTION_OVERLAY_PLANE_SLOT: usize = 4; }
mod intel {
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {}
    pub fn dma_flush_strided_rows(_: *mut u8, _: usize, _: usize, _: usize) -> bool { true }
    mod display {
        #[derive(Clone, Copy)] struct PipeInfo { slot: usize }
        struct OverlaySurface {
            width: u32, height: u32, pitch_bytes: u32, byte_len: usize,
            phys: u64, virt: *mut u8, gpu: u64, pipe: PipeInfo,
            plane_slot: usize, buffer_index: usize,
        }
        #[path = OVERLAY_PATH] mod microfont_overlay;
    }
}
mod log_os { #[path = CONSOLE_PATH] pub mod microfont_console; }
'''.replace("CONSOLE_PATH", console).replace("OVERLAY_PATH", overlay)
    with tempfile.TemporaryDirectory(prefix="trueos-microfont-") as temp:
        work = Path(temp)
        (work / "lib.rs").write_text(harness)
        (work / "Cargo.toml").write_text(
            '[package]\nname="trueos-microfont-host-test"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\n'
            'microfont="=3.7.8"\nspin="0.10"\n'
            'log_os_core={package="log-os",path=' + json.dumps(str(core)) + ',default-features=false}\n'
        )
        env = dict(os.environ, RUSTUP_TOOLCHAIN="stable")
        subprocess.run(["cargo", "test", "--manifest-path", str(work / "Cargo.toml"),
                        "--", "--test-threads=1"], cwd=work, env=env, check=True)
    print("PASS: actual console and overlay adapter compiled with microfont and log-os; Rust tests passed", flush=True)


if __name__ == "__main__":
    main()
